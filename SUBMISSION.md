# LP-0013 submission

This file holds (1) the exact content to put in `solutions/LP-0013.md` in the
`logos-co/lambda-prize` repo, and (2) the step-by-step PR process. The text below
follows the prize's `solutions/LP-0000.md` template verbatim.

Two placeholders must be filled before opening the PR:

- `<REPO_URL>` — the public repository URL. This deliverable currently lives inside
  a monorepo; push `companies/logos/lp-0013-mint-authority/` to its own public repo
  first (see "PR steps" below). The sibling RFP libs use `retraca/logos-authority-libs`;
  a natural slug here is `retraca/logos-mint-authority` (or add it to the same libs repo).
- `<VIDEO_URL>` — the narrated video walkthrough. The prize requires a *narrated*
  walkthrough (explain what/why, architecture, key decisions) that also shows the
  end-to-end flow with `RISC0_DEV_MODE=0`. The recorded terminal cast
  (`demo/demo.cast` / `demo/demo.gif`) is the demonstration material; the narration
  still has to be recorded by the builder over it (a silent screencast is explicitly
  not sufficient). Upload to YouTube/Loom and put the link here. The raw cast can
  also be published with `asciinema upload demo/demo.cast` and linked as supporting
  material.

---

## solutions/LP-0013.md (paste-ready)

