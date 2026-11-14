#!/usr/bin/env bash
# Up: start .cove/run.sh in the background and wait until APP_HEALTH answers.
# Already up is success; a module app (APP_KIND=module) is served by the
# platform and has nothing to start. The pid and log live in .cove/run/.
#
#   .cove/up.sh            wait up to 600 s (a first start compiles packages)
#   WAIT=30 .cove/up.sh
set -euo pipefail
cd "$(dirname "$0")/.."
. .cove/app.env

if [ "${APP_KIND:-}" != port ]; then
  echo "up: a ${APP_KIND:-module} app - the platform serves it, nothing to start"
  exit 0
fi
url="http://127.0.0.1:$APP_PORT$APP_HEALTH"
healthy() { curl -fs -o /dev/null --max-time 2 "$url"; }

if healthy; then
  echo "up: already serving on 127.0.0.1:$APP_PORT"
  exit 0
fi
if ss -ltn "sport = :$APP_PORT" | grep -q LISTEN; then
  echo "up: port $APP_PORT is held by a process that does not answer $APP_HEALTH" >&2
  exit 1
fi

mkdir -p .cove/run
setsid nohup .cove/run.sh >.cove/run/app.log 2>&1 </dev/null &
echo $! >.cove/run/app.pid
echo "up: started pid $(cat .cove/run/app.pid), log .cove/run/app.log"

wait_s="${WAIT:-600}"
for ((i = 0; i < wait_s; i++)); do
  if healthy; then
    echo "up: serving on 127.0.0.1:$APP_PORT after $i s"
    exit 0
  fi
  if ! kill -0 "$(cat .cove/run/app.pid)" 2>/dev/null; then
    echo "up: the app exited; last lines of .cove/run/app.log:" >&2
    tail -n 20 .cove/run/app.log >&2
    rm -f .cove/run/app.pid
    exit 1
  fi
  sleep 1
done
echo "up: still starting after $wait_s s; follow .cove/run/app.log" >&2
exit 1
