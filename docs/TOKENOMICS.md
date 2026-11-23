# $JTX and $OPTX

**Status:** Current public note (2026-11-23)
**Programs:** PoA Trust + Jett Vault in this repository

---

## Tokens

**$JTX v2** is the utility token. Mint `JTXGnx83s2QZ2MwYkRD1cBKrqQKSdG5oe8vSYW5Zjoe` on mainnet. Mint authority is revoked.

**$OPTX** is the public attention token. It is separate from $JTX.

Buy $JTX at [space.jettoptics.si/buy](https://space.jettoptics.si/buy).

---

## Retired hold tiers

Mojo, DOJO, and Space Cowboy are not mint tiers.

These claims are retired:

- Basic / Mojo / DOJO / Space Cowboy as an $OPTX emission ladder
- Hold thresholds of 12, 444, and 1,111 $JTX minting different $OPTX base rates
- A "Potential only" basic tier versus a paid real-mint tier

Do not publish those rates as the current model.

---

## What the programs still do

PoA Trust can record a successful attestation and add to an on-chain mint allowance. `mint_optx` can deduct that allowance. That is program accounting. It is not a product tier table, and it does not by itself define who receives $OPTX.

Jett Vault keeps community-vault, attestation, and stake-position instructions. Stake positions are not the retired Mojo / DOJO / Space Cowboy schedule.

---

*Last updated: 2026-11-23*
