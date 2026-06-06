# Example integration: fixed supply with revoked authority

A token whose entire supply is minted at creation and can never grow, achieved by
revoking the mint authority immediately after creation. This is the LEZ equivalent
of an SPL token with its mint authority set to `null`.

## Flow

```
create_token(name="FIX", initial_supply=1000, authority=A)   # A holds 1000, supply=1000
revoke_authority(name="FIX", current_authority=A)            # mint_authority -> None
mint_to(name="FIX", amount=1, authority=A)                   # REJECTED: ERR_MINT_REVOKED (9003)
```

After `revoke_authority`, `TokenState.mint_authority == None`. Every subsequent
`mint_to` is rejected deterministically with `ERR_MINT_REVOKED` (custom code 3003,
on-chain program error 9003), and total supply stays fixed at 1000 forever. No
account, including the original creator A, can ever mint again.

## CLI

```bash
spel --idl spel_token.idl.json --program spel_token.bin -- \
  create_token --name FIX --holder 0x<A_hex> --decimals 0 --initial-supply 1000 --authority <A>

spel --idl spel_token.idl.json --program spel_token.bin -- \
  revoke_authority --name FIX --current-authority <A>

# this now fails with program error 9003 (mint authority revoked; supply fixed):
spel --idl spel_token.idl.json --program spel_token.bin -- \
  mint_to --name FIX --holder 0x<A_hex> --amount 1 --authority <A>
```

## Host test

`revoke_then_mint_is_rejected_with_documented_code` in
`mint-authority/examples/spel-token/src/lib.rs` exercises exactly this flow and
asserts the `ERR_MINT_REVOKED` code. The library test
`revoke_fixes_supply_and_blocks_minting` proves the same at the library level.
