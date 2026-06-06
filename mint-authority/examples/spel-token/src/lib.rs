//! # spel-token
//!
//! End-to-end example program for the `mint-authority` library (Logos LP-0013).
//!
//! A minimal fungible token program whose lifecycle is governed by a mint
//! authority. Every handler is thin: the reusable logic lives in
//! `mint-authority`. This is the program that is built into a RISC Zero guest
//! (`spel-token-guest`), deployed to a LEZ standalone sequencer, and driven by the
//! demo script.
//!
//! Instructions:
//! 1. `create_token` records the signer as the initial mint authority and creates
//!    the token-state PDA plus the creator's holding PDA, seeded with
//!    `initial_supply`.
//! 2. `mint_to` mints new supply into a holder PDA. Gated on the mint authority.
//! 3. `rotate_authority` moves the mint authority to a new account.
//! 4. `revoke_authority` sets the authority to `None`, fixing the supply forever.
//!
//! The two example integrations the prize asks for are expressed as flows over
//! these instructions:
//! - **fixed supply with revoked authority**: `create_token` then `revoke_authority`.
//!   A later `mint_to` is rejected with the documented `ERR_MINT_REVOKED` code.
//! - **variable supply with mint authority**: `create_token` then repeated `mint_to`,
//!   optionally `rotate_authority` to hand minting to a treasury account.
//!
//! See `scripts/demo.sh` and `examples/` for the runnable versions.

#![allow(unused_imports)]

use borsh::{BorshDeserialize, BorshSerialize};
use mint_authority::{TokenHolding, TokenState};
use nssa_core::account::{Account, AccountId, Data};
use spel_framework::prelude::*;

#[lez_program]
mod spel_token {
    use super::*;

    /// Create a new token. The signer becomes the initial mint authority and is
    /// credited `initial_supply` in its holding. The token-state PDA is keyed by
    /// the token `name` so each name is a distinct token.
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
        let _ = &holder; // binds the holding PDA above; not needed in the body
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

    /// Mint `amount` new tokens into `holding`. Only the mint authority may call
    /// this; a revoked authority is rejected with `ERR_MINT_REVOKED`.
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
        let _ = (&name, &holder); // names bind the PDAs above; not needed in the body
        let token_id = state.account_id;
        mint_authority::mint(&mut state, &mut holding, &authority, token_id, amount)?;
        Ok(SpelOutput::execute_with_claims(
            &[state.account, holding.account, authority.account],
            &[
                AutoClaim::None,
                AutoClaim::Claimed(Claim::Authorized),
                AutoClaim::None,
            ],
            vec![],
        ))
    }

    /// Rotate the mint authority to `new_authority`. Only the current authority may
    /// call this.
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

    /// Revoke the mint authority, fixing the supply. Only the current authority may
    /// call this. After this, every `mint_to` fails with `ERR_MINT_REVOKED`.
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

#[cfg(test)]
mod tests {
    use super::spel_token::*;
    use super::*;
    use mint_authority::ERR_MINT_REVOKED;

