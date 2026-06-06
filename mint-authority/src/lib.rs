//! # mint-authority
//!
//! A reusable mint-authority model for fungible tokens on the Logos Execution
//! Zone (LEZ), built for Logos Lambda Prize LP-0013. It applies the standardised
//! admin-authority pattern from [RFP-001] to a token program's mint: a single
//! authority is set when the token is created, may mint new supply, and may be
//! rotated to another account or revoked (set to `None`). A revoked mint
//! authority makes the supply permanently fixed.
//!
//! [RFP-001]: https://github.com/logos-co/rfp/blob/master/RFPs/RFP-001-admin-authority-lib.md
//!
//! ## Why a separate config, not the token holder's signature
//!
//! The minimal LEZ token example authorises minting by checking that the *token
//! definition account itself* signed the transaction. That conflates "owns the
//! definition account key" with "is allowed to mint", offers no rotation, and no
//! way to make supply fixed. This library replaces that with an explicit mint
//! authority stored alongside the token definition, mirroring SPL Token's
//! `mintAuthority` / `SetAuthority` semantics:
//!
//! - mint authority set at initialization,
//! - minting gated on that authority,
//! - authority rotation (to a new account) and revocation (`None`), where `None`
//!   freezes the supply.
//!
//! ## Relationship to the admin-authority library
//!
//! The access-control core here is intentionally the same shape as
//! `admin-authority` (RFP-001): an `Option<AccountId>` authority, a `require_*`
//! gate, set / rotate / revoke operations, null-authority validation, and
//! namespaced error codes. This crate is self-sufficient and agnostic (it does
//! not depend on the admin-authority crate) so a token program can adopt the mint
//! authority without pulling in the unrelated admin surface, which is the
//! "self-sufficient, agnostic library that handles approval as defined in
//! RFP-001" the prize asks for. The shared pattern is documented in
//! `docs/AUTHORITY_MODEL.md`.

use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::account::{Account, AccountId, AccountWithMetadata, Data};
use spel_framework::account_type;
use spel_framework::error::SpelError;

// ── Error codes ─────────────────────────────────────────────────────────────
//
// Custom codes are namespaced in the 3000 range to stay distinct from
// admin-authority (1000) and freeze-authority (2000). On chain a `Custom { code }`
// surfaces as `6000 + code` (see SpelError::error_code), so e.g. ERR_MINT_REVOKED
// (3003) is reported as program error `9003`. Authorization failures use the
// framework's `Unauthorized` variant, which is the canonical `1008`.

/// The mint-authority candidate is the null (all-zero) account, which nobody can
/// sign for and which would permanently disable minting in an unintended way.
pub const ERR_NULL_AUTHORITY: u32 = 3001;

/// The token state account is already initialized.
pub const ERR_ALREADY_INITIALIZED: u32 = 3002;

/// Minting was attempted while the mint authority is revoked (`None`). Supply is
/// fixed and no new tokens can ever be created. This is the documented
/// deterministic rejection required by the prize.
pub const ERR_MINT_REVOKED: u32 = 3003;

/// Minting would overflow the token's `u128` total supply or the holder balance.
pub const ERR_SUPPLY_OVERFLOW: u32 = 3004;

/// The supplied holder/state accounts do not belong to the same token.
pub const ERR_MISMATCHED_TOKEN: u32 = 3005;

// ── On-chain state ──────────────────────────────────────────────────────────

/// On-chain state for one fungible token: its mint authority and current total
/// supply, plus immutable identity (name, decimals).
///
/// A `mint_authority` of `None` means the authority was revoked: supply is
/// permanently fixed and [`require_mint_authority`] rejects every minter.
#[account_type]
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TokenState {
    /// Human-readable token name.
    pub name: String,
    /// Number of decimal places (display only; balances are integers).
    pub decimals: u8,
    /// Total minted supply across all holders.
    pub total_supply: u128,
    /// The account allowed to mint, or `None` if revoked (supply fixed).
    pub mint_authority: Option<AccountId>,
}

