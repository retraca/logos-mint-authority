# Deliverables

Per-criterion delivery mapping for [LP-0013](https://github.com/logos-co/lambda-prize/blob/master/prizes/LP-0013.md).
Each row links the success criterion to where it is implemented, the test that
exercises it, and whether it is verified in-repo or produced by the recorded
standalone-sequencer demo.

Verify the in-repo items in one pass:

```bash
cargo test --workspace                                   # 31 host tests (24 lib + 7 example)
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cd mint-authority/examples/spel-token-guest && \
  cargo +risc0 build --release --target riscv32im-risc0-zkvm-elf   # guest compiles for-target
# end-to-end on a real sequencer (Docker + prover): ./scripts/demo.sh  (or demo/run_demo.sh; the narrated video walks this)
```

## Functionality

| Criterion | Implementation | Test |
|---|---|---|
| Mint authority set at initialization | `initialize_token` (`mint-authority/src/lib.rs`); program `create_token` | `initialize_records_the_authority_and_supply`, `create_token_records_authority_and_initial_supply` |
| Minting by the authority | `mint` + `require_mint_authority`; program `mint_to` | `authority_can_mint_and_supply_increases`, `minting_is_additive_across_calls`, `authority_can_mint_more_supply` |
| Authority rotation | `rotate_authority`; program `rotate_authority` | `rotate_moves_authority_when_called_by_authority`, `rotate_hands_minting_to_the_new_authority`, `exactly_one_authority_at_a_time_after_rotation` |
| Authority revocation (`None` = supply fixed) | `revoke_authority`; program `revoke_authority` | `revoke_fixes_supply_and_blocks_minting`, `revoke_then_mint_is_rejected_with_documented_code` |
| Two example integrations | `examples/fixed-supply-revoked-authority.md`, `examples/variable-supply-mint-authority.md` | the two flows above; demo script runs both |
| Self-sufficient, agnostic approval library per RFP-001 | the whole `mint-authority` crate (no dependency on admin-authority; same `Option<AccountId>` + `require_*` + set/rotate/revoke + null-validation pattern) | full library test suite; mapping in `docs/AUTHORITY_MODEL.md` |

## Usability

| Criterion | Implementation | Test |
|---|---|---|
| Module/SDK for interacting with the token program | `mint-authority` crate: `TokenState`/`TokenHolding` `store`/`load`, typed constructors, the operations and error codes used to build/read transactions | library tests; `state_round_trips_through_account_data` |
| IDL via the SPEL framework | auto-generated `PROGRAM_IDL_JSON` from `#[lez_program]`; committed deploy-ready `mint-authority/examples/spel-token/spel_token.idl.json` (instructions + account layouts + error codes) | `idl_exposes_all_instructions`, `committed_idl_describes_the_account_types` |

## Reliability

| Criterion | Implementation | Test |
|---|---|---|
| Rotation/revocation atomic (partial failure leaves prior state) | single `store` after gate+validate; mutate computed fully then committed | `rotate_rejected_for_non_authority_leaves_state_unchanged`, `rotate_rejects_a_null_new_authority_and_state_unchanged`, `revoke_rejected_for_non_authority`, `non_authority_cannot_mint_and_supply_unchanged`, `mint_overflow_is_rejected_and_state_unchanged` (all assert byte-for-byte unchanged state) |
| Minting with revoked authority rejected deterministically with a documented code | `require_mint_authority` returns `Custom(ERR_MINT_REVOKED = 3003)` on `None`; documented in `docs/AUTHORITY_MODEL.md` and the IDL `errors` | `gate_rejects_when_revoked_with_distinct_code`, `minting_with_revoked_authority_is_rejected_deterministically`, `revoke_then_mint_is_rejected_with_documented_code` |

## Performance

| Criterion | Implementation | Status |
|---|---|---|
| Document CU cost of mint, rotate, revoke on LEZ | `docs/CU_COST.md`: cycle-cost model + transaction-shape overhead + measured per-operation cycle counts | **DONE**. Measured on the `RISC0_DEV_MODE=0` standalone-sequencer run (revoke ~100.6k < rotate ~124.2k < mint ~187.9-191.3k < create ~205.8-208.8k user cycles), read from that run's sequencer log (not committed; held the demo wallet seed) and reproducible via `demo/run_demo.sh` |

## Supportability

| Criterion | Implementation | Status |
|---|---|---|
| Deployed and tested on LEZ devnet/testnet | deployed + both integrations run on a local LEZ standalone sequencer with `RISC0_DEV_MODE=0` | **DONE**. Image `34f3497a...`; on-chain addresses in the README, reproducible via `demo/run_demo.sh` (no public hosted sequencer exists; standalone-local is the supported path) |
| E2E integration tests against a LEZ sequencer (standalone) in CI | `scripts/demo.sh` / `demo/run_demo.sh` is the standalone-sequencer e2e; CI runs host tests + fmt + clippy + the for-target guest build | CI (`.github/workflows/ci.yml`) builds the guest for-target; the Docker+prover standalone run is heavy and runs via the demo script. See the CI honesty note below: the workflow is not yet on the public default branch |
| CI green on default branch | `.github/workflows/ci.yml` (host `test`/`fmt`/`clippy` job + `guest-build` job) | **PENDING**. The host job is verified locally (31 tests, clean fmt/clippy), but `ci.yml` is not yet pushed to the public repo (it needs a `workflow`-scope token). The builder refreshes the token, pushes the workflow, and confirms the Actions run is green. See honesty note below |
| README with deploy steps, program addresses, CLI mint/rotate/revoke | `README.md` + `docs/AUTHORITY_MODEL.md` | done |
| Reproducible e2e demo script, real local sequencer, `RISC0_DEV_MODE=0` | `scripts/demo.sh` + `demo/run_demo.sh` (export `RISC0_DEV_MODE=0`, reset chain, fail loudly on missing prereqs) | **DONE**. `demo/run_demo.sh` ran end to end against the standalone sequencer and is the published reproducible driver |
| Recorded narrated video demo showing terminal output incl. `RISC0_DEV_MODE=0` | the prize requires a *narrated* walkthrough that also shows the `RISC0_DEV_MODE=0` terminal flow; `demo/run_demo.sh` reproduces that flow | **PENDING**. The original terminal recording was made but is not committed (it captured the demo wallet seed). The builder records the narrated video over a fresh `demo/run_demo.sh` run and links it in `SUBMISSION.md` / the solution file |

## Honesty notes

- **CI green (PENDING)**: the host job (fmt, clippy, 31 tests) and the for-target
  guest build are verified locally on arm64 macOS with the LEZ circuits installed.
  The CI workflow (`.github/workflows/ci.yml`) is not yet on the public repo: it is
  gitignored in this working copy and pushing a workflow file needs a
  `workflow`-scope token. The builder refreshes the token
  (`gh auth refresh -h github.com -s workflow`), un-ignores and pushes `ci.yml`,
  and confirms the Actions run is green. Until then, "CI is green on the default
  branch" is not satisfied.
- **On-chain deploy / measured cycles / the `RISC0_DEV_MODE=0` run**: executed
  on 2026-06-05 against a local LEZ standalone sequencer (`nssa_core` v0.2.0-rc3),
  arm64 macOS. Deployable `.bin` built bare on the host and packaged with
  `scripts/package_r0bf.py` (see note below); deploy + both integrations confirmed
  on-chain; the two negative cases rejected with `Program error [1008]` and
  `[9003]`; per-operation cycle counts read from that run's sequencer log. The raw
  terminal recordings and log are not committed (they captured the demo wallet seed);
  the run is reproduced by `demo/run_demo.sh`.
- **Deployable `.bin` build path**: `cargo risczero build` does **not** work for
  this guest — its Docker build context is only the guest crate dir, so the
  `mint-authority = { path = "../.." }` path dependency is unreachable inside the
  container (`failed to read /Cargo.toml`). The guest is built bare with
  `cargo +risc0 build` and packaged into the R0BF container with
  `scripts/package_r0bf.py` (self-tests by reproducing a known-good `.bin`
  byte-for-byte; output validated with `spel program-id`).
- **`mint_to` claim fix**: on-chain testing surfaced a real defect not caught by
  host tests — `mint_to` unconditionally claimed the holding PDA, which
  `create_token` had already made program-owned, so the sequencer rejected it as
  `InvalidProgramBehavior(ClaimedNonDefaultAccount)`. Fixed with a conditional
  claim (claim only when still default-owned), the RFP-002 marker pattern.

## Remaining steps for the submitter

Done: the standalone-sequencer run, measured cycle counts in `docs/CU_COST.md`,
program addresses/image id in the README, and the public repo push. What is left:

1. **Push the CI workflow.** `.github/workflows/ci.yml` exists locally but is
   gitignored and not on the public repo; pushing it needs a `workflow`-scope
   token. Refresh the token (`gh auth refresh -h github.com -s workflow`), un-ignore
   `ci.yml`, push it, and confirm the GitHub Actions run is green on the default
   branch.
2. **Record the narrated video demo** walking through the architecture and the full
   end-to-end flow (reproduce it with `demo/run_demo.sh`), with the terminal showing
   `RISC0_DEV_MODE=0` and proof generation. A silent screencast is not sufficient.
   Link it in `SUBMISSION.md` and the staged `solutions/LP-0013.md`.
3. **Open the submission PR.** The fork and `lp-0013-solution` branch with
   `solutions/LP-0013.md` are staged on the builder's fork; once the video URL is
   filled in, run `gh pr create` per `SUBMISSION.md`.

## Licensing

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE-v2).
