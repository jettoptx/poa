use anchor_lang::prelude::*;

pub const FEE_BPS: u64 = 100;
pub const BPS_DENOMINATOR: u64 = 10_000;
pub const INDEX_SCALE: u128 = 1_000_000_000_000;
pub const MAX_SUPPLY: u64 = 1_000_000_000 * 1_000_000_000;
pub const MAX_AIRDROP_BATCH: usize = 8;
pub const JTX_UNIT: u64 = 1_000_000_000;
pub const BOOST_START_BPS: u16 = 10_000;
pub const DAY_SECONDS: i64 = 24 * 60 * 60;

/// Packed inside Config. Not an account and not a PDA.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace, Default)]
pub struct Tier {
    pub min_amount: u64,
    pub rate_bps: u16,
}

pub fn default_tiers() -> [Tier; 4] {
    [
        Tier { min_amount: 100 * JTX_UNIT, rate_bps: 180 },
        Tier { min_amount: 1_000 * JTX_UNIT, rate_bps: 320 },
        Tier { min_amount: 8_000 * JTX_UNIT, rate_bps: 460 },
        Tier { min_amount: 40_000 * JTX_UNIT, rate_bps: 550 },
    ]
}

#[account]
#[derive(InitSpace)]
pub struct Config {
    pub admin: Pubkey,
    pub jtx_mint: Pubkey,
    pub jto_mint: Pubkey,
    pub bump: u8,
    pub paused: bool,
    pub tiers: [Tier; 4],
    pub weight_cap: u64,
    pub boost_ramp_seconds: i64,
    pub boost_max_bps: u16,
}

#[account]
#[derive(InitSpace)]
pub struct Pool {
    pub jtx_reserve: u64,
    pub jto_reserve: u64,
    pub acc_jto_per_weight: u128,
    pub total_weight: u128,
    pub unindexed_jto: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace, Default)]
pub struct Position {
    pub staker: Pubkey,
    pub amount: u64,
    pub weight: u64,
    pub reward_debt: u128,
    pub pending_jto: u64,
    pub staked_at: i64,
    pub frozen_boost_bps: u16,
    pub unlock_epoch: u64,
    pub bump: u8,
}
