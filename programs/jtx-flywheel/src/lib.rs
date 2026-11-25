use anchor_lang::prelude::*;
use anchor_lang::solana_program::program_option::COption;
use anchor_lang::solana_program::program_pack::Pack;
use anchor_spl::{
    token::spl_token,
    token_2022::spl_token_2022,
    token_interface::{self, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked},
};

pub mod error;
pub mod math;
pub mod state;

use error::FlywheelError;
use math::{boost_bps, can_release, effective_weight, index_fee, quote_buy, quote_sell, tier_rate};
use state::*;

#[cfg(not(feature = "devnet"))]
declare_id!("Ewz1XZQWGJWUXZtBdXz5DR8sYHZkzKD8LGNGNc5uMWyJ");
#[cfg(feature = "devnet")]
declare_id!("7VwcGhwGygXjNyZVPpuL1mXmSTs9jZXnKDCxy2eMhA9C");

#[program]
pub mod jtx_flywheel {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.jtx_mint = ctx.accounts.jtx_mint.key();
        config.jto_mint = ctx.accounts.jto_mint.key();
        config.bump = ctx.bumps.config;
        config.paused = false;
        config.tiers = default_tiers();
        config.weight_cap = 5_000 * JTX_UNIT;
        config.boost_ramp_seconds = 90 * DAY_SECONDS;
        config.boost_max_bps = 20_000;
        require!(ctx.accounts.jtx_mint.decimals == 9, FlywheelError::BadTokenProgram);
        require!(ctx.accounts.jto_mint.decimals == 9, FlywheelError::BadTokenProgram);
        ctx.accounts.pool.bump = ctx.bumps.pool;
        // Vault creates run outside try_accounts. Four `init` constraints put
        // Initialize over the 4 KB SBF stack.
        init_token_vault(
            &ctx.accounts.payer,
            &ctx.accounts.pool_jtx,
            &ctx.accounts.jtx_mint,
            &ctx.accounts.config,
            &ctx.accounts.jtx_token_program,
            &ctx.accounts.system_program,
            b"pool_jtx",
            ctx.bumps.pool_jtx,
        )?;
        init_token_vault(
            &ctx.accounts.payer,
            &ctx.accounts.pool_jto,
            &ctx.accounts.jto_mint,
            &ctx.accounts.config,
            &ctx.accounts.jto_token_program,
            &ctx.accounts.system_program,
            b"pool_jto",
            ctx.bumps.pool_jto,
        )?;
        init_token_vault(
            &ctx.accounts.payer,
            &ctx.accounts.stake_jtx,
            &ctx.accounts.jtx_mint,
            &ctx.accounts.config,
            &ctx.accounts.jtx_token_program,
            &ctx.accounts.system_program,
            b"stake_jtx",
            ctx.bumps.stake_jtx,
        )?;
        init_token_vault(
            &ctx.accounts.payer,
            &ctx.accounts.reward_jto,
            &ctx.accounts.jto_mint,
            &ctx.accounts.config,
            &ctx.accounts.jto_token_program,
            &ctx.accounts.system_program,
            b"reward_jto",
            ctx.bumps.reward_jto,
        )?;
        Ok(())
    }

    pub fn seed_liquidity(ctx: Context<SeedLiquidity>, jtx_amount: u64, jto_amount: u64) -> Result<()> {
        require!(!ctx.accounts.config.paused, FlywheelError::Paused);
        require!(jtx_amount > 0 && jto_amount > 0, FlywheelError::ZeroAmount);
        require!(
            ctx.accounts.pool.jtx_reserve == 0 && ctx.accounts.pool.jto_reserve == 0,
            FlywheelError::AlreadySeeded
        );
        transfer_tokens(
            ctx.accounts.jtx_token_program.to_account_info(),
            ctx.accounts.admin_jtx.to_account_info(),
            ctx.accounts.jtx_mint.to_account_info(),
            ctx.accounts.pool_jtx.to_account_info(),
            ctx.accounts.admin.to_account_info(),
            jtx_amount,
            ctx.accounts.jtx_mint.decimals,
            None,
        )?;
        transfer_tokens(
            ctx.accounts.jto_token_program.to_account_info(),
            ctx.accounts.admin_jto.to_account_info(),
            ctx.accounts.jto_mint.to_account_info(),
            ctx.accounts.pool_jto.to_account_info(),
            ctx.accounts.admin.to_account_info(),
            jto_amount,
            ctx.accounts.jto_mint.decimals,
            None,
        )?;
        ctx.accounts.pool.jtx_reserve = jtx_amount;
        ctx.accounts.pool.jto_reserve = jto_amount;
        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        require!(!ctx.accounts.config.paused, FlywheelError::Paused);
        require!(amount > 0, FlywheelError::ZeroAmount);
        let clock = Clock::get()?;
        let position = &mut ctx.accounts.position;
        require!(position.unlock_epoch == 0, FlywheelError::UnstakePending);
        // A fresh position and a fully released position both start the boost clock.
        // A top-up leaves staked_at alone.
        if position.amount == 0 {
            position.staker = ctx.accounts.staker.key();
            position.bump = ctx.bumps.position;
            position.staked_at = clock.unix_timestamp;
            position.frozen_boost_bps = 0;
        }
        let new_amount = position.amount.checked_add(amount).ok_or(FlywheelError::Math)?;
        let weight = weight_now(&ctx.accounts.config, new_amount, position.staked_at, 0, clock.unix_timestamp)?;
        set_weight(&mut ctx.accounts.pool, position, weight)?;
        position.amount = new_amount;
        transfer_tokens(
            ctx.accounts.jtx_token_program.to_account_info(),
            ctx.accounts.staker_jtx.to_account_info(),
            ctx.accounts.jtx_mint.to_account_info(),
            ctx.accounts.stake_jtx.to_account_info(),
            ctx.accounts.staker.to_account_info(),
            amount,
            ctx.accounts.jtx_mint.decimals,
            None,
        )?;
        pay_pending(
            &ctx.accounts.config,
            position,
            ctx.accounts.reward_jto.to_account_info(),
            ctx.accounts.staker_jto.to_account_info(),
            ctx.accounts.jto_mint.to_account_info(),
            ctx.accounts.jto_token_program.to_account_info(),
        )?;
        Ok(())
    }

    pub fn request_unstake(ctx: Context<StakeMove>) -> Result<()> {
        let clock = Clock::get()?;
        let position = &mut ctx.accounts.position;
        require!(position.amount > 0, FlywheelError::NothingStaked);
        require!(position.unlock_epoch == 0, FlywheelError::UnstakePending);
        let live = boost_bps(
            clock.unix_timestamp,
            position.staked_at,
            ctx.accounts.config.boost_ramp_seconds,
            ctx.accounts.config.boost_max_bps,
            0,
        )
        .map_err(|_| error!(FlywheelError::Math))?;
        let refreshed = weight_now(
            &ctx.accounts.config,
            position.amount,
            position.staked_at,
            0,
            clock.unix_timestamp,
        )?;
        set_weight(&mut ctx.accounts.pool, position, refreshed)?;
        set_weight(&mut ctx.accounts.pool, position, 0)?;
        position.frozen_boost_bps = live;
        position.unlock_epoch = clock.epoch.checked_add(1).ok_or(FlywheelError::Math)?;
        pay_pending(
            &ctx.accounts.config,
            position,
            ctx.accounts.reward_jto.to_account_info(),
            ctx.accounts.staker_jto.to_account_info(),
            ctx.accounts.jto_mint.to_account_info(),
            ctx.accounts.jto_token_program.to_account_info(),
        )?;
        Ok(())
    }

    pub fn cancel_unstake(ctx: Context<StakeMove>) -> Result<()> {
        let clock = Clock::get()?;
        let position = &mut ctx.accounts.position;
        require!(position.unlock_epoch != 0, FlywheelError::NothingStaked);
        require!(clock.epoch < position.unlock_epoch, FlywheelError::EpochNotReached);
        position.frozen_boost_bps = 0;
        position.unlock_epoch = 0;
        let weight = weight_now(
            &ctx.accounts.config,
            position.amount,
            position.staked_at,
            0,
            clock.unix_timestamp,
        )?;
        set_weight(&mut ctx.accounts.pool, position, weight)?;
        Ok(())
    }

    pub fn release_stake(ctx: Context<StakeMove>) -> Result<()> {
        let clock = Clock::get()?;
        let position = &mut ctx.accounts.position;
        require!(
            can_release(clock.epoch, position.unlock_epoch),
            FlywheelError::EpochNotReached
        );
        let amount = position.amount;
        require!(amount > 0, FlywheelError::NothingStaked);
        let bump = ctx.accounts.config.bump;
        let seeds: &[&[u8]] = &[b"config", &[bump]];
        transfer_tokens(
            ctx.accounts.jtx_token_program.to_account_info(),
            ctx.accounts.stake_jtx.to_account_info(),
            ctx.accounts.jtx_mint.to_account_info(),
            ctx.accounts.staker_jtx.to_account_info(),
            ctx.accounts.config.to_account_info(),
            amount,
            ctx.accounts.jtx_mint.decimals,
            Some(&[seeds]),
        )?;
        position.amount = 0;
        position.staked_at = 0;
        position.unlock_epoch = 0;
        position.frozen_boost_bps = 0;
        Ok(())
    }

    pub fn buy_jtx(ctx: Context<Swap>, jto_in: u64, min_jtx_out: u64) -> Result<()> {
        require!(!ctx.accounts.config.paused, FlywheelError::Paused);
        let quote = quote_buy(
            ctx.accounts.pool.jtx_reserve,
            ctx.accounts.pool.jto_reserve,
            jto_in,
        )
        .map_err(|_| error!(FlywheelError::Math))?;
        require!(quote.jtx_out >= min_jtx_out, FlywheelError::Slippage);
        transfer_tokens(
            ctx.accounts.jto_token_program.to_account_info(),
            ctx.accounts.user_jto.to_account_info(),
            ctx.accounts.jto_mint.to_account_info(),
            ctx.accounts.pool_jto.to_account_info(),
            ctx.accounts.user.to_account_info(),
            quote.curve_in,
            ctx.accounts.jto_mint.decimals,
            None,
        )?;
        if quote.fee > 0 {
            transfer_tokens(
                ctx.accounts.jto_token_program.to_account_info(),
                ctx.accounts.user_jto.to_account_info(),
                ctx.accounts.jto_mint.to_account_info(),
                ctx.accounts.reward_jto.to_account_info(),
                ctx.accounts.user.to_account_info(),
                quote.fee,
                ctx.accounts.jto_mint.decimals,
                None,
            )?;
        }
        let bump = ctx.accounts.config.bump;
        let seeds: &[&[u8]] = &[b"config", &[bump]];
        transfer_tokens(
            ctx.accounts.jtx_token_program.to_account_info(),
            ctx.accounts.pool_jtx.to_account_info(),
            ctx.accounts.jtx_mint.to_account_info(),
            ctx.accounts.user_jtx.to_account_info(),
            ctx.accounts.config.to_account_info(),
            quote.jtx_out,
            ctx.accounts.jtx_mint.decimals,
            Some(&[seeds]),
        )?;
        ctx.accounts.pool.jto_reserve = ctx
            .accounts
            .pool
            .jto_reserve
            .checked_add(quote.curve_in)
            .ok_or(FlywheelError::Math)?;
        ctx.accounts.pool.jtx_reserve = ctx
            .accounts
            .pool
            .jtx_reserve
            .checked_sub(quote.jtx_out)
            .ok_or(FlywheelError::Math)?;
        apply_index(&mut ctx.accounts.pool, quote.fee)?;
        Ok(())
    }

    pub fn sell_jtx(ctx: Context<Swap>, jtx_in: u64, min_jto_out: u64) -> Result<()> {
        require!(!ctx.accounts.config.paused, FlywheelError::Paused);
        let quote = quote_sell(
            ctx.accounts.pool.jtx_reserve,
            ctx.accounts.pool.jto_reserve,
            jtx_in,
        )
        .map_err(|_| error!(FlywheelError::Math))?;
        require!(quote.jto_to_user >= min_jto_out, FlywheelError::Slippage);
        transfer_tokens(
            ctx.accounts.jtx_token_program.to_account_info(),
            ctx.accounts.user_jtx.to_account_info(),
            ctx.accounts.jtx_mint.to_account_info(),
            ctx.accounts.pool_jtx.to_account_info(),
            ctx.accounts.user.to_account_info(),
            jtx_in,
            ctx.accounts.jtx_mint.decimals,
            None,
        )?;
        let bump = ctx.accounts.config.bump;
        let seeds: &[&[u8]] = &[b"config", &[bump]];
        if quote.fee > 0 {
            transfer_tokens(
                ctx.accounts.jto_token_program.to_account_info(),
                ctx.accounts.pool_jto.to_account_info(),
                ctx.accounts.jto_mint.to_account_info(),
                ctx.accounts.reward_jto.to_account_info(),
                ctx.accounts.config.to_account_info(),
                quote.fee,
                ctx.accounts.jto_mint.decimals,
                Some(&[seeds]),
            )?;
        }
        transfer_tokens(
            ctx.accounts.jto_token_program.to_account_info(),
            ctx.accounts.pool_jto.to_account_info(),
            ctx.accounts.jto_mint.to_account_info(),
            ctx.accounts.user_jto.to_account_info(),
            ctx.accounts.config.to_account_info(),
            quote.jto_to_user,
            ctx.accounts.jto_mint.decimals,
            Some(&[seeds]),
        )?;
        ctx.accounts.pool.jtx_reserve = ctx
            .accounts
            .pool
            .jtx_reserve
            .checked_add(jtx_in)
            .ok_or(FlywheelError::Math)?;
        ctx.accounts.pool.jto_reserve = ctx
            .accounts
            .pool
            .jto_reserve
            .checked_sub(quote.jto_gross)
            .ok_or(FlywheelError::Math)?;
        apply_index(&mut ctx.accounts.pool, quote.fee)?;
        Ok(())
    }

    pub fn airdrop<'info>(ctx: Context<'_, '_, 'info, 'info, Airdrop<'info>>) -> Result<()> {
        let n = ctx.remaining_accounts.len();
        require!(
            n > 0 && n % 2 == 0 && n / 2 <= MAX_AIRDROP_BATCH,
            FlywheelError::BadBatch
        );
        let clock = Clock::get()?;
        let acc_before = ctx.accounts.pool.acc_jto_per_weight;
        let jto_mint = ctx.accounts.config.jto_mint;
        let mut i = 0;
        while i < n {
            let position_info = ctx.remaining_accounts[i].clone();
            let dest_info = ctx.remaining_accounts[i + 1].clone();
            i += 2;
            let mut position = load_position(&position_info)?;
            {
                let data = dest_info.try_borrow_data()?;
                let token = spl_token::state::Account::unpack(&data).map_err(|_| error!(FlywheelError::BadOwner))?;
                require!(token.owner == position.staker, FlywheelError::BadOwner);
                require!(token.mint == jto_mint, FlywheelError::BadOwner);
                require!(dest_info.owner == &spl_token::ID, FlywheelError::BadTokenProgram);
            }
            if position.unlock_epoch == 0 && position.amount > 0 {
                let weight = weight_now(
                    &ctx.accounts.config,
                    position.amount,
                    position.staked_at,
                    position.frozen_boost_bps,
                    clock.unix_timestamp,
                )?;
                set_weight(&mut ctx.accounts.pool, &mut position, weight)?;
            }
            pay_pending(
                &ctx.accounts.config,
                &mut position,
                ctx.accounts.reward_jto.to_account_info(),
                dest_info,
                ctx.accounts.jto_mint.to_account_info(),
                ctx.accounts.jto_token_program.to_account_info(),
            )?;
            store_position(&position_info, &position)?;
        }
        require!(
            ctx.accounts.pool.acc_jto_per_weight == acc_before,
            FlywheelError::Math
        );
        Ok(())
    }

    pub fn admin_mint(ctx: Context<AdminMint>, amount: u64) -> Result<()> {
        require!(amount > 0, FlywheelError::ZeroAmount);
        require!(
            ctx.accounts.jtx_mint.mint_authority == COption::Some(ctx.accounts.config.key()),
            FlywheelError::MintAuthority
        );
        let supply = ctx.accounts.jtx_mint.supply;
        let next = supply.checked_add(amount).ok_or(FlywheelError::Math)?;
        require!(next <= MAX_SUPPLY, FlywheelError::SupplyCap);
        let bump = ctx.accounts.config.bump;
        let seeds: &[&[u8]] = &[b"config", &[bump]];
        let cpi = MintTo {
            mint: ctx.accounts.jtx_mint.to_account_info(),
            to: ctx.accounts.destination.to_account_info(),
            authority: ctx.accounts.config.to_account_info(),
        };
        token_interface::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.jtx_token_program.to_account_info(),
                cpi,
                &[seeds],
            ),
            amount,
        )?;
        Ok(())
    }

    pub fn set_paused(ctx: Context<Admin>, paused: bool) -> Result<()> {
        ctx.accounts.config.paused = paused;
        Ok(())
    }

    pub fn set_tiers(
        ctx: Context<Admin>,
        tiers: [Tier; 4],
        weight_cap: u64,
        boost_ramp_seconds: i64,
        boost_max_bps: u16,
    ) -> Result<()> {
        require!(boost_ramp_seconds > 0, FlywheelError::BadTiers);
        require!(boost_max_bps >= BOOST_START_BPS, FlywheelError::BadTiers);
        require!(weight_cap > 0, FlywheelError::BadTiers);
        for i in 0..4 {
            require!(tiers[i].rate_bps > 0, FlywheelError::BadTiers);
            if i > 0 {
                require!(tiers[i].min_amount > tiers[i - 1].min_amount, FlywheelError::BadTiers);
            }
        }
        ctx.accounts.config.tiers = tiers;
        ctx.accounts.config.weight_cap = weight_cap;
        ctx.accounts.config.boost_ramp_seconds = boost_ramp_seconds;
        ctx.accounts.config.boost_max_bps = boost_max_bps;
        Ok(())
    }
}