impl TokenState {
    /// Create token state with an active mint authority and the given starting supply.
    #[must_use]
    pub fn new(name: String, decimals: u8, total_supply: u128, mint_authority: AccountId) -> Self {
        Self {
            name,
            decimals,
            total_supply,
            mint_authority: Some(mint_authority),
        }
    }

    /// Serialize this state into an account's data buffer.
    pub fn store(&self, account: &mut Account) -> Result<(), SpelError> {
        let bytes = borsh::to_vec(self).map_err(|e| SpelError::SerializationError {
            message: e.to_string(),
        })?;
        account.data = Data::try_from(bytes).map_err(|e| SpelError::SerializationError {
            message: format!("token state too large: {e:?}"),
        })?;
        Ok(())
    }

    /// Deserialize state from an account's data buffer.
    pub fn load(account: &Account) -> Result<Self, SpelError> {
        TokenState::try_from_slice(account.data.as_ref()).map_err(|e| {
            SpelError::DeserializationError {
                account_index: 0,
                message: e.to_string(),
            }
        })
    }
}

/// A holder's balance of one token. Keyed in practice by a `[holder, token]` PDA.
#[account_type]
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TokenHolding {
    /// The token this balance belongs to (the `TokenState` account id).
    pub token: Option<AccountId>,
    /// The holder's balance in base units.
    pub balance: u128,
}

impl TokenHolding {
    /// Serialize this holding into an account's data buffer.
    pub fn store(&self, account: &mut Account) -> Result<(), SpelError> {
        let bytes = borsh::to_vec(self).map_err(|e| SpelError::SerializationError {
            message: e.to_string(),
        })?;
        account.data = Data::try_from(bytes).map_err(|e| SpelError::SerializationError {
            message: format!("token holding too large: {e:?}"),
        })?;
        Ok(())
    }

    /// Deserialize a holding from an account's data buffer, treating an empty
    /// (uninitialized) buffer as a zero balance not yet bound to a token.
    pub fn load(account: &Account) -> Result<Self, SpelError> {
        if account.data.as_ref().is_empty() {
            return Ok(Self::default());
        }
        TokenHolding::try_from_slice(account.data.as_ref()).map_err(|e| {
            SpelError::DeserializationError {
                account_index: 1,
                message: e.to_string(),
            }
        })
    }
}

// ── Authority validation and gate (RFP-001 pattern) ─────────────────────────

/// Reject a mint-authority candidate that would render the authority unusable.
///
/// On LEZ every [`AccountId`] is a 32-byte hash output (user accounts derive from
/// a nullifier public key, PDAs from `SHA256(prefix || program_id || seed)`).
/// There is no Solana-style on-curve/off-curve distinction to test, so RFP-001's
/// soft validation reduces here to the one verifiable check that matters: reject
/// the null authority (the all-zero id of `Account::default`), which nobody can
/// ever sign for. Both real signer accounts and PDAs are accepted.
pub fn validate_authority_candidate(candidate: &AccountId) -> Result<(), SpelError> {
    if candidate.value() == &[0u8; 32] {
        return Err(SpelError::Custom {
            code: ERR_NULL_AUTHORITY,
            message: "mint authority cannot be the null account".to_string(),
        });
    }
    Ok(())
}

/// Gate a mint behind the token's mint authority.
///
/// Returns `Ok(state)` only when `signer` authorized the transaction and matches
/// the token's recorded mint authority. A different signer or an unsigned
/// authority returns [`SpelError::Unauthorized`] (on-chain code `1008`); a revoked
/// authority returns the distinct [`ERR_MINT_REVOKED`] custom code so callers can
/// tell "wrong minter" from "supply is fixed".
pub fn require_mint_authority(
    state: &AccountWithMetadata,
    signer: &AccountWithMetadata,
) -> Result<TokenState, SpelError> {
    if !signer.is_authorized {
        return Err(SpelError::Unauthorized {
            message: "signer did not authorize the transaction".to_string(),
        });
    }
    let st = TokenState::load(&state.account)?;
    match st.mint_authority {
        Some(authority) if authority == signer.account_id => Ok(st),
        Some(_) => Err(SpelError::Unauthorized {
            message: "signer is not the mint authority".to_string(),
        }),
        None => Err(SpelError::Custom {
            code: ERR_MINT_REVOKED,
            message: "mint authority has been revoked; supply is fixed".to_string(),
        }),
    }
}

