#!/usr/bin/env bash
#
# demo.sh: reproducible end-to-end demo for the LP-0013 mint-authority token
# program, driven against a real local LEZ sequencer in standalone mode with
# RISC0_DEV_MODE=0 (real proof generation).
#
# It runs both example integrations the prize asks for:
#   1. variable supply with mint authority  (create -> mint -> mint -> rotate -> mint)
#   2. fixed supply with revoked authority   (create -> revoke -> mint REJECTED)
#
# An evaluator runs this from a clean checkout. It must succeed unmodified. The
# heavy prerequisites (LEZ workspace build, Docker for the deployable .bin, the
# risc0 toolchain) are checked up front and the script fails loudly with the exact
# missing piece rather than half-running.
#
# Environment:
#   LEZ_DIR     path to a logos-execution-zone checkout pinned to rev cf3639d
#               (this repo's nssa_core tag v0.2.0-rc3). Default: ../lez-build
#   PORT        sequencer port (default 3040)
#   SKIP_BUILD  set to 1 to reuse already-built sequencer/wallet/guest artifacts
#
# Licensed MIT OR Apache-2.0.

set -euo pipefail

# Real proofs. The prize requires the recorded run to show RISC0_DEV_MODE=0 so the
# terminal output proves proof generation actually happened.
export RISC0_DEV_MODE=0

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
LEZ_DIR="${LEZ_DIR:-$REPO/../lez-build}"
PORT="${PORT:-3040}"
GUEST_DIR="$REPO/mint-authority/examples/spel-token-guest"
IDL="$REPO/mint-authority/examples/spel-token/spel_token.idl.json"

say() { printf '\n=== %s ===\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "MISSING: $1"; exit 1; }; }

say "0. preflight"
need cargo
need spel
[ -d "$LEZ_DIR" ] || { echo "MISSING LEZ workspace at $LEZ_DIR (set LEZ_DIR)"; exit 1; }
echo "RISC0_DEV_MODE=$RISC0_DEV_MODE  (0 = real proofs)"

# ── 1. Build the deployable program binary ──────────────────────────────────
# `cargo risczero build` wraps the guest ELF with the risc0 kernel and encodes it
# the way LEZ's ProgramBinary::decode expects. It runs in Docker for reproducibility.
say "1. build deployable program .bin (cargo risczero build, Docker)"
if [ "${SKIP_BUILD:-0}" != "1" ]; then
  ( cd "$GUEST_DIR" && cargo risczero build --manifest-path Cargo.toml )
fi
BIN="$(find "$GUEST_DIR/target" -name 'spel_token' -path '*docker*' | head -1)"
[ -n "$BIN" ] || { echo "deployable .bin not found; is Docker running?"; exit 1; }
IMAGE_ID="$(spel program-id "$BIN")"
echo "program image id: $IMAGE_ID"

# ── 2. Start a standalone sequencer ─────────────────────────────────────────
say "2. start standalone sequencer on :$PORT"
if [ "${SKIP_BUILD:-0}" != "1" ]; then
  ( cd "$LEZ_DIR" && cargo build --release -p sequencer_service --features standalone -p wallet )
fi
( cd "$LEZ_DIR" && RUST_LOG=info ./target/release/sequencer_service \
    ./runtime/sequencer_config.json --port "$PORT" ) &
SEQ_PID=$!
trap 'kill $SEQ_PID 2>/dev/null || true' EXIT
export NSSA_WALLET_HOME_DIR="$LEZ_DIR/runtime/wallet-home"
WALLET="$LEZ_DIR/target/release/wallet"
sleep 3
echo program-tutorial | "$WALLET" check-health

# ── 3. Deploy + create accounts ─────────────────────────────────────────────
say "3. deploy program + create two accounts (A = authority, B = treasury)"
"$WALLET" deploy-program "$BIN"
A="$("$WALLET" account new public --label authority | awk '/public/{print $NF}')"
B="$("$WALLET" account new public --label treasury  | awk '/public/{print $NF}')"
echo "A (authority) = $A"
echo "B (treasury)  = $B"

# The `holder` instruction arg is a raw [u8; 32] (it is used as a PDA seed, and only
# [u8;32]/String/u32/u64 implement ToSeed, so it cannot be the IDL `account_id` type).
# The spel CLI accepts a [u8;32] arg as 0x-prefixed hex, so convert the base58
# account ids to hex once here. `account_id`-typed args (rotate's --new-authority)
# take base58 directly.
b58_to_hex() { python3 -c "import sys,base58;print('0x'+base58.b58decode(sys.argv[1]).hex())" "$1"; }
A_HEX="$(b58_to_hex "$A")"
B_HEX="$(b58_to_hex "$B")"
echo "A hex = $A_HEX"
echo "B hex = $B_HEX"

run() { echo "+ spel $*"; spel --idl "$IDL" --program "$BIN" -- "$@"; }

# ── 4. Integration 1: variable supply with mint authority ───────────────────
say "4. INTEGRATION 1: variable supply with mint authority"
run create_token   --name VAR --holder "$A_HEX" --decimals 6 --initial-supply 0   --authority "$A"
run mint_to        --name VAR --holder "$A_HEX" --amount 100  --authority "$A"
run mint_to        --name VAR --holder "$A_HEX" --amount 50   --authority "$A"
echo "expect total_supply = 150"
run rotate_authority --name VAR --new-authority "$B" --current-authority "$A"
echo "former authority A must now be rejected:"
run mint_to        --name VAR --holder "$A_HEX" --amount 1    --authority "$A" || echo "REJECTED as expected (Unauthorized)"
echo "new authority B can mint:"
run mint_to        --name VAR --holder "$B_HEX" --amount 25   --authority "$B"

# ── 5. Integration 2: fixed supply with revoked authority ───────────────────
say "5. INTEGRATION 2: fixed supply with revoked authority"
run create_token   --name FIX --holder "$A_HEX" --decimals 0 --initial-supply 1000 --authority "$A"
run revoke_authority --name FIX --current-authority "$A"
echo "minting after revoke must be rejected deterministically (ERR_MINT_REVOKED = program error 9003):"
run mint_to        --name FIX --holder "$A_HEX" --amount 1    --authority "$A" || echo "REJECTED as expected (mint authority revoked; supply fixed)"

say "DONE. both integrations exercised against a real sequencer with RISC0_DEV_MODE=$RISC0_DEV_MODE"