fn weight_now(config: &Account<Config>, amount: u64, staked_at: i64, frozen: u16, now: i64) -> Result<u64> {
    let rate = tier_rate(&config.tiers, amount);
    let boost = boost_bps(now, staked_at, config.boost_ramp_seconds, config.boost_max_bps, frozen)
        .map_err(|_| error!(FlywheelError::Math))?;
    effective_weight(amount, rate, boost, config.weight_cap).map_err(|_| error!(FlywheelError::Math))
}

fn settle(position: &mut Position, acc: u128) -> Result<()> {
    let accrued = (position.weight as u128)
        .checked_mul(acc)
        .ok_or(FlywheelError::Math)?
        / INDEX_SCALE;
    let delta = accrued
        .checked_sub(position.reward_debt)
        .ok_or(FlywheelError::Math)?;
    if delta > u64::MAX as u128 {
        return err!(FlywheelError::Math);
    }
    position.pending_jto = position
        .pending_jto
        .checked_add(delta as u64)
        .ok_or(FlywheelError::Math)?;
    position.reward_debt = accrued;
    Ok(())
}

fn set_weight(pool: &mut Account<Pool>, position: &mut Position, new_weight: u64) -> Result<()> {
    settle(position, pool.acc_jto_per_weight)?;
    pool.total_weight = pool
        .total_weight
        .checked_sub(position.weight as u128)
        .ok_or(FlywheelError::Math)?
        .checked_add(new_weight as u128)
        .ok_or(FlywheelError::Math)?;
    position.weight = new_weight;
    position.reward_debt = (new_weight as u128)
        .checked_mul(pool.acc_jto_per_weight)
        .ok_or(FlywheelError::Math)?
        / INDEX_SCALE;
    Ok(())
}

