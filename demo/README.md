# Recorded end-to-end demo (RISC0_DEV_MODE=0)

Captured 2026-06-05 on a local LEZ standalone sequencer (`sequencer_service
--features standalone`, LEZ `nssa_core` tag `v0.2.0-rc3`), arm64 macOS, with
`RISC0_DEV_MODE=0` (real guest execution, not dev mode).

## Files

- `demo.cast` — asciinema v3 recording of the full terminal session.
- `demo.gif` — rendered animation of the same (via `agg`).
- `demo.txt` — plain-text transcript of the terminal output.
- `sequencer.log` — the sequencer's log for the run: per-execution zkVM cycle
  counts (`RISC0_INFO=1`) and the on-chain rejection lines with their program
  error codes.
- `run_demo.sh` — the script that produced the recording (resets the local chain,
  starts the sequencer, deploys the R0BF `.bin`, runs both integrations).

## What the run shows

Program image id `34f3497a60dee9fb1f51d7109447336b26f041175157240f948bdfa86e148155`.

Integration 1 (variable supply with mint authority):
`create_token VAR` → `mint 100` → `mint 50` (supply 150) → `rotate A→B` →
`mint by old A` **rejected `[1008]` Unauthorized** → `mint by new B` ok.

Integration 2 (fixed supply with revoked authority):
`create_token FIX` (supply 1000) → `revoke_authority` → `mint after revoke`
**rejected `[9003]` `ERR_MINT_REVOKED` (custom 3003)**.

Each successful transaction is confirmed ("included in a block"); the two negative
cases are rejected by the sequencer and never mutate state. The exact rejection
codes are in `sequencer.log` (`Program error [1008]` and `Program error [9003]`).

Note on playback: the demo waits ~12s per transaction for block confirmation, so
the cast has real idle gaps. Measured per-operation cycle counts derived from this
run's `sequencer.log` are in `../docs/CU_COST.md`.
