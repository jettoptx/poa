# poa

**Canonical Solana programs for Jett Optics Proof-of-Attention (PoA) and the community vault.** The runtime model is Grok.

On-chain source of record for:

1. **PoA Trust** — gaze attestation handshakes, compute proofs, OPTX entropy accounting
2. **Jett Vault** — community vault, AGT attestations, stake positions, AARON audit stamps

Stack: **Anchor 0.30.1** · **Solana mainnet-beta** (+ devnet test IDs) · **Apache-2.0**

| | |
|--|--|
| Site | [jettoptics.si](https://jettoptics.si) |
| Docs | [docs.jettoptics.si](https://docs.jettoptics.si/docs) · [jettoptx/docs](https://github.com/jettoptx/docs) |
| Product | [dojo.jettoptics.si](https://dojo.jettoptics.si) |
| Buy $JTX | [space.jettoptics.si/buy](https://space.jettoptics.si/buy) |
| Token notes | [docs/TOKENOMICS.md](docs/TOKENOMICS.md) |

---

## $JTX and $OPTX

**$JTX v2** is the utility token. **$OPTX** is the public attention token.

Mojo, DOJO, and Space Cowboy are not mint tiers. They do not set an $OPTX allowance, and they are not an emission schedule.

PoA Trust still records entropy and an on-chain mint allowance. That allowance is program accounting. It is not a hold-tier table.

Notes: [docs/TOKENOMICS.md](docs/TOKENOMICS.md)

---

## Programs (live)

### Mainnet

| Role | On-disk crate / module* | Program ID | Explorer |
|------|-------------------------|------------|----------|
| **PoA Trust** | `programs/jtx-optx-devnet-poa-trustjoe` → `jtx_optx_devnet_poa_trustjoe` | `85sqs4upQiPrvk1NMuyfHVQoW1EGdgk8m2cQb7uMxXTF` | [view](https://explorer.solana.com/address/85sqs4upQiPrvk1NMuyfHVQoW1EGdgk8m2cQb7uMxXTF) |
| **Jett Vault** | `programs/jett-vault` → `jett_vault` | `JTX5uXTiZ1M3hJkjv5Cp5F8dr3Jc7nhJbQjCFmgEYA7` | [view](https://explorer.solana.com/address/JTX5uXTiZ1M3hJkjv5Cp5F8dr3Jc7nhJbQjCFmgEYA7) |

\*Renamed from historical `jtx_cstb_trust`. **Product scope is PoA / OPTX trust — not CompuStable / $CSTB.** See [Out of product scope](#out-of-product-scope).

### Devnet (`--features devnet`)

| Role | Program ID |
|------|------------|
| PoA Trust | `79nQsecDspUWxvAMyJvK36EUty4yEoP5ssLvHZuNiugF` |
| Jett Vault | `ExjFkkX3Zogyb5uBeaff8FdKZbjjVHcCHgcaQiP3qQDS` |

Legacy program `91SqPNGRFrTgwSM3S7grZK8A6TCqn5STFGK4mAfqWMbQ` is **deprecated** (superseded by mainnet PoA Trust).

---

## Upgrade authority (audit-critical)

Both mainnet programs use **NEW_JOE** (`GtAkS5tY…` / SNS `astro.knots.sol`) as BPF upgrade authority as of **2026-07-27**. Programs remain upgradeable (not immutable). Prior authority was the Squads treasury vault `9Wss…`.

| Field | Value |
|-------|--------|
| **Upgrade authority (live)** | `GtAkS5tYaqi6XQrinuFyqKQkK29SFQsUY9gQ2XpLXLwq` |
| **SNS** | `astro.knots.sol` (NEW_JOE ops) |
| **Previous authority** | `9WssADzftzptNnMHLzPZYAFApUfE7qLYChicH1Wh6YD7` (Squads vault / treasury) |
| **Control path** | Squads v4 multisig `97e8mY66…` executed the transfer (tx index **63**) |
| **Immutable?** | No — upgradeable; authority is a governed ops key |
| **Assure handover** | [`docs/HANDOVER-ASSURE-2026-07-27.md`](docs/HANDOVER-ASSURE-2026-07-27.md) |

**Verify anytime:**

```bash
solana program show 85sqs4upQiPrvk1NMuyfHVQoW1EGdgk8m2cQb7uMxXTF --url mainnet-beta
# Authority: GtAkS5tYaqi6XQrinuFyqKQkK29SFQsUY9gQ2XpLXLwq

solana program show JTX5uXTiZ1M3hJkjv5Cp5F8dr3Jc7nhJbQjCFmgEYA7 --url mainnet-beta
# Authority: GtAkS5tYaqi6XQrinuFyqKQkK29SFQsUY9gQ2XpLXLwq
```

---

## Out of product scope

Be explicit with auditors and integrators:

| Topic | Status |
|-------|--------|
| **CompuStable / $CSTB** | **Not a product dependency.** Account field `cstb_mint` is a **legacy layout slot** written at `initialize`; it does not gate minting or prove an external PoW token. |
| **`submit_compute_proof`** | Stores a **caller-supplied opaque hash** + difficulty floor. It does **not** verify an external CompuStable network. |
| **Off-chain services** | Edge verification, web and mobile clients, and marketing sites sit outside these programs unless an engagement scopes them in. |
| **EVM Tempo wallet** | Settlement convenience only; not part of these Solana programs. |

---

## What these programs do

### PoA Trust (`jtx_optx_devnet_poa_trustjoe` module)

Gaze-centric attestation pipeline on Solana:

1. `initiate_handshake` — open time-bounded handshake  
2. `submit_gaze_attestation` — AGT tensor vectors + gaze proof hash (opaque; raw biometrics stay client-side)  
3. `submit_compute_proof` — opaque compute hash + difficulty  
4. `finalize_attestation` — permanent record + entropy / OPTX allowance accounting  
5. `mint_optx` — mint against accumulated allowance (when mint/config is live for that environment)

Also: config updates, pause, revoke, handshake close, attestation verify.

### Jett Vault (`jett_vault`)

Community vault + AGT surface:

- SOL / agent USDC contributions  
- AGT attestation create / weight update  
- Optional CPI link into PoA Trust  
- AARON operator audit hash (immutable once set)  
- Stake / subscription surfaces (prefer stake paths; some legacy ix may be fail-closed)  
- Refunds, pause (multisig-gated), donor NFT mint (mpl-core)

---

## Tokens (context)

| Token | Mint | Network | Notes |
|-------|------|---------|--------|
| **$JTX v2** (canonical) | `JTXGnx83s2QZ2MwYkRD1cBKrqQKSdG5oe8vSYW5Zjoe` | Mainnet | Token-2022, 9 decimals — **use this** |
| $JTX v1 (legacy) | `9XpJiKEYzq5yDo5pJzRfjSRMPL2yPfDQXgiN7uYtBhUj` | Mainnet | Mint authority **revoked** |
| $OPTX (devnet reward mint) | `DSyauRAZwUd2BrTk3P8k2yUxxvcx5X4BBg3Gh3VbeRG3` | Devnet | Gaze/attestation rewards in test environments |
| $CSTB | — | — | **Not product.** Ignore for audits and client integrations. |

**$JTX mint authority:** revoked (fixed supply).  
**Meteora DLMM (liquidity):** `54ecLhTa8HZg1bhcDNWiddd8p7UN7jq4HLWeLr1sRHMz`  
**Buy:** [space.jettoptics.si/buy](https://space.jettoptics.si/buy)

---

## System context (off-chain)

```text
Dojo / web  →  JETT Auth (AGT client)  →  edge verification
                                              │
                    opaque proofs  ───────────┤
                                              ▼
                                    Solana mainnet
                              PoA Trust  +  Jett Vault
```

| Name | Role |
|------|------|
| **JETT Auth** | Client-side gaze / biometric proof hashing |
| **AARON** | Edge router: verification, payments, DePIN APIs |
| **AGT** | COG / EMO / ENV tensor representation of attention |
| **JOE** | Operator agent (Hermes / Grok) — not an on-chain program |
| **OPTX** | Token + protocol network branding |

Biometric **raw data never goes on-chain** — only 32-byte opaque hashes and fixed-point tensor fields.

---

## Development

### Prerequisites

- Rust 1.75+  
- Solana CLI 1.18+  
- Anchor 0.30.1+  
- Node.js 18+  

### Build

```bash
yarn install

# Mainnet-shaped IDs (default declare_id!)
anchor build -p jtx-optx-devnet-poa-trustjoe
anchor build -p jett-vault

# Devnet IDs
anchor build -p jett-vault -- --features devnet
anchor build -p jtx-optx-devnet-poa-trustjoe -- --features devnet   # if feature-gated in crate
```

### Test

```bash
anchor test
```

### Deploy / upgrade

```bash
# Devnet only without Squads
solana config set --url devnet
anchor deploy --provider.cluster devnet

# Mainnet: Squads-only process for upgrades
# Do not use a personal keypair as upgrade authority.
```

---

## SDK

| Module | Role |
|--------|------|
| `sdk/jett-sdk.ts` | AGT / biometric proof / AARON risk helpers |
| `sdk/trust-client.ts` | Anchor client for PoA Trust program |
| `sdk/vault-client.ts` | Anchor client for Jett Vault |

```typescript
import { BiometricProof, AgtTensor, AaronRisk } from './sdk/jett-sdk'

const tensor = AgtTensor.fromGaze(cogValue, envValue, emoValue)
const proof = BiometricProof.generate(tensor, entropy)
const proofHash = proof.hash() // 32-byte opaque hash only on-chain
```

---

## Security posture

### Engineering controls (in program design)

- Checked arithmetic / overflow guards  
- Double-mint and double-finalize guards  
- Handshake claim / replay flags  
- Pause switches  
- Opaque biometric hashes only  
- AARON audit hash write-once behavior  
- Vault multisig roster for sensitive vault ops  
- No private keys in this repository  

### Governance controls (mainnet)

- **BPF upgrade authority = NEW_JOE `GtAkS5tY…` (`astro.knots.sol`)**  
- Prior Squads vault authority (`9Wss…`) recorded; retired single-key ops key (`EFvg…`)  

### Audit trail (internal / pre-engagement)

| Version | Date | Notes |
|---------|------|--------|
| v2.0.0 | 2026-01-30 | Internal security pass |
| v2.1.x | 2026-02 → 05 | Pre-mainnet + mainnet deploy review |
| v2.2.0 | 2026-07-26 | Upgrade authority → Squads treasury (PoA) |
| v2.3.0 | 2026-07-27 | Upgrade authority → NEW_JOE `GtAk…` (PoA + Vault; Assure freeze) |

For third-party engagements (e.g. Assure DeFi): freeze a **git tag + commit SHA**, publish IDL / verifiable builds, and attach this README + program sources. Audit brief: [`docs/AUDIT.md`](docs/AUDIT.md).

---

## Mainnet checklist

- [x] PoA Trust deployed (`85sqs…xXTF`)  
- [x] Jett Vault deployed (`JTX5…EYA7`)  
- [x] $JTX v2 live; mint authority revoked  
- [x] Meteora pool seeded / LP locked (see trading runbooks)  
- [x] Treasury / Squads vault `9Wss…` as operational treasury  
- [x] **Upgrade authority → NEW_JOE `GtAk…` for both programs** (2026-07-27)  
- [x] TypeScript SDK present  
- [ ] External audit engagement (Assure DeFi or equivalent) — freeze tag  
- [ ] Verifiable build artifacts published for freeze SHA  
- [ ] First mainnet gaze attestation + OPTX path (product)  

---

## Repository layout

```text
programs/jtx-optx-devnet-poa-trustjoe/   # PoA Trust (legacy crate name)
programs/jett-vault/       # Community vault + AGT
crates/shared-constants/   # Shared IDs / constants
sdk/                       # TypeScript clients
tests/                     # Anchor / bankrun tests
scripts/                   # Deploy / init helpers
docs/                      # Design + handoff notes + TOKENOMICS.md
```

---

## Links

| Resource | URL |
|----------|-----|
| Jett Optics | https://jettoptics.si |
| Buy $JTX | https://space.jettoptics.si/buy |
| Docs repo | https://github.com/jettoptx/docs |
| Developer docs | https://docs.jettoptics.si/docs |
| Dojo | https://dojo.jettoptics.si |
| $JTX v2 Solscan | https://solscan.io/token/JTXGnx83s2QZ2MwYkRD1cBKrqQKSdG5oe8vSYW5Zjoe |
| Meteora pool | https://app.meteora.ag/dlmm/54ecLhTa8HZg1bhcDNWiddd8p7UN7jq4HLWeLr1sRHMz |
| PoA program | https://explorer.solana.com/address/85sqs4upQiPrvk1NMuyfHVQoW1EGdgk8m2cQb7uMxXTF |
| Vault program | https://explorer.solana.com/address/JTX5uXTiZ1M3hJkjv5Cp5F8dr3Jc7nhJbQjCFmgEYA7 |
| **Tokenomics** | [docs/TOKENOMICS.md](docs/TOKENOMICS.md) |

---

## License

Apache-2.0 — see [LICENSE](LICENSE).

## Contact

**Joshua Martinez** — [joe@jettoptics.ai](mailto:joe@jettoptics.ai)  
**X:** [@jettoptx](https://x.com/jettoptx)

---

2026-11-23

-JOE origin ready

