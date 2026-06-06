//! Guest-binary form of the `spel-token` example: the on-chain program.
//!
//! Identical logic to `examples/spel-token`, packaged as a RISC Zero guest so it
//! can be built with `cargo risczero build` (or `cargo +risc0 build` for a bare
//! ELF) and deployed to LEZ. The reusable mint-authority logic lives in the
//! `mint-authority` library; every handler here is thin.

#![no_main]
#![allow(unused_imports)]

use borsh::{BorshDeserialize, BorshSerialize};
use mint_authority::{TokenHolding, TokenState};
use nssa_core::account::{Account, AccountId, Data};
use nssa_core::program::DEFAULT_PROGRAM_ID;
use spel_framework::pda::{seed_from_str, ToSeed};
use spel_framework::prelude::*;
use spel_framework::spel_output::AutoClaim;

risc0_zkvm::guest::entry!(main);

#[lez_program]
mod spel_token {
    use super::*;

    #[instruction]
    pub fn create_token(
        #[account(init, pda = [literal("token"), arg("name")])] mut state: AccountWithMetadata,
        #[account(init, pda = [literal("holding"), arg("name"), arg("holder")])]
        mut holding: AccountWithMetadata,
        #[account(signer)] authority: AccountWithMetadata,
        name: String,
        holder: [u8; 32],
        decimals: u8,
        initial_supply: u128,
    ) -> SpelResult {
        let _ = &holder;
        let token_id = state.account_id;
        mint_authority::initialize_token(
            &mut state.account,
            name.clone(),
            decimals,
            initial_supply,
            authority.account_id,
        )?;
        TokenHolding {
            token: Some(token_id),
            balance: initial_supply,
        }
        .store(&mut holding.account)?;
        Ok(SpelOutput::execute(vec![state, holding, authority], vec![]))
    }

    #[instruction]
    pub fn mint_to(
        #[account(mut, pda = [literal("token"), arg("name")])] mut state: AccountWithMetadata,
        #[account(mut, pda = [literal("holding"), arg("name"), arg("holder")])]
        mut holding: AccountWithMetadata,
        #[account(signer)] authority: AccountWithMetadata,
        name: String,
        holder: [u8; 32],
        amount: u128,
    ) -> SpelResult {
        let token_id = state.account_id;
        mint_authority::mint(&mut state, &mut holding, &authority, token_id, amount)?;
        // The holding PDA is normally created by `create_token` (so it is already
        // program-owned and must be mutated, not re-claimed: claiming a non-default
        // account is rejected by the sequencer as InvalidProgramBehavior). Claim it
        // only on first touch (still default-owned), which keeps `mint_to` valid even
        // for a holding that was never pre-created. Same conditional-claim pattern as
        // RFP-002's freeze marker.
        let holding_claim = if holding.account.program_owner == DEFAULT_PROGRAM_ID {
            AutoClaim::pda_from_seeds(&[
                &seed_from_str("holding"),
                &name.to_seed(),
                &holder.to_seed(),
            ])
        } else {
            AutoClaim::None
        };
        Ok(SpelOutput::execute_with_claims(
            &[state.account, holding.account, authority.account],
            &[AutoClaim::None, holding_claim, AutoClaim::None],
            vec![],
        ))
    }

    #[instruction]
    pub fn rotate_authority(
        #[account(mut, pda = [literal("token"), arg("name")])] mut state: AccountWithMetadata,
        #[account(signer)] current_authority: AccountWithMetadata,
        name: String,
        new_authority: AccountId,
    ) -> SpelResult {
        let _ = &name;
        mint_authority::rotate_authority(&mut state, &current_authority, new_authority)?;
        Ok(SpelOutput::execute(vec![state, current_authority], vec![]))
    }

    #[instruction]
    pub fn revoke_authority(
        #[account(mut, pda = [literal("token"), arg("name")])] mut state: AccountWithMetadata,
        #[account(signer)] current_authority: AccountWithMetadata,
        name: String,
    ) -> SpelResult {
        let _ = &name;
        mint_authority::revoke_authority(&mut state, &current_authority)?;
        Ok(SpelOutput::execute(vec![state, current_authority], vec![]))
    }
}