// ── Privileged operations ───────────────────────────────────────────────────
//
// Each encapsulates the full logic (gate + validate + mutate) so a consuming SPEL
// program adopts the pattern by delegating one call per instruction, with no
// access-control logic of its own. Mutations are computed in full and only then
// written, so a rejected call leaves state byte-for-byte unchanged (atomicity).

/// Create a new token with `signer` as the initial mint authority.
///
/// Call inside an `#[account(init, ...)]` handler for the token-state account.
/// `initial_supply` is minted to the creator's holding (the example program wires
/// that holding write); the authority is validated as non-null first.
pub fn initialize_token(
    state: &mut Account,
    name: String,
    decimals: u8,
    initial_supply: u128,
    mint_authority: AccountId,
) -> Result<TokenState, SpelError> {
    validate_authority_candidate(&mint_authority)?;
    if !state.data.as_ref().is_empty() {
        return Err(SpelError::Custom {
            code: ERR_ALREADY_INITIALIZED,
            message: "token state is already initialized".to_string(),
        });
    }
    let st = TokenState::new(name, decimals, initial_supply, mint_authority);
    st.store(state)?;
    Ok(st)
}

/// Mint `amount` new tokens into `holding`, increasing total supply.
///
/// Gated: `signer` must be the current mint authority (else `Unauthorized`); if the
/// authority is revoked the call fails with [`ERR_MINT_REVOKED`]. Overflow of the
/// `u128` supply or balance is rejected with [`ERR_SUPPLY_OVERFLOW`]. On success
/// both the token-state supply and the holding balance are rewritten atomically.
pub fn mint(
    state: &mut AccountWithMetadata,
    holding: &mut AccountWithMetadata,
    signer: &AccountWithMetadata,
    token_id: AccountId,
    amount: u128,
) -> Result<(), SpelError> {
    let mut st = require_mint_authority(state, signer)?;
    let mut hold = TokenHolding::load(&holding.account)?;

    // Bind a fresh holding to this token, or require an existing one to match.
    match hold.token {
        Some(t) if t == token_id => {}
        Some(_) => {
            return Err(SpelError::Custom {
                code: ERR_MISMATCHED_TOKEN,
                message: "holding belongs to a different token".to_string(),
            })
        }
        None => hold.token = Some(token_id),
    }

    let new_supply = st
        .total_supply
        .checked_add(amount)
        .ok_or_else(|| SpelError::Custom {
            code: ERR_SUPPLY_OVERFLOW,
            message: "total supply overflow".to_string(),
        })?;
    let new_balance = hold
        .balance
        .checked_add(amount)
        .ok_or_else(|| SpelError::Custom {
            code: ERR_SUPPLY_OVERFLOW,
            message: "holder balance overflow".to_string(),
        })?;

    // Compute fully, then commit, so a failure above leaves both accounts untouched.
    st.total_supply = new_supply;
    hold.balance = new_balance;
    st.store(&mut state.account)?;
    hold.store(&mut holding.account)?;
    Ok(())
}

/// Rotate the mint authority to `new_authority`.
///
/// Gated: `signer` must be the current mint authority. `new_authority` is
/// validated as non-null. The token-state account is rewritten in a single store,
/// so the authority is never left in an intermediate state: it is either the old
/// authority (on any failure) or the new one.
pub fn rotate_authority(
    state: &mut AccountWithMetadata,
    signer: &AccountWithMetadata,
    new_authority: AccountId,
) -> Result<(), SpelError> {
    let mut st = require_mint_authority(state, signer)?;
    validate_authority_candidate(&new_authority)?;
    st.mint_authority = Some(new_authority);
    st.store(&mut state.account)
}