fn apply_index(pool: &mut Account<Pool>, fee: u64) -> Result<()> {
    let next = index_fee(pool.acc_jto_per_weight, pool.total_weight, pool.unindexed_jto, fee)
        .map_err(|_| error!(FlywheelError::Math))?;
    pool.acc_jto_per_weight = next.acc;
    pool.unindexed_jto = next.unindexed;
    Ok(())
}

fn pay_pending<'info>(
    config: &Account<'info, Config>,
    position: &mut Position,
    reward: AccountInfo<'info>,
    dest: AccountInfo<'info>,
    mint: AccountInfo<'info>,
    token_program: AccountInfo<'info>,
) -> Result<()> {
    let amount = position.pending_jto;
    if amount == 0 {
        return Ok(());
    }
    let bump = config.bump;
    let seeds: &[&[u8]] = &[b"config", &[bump]];
    transfer_tokens(
        token_program,
        reward,
        mint,
        dest,
        config.to_account_info(),
        amount,
        9,
        Some(&[seeds]),
    )?;
    position.pending_jto = 0;
    Ok(())
}

fn load_position(info: &AccountInfo) -> Result<Position> {
    require!(info.owner == &crate::ID, FlywheelError::BadOwner);
    let data = info.try_borrow_data()?;
    let mut slice: &[u8] = &data;
    Position::try_deserialize(&mut slice).map_err(|_| error!(FlywheelError::BadOwner))
}

