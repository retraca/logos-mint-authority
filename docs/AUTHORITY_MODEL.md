# Mint authority model and lifecycle

This document specifies the authority semantics for the LP-0013 token program. It
is the design contract the implementation and tests follow.

## What a mint authority is

A token's mint authority is a single account that is allowed to create new supply.
It is stored in the token's on-chain state next to the supply:

```rust
pub struct TokenState {
    pub name: String,
    pub decimals: u8,
    pub total_supply: u128,
    pub mint_authority: Option<AccountId>, // None == revoked == supply fixed
}
```

`mint_authority` is an `Option<AccountId>`. `Some(a)` means `a` may mint; `None`
means the authority has been revoked and the supply is permanently fixed. This is
the same shape as the RFP-001 admin-authority config (`admin: Option<AccountId>`),
deliberately, so the access-control reasoning is identical and auditable.

## Lifecycle

```
create_token(authority = A)          state.mint_authority = Some(A)
        │
        ├── mint_to (signer A)        total_supply += amount        [variable supply]
        │
        ├── rotate_authority(B)       state.mint_authority = Some(B) (A locked out)
        │
        └── revoke_authority()        state.mint_authority = None    [supply fixed]
                  │
                  └── mint_to         REJECTED: ERR_MINT_REVOKED (deterministic)
```

There is always exactly one mint authority, or none. There is no multi-authority
or pending-transfer intermediate state.

## The gate

Every privileged operation calls `require_mint_authority(state, signer)` first. It
returns `Ok(state)` only when both hold:

1. `signer.is_authorized` is true (the transaction was actually signed by that
   account: on LEZ a public account is authorized iff the tx carries its
   signature), and
2. `signer.account_id == state.mint_authority`.

Authorization is necessary but not sufficient: a validly signed transaction from a
non-authority is still rejected.

## Outcomes and error codes

| Situation | Result | Code |
|---|---|---|
| Authorized authority mints / rotates / revokes | Ok | (none) |
| Signer did not sign the tx | `Unauthorized` | 1008 |
| Signer signed but is not the authority | `Unauthorized` | 1008 |
| Authority is revoked (`None`), mint attempted | `Custom(ERR_MINT_REVOKED)` | 3003 → on-chain 9003 |
| New authority candidate is the null account | `Custom(ERR_NULL_AUTHORITY)` | 3001 → 9001 |
| Re-initialize an existing token | `Custom(ERR_ALREADY_INITIALIZED)` | 3002 → 9002 |
| Supply or balance would overflow `u128` | `Custom(ERR_SUPPLY_OVERFLOW)` | 3004 → 9004 |
| Holding belongs to a different token | `Custom(ERR_MISMATCHED_TOKEN)` | 3005 → 9005 |

`SpelError::Custom { code }` is reported on-chain as `6000 + code` (see
`SpelError::error_code` in the SPEL framework), so the mint-namespace 3000 codes
surface as 9000-range program errors. `Unauthorized` is the framework's canonical
`1008`. The "revoked" case has a code distinct from the "wrong signer" case so a
caller can tell "you may not mint" from "nobody may mint, ever". This is the
documented deterministic rejection required by the prize.

Error codes are namespaced in the 3000 range to stay clear of admin-authority
(1000) and freeze-authority (2000), so the three libraries compose in one program
without code collisions.

## Atomicity

Rotation and revocation each rewrite the token-state account in a single `store`
after the gate and validation pass. The mutated value is computed in full first;
the account is written only once, at the end. So any failure (wrong signer, null
candidate, serialization) returns `Err` having written nothing, and the authority
is left in its prior state, never an undefined or half-updated one. The library
tests assert byte-for-byte equality of the state account before and after every
rejected operation (`rotate_rejected_for_non_authority_leaves_state_unchanged`,
`rotate_rejects_a_null_new_authority_and_state_unchanged`,
`revoke_rejected_for_non_authority`, `non_authority_cannot_mint_and_supply_unchanged`,
`minting_with_revoked_authority_is_rejected_deterministically`,
`mint_overflow_is_rejected_and_state_unchanged`).

On chain the same property holds end to end: a transaction whose program returns
`Err` produces no post-state, so the sequencer commits no account changes. A
rejected mint leaves total supply and every balance untouched.

## Relationship to RFP-001 (admin authority)

The prize asks for a "self-sufficient, agnostic library that handles approval as
defined in RFP-001". The mint authority is the RFP-001 admin-authority pattern
applied to a token's mint:

| RFP-001 admin-authority | mint-authority (this library) |
|---|---|
| `AdminConfig { admin: Option<AccountId> }` | `TokenState { mint_authority: Option<AccountId>, .. }` |
| `require_admin(config, signer)` | `require_mint_authority(state, signer)` |
| `initialize_admin` | `initialize_token` (sets the authority) |
| `transfer_admin(new)` | `rotate_authority(new)` |
| `revoke_admin()` → `None` | `revoke_authority()` → `None` (supply fixed) |
| `validate_admin_candidate` (reject null) | `validate_authority_candidate` (reject null) |
| codes in the 1000 range | codes in the 3000 range |

This crate does not depend on the `admin-authority` crate. It re-implements the
same small, audited pattern locally so a token program can adopt mint authority
without pulling in the unrelated admin surface, which is what "self-sufficient,
agnostic" requires. The pattern, not the dependency, is what is reused.

## A note on "on-curve" validation

RFP-001 phrases its candidate validation in Solana terms (reject keys not on the
ed25519 curve). LEZ has no such distinction: every `AccountId` is a 32-byte hash
output (user accounts from a nullifier public key, PDAs from
`SHA256(prefix || program_id || seed)`). The one verifiable safety check that
remains is rejecting the null (all-zero) account, which nobody can sign for and
which would silently disable minting. Both real signer accounts and program PDAs
are accepted as authorities.
