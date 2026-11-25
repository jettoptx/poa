use crate::state::{
    BPS_DENOMINATOR, BOOST_START_BPS, FEE_BPS, INDEX_SCALE, JTX_UNIT, Tier,
};

#[derive(Debug, PartialEq, Eq)]
pub enum MathError {
    Overflow,
    Zero,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BuyQuote {
    pub jtx_out: u64,
    pub fee: u64,
    pub curve_in: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SellQuote {
    pub jto_to_user: u64,
    pub fee: u64,
    pub jto_gross: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct IndexUpdate {
    pub acc: u128,
    pub unindexed: u64,
}

pub fn quote_buy(jtx_reserve: u64, jto_reserve: u64, jto_in: u64) -> Result<BuyQuote, MathError> {
    if jtx_reserve == 0 || jto_reserve == 0 || jto_in == 0 {
        return Err(MathError::Zero);
    }
    let fee = jto_in
        .checked_mul(FEE_BPS)
        .ok_or(MathError::Overflow)?
        / BPS_DENOMINATOR;
    let curve_in = jto_in.checked_sub(fee).ok_or(MathError::Overflow)?;
    if curve_in == 0 {
        return Err(MathError::Zero);
    }
    let jtx_out_u = (jtx_reserve as u128)
        .checked_mul(curve_in as u128)
        .ok_or(MathError::Overflow)?
        .checked_div(
            (jto_reserve as u128)
                .checked_add(curve_in as u128)
                .ok_or(MathError::Overflow)?,
        )
        .ok_or(MathError::Overflow)?;
    if jtx_out_u == 0 || jtx_out_u >= jtx_reserve as u128 {
        return Err(MathError::Zero);
    }
    Ok(BuyQuote {
        jtx_out: jtx_out_u as u64,
        fee,
        curve_in,
    })
}

pub fn quote_sell(jtx_reserve: u64, jto_reserve: u64, jtx_in: u64) -> Result<SellQuote, MathError> {
    if jtx_reserve == 0 || jto_reserve == 0 || jtx_in == 0 {
        return Err(MathError::Zero);
    }
    let jto_gross_u = (jto_reserve as u128)
        .checked_mul(jtx_in as u128)
        .ok_or(MathError::Overflow)?
        .checked_div(
            (jtx_reserve as u128)
                .checked_add(jtx_in as u128)
                .ok_or(MathError::Overflow)?,
        )
        .ok_or(MathError::Overflow)?;
    if jto_gross_u == 0 || jto_gross_u > u64::MAX as u128 {
        return Err(MathError::Zero);
    }
    let jto_gross = jto_gross_u as u64;
    let fee = jto_gross
        .checked_mul(FEE_BPS)
        .ok_or(MathError::Overflow)?
        / BPS_DENOMINATOR;
    let jto_to_user = jto_gross.checked_sub(fee).ok_or(MathError::Overflow)?;
    if jto_to_user == 0 {
        return Err(MathError::Zero);
    }
    Ok(SellQuote {
        jto_to_user,
        fee,
        jto_gross,
    })
}

pub fn index_fee(
    acc: u128,
    total_weight: u128,
    unindexed: u64,
    fee: u64,
) -> Result<IndexUpdate, MathError> {
    let pot = unindexed.checked_add(fee).ok_or(MathError::Overflow)?;
    if total_weight == 0 {
        return Ok(IndexUpdate {
            acc,
            unindexed: pot,
        });
    }
    let pot_u = pot as u128;
    let add = pot_u
        .checked_mul(INDEX_SCALE)
        .ok_or(MathError::Overflow)?
        .checked_div(total_weight)
        .ok_or(MathError::Overflow)?;
    let distributed = add
        .checked_mul(total_weight)
        .ok_or(MathError::Overflow)?
        / INDEX_SCALE;
    let remainder = pot_u.checked_sub(distributed).ok_or(MathError::Overflow)?;
    if remainder > u64::MAX as u128 {
        return Err(MathError::Overflow);
    }
    Ok(IndexUpdate {
        acc: acc.checked_add(add).ok_or(MathError::Overflow)?,
        unindexed: remainder as u64,
    })
}

pub fn tier_rate(tiers: &[Tier], amount: u64) -> u16 {
    let mut rate = 0u16;
    for tier in tiers {
        if amount >= tier.min_amount {
            rate = tier.rate_bps;
        }
    }
    rate
}

pub fn boost_bps(
    now: i64,
    staked_at: i64,
    ramp_seconds: i64,
    max_bps: u16,
    frozen: u16,
) -> Result<u16, MathError> {
    if frozen != 0 {
        return Ok(frozen);
    }
    if ramp_seconds <= 0 || max_bps < BOOST_START_BPS {
        return Err(MathError::Zero);
    }
    let elapsed = now.saturating_sub(staked_at).max(0) as u128;
    let ramp = ramp_seconds as u128;
    if elapsed >= ramp {
        return Ok(max_bps);
    }
    let extra = (max_bps as u128 - BOOST_START_BPS as u128)
        .checked_mul(elapsed)
        .ok_or(MathError::Overflow)?
        / ramp;
    Ok((BOOST_START_BPS as u128 + extra) as u16)
}

pub fn effective_weight(amount: u64, rate_bps: u16, boost: u16, cap: u64) -> Result<u64, MathError> {
    if amount == 0 || rate_bps == 0 || boost == 0 {
        return Ok(0);
    }
    let base = (amount as u128)
        .checked_mul(rate_bps as u128)
        .ok_or(MathError::Overflow)?
        / BPS_DENOMINATOR as u128;
    let weighted = base
        .checked_mul(boost as u128)
        .ok_or(MathError::Overflow)?
        / BOOST_START_BPS as u128;
    if weighted > u64::MAX as u128 {
        return Err(MathError::Overflow);
    }
    Ok((weighted as u64).min(cap))
}

pub fn can_release(now_epoch: u64, unlock_epoch: u64) -> bool {
    unlock_epoch != 0 && now_epoch >= unlock_epoch
}

pub fn default_test_tiers() -> [Tier; 4] {
    [
        Tier { min_amount: 100 * JTX_UNIT, rate_bps: 180 },
        Tier { min_amount: 1_000 * JTX_UNIT, rate_bps: 320 },
        Tier { min_amount: 8_000 * JTX_UNIT, rate_bps: 460 },
        Tier { min_amount: 40_000 * JTX_UNIT, rate_bps: 550 },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buy_fee_and_curve_in() {
        let q = quote_buy(1_000, 1_000, 100).unwrap();
        assert_eq!(q.fee, 1);
        assert_eq!(q.curve_in, 99);
        assert_eq!(q.jtx_out, 1_000 * 99 / 1_099);
    }

    #[test]
    fn sell_fee_is_one_percent_of_gross() {
        let q = quote_sell(10_000, 10_000, 10_000).unwrap();
        assert_eq!(q.jto_gross, 5_000);
        assert_eq!(q.fee, q.jto_gross * FEE_BPS / BPS_DENOMINATOR);
        assert_eq!(q.fee, 50);
        assert_eq!(q.jto_to_user, 4_950);
    }

    #[test]
    fn unindexed_when_nobody_staked() {
        let u = index_fee(7, 0, 3, 4).unwrap();
        assert_eq!(u.acc, 7);
        assert_eq!(u.unindexed, 7);
    }

    #[test]
    fn equal_weights_split_fee() {
        let u = index_fee(0, 100, 0, 100).unwrap();
        let share = 50u128 * u.acc / INDEX_SCALE;
        assert_eq!(share, 50);
        assert_eq!(share * 2, 100);
    }

    #[test]
    fn whale_cap_bounds_share() {
        let tiers = default_test_tiers();
        let cap = 5_000 * JTX_UNIT;
        let amount = 1_000_000 * JTX_UNIT;
        let rate = tier_rate(&tiers, amount);
        let w = effective_weight(amount, rate, 20_000, cap).unwrap();
        assert_eq!(w, cap);
        let total = w as u128 + cap as u128;
        let fee = 1_000u64;
        let u = index_fee(0, total, 0, fee).unwrap();
        let whale_share = (w as u128) * u.acc / INDEX_SCALE;
        assert!(whale_share <= fee as u128 / 2 + 1);
    }

    #[test]
    fn tier_curve_and_boost() {
        let tiers = default_test_tiers();
        let cap = 5_000 * JTX_UNIT;
        assert_eq!(tier_rate(&tiers, 50 * JTX_UNIT), 0);
        assert_eq!(effective_weight(50 * JTX_UNIT, 0, 10_000, cap).unwrap(), 0);

        let crab = 100 * JTX_UNIT;
        assert_eq!(tier_rate(&tiers, crab), 180);
        assert_eq!(
            effective_weight(crab, 180, 10_000, cap).unwrap(),
            crab * 180 / 10_000
        );

        let fish = 1_000 * JTX_UNIT;
        assert_eq!(tier_rate(&tiers, fish), 320);
        assert_ne!(tier_rate(&tiers, fish), 180);

        let whale = 40_000 * JTX_UNIT;
        assert_eq!(tier_rate(&tiers, whale), 550);
        assert_eq!(
            effective_weight(whale, 550, 10_000, cap).unwrap(),
            whale * 550 / 10_000
        );

        let ramp = 90 * 24 * 60 * 60;
        assert_eq!(boost_bps(1_000, 1_000, ramp, 20_000, 0).unwrap(), 10_000);
        assert_eq!(
            boost_bps(1_000 + ramp / 2, 1_000, ramp, 20_000, 0).unwrap(),
            15_000
        );
        assert_eq!(boost_bps(1_000 + ramp, 1_000, ramp, 20_000, 0).unwrap(), 20_000);
        assert_eq!(
            boost_bps(1_000 + 200 * 24 * 60 * 60, 1_000, ramp, 20_000, 0).unwrap(),
            20_000
        );
        let boosted = effective_weight(whale, 550, 20_000, cap).unwrap();
        assert!(boosted <= cap);
    }

    #[test]
    fn release_epoch() {
        assert!(!can_release(4, 0));
        assert!(!can_release(4, 5));
        assert!(can_release(5, 5));
        assert!(can_release(6, 5));
    }
}