    fn acct(id: u8, authorized: bool) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: authorized,
            account_id: AccountId::new([id; 32]),
        }
    }

    fn empty() -> AccountWithMetadata {
        acct(0, false)
    }

    fn state_after(out: &SpelOutput) -> AccountWithMetadata {
        AccountWithMetadata {
            account: out.clone().into_parts().post_states[0].account().clone(),
            is_authorized: false,
            account_id: AccountId::new([0u8; 32]),
        }
    }

    #[test]
    fn create_token_records_authority_and_initial_supply() {
        let out = create_token(
            empty(),
            empty(),
            acct(1, true),
            "Gold".into(),
            [1u8; 32],
            6,
            1000,
        )
        .expect("create ok");
        let st = TokenState::load(&out.into_parts().post_states[0].account().clone()).unwrap();
        assert_eq!(st.total_supply, 1000);
        assert_eq!(st.mint_authority, Some(AccountId::new([1u8; 32])));
    }

    #[test]
    fn authority_can_mint_more_supply() {
        let created = create_token(
            empty(),
            empty(),
            acct(1, true),
            "Gold".into(),
            [1u8; 32],
            6,
            100,
        )
        .expect("create");
        let st = state_after(&created);
        let out =
            mint_to(st, empty(), acct(1, true), "Gold".into(), [2u8; 32], 50).expect("mint ok");
        let st = TokenState::load(out.into_parts().post_states[0].account()).unwrap();
        assert_eq!(st.total_supply, 150);
    }

    #[test]
    fn non_authority_cannot_mint() {
        let created = create_token(
            empty(),
            empty(),
            acct(1, true),
            "Gold".into(),
            [1u8; 32],
            6,
            100,
        )
        .expect("create");
        let err = mint_to(
            state_after(&created),
            empty(),
            acct(2, true),
            "Gold".into(),
            [2u8; 32],
            50,
        )
        .unwrap_err();
        assert!(matches!(err, SpelError::Unauthorized { .. }));
    }

    #[test]
    fn revoke_then_mint_is_rejected_with_documented_code() {
        let created = create_token(
            empty(),
            empty(),
            acct(1, true),
            "Fix".into(),
            [1u8; 32],
            0,
            1000,
        )
        .expect("create");
        let revoked =
            revoke_authority(state_after(&created), acct(1, true), "Fix".into()).expect("revoke");
        let st = state_after(&revoked);
        assert_eq!(TokenState::load(&st.account).unwrap().mint_authority, None);
        let err = mint_to(st, empty(), acct(1, true), "Fix".into(), [1u8; 32], 1).unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_MINT_REVOKED));
    }

    #[test]
    fn rotate_hands_minting_to_the_new_authority() {
        let created = create_token(
            empty(),
            empty(),
            acct(1, true),
            "Var".into(),
            [1u8; 32],
            0,
            0,
        )
        .expect("create");
        let rotated = rotate_authority(
            state_after(&created),
            acct(1, true),
            "Var".into(),
            AccountId::new([2u8; 32]),
        )
        .expect("rotate");
        let st = state_after(&rotated);
        // old authority is locked out
        assert!(mint_to(
            st.clone(),
            empty(),
            acct(1, true),
            "Var".into(),
            [9u8; 32],
            1
        )
        .is_err());
        // new authority can mint
        assert!(mint_to(st, empty(), acct(2, true), "Var".into(), [9u8; 32], 1).is_ok());
    }

    /// The auto-generated IDL (from `#[lez_program]`) exposes every instruction and
    /// its PDA-seeded accounts. The program's `#[account_type]` data layouts
    /// (`TokenState`, `TokenHolding`) live in the `mint-authority` library, so they
    /// are documented in the committed, complete `spel_token.idl.json` deliverable
    /// rather than in this auto-IDL (the `#[account_type]` collector only sees
    /// types declared in the program source file). This test checks the auto-IDL's
    /// instruction surface; `committed_idl_describes_the_account_types` checks the
    /// shipped IDL file.
    #[test]
    fn idl_exposes_all_instructions() {
        let idl: spel_framework::idl::SpelIdl =
            serde_json::from_str(PROGRAM_IDL_JSON).expect("embedded IDL should parse");
        let instructions: Vec<&str> = idl.instructions.iter().map(|i| i.name.as_str()).collect();
        for expected in [
            "create_token",
            "mint_to",
            "rotate_authority",
            "revoke_authority",
        ] {
            assert!(
                instructions.contains(&expected),
                "IDL missing `{expected}`: {instructions:?}"
            );
        }
    }

    /// The committed, deploy-ready IDL file parses and describes both the four
    /// instructions and the `TokenState` / `TokenHolding` account layouts.
    #[test]
    fn committed_idl_describes_the_account_types() {
        let raw = include_str!("../spel_token.idl.json");
        let idl: spel_framework::idl::SpelIdl =
            serde_json::from_str(raw).expect("committed IDL should parse");
        let accounts: Vec<&str> = idl.accounts.iter().map(|a| a.name.as_str()).collect();
        for expected in ["TokenState", "TokenHolding"] {
            assert!(
                accounts.contains(&expected),
                "committed IDL missing `{expected}`: {accounts:?}"
            );
        }
        let instructions: Vec<&str> = idl.instructions.iter().map(|i| i.name.as_str()).collect();
        assert!(instructions.contains(&"mint_to"));
    }
}
