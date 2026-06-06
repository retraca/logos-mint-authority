# Example integration: variable supply with mint authority

A token that starts empty (or small) and grows over time under the control of a
mint authority, with the authority optionally rotated to a treasury or governance
account. This is the LEZ equivalent of an SPL token that keeps an active mint
authority for ongoing or permissioned issuance.

## Flow

```
create_token(name="VAR", initial_supply=0, authority=A)   # supply=0, mint_authority=A
mint_to(name="VAR", amount=100, authority=A)              # supply=100
mint_to(name="VAR", amount=50,  authority=A)              # supply=150
rotate_authority(name="VAR", new_authority=B, current_authority=A)  # mint_authority=B
mint_to(name="VAR", amount=1, authority=A)                # REJECTED: A is no longer authority (1008)
mint_to(name="VAR", amount=25, authority=B)               # supply=175 (B can mint)
```

The authority can mint repeatedly, growing the supply. `rotate_authority` hands the
mint right to a new account atomically: after it, the former authority A is locked
out (`Unauthorized`, 1008) and the new authority B can mint. There is exactly one
mint authority at any time. If at any later point the holder wants to fix the
supply, `revoke_authority` (see the fixed-supply example) converts this into a
fixed-supply token.

## CLI

```bash
spel --idl spel_token.idl.json --program spel_token.bin -- \
  create_token --name VAR --holder 0x<A_hex> --decimals 6 --initial-supply 0 --authority <A>

spel --idl spel_token.idl.json --program spel_token.bin -- \
  mint_to --name VAR --holder 0x<A_hex> --amount 100 --authority <A>

spel --idl spel_token.idl.json --program spel_token.bin -- \
  rotate_authority --name VAR --new-authority <B> --current-authority <A>

# B (the new authority) can now mint; A can no longer:
spel --idl spel_token.idl.json --program spel_token.bin -- \
  mint_to --name VAR --holder 0x<B_hex> --amount 25 --authority <B>
```

## Host tests

`authority_can_mint_more_supply` and `rotate_hands_minting_to_the_new_authority` in
`mint-authority/examples/spel-token/src/lib.rs` exercise this flow. The library
tests `minting_is_additive_across_calls`, `rotate_moves_authority_when_called_by_authority`,
and `exactly_one_authority_at_a_time_after_rotation` prove the underlying behavior.
