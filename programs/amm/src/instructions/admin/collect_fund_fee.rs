use crate::error::ErrorCode;
use crate::states::*;
use crate::util::*;
use anchor_lang::prelude::*;
use anchor_spl::token::Token;
use anchor_spl::token_interface::{Mint, Token2022, TokenAccount};
#[derive(Accounts)]
pub struct CollectFundFee<'info> {
    /// Only admin or fund_owner can collect fee now
    #[account(constraint = (owner.key() == amm_config.fund_owner || owner.key() == crate::admin::ID) @ ErrorCode::NotApproved)]
    pub owner: Signer<'info>,

    /// Pool state stores accumulated protocol fee amount
    #[account(mut)]
    pub pool_state: AccountLoader<'info, PoolState>,

    /// Amm config account stores fund_owner
    #[account(address = pool_state.load()?.amm_config)]
    pub amm_config: Account<'info, AmmConfig>,

    /// The address that holds pool tokens for token_0
    #[account(
        mut,
        constraint = token_vault_0.key() == pool_state.load()?.token_vault_0
    )]
    pub token_vault_0: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The address that holds pool tokens for token_1
    #[account(
        mut,
        constraint = token_vault_1.key() == pool_state.load()?.token_vault_1
    )]
    pub token_vault_1: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The mint of token vault 0
    #[account(
        address = token_vault_0.mint
    )]
    pub vault_0_mint: Box<InterfaceAccount<'info, Mint>>,

    /// The mint of token vault 1
    #[account(
        address = token_vault_1.mint
    )]
    pub vault_1_mint: Box<InterfaceAccount<'info, Mint>>,

    /// The address that receives the collected token_0 protocol fees
    #[account(mut)]
    pub recipient_token_account_0: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The address that receives the collected token_1 protocol fees
    #[account(mut)]
    pub recipient_token_account_1: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The SPL program to perform token transfers
    pub token_program: Program<'info, Token>,

    /// The SPL program 2022 to perform token transfers
    pub token_program_2022: Program<'info, Token2022>,
}

pub fn collect_fund_fee(
    ctx: Context<CollectFundFee>,
    amount_0_requested: u64,
    amount_1_requested: u64,
) -> Result<()> {
    collect_fund_fee_inner(
        &ctx.accounts,
        amount_0_requested,
        amount_1_requested,
        &[],
        &[],
    )
}

/// `collect_fund_fee` for a pool with Transfer Hook mints. The remaining accounts are the token_0
/// transfer's hook slice, then the token_1 transfer's; each count is the whole slice.
pub fn collect_fund_fee_v2<'info>(
    ctx: Context<'info, CollectFundFee<'info>>,
    amount_0_requested: u64,
    amount_1_requested: u64,
    token_0_hook_account_count: u16,
    token_1_hook_account_count: u16,
) -> Result<()> {
    let (rest, token_0_hook_accounts, token_1_hook_accounts) = split_hook_tail(
        ctx.remaining_accounts,
        token_0_hook_account_count,
        token_1_hook_account_count,
    )?;
    require!(rest.is_empty(), ErrorCode::InvalidHookAccountFraming);
    collect_fund_fee_inner(
        &ctx.accounts,
        amount_0_requested,
        amount_1_requested,
        token_0_hook_accounts,
        token_1_hook_accounts,
    )
}

fn collect_fund_fee_inner<'info>(
    accounts: &CollectFundFee<'info>,
    amount_0_requested: u64,
    amount_1_requested: u64,
    token_0_hook_accounts: &[AccountInfo<'info>],
    token_1_hook_accounts: &[AccountInfo<'info>],
) -> Result<()> {
    let amount_0: u64;
    let amount_1: u64;
    {
        let mut pool_state = accounts.pool_state.load_mut()?;
        amount_0 = amount_0_requested.min(pool_state.fund_fees_token_0);
        amount_1 = amount_1_requested.min(pool_state.fund_fees_token_1);

        pool_state.fund_fees_token_0 = pool_state
            .fund_fees_token_0
            .checked_sub(amount_0)
            .ok_or(ErrorCode::CalculateOverflow)?;
        pool_state.fund_fees_token_1 = pool_state
            .fund_fees_token_1
            .checked_sub(amount_1)
            .ok_or(ErrorCode::CalculateOverflow)?;
    }
    transfer_from_pool_vault_to_user_with_hook_accounts(
        &accounts.pool_state,
        &accounts.token_vault_0.to_account_info(),
        &accounts.recipient_token_account_0.to_account_info(),
        Some(accounts.vault_0_mint.clone()),
        &accounts.token_program,
        Some(accounts.token_program_2022.to_account_info()),
        amount_0,
        token_0_hook_accounts,
    )?;

    transfer_from_pool_vault_to_user_with_hook_accounts(
        &accounts.pool_state,
        &accounts.token_vault_1.to_account_info(),
        &accounts.recipient_token_account_1.to_account_info(),
        Some(accounts.vault_1_mint.clone()),
        &accounts.token_program,
        Some(accounts.token_program_2022.to_account_info()),
        amount_1,
        token_1_hook_accounts,
    )?;

    emit!(CollectProtocolFeeEvent {
        pool_state: accounts.pool_state.key(),
        recipient_token_account_0: accounts.recipient_token_account_0.key(),
        recipient_token_account_1: accounts.recipient_token_account_1.key(),
        amount_0,
        amount_1,
    });

    Ok(())
}