fn store_position(info: &AccountInfo, position: &Position) -> Result<()> {
    let mut data = info.try_borrow_mut_data()?;
    let mut cursor = &mut data[..];
    position.try_serialize(&mut cursor)?;
    Ok(())
}

#[inline(never)]
fn token_account_space<'info>(mint: &AccountInfo<'info>) -> Result<usize> {
    if *mint.owner == spl_token_2022::ID {
        use spl_token_2022::extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions};
        use spl_token_2022::state::{Account, Mint};
        let data = mint.try_borrow_data()?;
        let state = StateWithExtensions::<Mint>::unpack(&data)
            .map_err(|_| error!(FlywheelError::BadTokenProgram))?;
        let extensions = state
            .get_extension_types()
            .map_err(|_| error!(FlywheelError::BadTokenProgram))?;
        let required = ExtensionType::get_required_init_account_extensions(&extensions);
        ExtensionType::try_calculate_account_len::<Account>(&required)
            .map_err(|_| error!(FlywheelError::BadTokenProgram))
    } else {
        Ok(spl_token::state::Account::LEN)
    }
}

#[inline(never)]
fn init_token_vault<'info>(
    payer: &Signer<'info>,
    vault: &UncheckedAccount<'info>,
    mint: &InterfaceAccount<'info, Mint>,
    authority: &Account<'info, Config>,
    token_program: &Interface<'info, TokenInterface>,
    system_program: &Program<'info, System>,
    seed: &'static [u8],
    bump: u8,
) -> Result<()> {
    let payer = payer.to_account_info();
    let vault = vault.to_account_info();
    let mint_info = mint.to_account_info();
    let authority = authority.to_account_info();
    let token_program = token_program.to_account_info();
    let system_program = system_program.to_account_info();
    require!(
        vault.lamports() == 0 && vault.owner == &System::id(),
        FlywheelError::VaultExists
    );
    let space = token_account_space(&mint_info)?;
    let lamports = Rent::get()?.minimum_balance(space);
    let bump_seed = [bump];
    let signer_seeds: &[&[u8]] = &[seed, &bump_seed];
    let cpi_accounts = anchor_lang::system_program::CreateAccount {
        from: payer,
        to: vault.clone(),
    };
    let cpi_context = CpiContext::new(system_program, cpi_accounts);
    anchor_lang::system_program::create_account(
        cpi_context.with_signer(&[signer_seeds]),
        lamports,
        space as u64,
        token_program.key,
    )?;
    let init_accounts = token_interface::InitializeAccount3 {
        account: vault,
        mint: mint_info,
        authority,
    };
    token_interface::initialize_account3(CpiContext::new(token_program, init_accounts))?;
    Ok(())
}