```markdown
# Solution: LP-0013 — Token program mint authority for the Logos Execution Zone

**Submitted by:** Gon (Goncalo)

## Summary

A mint-authority model for fungible tokens on the LEZ: tokens are created with an
explicit mint authority, only that authority can mint new supply, the authority can
be rotated to another account, or revoked (set to `None`) which fixes the supply
forever. It mirrors SPL Token's `mintAuthority` / `SetAuthority` semantics and
reuses the standardised admin-authority approval pattern defined in RFP-001. The
access-control core is shipped as a self-sufficient, agnostic library (state +
authority logic + host SDK), with a SPEL example program, a committed deploy-ready
IDL, and two example integrations (variable supply, and fixed supply via
revocation). It was deployed and exercised end to end on a local LEZ standalone
sequencer with `RISC0_DEV_MODE=0`.

## Repository

- **Repo:** <REPO_URL>

## Approach

The minimal LEZ token program authorises minting by checking that the token
definition account itself signed the transaction, which gives no rotation and no
way to fix supply. This work replaces that with an explicit `mint_authority:
Option<AccountId>` on the token state and a gate, `require_mint_authority(state,
signer)`, that admits only the stored authority. The lifecycle is
`initialize_token` (sets the authority) → `mint` (gated) → `rotate_authority`
(atomic hand-off) → `revoke_authority` (sets `None`; supply fixed). A revoked
authority is reported with a distinct custom code so callers can tell "wrong
signer" apart from "minting is permanently closed".

Key decisions and tradeoffs:

- **`Option<AccountId>` instead of a separate "frozen" flag.** Revocation is just
  `None`, so "no authority" and "supply fixed" are the same state and cannot drift
  apart. The alternative (a boolean alongside an always-present authority) admits
  contradictory states.
- **Authority rotation/revocation are a single `store` after the gate and
  validation.** The new state is computed fully, then committed in one write, so a
  failure leaves the prior state byte-for-byte unchanged (asserted by host tests).
- **Reuse the RFP-001 approval shape, but stay agnostic.** The library does not
  depend on the admin-authority crate; it re-implements the same
  `Option<AccountId>` + `require_*` + set/rotate/revoke + null-candidate-validation
  pattern so it is self-contained, as the prize asks.

What was tried and did not work (genuine problem-space findings):

- **`cargo risczero build` cannot build this guest.** It runs the guest build in a
  Docker container whose build context is only the guest crate directory, so the
  crate's path dependency (`mint-authority = { path = "../.." }`) is unreachable
  inside the container and `cargo +risc0 fetch` fails with `failed to read
  /Cargo.toml`. (On arm64 the amd64 builder image also runs under qemu.) The guest
  is instead built bare on the host with `cargo +risc0 build --release --target
  riscv32im-risc0-zkvm-elf` and packaged into the deployable R0BF container with
  `scripts/package_r0bf.py` (which lifts the header + risc0 kernel ELF from a
  known-good `.bin` and splices in the user ELF; it self-tests by reproducing the
  reference `.bin` byte-for-byte). The result validates with `spel program-id`.
- **The first on-chain `mint_to` was rejected as `InvalidProgramBehavior
  (ClaimedNonDefaultAccount)`.** The holding PDA is created by `create_token`, so by
  the time `mint_to` runs it is already program-owned; unconditionally *claiming* it
  is rejected (you may only claim a default/unowned account). Fixed by computing a
  conditional claim (claim only when the holding is still default-owned, mutate
  otherwise), the same pattern RFP-002 uses for its per-account freeze marker. Host
  tests did not catch this because they construct accounts directly and bypass the
  on-chain claim/ownership semantics.

### Why the Logos stack

The mint authority is an on-chain access-control rule whose value is that nobody,
including the issuer, can bypass or forge it. On LEZ each instruction is executed
in a RISC Zero zkVM and the state transition is verifiable, so "only the current
mint authority may mint, and a revoked authority can never mint again" is enforced
by trustless execution rather than by trusting an operator. On a centralised
issuer the same API could be silently overridden (mint despite a "revoked"
authority, or front-run a rotation); that guarantee is exactly what would be lost.

## Success Criteria Checklist

- [x] Mint authority set at initialization: `initialize_token` / program
  `create_token` (verified on-chain; tests `initialize_records_the_authority_and_supply`).
- [x] Minting by the authority: `mint` + `require_mint_authority` / `mint_to`
  (on-chain `mint 100` then `mint 50` → supply 150; `minting_is_additive_across_calls`).
- [x] Authority rotation: `rotate_authority` (on-chain `rotate A→B`, then the new
  authority B mints and former authority A is rejected; `exactly_one_authority_at_a_time_after_rotation`).
- [x] Revocation fixes supply: `revoke_authority` sets `None`
  (`revoke_fixes_supply_and_blocks_minting`).
- [x] Two example integrations: `examples/variable-supply-mint-authority.md`,
  `examples/fixed-supply-revoked-authority.md` (both run by the demo).
- [x] Self-sufficient agnostic approval library per RFP-001: the `mint-authority`
  crate (no dependency on admin-authority).
- [x] Module/SDK + IDL via SPEL: `TokenState`/`TokenHolding` `store`/`load` + typed
  constructors; committed `spel_token.idl.json`.
- [x] Rotation/revocation atomic: single `store`; failure leaves prior state
  unchanged (tests assert byte-for-byte).
- [x] Revoked-authority mint rejected with a documented code: on-chain `Program
  error [9003]` (`ERR_MINT_REVOKED`, custom 3003); wrong-signer is `[1008]`.
- [x] CU cost documented for mint/rotate/revoke: `docs/CU_COST.md`, measured from
  the `RISC0_DEV_MODE=0` run.
- [x] Deployed and tested on a LEZ standalone sequencer with `RISC0_DEV_MODE=0`:
  `demo/` (cast + gif + sequencer log).
- [x] Reproducible demo script: `scripts/demo.sh` / `demo/run_demo.sh`.
- [ ] CI green on the default branch: host tests + fmt + clippy + the for-target
  guest build are green locally; confirm once pushed and Actions runs.
- [ ] Narrated video demo: terminal flow recorded (`demo/demo.cast`); narration to
  be recorded over it by the builder.

## FURPS Self-Assessment

### Functionality
Create token with a mint authority, mint (gated, additive), rotate authority
(atomic hand-off, exactly one authority at a time), revoke (fixes supply). Both
example integrations run on-chain. Limitation: no burn / transfer in this scope
(out of the prize); minting targets a single holding account per call.

### Usability
One `spel` call per operation against the committed IDL. The `mint-authority` crate
is the host SDK: read token/holding state with `load`, build payloads with the
typed constructors, without re-deriving the borsh layout. `scripts/demo.sh` runs
the whole flow from a clean checkout.

### Reliability
Rotation/revocation commit in a single `store` after gate + validation, so partial
failures cannot leave a half-updated authority (host tests assert unchanged state).
Revoked minting is rejected deterministically with a distinct documented code
(`9003`), separate from the wrong-signer code (`1008`); both were observed on-chain
and rejected transactions wrote no state.

### Performance
Measured per-operation cycle counts (real `RISC0_DEV_MODE=0` execution) in
`docs/CU_COST.md`: revoke ~100.6k user cycles < rotate ~124.2k < mint ~187.9–191.3k
< create_token ~205.8–208.8k. A large fixed cost (zkVM bootstrap + SPEL input
deserialization) dominates; the authority model's marginal cost is the small delta
between operations. The model adds 33 bytes to token state and 0 extra accounts to
any transaction.

### Supportability
24 library + 8 example host tests pass; fmt and clippy clean; the guest compiles
for the `riscv32im-risc0-zkvm-elf` target (ELF checked in). README has deploy
steps, program image id, PDA addresses, and CLI usage; `docs/AUTHORITY_MODEL.md`
documents semantics and error codes; `DELIVERABLES.md` maps each criterion to
code/tests/evidence. The R0BF packaging workaround is scripted and self-testing.

## Supporting Materials

- Recorded terminal demo: `demo/demo.cast`, `demo/demo.gif`, transcript
  `demo/demo.txt`, sequencer log `demo/sequencer.log`.
- Narrated walkthrough: <VIDEO_URL>
- Design docs: `docs/AUTHORITY_MODEL.md`, `docs/CU_COST.md`.
- Per-criterion mapping: `DELIVERABLES.md`.

## Terms & Conditions

By submitting this solution, I confirm that I have read and agree to the
[Terms & Conditions](../TERMS.md).
```

