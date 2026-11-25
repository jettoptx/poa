use anchor_lang::prelude::*;

#[error_code]
pub enum FlywheelError {
    #[msg("Arithmetic overflow or zero output")]
    Math,
    #[msg("Slippage limit")]
    Slippage,
    #[msg("Unstake is pending until the next epoch")]
    UnstakePending,
    #[msg("Release epoch has not arrived")]
    EpochNotReached,
    #[msg("Admin signature required")]
    NotAdmin,
    #[msg("Mint would pass the 1 billion JTX cap")]
    SupplyCap,
    #[msg("Tier bounds are not strictly increasing")]
    BadTiers,
    #[msg("Pool is paused")]
    Paused,
    #[msg("Amount is zero")]
    ZeroAmount,
    #[msg("Pool has no reserves")]
    EmptyPool,
    #[msg("Liquidity is already seeded")]
    AlreadySeeded,
    #[msg("Config PDA is not the JTX mint authority")]
    MintAuthority,
    #[msg("Wrong token program")]
    BadTokenProgram,
    #[msg("Token account owner or mint does not match")]
    BadOwner,
    #[msg("Airdrop batch is empty or larger than 8")]
    BadBatch,
    #[msg("Nothing is staked")]
    NothingStaked,
    #[msg("Vault PDA is not an empty system account")]
    VaultExists,
}
