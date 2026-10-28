#!/usr/bin/env bash
# Up: start run.sh in the background and wait until APP_HEALTH answers.
# Already up is success. The pid and log live in .run/.
#
#   ./up.sh            wait up to 600 s (a first start compiles packages)
#   WAIT=30 ./up.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./app.env

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

mkdir -p .run
setsid nohup ./run.sh >.run/app.log 2>&1 </dev/null &
echo $! >.run/app.pid
echo "up: started pid $(cat .run/app.pid), log .run/app.log"

wait_s="${WAIT:-600}"
for ((i = 0; i < wait_s; i++)); do
  if healthy; then
    echo "up: serving on 127.0.0.1:$APP_PORT after $i s"
    exit 0
  fi
  if ! kill -0 "$(cat .run/app.pid)" 2>/dev/null; then
    echo "up: the app exited; last lines of .run/app.log:" >&2
    tail -n 20 .run/app.log >&2
    rm -f .run/app.pid
    exit 1
  fi
  sleep 1
done
echo "up: still starting after $wait_s s; follow .run/app.log" >&2
exit 1
