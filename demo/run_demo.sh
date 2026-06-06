#!/usr/bin/env bash
#
# run_demo.sh: the recorded LP-0013 end-to-end demo.
#
# Same instruction sequence as scripts/demo.sh, but it consumes the already-built
# artifacts (so the asciinema recording shows the on-chain run and real proof
# generation, not a 9-minute compile):
#   - the deployable R0BF program binary (scripts/package_r0bf.py output), and
#   - the prebuilt standalone sequencer + wallet in ../lez-build/target/release.
#
# RISC0_DEV_MODE=0 => real proofs. Each step's on-chain result is read back from
# wallet state and the sequencer log, and every rejection is shown with its real
# program error code.
#
# Licensed MIT OR Apache-2.0.

set -uo pipefail
export RISC0_DEV_MODE=0
# RISC0_INFO=1 makes the risc0 executor log measured cycle counts (total/user/segments)
# for every guest execution, so the sequencer log carries the per-operation CU cost.
export RISC0_INFO=1

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
LEZ_DIR="${LEZ_DIR:-$REPO/../lez-build}"
PORT="${PORT:-3040}"
BIN="$REPO/mint-authority/examples/spel-token-guest/bin/spel_token.bin"
IDL="$REPO/mint-authority/examples/spel-token/spel_token.idl.json"
WALLET="$LEZ_DIR/target/release/wallet"
SEQ="$LEZ_DIR/target/release/sequencer_service"
export NSSA_WALLET_HOME_DIR="$LEZ_DIR/runtime/wallet-home"

say()  { printf '\n=== %s ===\n' "$*"; }

say "0. preflight"
command -v spel >/dev/null || { echo "MISSING spel"; exit 1; }
[ -f "$BIN" ]    || { echo "MISSING deployable .bin ($BIN); run scripts/package_r0bf.py"; exit 1; }
[ -x "$WALLET" ] || { echo "MISSING wallet ($WALLET)"; exit 1; }
[ -x "$SEQ" ]    || { echo "MISSING sequencer_service ($SEQ)"; exit 1; }
echo "RISC0_DEV_MODE=$RISC0_DEV_MODE  (0 = real proofs)"
echo "program image id: $(spel program-id "$BIN" | awk '/ImageID/{print $NF}')"

# Fresh chain + wallet each run so the demo is self-contained and reproducible from
# any prior state (the standalone sequencer persists rocksdb between runs otherwise).
say "0b. reset local chain + wallet to a clean state"
pkill -f sequencer_service 2>/dev/null || true
sleep 1
RUNTIME="$LEZ_DIR/runtime"
# The sequencer config sets home="." so the chain DB is created in the sequencer's
# cwd ($LEZ_DIR/rocksdb), not under runtime/. Clear both possible locations.
rm -rf "$LEZ_DIR/rocksdb" "$RUNTIME/rocksdb"
CFG_KEEP="$(mktemp)"; cp "$RUNTIME/wallet-home/wallet_config.json" "$CFG_KEEP" 2>/dev/null || true
rm -rf "$RUNTIME/wallet-home"; mkdir -p "$RUNTIME/wallet-home"
if [ -s "$CFG_KEEP" ]; then
  cp "$CFG_KEEP" "$RUNTIME/wallet-home/wallet_config.json"
else
  cat > "$RUNTIME/wallet-home/wallet_config.json" <<JSON
{
  "sequencer_addr": "http://127.0.0.1:$PORT/",
  "seq_poll_timeout": "12s",
  "seq_tx_poll_max_blocks": 5,
  "seq_poll_max_retries": 5,
  "seq_block_poll_max_amount": 100
}
JSON
fi
echo "chain + wallet reset"

say "1. start standalone sequencer on :$PORT"
( cd "$LEZ_DIR" && RUST_LOG=info "$SEQ" ./runtime/sequencer_config.json --port "$PORT" \
    > "$HERE/sequencer.log" 2>&1 ) &
SEQ_PID=$!
trap 'kill $SEQ_PID 2>/dev/null || true' EXIT
sleep 4
echo program-tutorial | "$WALLET" check-health

say "2. deploy program + create two accounts (A = authority, B = treasury)"
"$WALLET" deploy-program "$BIN"
# `account new` prints: "Generated new account with account_id Public/<id> at path ..."
# spel wants the bare base58 id, so strip the Public/ prefix.
acct_id() { sed -n 's#.*account_id Public/\([0-9A-Za-z]*\).*#\1#p' | head -1; }
A="$("$WALLET" account new public --label "authority-$$" | acct_id)"
B="$("$WALLET" account new public --label "treasury-$$"  | acct_id)"
[ -n "$A" ] && [ -n "$B" ] || { echo "FATAL: account creation failed (A='$A' B='$B')"; exit 1; }
echo "A (authority) = $A"
echo "B (treasury)  = $B"

b58_to_hex() { python3 -c "import sys,base58;print('0x'+base58.b58decode(sys.argv[1]).hex())" "$1"; }
A_HEX="$(b58_to_hex "$A")"
B_HEX="$(b58_to_hex "$B")"

run() { echo "+ spel $*"; spel --idl "$IDL" --program "$BIN" -- "$@"; }

say "3. INTEGRATION 1: variable supply with mint authority"
run create_token     --name VAR --holder "$A_HEX" --decimals 6 --initial-supply 0  --authority "$A"
run mint_to          --name VAR --holder "$A_HEX" --amount 100 --authority "$A"
run mint_to          --name VAR --holder "$A_HEX" --amount 50  --authority "$A"
echo ">> expect total_supply = 150"
run rotate_authority --name VAR --new-authority "$B" --current-authority "$A"
echo ">> former authority A must now be REJECTED (Unauthorized / 1008):"
run mint_to          --name VAR --holder "$A_HEX" --amount 1   --authority "$A" && echo "!! UNEXPECTED OK" || echo ">> REJECTED as expected"
echo ">> new authority B can mint:"
run mint_to          --name VAR --holder "$B_HEX" --amount 25  --authority "$B"

say "4. INTEGRATION 2: fixed supply with revoked authority"
run create_token     --name FIX --holder "$A_HEX" --decimals 0 --initial-supply 1000 --authority "$A"
run revoke_authority --name FIX --current-authority "$A"
echo ">> mint after revoke must be REJECTED (ERR_MINT_REVOKED = program error 9003):"
run mint_to          --name FIX --holder "$A_HEX" --amount 1   --authority "$A" && echo "!! UNEXPECTED OK" || echo ">> REJECTED as expected"

say "DONE. both integrations exercised against a real sequencer with RISC0_DEV_MODE=$RISC0_DEV_MODE"
