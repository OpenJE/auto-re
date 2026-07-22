#!/usr/bin/env sh
#
# Helper to launch a Windows EXE under Wine and attach gdbserver for the
# IDA provider's GDB backend (spec §9.6).
#
# Usage:
#   tools/wine-launch-vanburen-gdb.sh /path/to/vanburen.exe [args...]
#
# The script launches `wine <exe> "$@"` in the background, then attaches
# gdbserver on :2345 to the Wine process. The IDA provider connects to
# gdbserver and drives execution through the typed scenario AST.

set -eu

WINE="${AUTORE_WINE_PATH:-wine}"
GDBSERVER="${AUTORE_GDBSERVER_PATH:-gdbserver}"
GDB_HOST="${AUTORE_GDBSERVER_HOST:-0.0.0.0}"
GDB_PORT="${AUTORE_GDBSERVER_PORT:-2345}"

if [ "$#" -lt 1 ]; then
  echo "Usage: $0 <exe> [args...]" >&2
  exit 1
fi

EXE="$1"
shift

echo "Launching: $WINE $EXE $*"
# Launch the Windows executable under Wine in the background.
$WINE "$EXE" "$@" &
WINE_PID=$!

echo "Wine PID: $WINE_PID"
echo "Attaching gdbserver on ${GDB_HOST}:${GDB_PORT}"
exec "$GDBSERVER" "${GDB_HOST}:${GDB_PORT}" --attach "$WINE_PID"
