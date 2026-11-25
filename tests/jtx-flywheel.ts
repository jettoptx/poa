// npx ts-mocha -p ./tsconfig.json -t 1000000 tests/jtx-flywheel.ts

import * as path from "path"
import * as anchor from "@coral-xyz/anchor"
import { BN, Program } from "@coral-xyz/anchor"
import {
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
  Transaction,
} from "@solana/web3.js"
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  MINT_SIZE,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createInitializeMint2Instruction,
} from "@solana/spl-token"
import {
  createAssociatedTokenAccount,
  mintTo,
} from "spl-token-bankrun"
import { Clock, startAnchor, type ProgramTestContext } from "solana-bankrun"
import type { BanksClient } from "solana-bankrun"
import { BankrunProvider } from "anchor-bankrun"
import { expect } from "chai"
import { IDL, JtxFlywheel } from "../target/types/jtx_flywheel"

const PROGRAM_ID = new PublicKey("Ewz1XZQWGJWUXZtBdXz5DR8sYHZkzKD8LGNGNc5uMWyJ")
const ROOT = path.resolve(__dirname, "..")

const BPS = 10_000
const FEE_BPS = 100
const UNIT = new BN("1000000000")
const INDEX_SCALE = new BN("1000000000000")
const SEED = UNIT.muln(1_000_000)
const JTO_IN = UNIT.muln(100)
const STAKE_HALF = UNIT.muln(50)
const STAKE_FULL = UNIT.muln(100)
const WEIGHT_AT_100 = STAKE_FULL.muln(180).divn(BPS)
const WEIGHT_CAP = UNIT.muln(5_000)
const RAMP_SECONDS = 90 * 24 * 60 * 60
const BOOST_MAX = 20_000
const DEFAULT_TIERS: ReadonlyArray<readonly [number, number]> = [
  [100, 180],
  [1_000, 320],
  [8_000, 460],
  [40_000, 550],
]

const find = (seeds: Buffer[]) =>
  PublicKey.findProgramAddressSync(seeds, PROGRAM_ID)[0]

const configPda = find([Buffer.from("config")])
const poolPda = find([Buffer.from("pool")])
const poolJtx = find([Buffer.from("pool_jtx")])
const poolJto = find([Buffer.from("pool_jto")])
const stakeJtx = find([Buffer.from("stake_jtx")])
const rewardJto = find([Buffer.from("reward_jto")])

const asBn = (value: { toString(): string }) => new BN(value.toString())

function expectBn(actual: BN, expected: BN) {
  expect(actual.toString()).to.equal(expected.toString())
}

function quoteBuy(jtxReserve: BN, jtoReserve: BN, jtoIn: BN) {
  const fee = jtoIn.muln(FEE_BPS).divn(BPS)
  const curveIn = jtoIn.sub(fee)
  const jtxOut = jtxReserve.mul(curveIn).div(jtoReserve.add(curveIn))
  return { fee, curveIn, jtxOut }
}

function quoteSell(jtxReserve: BN, jtoReserve: BN, jtxIn: BN) {
  const jtoGross = jtoReserve.mul(jtxIn).div(jtxReserve.add(jtxIn))
  const fee = jtoGross.muln(FEE_BPS).divn(BPS)
  const jtoToUser = jtoGross.sub(fee)
  return { jtoGross, fee, jtoToUser }
}

function errorText(err: unknown): string {
  if (!err || typeof err !== "object") return String(err)
  const value = err as {
    message?: string
    logs?: string[]
    logMessages?: string[]
    meta?: { logMessages?: string[] }
    error?: { errorCode?: { code?: string }; errorMessage?: string }
  }
  return [
    value.message ?? "",
    value.error?.errorCode?.code ?? "",
    value.error?.errorMessage ?? "",
    ...(value.logs ?? []),
    ...(value.logMessages ?? []),
    ...(value.meta?.logMessages ?? []),
    String(err),
  ].join("\n")
}

async function expectError(run: () => Promise<unknown>, code: string) {
  let caught: unknown
  try {
    await run()
  } catch (err) {
    caught = err
  }
  if (caught === undefined) expect.fail(`expected ${code}`)
  expect(errorText(caught)).to.contain(code)
}