---

## PR steps (logos-co/lambda-prize)

1. **Publish the implementation repo.** Push
   `companies/logos/lp-0013-mint-authority/` to a public repo, e.g.
   `retraca/logos-mint-authority`, dual-licensed MIT OR Apache-2.0 (license files
   are already in the tree). Commit the `demo/` artifacts. Set `<REPO_URL>` above
   to that URL.
2. **Record + upload the narrated video.** Narrate over the `demo/demo.cast`
   playback (or re-run `demo/run_demo.sh` live) explaining the authority model,
   the R0BF packaging workaround, and the on-chain rejections, with the terminal
   showing `RISC0_DEV_MODE=0`. Upload and set `<VIDEO_URL>`.
3. **Confirm CI is green** on the implementation repo's default branch (host
   tests + fmt + clippy + for-target guest build) once Actions has run.
4. **Fork and branch** `logos-co/lambda-prize`:
   ```bash
   gh repo fork logos-co/lambda-prize --clone
   cd lambda-prize
   git checkout -b solution-lp-0013
   ```
5. **Add the solution file** `solutions/LP-0013.md` with the paste-ready content
   above (placeholders filled). Read `solutions/LP-0000.md` and `TERMS.md` in the
   repo first to confirm the template/terms have not changed.
6. **Open the PR**, titled exactly:
   `Solution: LP-0013 — Token program mint authority for the Logos Execution Zone`
   ```bash
   git add solutions/LP-0013.md
   git commit -m "Solution: LP-0013 — Token program mint authority for the LEZ"
   git push -u origin solution-lp-0013
   gh pr create --repo logos-co/lambda-prize \
     --title "Solution: LP-0013 — Token program mint authority for the Logos Execution Zone" \
     --body-file <(sed -n '/^# Solution: LP-0013/,/Terms & Conditions/p' solutions/LP-0013.md)
   ```
7. **After the PR merges**, file the payment claim using the Lambda Prize payment
   issue template (do not file it before merge). Limits: max 3 submissions per
   prize per builder, at most one submission/review per week.
