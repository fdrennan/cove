#!/usr/bin/env bash
# Down: stop what .cove/up.sh started (its whole process group), TERM then
# KILL. Already down is success.
#
#   .cove/down.sh
set -euo pipefail
cd "$(dirname "$0")/.."
. .cove/app.env

pid="$(cat .cove/run/app.pid 2>/dev/null || true)"
if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
  rm -f .cove/run/app.pid
  echo "down: not running"
  exit 0
fi

kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid"
for _ in $(seq 1 10); do
  kill -0 "$pid" 2>/dev/null || break
  sleep 1
done
kill -0 "$pid" 2>/dev/null && kill -KILL -- "-$pid" 2>/dev/null
rm -f .cove/run/app.pid
echo "down: stopped pid $pid, port ${APP_PORT:-} free"