function funded(pubkey: PublicKey, sol: number) {
  return {
    address: pubkey,
    info: {
      lamports: sol * LAMPORTS_PER_SOL,
      data: Buffer.alloc(0),
      owner: SystemProgram.programId,
      executable: false,
    },
  }
}

async function createMint(
  banks: BanksClient,
  payer: Keypair,
  decimals: number,
  mintAuthority: PublicKey,
  programId: PublicKey
): Promise<PublicKey> {
  const mintKeypair = Keypair.generate()
  const rent = await banks.getRent()
  const lamports = Number(await rent.minimumBalance(BigInt(MINT_SIZE)))
  const blockhash = await banks.getLatestBlockhash()
  if (!blockhash) throw new Error("no recent blockhash")
  const tx = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: mintKeypair.publicKey,
      space: MINT_SIZE,
      lamports,
      programId,
    }),
    createInitializeMint2Instruction(
      mintKeypair.publicKey,
      decimals,
      mintAuthority,
      null,
      programId
    )
  )
  tx.recentBlockhash = blockhash[0]
  tx.feePayer = payer.publicKey
  tx.sign(payer, mintKeypair)
  await banks.processTransaction(tx)
  return mintKeypair.publicKey
}

describe("jtx-flywheel", function () {
  this.timeout(120_000)

  let context: ProgramTestContext
  let program: Program<JtxFlywheel>
  let jtxMint: PublicKey
  let jtoMint: PublicKey
  let adminJtx: PublicKey
  let adminJto: PublicKey
  let stakerJtx: PublicKey
  let stakerJto: PublicKey
  let traderJtx: PublicKey
  let traderJto: PublicKey

  const admin = Keypair.generate()
  const staker = Keypair.generate()
  const trader = Keypair.generate()
  const positionPda = find([
    Buffer.from("position"),
    staker.publicKey.toBuffer(),
  ])

  async function accountData(address: PublicKey): Promise<Buffer> {
    const acc = await context.banksClient.getAccount(address)
    if (!acc) throw new Error(`missing account ${address.toBase58()}`)
    return Buffer.from(acc.data)
  }

  async function tokenAmount(address: PublicKey): Promise<BN> {
    const data = await accountData(address)
    return new BN(data.readBigUInt64LE(64).toString())
  }

  async function mintSupply(mint: PublicKey): Promise<BN> {
    const data = await accountData(mint)
    return new BN(data.readBigUInt64LE(36).toString())
  }

  function initAccounts() {
    return {
      payer: admin.publicKey,
      admin: admin.publicKey,
      config: configPda,
      pool: poolPda,
      jtxMint,
      jtoMint,
      poolJtx,
      poolJto,
      stakeJtx,
      rewardJto,
      jtxTokenProgram: TOKEN_2022_PROGRAM_ID,
      jtoTokenProgram: TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    }
  }

  function seedAccounts() {
    return {
      admin: admin.publicKey,
      config: configPda,
      pool: poolPda,
      jtxMint,
      jtoMint,
      poolJtx,
      poolJto,
      adminJtx,
      adminJto,
      jtxTokenProgram: TOKEN_2022_PROGRAM_ID,
      jtoTokenProgram: TOKEN_PROGRAM_ID,
    }
  }

  function stakeAccounts() {
    return {
      staker: staker.publicKey,
      config: configPda,
      pool: poolPda,
      jtxMint,
      jtoMint,
      stakeJtx,
      rewardJto,
      position: positionPda,
      stakerJtx,
      stakerJto,
      jtxTokenProgram: TOKEN_2022_PROGRAM_ID,
      jtoTokenProgram: TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    }
  }

  function stakeMoveAccounts() {
    return {
      staker: staker.publicKey,
      config: configPda,
      pool: poolPda,
      jtxMint,
      jtoMint,
      stakeJtx,
      rewardJto,
      position: positionPda,
      stakerJtx,
      stakerJto,
      jtxTokenProgram: TOKEN_2022_PROGRAM_ID,
      jtoTokenProgram: TOKEN_PROGRAM_ID,
    }
  }

  function swapAccounts() {
    return {
      user: trader.publicKey,
      config: configPda,
      pool: poolPda,
      jtxMint,
      jtoMint,
      poolJtx,
      poolJto,
      rewardJto,
      userJtx: traderJtx,
      userJto: traderJto,
      jtxTokenProgram: TOKEN_2022_PROGRAM_ID,
      jtoTokenProgram: TOKEN_PROGRAM_ID,
    }
  }

  before(async () => {
    context = await startAnchor(
      ROOT,
      [{ name: "jtx_flywheel", programId: PROGRAM_ID }],
      [funded(admin.publicKey, 50), funded(staker.publicKey, 10), funded(trader.publicKey, 10)]
    )
    const provider = new BankrunProvider(context, new anchor.Wallet(admin))
    anchor.setProvider(provider)
    program = new Program<JtxFlywheel>(IDL, provider)

    const banks = context.banksClient
    jtxMint = await createMint(banks, admin, 9, admin.publicKey, TOKEN_2022_PROGRAM_ID)
    jtoMint = await createMint(banks, admin, 9, admin.publicKey, TOKEN_PROGRAM_ID)

    adminJtx = await createAssociatedTokenAccount(
      banks,
      admin,
      jtxMint,
      admin.publicKey,
      TOKEN_2022_PROGRAM_ID,
      ASSOCIATED_TOKEN_PROGRAM_ID
    )
    stakerJtx = await createAssociatedTokenAccount(
      banks,
      admin,
      jtxMint,
      staker.publicKey,
      TOKEN_2022_PROGRAM_ID,
      ASSOCIATED_TOKEN_PROGRAM_ID
    )
    traderJtx = await createAssociatedTokenAccount(
      banks,
      admin,
      jtxMint,
      trader.publicKey,
      TOKEN_2022_PROGRAM_ID,
      ASSOCIATED_TOKEN_PROGRAM_ID
    )
    adminJto = await createAssociatedTokenAccount(
      banks,
      admin,
      jtoMint,
      admin.publicKey,
      TOKEN_PROGRAM_ID,
      ASSOCIATED_TOKEN_PROGRAM_ID
    )
    stakerJto = await createAssociatedTokenAccount(
      banks,
      admin,
      jtoMint,
      staker.publicKey,
      TOKEN_PROGRAM_ID,
      ASSOCIATED_TOKEN_PROGRAM_ID
    )
    traderJto = await createAssociatedTokenAccount(
      banks,
      admin,
      jtoMint,
      trader.publicKey,
      TOKEN_PROGRAM_ID,
      ASSOCIATED_TOKEN_PROGRAM_ID
    )

    await mintTo(
      banks,
      admin,
      jtxMint,
      adminJtx,
      admin,
      BigInt(SEED.toString()),
      [],
      TOKEN_2022_PROGRAM_ID
    )
    await mintTo(
      banks,
      admin,
      jtxMint,
      stakerJtx,
      admin,
      BigInt(STAKE_FULL.toString()),
      [],
      TOKEN_2022_PROGRAM_ID
    )
    await mintTo(
      banks,
      admin,
      jtoMint,
      adminJto,
      admin,
      BigInt(SEED.toString()),
      [],
      TOKEN_PROGRAM_ID
    )
    await mintTo(
      banks,
      admin,
      jtoMint,
      traderJto,
      admin,
      BigInt(JTO_IN.toString()),
      [],
      TOKEN_PROGRAM_ID
    )
  })

  it("initialize stores admin, tiers, cap, and boost", async () => {
    await program.methods.initialize().accounts(initAccounts()).rpc()

    const cfg = await program.account.config.fetch(configPda)
    expect(cfg.admin.equals(admin.publicKey)).to.equal(true)
    expect(cfg.jtxMint.equals(jtxMint)).to.equal(true)
    expect(cfg.jtoMint.equals(jtoMint)).to.equal(true)
    expect(cfg.paused).to.equal(false)
    expect(cfg.tiers).to.have.length(4)
    DEFAULT_TIERS.forEach(([min, rate], i) => {
      expectBn(asBn(cfg.tiers[i].minAmount), UNIT.muln(min))
      expect(Number(cfg.tiers[i].rateBps)).to.equal(rate)
    })
    expectBn(asBn(cfg.weightCap), WEIGHT_CAP)
    expect(asBn(cfg.boostRampSeconds).toNumber()).to.equal(RAMP_SECONDS)
    expect(Number(cfg.boostMaxBps)).to.equal(BOOST_MAX)
  })

  it("seed_liquidity runs once", async () => {
    await program.methods.seedLiquidity(SEED, SEED).accounts(seedAccounts()).rpc()

    const pool = await program.account.pool.fetch(poolPda)
    expectBn(asBn(pool.jtxReserve), SEED)
    expectBn(asBn(pool.jtoReserve), SEED)
    expectBn(await tokenAmount(poolJtx), SEED)
    expectBn(await tokenAmount(poolJto), SEED)

    await expectError(
      () => program.methods.seedLiquidity(new BN(1), new BN(1)).accounts(seedAccounts()).rpc(),
      "AlreadySeeded"
    )
    const again = await program.account.pool.fetch(poolPda)
    expectBn(asBn(again.jtxReserve), SEED)
    expectBn(asBn(again.jtoReserve), SEED)
  })

  it("stake weight is zero under 100 JTX and tiered at 100", async () => {
    await program.methods
      .stake(STAKE_HALF)
      .accounts(stakeAccounts())
      .signers([staker])
      .rpc()

    const opened = await program.account.position.fetch(positionPda)
    expect(opened.staker.equals(staker.publicKey)).to.equal(true)
    expectBn(asBn(opened.amount), STAKE_HALF)
    expectBn(asBn(opened.weight), new BN(0))
    const stakedAt = asBn(opened.stakedAt)

    await program.methods
      .stake(STAKE_HALF)
      .accounts(stakeAccounts())
      .signers([staker])
      .rpc()

    const topped = await program.account.position.fetch(positionPda)
    expectBn(asBn(topped.stakedAt), stakedAt)
    expectBn(asBn(topped.amount), STAKE_FULL)
    expectBn(asBn(topped.weight), WEIGHT_AT_100)
    const pool = await program.account.pool.fetch(poolPda)
    expectBn(asBn(pool.totalWeight), WEIGHT_AT_100)
    expectBn(await tokenAmount(stakeJtx), STAKE_FULL)
    expectBn(await tokenAmount(stakerJtx), new BN(0))
  })

  it("buy_jtx takes a 1% JTO fee and rejects slippage", async () => {
    const pool0 = await program.account.pool.fetch(poolPda)
    const jtxReserve = asBn(pool0.jtxReserve)
    const jtoReserve = asBn(pool0.jtoReserve)
    const quoted = quoteBuy(jtxReserve, jtoReserve, JTO_IN)
    expectBn(quoted.fee, UNIT)
    expect(quoted.jtxOut.gtn(0)).to.equal(true)
    expectBn(quoted.fee.add(quoted.curveIn), JTO_IN)

    const supplyBefore = await mintSupply(jtxMint)
    const rewardBefore = await tokenAmount(rewardJto)
    const poolJtxBefore = await tokenAmount(poolJtx)
    const poolJtoBefore = await tokenAmount(poolJto)
    const traderJtxBefore = await tokenAmount(traderJtx)
    const traderJtoBefore = await tokenAmount(traderJto)

    await expectError(
      () =>
        program.methods
          .buyJtx(JTO_IN, quoted.jtxOut.addn(1))
          .accounts(swapAccounts())
          .signers([trader])
          .rpc(),
      "Slippage"
    )

    const unchanged = await program.account.pool.fetch(poolPda)
    expectBn(asBn(unchanged.jtxReserve), jtxReserve)
    expectBn(asBn(unchanged.jtoReserve), jtoReserve)

    await program.methods
      .buyJtx(JTO_IN, quoted.jtxOut)
      .accounts(swapAccounts())
      .signers([trader])
      .rpc()

    const pool1 = await program.account.pool.fetch(poolPda)
    expectBn(asBn(pool1.jtoReserve), jtoReserve.add(quoted.curveIn))
    expectBn(asBn(pool1.jtxReserve), jtxReserve.sub(quoted.jtxOut))
    expectBn(await tokenAmount(poolJto), poolJtoBefore.add(quoted.curveIn))
    expectBn(await tokenAmount(poolJtx), poolJtxBefore.sub(quoted.jtxOut))
    expectBn(await tokenAmount(rewardJto), rewardBefore.add(quoted.fee))
    expectBn(await tokenAmount(traderJtx), traderJtxBefore.add(quoted.jtxOut))
    expectBn(await tokenAmount(traderJto), traderJtoBefore.sub(JTO_IN))
    expectBn(await mintSupply(jtxMint), supplyBefore)

    const expectedAcc = asBn(pool0.accJtoPerWeight).add(
      asBn(pool0.unindexedJto).add(quoted.fee).mul(INDEX_SCALE).div(asBn(pool0.totalWeight))
    )
    const accAfter = asBn(pool1.accJtoPerWeight)
    expect(accAfter.gt(asBn(pool0.accJtoPerWeight))).to.equal(true)
    expectBn(accAfter, expectedAcc)
  })

  it("airdrop pays pending JTO without moving the index", async () => {
    const poolBefore = await program.account.pool.fetch(poolPda)
    const posBefore = await program.account.position.fetch(positionPda)
    const accBefore = asBn(poolBefore.accJtoPerWeight)
    const accrued = asBn(posBefore.weight).mul(accBefore).div(INDEX_SCALE)
    const expectedPay = accrued.sub(asBn(posBefore.rewardDebt)).add(asBn(posBefore.pendingJto))
    expect(expectedPay.gtn(0)).to.equal(true)

    const stakerBefore = await tokenAmount(stakerJto)
    const rewardBefore = await tokenAmount(rewardJto)

    await program.methods
      .airdrop()
      .accounts({
        config: configPda,
        pool: poolPda,
        jtoMint,
        rewardJto,
        jtoTokenProgram: TOKEN_PROGRAM_ID,
      })
      .remainingAccounts([
        { pubkey: positionPda, isWritable: true, isSigner: false },
        { pubkey: stakerJto, isWritable: true, isSigner: false },
      ])
      .rpc()

    const poolAfter = await program.account.pool.fetch(poolPda)
    const posAfter = await program.account.position.fetch(positionPda)
    expectBn(asBn(poolAfter.accJtoPerWeight), accBefore)
    expectBn(asBn(posAfter.pendingJto), new BN(0))
    expectBn(await tokenAmount(stakerJto), stakerBefore.add(expectedPay))
    expectBn(await tokenAmount(rewardJto), rewardBefore.sub(expectedPay))
  })

  it("sell_jtx sends 1% of gross JTO to reward_jto", async () => {
    const pool0 = await program.account.pool.fetch(poolPda)
    const jtxIn = await tokenAmount(traderJtx)
    const quoted = quoteSell(asBn(pool0.jtxReserve), asBn(pool0.jtoReserve), jtxIn)
    expect(quoted.jtoGross.gtn(0)).to.equal(true)
    expect(quoted.fee.gtn(0)).to.equal(true)
    expectBn(quoted.fee, quoted.jtoGross.muln(FEE_BPS).divn(BPS))
    expectBn(quoted.jtoToUser, quoted.jtoGross.sub(quoted.fee))

    const rewardBefore = await tokenAmount(rewardJto)
    const userJtoBefore = await tokenAmount(traderJto)
    const poolJtxBefore = await tokenAmount(poolJtx)

    await program.methods
      .sellJtx(jtxIn, quoted.jtoToUser)
      .accounts(swapAccounts())
      .signers([trader])
      .rpc()

    const pool1 = await program.account.pool.fetch(poolPda)
    expectBn(asBn(pool1.jtxReserve), asBn(pool0.jtxReserve).add(jtxIn))
    expectBn(asBn(pool1.jtoReserve), asBn(pool0.jtoReserve).sub(quoted.jtoGross))
    expectBn(await tokenAmount(poolJtx), poolJtxBefore.add(jtxIn))
    expectBn(await tokenAmount(traderJtx), new BN(0))
    expectBn(await tokenAmount(traderJto), userJtoBefore.add(quoted.jtoToUser))
    expectBn(await tokenAmount(rewardJto), rewardBefore.add(quoted.fee))
  })

  it("unstake freezes, cancels, then releases on the next epoch", async () => {
    const clock = await context.banksClient.getClock()
    await program.methods
      .requestUnstake()
      .accounts(stakeMoveAccounts())
      .signers([staker])
      .rpc()

    const pending = await program.account.position.fetch(positionPda)
    expectBn(asBn(pending.weight), new BN(0))
    expect(Number(pending.frozenBoostBps)).to.be.greaterThan(0)
    expectBn(asBn(pending.unlockEpoch), new BN(clock.epoch.toString()).addn(1))
    expectBn(asBn(pending.amount), STAKE_FULL)

    await expectError(
      () =>
        program.methods
          .stake(UNIT)
          .accounts(stakeAccounts())
          .signers([staker])
          .rpc(),
      "UnstakePending"
    )
    await expectError(
      () =>
        program.methods
          .releaseStake()
          .accounts(stakeMoveAccounts())
          .signers([staker])
          .rpc(),
      "EpochNotReached"
    )

    await program.methods
      .cancelUnstake()
      .accounts(stakeMoveAccounts())
      .signers([staker])
      .rpc()

    const restored = await program.account.position.fetch(positionPda)
    expectBn(asBn(restored.weight), WEIGHT_AT_100)
    expectBn(asBn(restored.unlockEpoch), new BN(0))
    expect(Number(restored.frozenBoostBps)).to.equal(0)
    expectBn(asBn(restored.amount), STAKE_FULL)

    await program.methods
      .requestUnstake()
      .accounts(stakeMoveAccounts())
      .signers([staker])
      .rpc()
    const locked = await program.account.position.fetch(positionPda)
    const stakerBefore = await tokenAmount(stakerJtx)
    const vaultBefore = await tokenAmount(stakeJtx)

    const now = await context.banksClient.getClock()
    context.setClock(
      new Clock(
        now.slot,
        now.epochStartTimestamp,
        BigInt(locked.unlockEpoch.toString()),
        now.leaderScheduleEpoch,
        now.unixTimestamp
      )
    )

    await program.methods
      .releaseStake()
      .accounts(stakeMoveAccounts())
      .signers([staker])
      .rpc()

    const released = await program.account.position.fetch(positionPda)
    expectBn(asBn(released.amount), new BN(0))
    expectBn(await tokenAmount(stakerJtx), stakerBefore.add(STAKE_FULL))
    expectBn(await tokenAmount(stakeJtx), vaultBefore.sub(STAKE_FULL))
  })

  it("admin_mint rejects a mint the config PDA does not control", async () => {
    await expectError(
      () =>
        program.methods
          .adminMint(new BN(1))
          .accounts({
            admin: admin.publicKey,
            config: configPda,
            jtxMint,
            destination: adminJtx,
            jtxTokenProgram: TOKEN_2022_PROGRAM_ID,
          })
          .rpc(),
      "MintAuthority"
    )
  })

  it("set_tiers rejects a non-increasing min_amount", async () => {
    const flat = UNIT.muln(100)
    await expectError(
      () =>
        program.methods
          .setTiers(
            [
              { minAmount: flat, rateBps: 180 },
              { minAmount: flat, rateBps: 320 },
              { minAmount: UNIT.muln(8_000), rateBps: 460 },
              { minAmount: UNIT.muln(40_000), rateBps: 550 },
            ],
            WEIGHT_CAP,
            new BN(RAMP_SECONDS),
            BOOST_MAX
          )
          .accounts({
            admin: admin.publicKey,
            config: configPda,
          })
          .rpc(),
      "BadTiers"
    )

    const cfg = await program.account.config.fetch(configPda)
    expectBn(asBn(cfg.tiers[1].minAmount), UNIT.muln(1_000))
    expect(Number(cfg.tiers[1].rateBps)).to.equal(320)
  })
})
