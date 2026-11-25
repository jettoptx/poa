# jtx-flywheel

JTX/JTO pool and stake index. Localnet program id `Ewz1XZQWGJWUXZtBdXz5DR8sYHZkzKD8LGNGNc5uMWyJ`. Devnet feature id `7VwcGhwGygXjNyZVPpuL1mXmSTs9jZXnKDCxy2eMhA9C`.

Every buy and sell through this program pays 100 bps. The whole fee is JTO and is indexed on-chain to staked weight. Tier rates and the 90-day boost only change the share of that fee. They do not mint a second reward.

Seeds: `config`, `pool`, `pool_jtx`, `pool_jto`, `stake_jtx`, `reward_jto`, `position` + staker.

`Tier` is four inline values on the config account, not a PDA.

Mainnet mint authority has not moved. `admin_mint` works only when the JTX mint authority is the config PDA. Do not point this at the live mint until a Squads proposal says so.

NFTs are not in this program. Collection `FP1aYKSEBdcehTHjxUYjbvRCCnNub7bzAq3QWN7mDgYw` stays where it is. There is no NFT price.

`cargo test -p jtx-flywheel` covers the quote, tier, boost, and index math. `tests/jtx-flywheel.ts` is the bankrun suite. It needs `target/deploy/jtx_flywheel.so` and `target/types/jtx_flywheel.ts` from an Anchor build. Do not deploy this program until that suite has been run.

On this Windows host, Solana 1.18 platform-tools cargo 1.75 cannot compile the workspace lock. `blake3 1.8.3` is edition 2024. `cargo build-sbf` also needs `RUSTC_BOOTSTRAP=1` and `-Znext-lockfile-bump` because the lockfile is version 4. Workspace members are listed explicitly so that cargo does not leave `programs/*` unexpanded under the `\\?\` path prefix.