fn transfer_tokens<'info>(
    token_program: AccountInfo<'info>,
    from: AccountInfo<'info>,
    mint: AccountInfo<'info>,
    to: AccountInfo<'info>,
    authority: AccountInfo<'info>,
    amount: u64,
    decimals: u8,
    signer: Option<&[&[&[u8]]]>,
) -> Result<()> {
    let cpi = TransferChecked { from, mint, to, authority };
    let ctx = match signer {
        Some(seeds) => CpiContext::new_with_signer(token_program, cpi, seeds),
        None => CpiContext::new(token_program, cpi),
    };
    token_interface::transfer_checked(ctx, amount, decimals)
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + Config::INIT_SPACE,
        seeds = [b"config"],
        bump
    )]
    pub config: Box<Account<'info, Config>>,
    #[account(
        init,
        payer = payer,
        space = 8 + Pool::INIT_SPACE,
        seeds = [b"pool"],
        bump
    )]
    pub pool: Box<Account<'info, Pool>>,
    #[account(mint::token_program = jtx_token_program)]
    pub jtx_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mint::token_program = jto_token_program)]
    pub jto_mint: Box<InterfaceAccount<'info, Mint>>,
    /// CHECK: empty PDA. initialize creates it as the config-owned JTX pool vault.
    #[account(mut, seeds = [b"pool_jtx"], bump)]
    pub pool_jtx: UncheckedAccount<'info>,
    /// CHECK: empty PDA. initialize creates it as the config-owned JTO pool vault.
    #[account(mut, seeds = [b"pool_jto"], bump)]
    pub pool_jto: UncheckedAccount<'info>,
    /// CHECK: empty PDA. initialize creates it as the config-owned JTX stake vault.
    #[account(mut, seeds = [b"stake_jtx"], bump)]
    pub stake_jtx: UncheckedAccount<'info>,
    /// CHECK: empty PDA. initialize creates it as the config-owned JTO reward vault.
    #[account(mut, seeds = [b"reward_jto"], bump)]
    pub reward_jto: UncheckedAccount<'info>,
    #[account(address = spl_token_2022::ID)]
    pub jtx_token_program: Interface<'info, TokenInterface>,
    #[account(address = spl_token::ID)]
    pub jto_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SeedLiquidity<'info> {
    pub admin: Signer<'info>,
    #[account(seeds = [b"config"], bump = config.bump, has_one = admin @ FlywheelError::NotAdmin, has_one = jtx_mint, has_one = jto_mint)]
    pub config: Box<Account<'info, Config>>,
    #[account(mut, seeds = [b"pool"], bump = pool.bump)]
    pub pool: Box<Account<'info, Pool>>,
    pub jtx_mint: Box<InterfaceAccount<'info, Mint>>,
    pub jto_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [b"pool_jtx"],
        bump,
        token::mint = jtx_mint,
        token::authority = config,
        token::token_program = jtx_token_program
    )]
    pub pool_jtx: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [b"pool_jto"],
        bump,
        token::mint = jto_mint,
        token::authority = config,
        token::token_program = jto_token_program
    )]
    pub pool_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = jtx_mint, token::authority = admin, token::token_program = jtx_token_program)]
    pub admin_jtx: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = jto_mint, token::authority = admin, token::token_program = jto_token_program)]
    pub admin_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = spl_token_2022::ID)]
    pub jtx_token_program: Interface<'info, TokenInterface>,
    #[account(address = spl_token::ID)]
    pub jto_token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub staker: Signer<'info>,
    #[account(seeds = [b"config"], bump = config.bump, has_one = jtx_mint, has_one = jto_mint)]
    pub config: Box<Account<'info, Config>>,
    #[account(mut, seeds = [b"pool"], bump = pool.bump)]
    pub pool: Box<Account<'info, Pool>>,
    pub jtx_mint: Box<InterfaceAccount<'info, Mint>>,
    pub jto_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [b"stake_jtx"],
        bump,
        token::mint = jtx_mint,
        token::authority = config,
        token::token_program = jtx_token_program
    )]
    pub stake_jtx: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [b"reward_jto"],
        bump,
        token::mint = jto_mint,
        token::authority = config,
        token::token_program = jto_token_program
    )]
    pub reward_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = staker,
        space = 8 + Position::INIT_SPACE,
        seeds = [b"position", staker.key().as_ref()],
        bump
    )]
    pub position: Box<Account<'info, Position>>,
    #[account(mut, token::mint = jtx_mint, token::authority = staker, token::token_program = jtx_token_program)]
    pub staker_jtx: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = jto_mint, token::authority = staker, token::token_program = jto_token_program)]
    pub staker_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = spl_token_2022::ID)]
    pub jtx_token_program: Interface<'info, TokenInterface>,
    #[account(address = spl_token::ID)]
    pub jto_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct StakeMove<'info> {
    pub staker: Signer<'info>,
    #[account(seeds = [b"config"], bump = config.bump, has_one = jtx_mint, has_one = jto_mint)]
    pub config: Box<Account<'info, Config>>,
    #[account(mut, seeds = [b"pool"], bump = pool.bump)]
    pub pool: Box<Account<'info, Pool>>,
    pub jtx_mint: Box<InterfaceAccount<'info, Mint>>,
    pub jto_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [b"stake_jtx"],
        bump,
        token::mint = jtx_mint,
        token::authority = config,
        token::token_program = jtx_token_program
    )]
    pub stake_jtx: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [b"reward_jto"],
        bump,
        token::mint = jto_mint,
        token::authority = config,
        token::token_program = jto_token_program
    )]
    pub reward_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, seeds = [b"position", staker.key().as_ref()], bump = position.bump, has_one = staker)]
    pub position: Box<Account<'info, Position>>,
    #[account(mut, token::mint = jtx_mint, token::authority = staker, token::token_program = jtx_token_program)]
    pub staker_jtx: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = jto_mint, token::authority = staker, token::token_program = jto_token_program)]
    pub staker_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = spl_token_2022::ID)]
    pub jtx_token_program: Interface<'info, TokenInterface>,
    #[account(address = spl_token::ID)]
    pub jto_token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct Swap<'info> {
    pub user: Signer<'info>,
    #[account(seeds = [b"config"], bump = config.bump, has_one = jtx_mint, has_one = jto_mint)]
    pub config: Box<Account<'info, Config>>,
    #[account(mut, seeds = [b"pool"], bump = pool.bump)]
    pub pool: Box<Account<'info, Pool>>,
    pub jtx_mint: Box<InterfaceAccount<'info, Mint>>,
    pub jto_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [b"pool_jtx"],
        bump,
        token::mint = jtx_mint,
        token::authority = config,
        token::token_program = jtx_token_program
    )]
    pub pool_jtx: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [b"pool_jto"],
        bump,
        token::mint = jto_mint,
        token::authority = config,
        token::token_program = jto_token_program
    )]
    pub pool_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [b"reward_jto"],
        bump,
        token::mint = jto_mint,
        token::authority = config,
        token::token_program = jto_token_program
    )]
    pub reward_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = jtx_mint, token::authority = user, token::token_program = jtx_token_program)]
    pub user_jtx: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, token::mint = jto_mint, token::authority = user, token::token_program = jto_token_program)]
    pub user_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = spl_token_2022::ID)]
    pub jtx_token_program: Interface<'info, TokenInterface>,
    #[account(address = spl_token::ID)]
    pub jto_token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct Airdrop<'info> {
    #[account(seeds = [b"config"], bump = config.bump, has_one = jto_mint)]
    pub config: Box<Account<'info, Config>>,
    #[account(mut, seeds = [b"pool"], bump = pool.bump)]
    pub pool: Box<Account<'info, Pool>>,
    pub jto_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        seeds = [b"reward_jto"],
        bump,
        token::mint = jto_mint,
        token::authority = config,
        token::token_program = jto_token_program
    )]
    pub reward_jto: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = spl_token::ID)]
    pub jto_token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct AdminMint<'info> {
    pub admin: Signer<'info>,
    #[account(seeds = [b"config"], bump = config.bump, has_one = admin @ FlywheelError::NotAdmin, has_one = jtx_mint)]
    pub config: Box<Account<'info, Config>>,
    #[account(mut, mint::token_program = jtx_token_program)]
    pub jtx_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut, token::mint = jtx_mint, token::token_program = jtx_token_program)]
    pub destination: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = spl_token_2022::ID)]
    pub jtx_token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct Admin<'info> {
    pub admin: Signer<'info>,
    #[account(mut, seeds = [b"config"], bump = config.bump, has_one = admin @ FlywheelError::NotAdmin)]
    pub config: Box<Account<'info, Config>>,
}
