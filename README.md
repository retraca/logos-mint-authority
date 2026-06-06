# LP-0013: Token program mint authority for the Logos Execution Zone

A mint-authority model for fungible tokens on the Logos Execution Zone (LEZ),
built for Logos Lambda Prize [LP-0013](https://github.com/logos-co/lambda-prize/blob/master/prizes/LP-0013.md).
It adds controlled, real-world token issuance to the minimal LEZ token example:
variable supply, permissioned minting, authority rotation, and revocation that
makes the supply permanently fixed. The access-control core reuses the
standardised admin-authority pattern from
[RFP-001](https://github.com/logos-co/rfp/blob/master/RFPs/RFP-001-admin-authority-lib.md).

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE-v2).

## What this is

The minimal LEZ token program authorises minting by checking that the token
definition account itself signed the transaction. That offers no rotation and no
way to fix the supply. This work replaces it with an explicit **mint authority**,
mirroring SPL Token's `mintAuthority` / `SetAuthority` semantics:

- the mint authority is set when the token is created,
- only that authority may mint new supply,
- the authority can be rotated to another account, or
- revoked (set to `None`), which fixes the supply forever.

```
companies/logos/lp-0013-mint-authority/
  mint-authority/                       the reusable library (state + authority logic + SDK)
    src/lib.rs
    examples/
      spel-token/                       the SPEL token program (host crate, host tests, IDL)
        src/lib.rs
        spel_token.idl.json             the committed, deploy-ready IDL
      spel-token-guest/                 detached guest crate -> on-chain riscv32im ELF
        src/bin/spel_token.rs
        bin/spel_token.elf              prebuilt guest ELF (verified to compile for-target)
  docs/
    AUTHORITY_MODEL.md                  authority semantics, lifecycle, error codes, atomicity
    CU_COST.md                          compute-unit / cycle cost of the new operations
  examples/
    fixed-supply-revoked-authority.md   example integration 1
    variable-supply-mint-authority.md   example integration 2
  demo/run_demo.sh                      reproducible end-to-end demo (RISC0_DEV_MODE=0)
  scripts/demo.sh                       from-scratch build variant (Docker caveat, see usage)
  DELIVERABLES.md                       per-criterion mapping to code, tests, evidence
```

## The library and the SDK

`mint-authority` (`mint-authority/src/lib.rs`) is the self-sufficient, agnostic
library. It is both the on-chain authority logic and the host-side SDK for building
Logos modules that interact with the token program:

- on-chain state: `TokenState { name, decimals, total_supply, mint_authority: Option<AccountId> }`
  and `TokenHolding { token, balance }`, each with `store`/`load`;
- the gate: `require_mint_authority(state, signer)`;
- the operations: `initialize_token`, `mint`, `rotate_authority`, `revoke_authority`;
- validation: `validate_authority_candidate` (rejects the null account);
- the error codes used by the program (`ERR_MINT_REVOKED`, etc.).

A module builder uses `TokenState::load` / `TokenHolding::load` to read token state
off accounts, and the typed constructors to build instruction payloads, without
re-deriving the borsh layout by hand. The committed IDL
(`mint-authority/examples/spel-token/spel_token.idl.json`) drives the `spel` CLI
and any generated client.

See [docs/AUTHORITY_MODEL.md](docs/AUTHORITY_MODEL.md) for the full semantics.

## Building and testing

The libraries build against the LEZ `nssa_core` (tag `v0.2.0-rc3`) and the
[SPEL framework](https://github.com/logos-co/spel). Building `nssa_core` needs the
LEZ ZK circuits installed once (CI does this automatically; see `.github/workflows/ci.yml`):

```bash
# one-time: install the LEZ ZK circuits (v0.4.2) into ~/.logos-blockchain-circuits
# (see the CI workflow for the exact release-download steps)

cargo test --workspace                  # 31 host tests (24 library + 7 example)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Build the on-chain guest ELF (no Docker), proving the program and the library
compile for the zkVM target:

```bash
cd mint-authority/examples/spel-token-guest
cargo +risc0 build --release --target riscv32im-risc0-zkvm-elf
# -> target/riscv32im-risc0-zkvm-elf/release/spel_token  (RISC-V ELF, also checked into bin/)
```

## End-to-end usage (deploy, mint, rotate, revoke)

The reproducible end-to-end flow runs against a real local LEZ sequencer in
standalone mode with `RISC0_DEV_MODE=0` (real proof generation), using the
deployable `.bin` checked into this repo. One command:

```bash
LEZ_DIR=../lez-build demo/run_demo.sh
```

`demo/run_demo.sh` deploys the committed program binary, creates two accounts,
resets the local chain to a clean state each run, and runs both example
integrations end to end, showing the revoked-authority mint being rejected. The
deployable `.bin` is packaged with `scripts/package_r0bf.py`: the `cargo
risczero build` Docker path does not resolve this guest's path dependency, so the
R0BF binary is packaged directly (see `scripts/demo.sh` for the from-scratch
build variant). Requirements:

- the `spel` CLI on `PATH`,
- a `logos-execution-zone` checkout (rev `cf3639d` / tag `v0.2.0-rc3`) at `$LEZ_DIR`,
- the risc0 toolchain (`rzup install rust 1.94.0`),
- `python3` with `base58` (for the holder-hex conversion in the demo).

### Manual CLI walkthrough

After `demo.sh` has deployed the program (or following the deploy steps in
[docs/AUTHORITY_MODEL.md](docs/AUTHORITY_MODEL.md) and the script), each operation
is a single `spel` call. `name` selects the token; `holder` is a 32-byte account id
passed as `0x`-hex (it is a PDA seed); the signer is passed by the IDL account name
(`--authority`, `--current-authority`) as a base58 account id the wallet can sign for:

```bash
IDL=mint-authority/examples/spel-token/spel_token.idl.json
BIN=<deployable spel_token .bin>

# create a token with A as mint authority, minting 1000 to A
spel --idl $IDL --program $BIN -- create_token \
  --name USD --holder 0x<A_hex> --decimals 6 --initial-supply 1000 --authority <A>

# mint 500 more to A
spel --idl $IDL --program $BIN -- mint_to \
  --name USD --holder 0x<A_hex> --amount 500 --authority <A>

# rotate the mint authority to B
spel --idl $IDL --program $BIN -- rotate_authority \
  --name USD --new-authority <B> --current-authority <A>

# revoke the mint authority (supply becomes fixed)
spel --idl $IDL --program $BIN -- revoke_authority \
  --name USD --current-authority <B>
```

### Program addresses (from the recorded `RISC0_DEV_MODE=0` run, 2026-06-05)

Measured on a local LEZ standalone sequencer (`nssa_core` tag `v0.2.0-rc3`):

- **Program image id:** `34f3497a60dee9fb1f51d7109447336b26f041175157240f948bdfa86e148155`
- Accounts: A (authority) `GbunAzPxrEeJK6y7HbN22QPrzjReDsQ9PzUvtFstEBSa`, B (treasury) `9VpQd8DBLsBCuaishL4xvch84EwuTxQKuxnCiNn2gC2S`
- `VAR` token: `state` PDA `7JXGyNmUsRqsLQCzq3n9s7RJM8eVWCE8Nd8J8C2z4nth`; `holding` PDAs `5sAnoVkBT9HrrnG6hbnwZzMWEJqvXMxav5q1UEshSYAG` (A), `8Q6X4YnZBdujkkWMCcoinRdRy8txUrfAfecvVXHhfRpY` (B)
- `FIX` token: `state` PDA `FB7aPxW1aCcvcqJHncfoCJhzG6ECS3GaTeijAabsHsYm`; `holding` PDA `38MVo1eEK3CVyMu28xVENyKywoJsUhGQqMgJNMK5zuAZ`

The two negative cases were rejected on-chain with the documented codes: minting by
the former authority after a rotation returns `Program error [1008]` (Unauthorized),
and minting after `revoke_authority` returns `Program error [9003]` (`ERR_MINT_REVOKED`,
custom code 3003). The flow is reproduced by `demo/run_demo.sh` against a real local
sequencer, and is shown end to end in the narrated video walkthrough (see
[demo/README.md](demo/README.md)). Measured per-operation cycle counts are in
[docs/CU_COST.md](docs/CU_COST.md).

## Status of verification

What is verified in this repository:

- 31 host tests pass (`cargo test --workspace`); fmt and clippy are clean.
- the guest compiles for the on-chain `riscv32im-risc0-zkvm-elf` target (the ELF is
  checked into `mint-authority/examples/spel-token-guest/bin/`).
- the committed IDL parses and describes all four instructions and both account types.

What was exercised on a local LEZ standalone sequencer with `RISC0_DEV_MODE=0`
(reproduced by `demo/run_demo.sh` and captured in the narrated video walkthrough,
addresses above):

- the deployable program `.bin` (built and validated with `spel program-id`),
- deploy + both example integrations end to end (create / mint / rotate / revoke),
- the two rejections with their documented codes (`1008`, `9003`),
- the measured per-operation cycle counts in [docs/CU_COST.md](docs/CU_COST.md).

Note on the deployable `.bin`: the prize's suggested `cargo risczero build` runs
the guest build inside a Docker container whose build context is only the guest
crate directory, so this crate's path dependency (`mint-authority = { path =
"../.." }`) is unreachable inside the container and the build fails. The guest is
instead built bare on the host (`cargo +risc0 build`, no Docker) and packaged into
the deployable R0BF container with `scripts/package_r0bf.py`, which the demo uses.

See [DELIVERABLES.md](DELIVERABLES.md) for the full per-criterion mapping.