/// Revoke the mint authority permanently, fixing the supply.
///
/// Gated: `signer` must be the current mint authority. After this, every
/// [`mint`] fails with [`ERR_MINT_REVOKED`] and no new supply can ever be created.
/// The rewrite is a single store (atomic): on failure the authority is unchanged.
pub fn revoke_authority(
    state: &mut AccountWithMetadata,
    signer: &AccountWithMetadata,
) -> Result<(), SpelError> {
    let mut st = require_mint_authority(state, signer)?;
    st.mint_authority = None;
    st.store(&mut state.account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nssa_core::program::PdaSeed;

    const TOKEN_ID: [u8; 32] = [9u8; 32];

    fn signer(id: [u8; 32], authorized: bool) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: authorized,
            account_id: AccountId::new(id),
        }
    }

    fn state_with(st: &TokenState) -> AccountWithMetadata {
        let mut account = Account::default();
        st.store(&mut account).unwrap();
        AccountWithMetadata {
            account,
            is_authorized: false,
            account_id: AccountId::new(TOKEN_ID),
        }
    }

    fn empty_holding() -> AccountWithMetadata {
        signer([0u8; 32], false)
    }

    fn token(authority: [u8; 32]) -> TokenState {
        TokenState::new("Test".to_string(), 6, 0, AccountId::new(authority))
    }

    // ── validation ──
    #[test]
    fn validate_rejects_the_null_authority() {
        let err = validate_authority_candidate(&AccountId::new([0u8; 32])).unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_NULL_AUTHORITY));
    }

    #[test]
    fn validate_accepts_a_normal_account() {
        assert!(validate_authority_candidate(&AccountId::new([5u8; 32])).is_ok());
    }

    #[test]
    fn validate_accepts_a_program_derived_pda() {
        let pda = AccountId::for_public_pda(&[1u32; 8], &PdaSeed::new([5u8; 32]));
        assert!(validate_authority_candidate(&pda).is_ok());
    }

    // ── initialize ──
    #[test]
    fn initialize_records_the_authority_and_supply() {
        let mut acc = Account::default();
        let st =
            initialize_token(&mut acc, "Gold".into(), 8, 100, AccountId::new([1u8; 32])).unwrap();
        assert_eq!(st.mint_authority, Some(AccountId::new([1u8; 32])));
        assert_eq!(st.total_supply, 100);
        assert_eq!(st.decimals, 8);
        assert_eq!(TokenState::load(&acc).unwrap(), st);
    }

    #[test]
    fn initialize_rejects_reinitialization() {
        let mut acc = Account::default();
        initialize_token(&mut acc, "A".into(), 0, 0, AccountId::new([1u8; 32])).unwrap();
        let err =
            initialize_token(&mut acc, "B".into(), 0, 0, AccountId::new([2u8; 32])).unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_ALREADY_INITIALIZED));
        // original is preserved
        assert_eq!(
            TokenState::load(&acc).unwrap().mint_authority,
            Some(AccountId::new([1u8; 32]))
        );
    }

    #[test]
    fn initialize_rejects_the_null_authority() {
        let mut acc = Account::default();
        let err =
            initialize_token(&mut acc, "A".into(), 0, 0, AccountId::new([0u8; 32])).unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_NULL_AUTHORITY));
    }

    // ── gate ──
    #[test]
    fn gate_accepts_the_authorized_authority() {
        let st = state_with(&token([1u8; 32]));
        assert!(require_mint_authority(&st, &signer([1u8; 32], true)).is_ok());
    }

    #[test]
    fn gate_rejects_a_different_signer() {
        let st = state_with(&token([1u8; 32]));
        let err = require_mint_authority(&st, &signer([2u8; 32], true)).unwrap_err();
        assert!(matches!(err, SpelError::Unauthorized { .. }));
    }

    #[test]
    fn gate_rejects_authority_that_did_not_sign() {
        let st = state_with(&token([1u8; 32]));
        let err = require_mint_authority(&st, &signer([1u8; 32], false)).unwrap_err();
        assert!(matches!(err, SpelError::Unauthorized { .. }));
    }

    #[test]
    fn gate_rejects_when_revoked_with_distinct_code() {
        let mut t = token([1u8; 32]);
        t.mint_authority = None;
        let st = state_with(&t);
        let err = require_mint_authority(&st, &signer([1u8; 32], true)).unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_MINT_REVOKED));
    }

    // ── mint ──
    #[test]
    fn authority_can_mint_and_supply_increases() {
        let mut st = state_with(&token([1u8; 32]));
        let mut hold = empty_holding();
        mint(
            &mut st,
            &mut hold,
            &signer([1u8; 32], true),
            AccountId::new(TOKEN_ID),
            50,
        )
        .unwrap();
        assert_eq!(TokenState::load(&st.account).unwrap().total_supply, 50);
        let h = TokenHolding::load(&hold.account).unwrap();
        assert_eq!(h.balance, 50);
        assert_eq!(h.token, Some(AccountId::new(TOKEN_ID)));
    }

    #[test]
    fn minting_is_additive_across_calls() {
        let mut st = state_with(&token([1u8; 32]));
        let mut hold = empty_holding();
        let a = signer([1u8; 32], true);
        mint(&mut st, &mut hold, &a, AccountId::new(TOKEN_ID), 10).unwrap();
        mint(&mut st, &mut hold, &a, AccountId::new(TOKEN_ID), 5).unwrap();
        assert_eq!(TokenState::load(&st.account).unwrap().total_supply, 15);
        assert_eq!(TokenHolding::load(&hold.account).unwrap().balance, 15);
    }

    #[test]
    fn non_authority_cannot_mint_and_supply_unchanged() {
        let mut st = state_with(&token([1u8; 32]));
        let before = st.account.clone();
        let mut hold = empty_holding();
        let err = mint(
            &mut st,
            &mut hold,
            &signer([2u8; 32], true),
            AccountId::new(TOKEN_ID),
            50,
        )
        .unwrap_err();
        assert!(matches!(err, SpelError::Unauthorized { .. }));
        assert_eq!(st.account, before, "rejected mint must not mutate state");
        assert!(hold.account.data.as_ref().is_empty());
    }

    #[test]
    fn minting_with_revoked_authority_is_rejected_deterministically() {
        let mut t = token([1u8; 32]);
        t.mint_authority = None;
        let mut st = state_with(&t);
        let before = st.account.clone();
        let mut hold = empty_holding();
        let err = mint(
            &mut st,
            &mut hold,
            &signer([1u8; 32], true),
            AccountId::new(TOKEN_ID),
            1,
        )
        .unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_MINT_REVOKED));
        assert_eq!(st.account, before, "fixed supply must not change");
    }

    #[test]
    fn mint_overflow_is_rejected_and_state_unchanged() {
        let mut t = token([1u8; 32]);
        t.total_supply = u128::MAX;
        let mut st = state_with(&t);
        let before = st.account.clone();
        let mut hold = empty_holding();
        let err = mint(
            &mut st,
            &mut hold,
            &signer([1u8; 32], true),
            AccountId::new(TOKEN_ID),
            1,
        )
        .unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_SUPPLY_OVERFLOW));
        assert_eq!(st.account, before);
    }

    #[test]
    fn mint_rejects_holding_bound_to_another_token() {
        let mut st = state_with(&token([1u8; 32]));
        let mut hold = empty_holding();
        TokenHolding {
            token: Some(AccountId::new([42u8; 32])),
            balance: 0,
        }
        .store(&mut hold.account)
        .unwrap();
        let err = mint(
            &mut st,
            &mut hold,
            &signer([1u8; 32], true),
            AccountId::new(TOKEN_ID),
            1,
        )
        .unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_MISMATCHED_TOKEN));
    }

    // ── rotate ──
    #[test]
    fn rotate_moves_authority_when_called_by_authority() {
        let mut st = state_with(&token([1u8; 32]));
        rotate_authority(&mut st, &signer([1u8; 32], true), AccountId::new([2u8; 32])).unwrap();
        assert_eq!(
            TokenState::load(&st.account).unwrap().mint_authority,
            Some(AccountId::new([2u8; 32]))
        );
        // new authority can mint, old cannot
        assert!(require_mint_authority(&st, &signer([2u8; 32], true)).is_ok());
        assert!(require_mint_authority(&st, &signer([1u8; 32], true)).is_err());
    }

    #[test]
    fn rotate_rejected_for_non_authority_leaves_state_unchanged() {
        let mut st = state_with(&token([1u8; 32]));
        let before = st.account.clone();
        let err = rotate_authority(&mut st, &signer([2u8; 32], true), AccountId::new([3u8; 32]))
            .unwrap_err();
        assert!(matches!(err, SpelError::Unauthorized { .. }));
        assert_eq!(st.account, before);
    }

    #[test]
    fn rotate_rejects_a_null_new_authority_and_state_unchanged() {
        let mut st = state_with(&token([1u8; 32]));
        let before = st.account.clone();
        let err = rotate_authority(&mut st, &signer([1u8; 32], true), AccountId::new([0u8; 32]))
            .unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_NULL_AUTHORITY));
        assert_eq!(
            st.account, before,
            "atomic: null candidate leaves old authority"
        );
    }

    // ── revoke ──
    #[test]
    fn revoke_fixes_supply_and_blocks_minting() {
        let mut st = state_with(&token([1u8; 32]));
        revoke_authority(&mut st, &signer([1u8; 32], true)).unwrap();
        assert_eq!(TokenState::load(&st.account).unwrap().mint_authority, None);
        let mut hold = empty_holding();
        let err = mint(
            &mut st,
            &mut hold,
            &signer([1u8; 32], true),
            AccountId::new(TOKEN_ID),
            1,
        )
        .unwrap_err();
        assert!(matches!(err, SpelError::Custom { code, .. } if code == ERR_MINT_REVOKED));
    }

    #[test]
    fn revoke_rejected_for_non_authority() {
        let mut st = state_with(&token([1u8; 32]));
        let before = st.account.clone();
        let err = revoke_authority(&mut st, &signer([2u8; 32], true)).unwrap_err();
        assert!(matches!(err, SpelError::Unauthorized { .. }));
        assert_eq!(st.account, before);
    }

    #[test]
    fn exactly_one_authority_at_a_time_after_rotation() {
        let mut st = state_with(&token([1u8; 32]));
        rotate_authority(&mut st, &signer([1u8; 32], true), AccountId::new([2u8; 32])).unwrap();
        assert!(require_mint_authority(&st, &signer([1u8; 32], true)).is_err());
        assert!(require_mint_authority(&st, &signer([2u8; 32], true)).is_ok());
    }

    #[test]
    fn state_round_trips_through_account_data() {
        let st = TokenState::new("Round".into(), 2, 7, AccountId::new([7u8; 32]));
        let mut acc = Account::default();
        st.store(&mut acc).unwrap();
        assert_eq!(TokenState::load(&acc).unwrap(), st);
    }

    #[test]
    fn error_codes_are_in_the_mint_namespace() {
        assert_eq!(ERR_NULL_AUTHORITY, 3001);
        assert_eq!(ERR_ALREADY_INITIALIZED, 3002);
        assert_eq!(ERR_MINT_REVOKED, 3003);
        assert_eq!(ERR_SUPPLY_OVERFLOW, 3004);
        assert_eq!(ERR_MISMATCHED_TOKEN, 3005);
    }
}
