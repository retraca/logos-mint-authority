# Compute unit (CU) cost of the new operations

The prize asks for the CU cost of each new operation (mint, rotate authority,
revoke authority) on LEZ devnet/testnet, and notes the per-transaction compute
budget may change during testnet.

## What "CU" means on LEZ

LEZ programs execute inside a RISC Zero zkVM and are proven, so the natural compute
measure is zkVM **cycles** (the quantity the prover charges for and the sequencer
budgets), which is the LEZ analog of a Solana compute unit. Two things drive it:

1. a fixed per-instruction overhead (program load, account deserialization, witness
   checks), which dominates, and
2. the marginal work of the handler, which for these three operations is tiny: one
   borsh decode of `TokenState`, one `Option<AccountId>` comparison, one or two
   integer ops, and one borsh encode + store.

All three new operations touch the same single `TokenState` account (mint also
touches one `TokenHolding` account) and do no loops, no hashing, and no allocation
beyond the fixed-size state buffer, so their marginal cost is small and nearly
constant. `mint` is marginally more expensive than `rotate`/`revoke` because it
also decodes, mutates, and stores the holding account.

## Transaction shape (measurable without a sequencer)

The authority model adds no extra accounts to a transaction beyond the token-state
account that any token operation already needs. Relative to the minimal token
example, the per-operation account and argument overhead is:

| Operation | Writable accounts | Signer | Extra args over a bare op | Authority overhead |
|---|---|---|---|---|
| `mint_to` | `TokenState`, `TokenHolding` | authority | `holder` (32 B), `amount` (16 B) | gate reads the existing `mint_authority` field; 0 extra accounts |
| `rotate_authority` | `TokenState` | authority | `new_authority` (32 B) | 1 account, 32-byte id arg |
| `revoke_authority` | `TokenState` | authority | (none) | 1 account, 0 args |

The `mint_authority` field adds 33 bytes to the token-state account (1 tag byte for
the `Option` plus a 32-byte `AccountId` when present, 1 byte when `None`). That is
the entire on-chain storage overhead of the model.

## Measured cycle counts

Measured on a local LEZ standalone sequencer (`sequencer_service --features
standalone`, LEZ `nssa_core` tag `v0.2.0-rc3`) with `RISC0_DEV_MODE=0`, on arm64
macOS, 2026-06-05. The deployable program image id for this run was
`34f3497a60dee9fb1f51d7109447336b26f041175157240f948bdfa86e148155`.

### How these were measured

LEZ validates a program transaction by running its guest ELF in the RISC Zero
**executor** inside `nssa::program::Program::execute` (`executor.execute(env,
elf)`). The standalone sequencer uses that same execution path. The executor's
session reports the zkVM cycle accounting; it is logged per execution when the
sequencer is run with `RISC0_INFO=1` (which `scripts/demo.sh` / `demo/run_demo.sh`
set), as `N user cycles` and `M total cycles`. `user_cycles` is the program's
actual executed instruction count (the meaningful CU measure); `total_cycles` is
that padded up to the next power-of-two zkVM segment size that the prover would
charge for. The numbers below are read directly from the sequencer log of the
`RISC0_DEV_MODE=0` run reproduced by `demo/run_demo.sh` (the raw log is not
committed because it captured the demo wallet's seed phrase).

| Operation | user cycles | total cycles (= prover segment) | segments | Notes |
|---|---|---|---|---|
| `create_token` | 205,794 / 208,820 | 524,288 (2^19) | 1 | two account inits (state + holding), largest op; exceeds the 2^18 segment |
| `mint_to` | 187,878 – 191,304 | 262,144 (2^18) | 1 | state + holding write, plus the authority gate and the conditional holding claim |
| `rotate_authority` | 124,153 | 262,144 (2^18) | 1 | single state write; gate + null-validation of the new authority |
| `revoke_authority` | 100,608 | 262,144 (2^18) | 1 | single state write; gate only (cheapest op) |

(The two `create_token` and the three `mint_to` rows show the spread observed
across the calls in the run; the small variation is input-size dependent, e.g.
the holder/authority bytes and the `rotate` `account_id` argument.)

### What this shows

The ordering matches the transaction-shape model above: `revoke` (gate + 1 write)
< `rotate` (gate + null-check + 1 write) < `mint_to` (gate + 2 writes + claim) <
`create_token` (2 inits). A large fixed cost (~100k+ cycles) is the zkVM bootstrap
plus the SPEL framework's input deserialization (program id, caller, pre-state
accounts, instruction data) and dispatch; this is shared by every LEZ program and
is the same fixed floor the minimal token example pays. The authority model's
marginal cost is the delta between these ops, which is small relative to that
floor.

Reproduce:

```bash
RISC0_DEV_MODE=0 RISC0_INFO=1 RUST_LOG=info ./target/release/sequencer_service \
  ./runtime/sequencer_config.json --port 3040   # logs "N user cycles" per execution
# then drive each instruction via demo/run_demo.sh and read the per-execution
# cycle line from the sequencer log (clock-tx executions show a constant
# 137,022 user cycles and are easy to exclude).
```

Honesty notes:
- The standalone sequencer runs the **executor**, not the prover: it executes the
  guest and checks the state transition, but does not generate a STARK receipt
  (proving is delegated to Bedrock, which standalone mode mocks out). So
  `total_cycles` here is the segment size the prover *would* charge, measured from
  a real `RISC0_DEV_MODE=0` execution of the exact deployable guest; it is not the
  wall-clock of a full proof. `user_cycles` is exact.
- The per-transaction compute budget on testnet may change (the prize flags this);
  this table records the cycle counts measured at the time of the recorded run.
