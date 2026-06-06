# End-to-end demo (RISC0_DEV_MODE=0)

The flow was run on 2026-06-05 on a local LEZ standalone sequencer
(`sequencer_service --features standalone`, LEZ `nssa_core` tag `v0.2.0-rc3`),
arm64 macOS, with `RISC0_DEV_MODE=0` (real guest execution and proof generation,
not dev mode).

## Files

- `run_demo.sh`: the reproducible driver that produced the run. It resets the
  local chain, starts the sequencer with `RISC0_INFO=1`, deploys the R0BF `.bin`,
  and runs both example integrations end to end.

The raw terminal recordings from that run (asciinema cast, rendered GIF,
plain-text transcript, and the sequencer log) are deliberately **not** committed:
they captured the local wallet seed phrase used to sign the demo transactions, so
publishing them would leak a key. The reproducible driver above plus the narrated
video walkthrough are the published evidence. The measured per-operation cycle
counts pulled from that run's sequencer log are recorded in `../docs/CU_COST.md`.

## What the run shows

Program image id `34f3497a60dee9fb1f51d7109447336b26f041175157240f948bdfa86e148155`.

Integration 1 (variable supply with mint authority):
`create_token VAR` → `mint 100` → `mint 50` (supply 150) → `rotate A→B` →
`mint by old A` **rejected `[1008]` Unauthorized** → `mint by new B` ok.

Integration 2 (fixed supply with revoked authority):
`create_token FIX` (supply 1000) → `revoke_authority` → `mint after revoke`
**rejected `[9003]` `ERR_MINT_REVOKED` (custom 3003)**.

Each successful transaction is confirmed ("included in a block"); the two negative
cases are rejected by the sequencer and never mutate state.

Note: the demo waits ~12s per transaction for block confirmation, so the run has
real idle gaps between operations.
