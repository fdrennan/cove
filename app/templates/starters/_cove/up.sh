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
log=.cove/run/app.log
# Cove's own lines in the app's log, in the starters' format with [cove]
# (operator, 2026-11-17: "logs arent helpful if they never say anything"):
# when it starts, when it answers, when it stops or exits. The log is kept
# across starts - appended, never overwritten - and past 2 MB its last 1 MB.
mark() { printf '%s %-7s [cove] %s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$1" "$2" >>"$log"; }
if [ -f "$log" ] && [ "$(wc -c <"$log")" -gt 2097152 ]; then
  tail -c 1048576 "$log" >"$log.tmp" && mv -f "$log.tmp" "$log"
fi
mark INFO "up · ${APP_LANGUAGE:-app} · port $APP_PORT"
# The app's own exit is said too: a crash leaves "exited 1" as the log's last
# line. down.sh stops the whole group, this shell with it, so a stop is not.
setsid nohup bash -c '.cove/run.sh; rc=$?; printf "%s %-7s [cove] exited %s\n" "$(date -u "+%Y-%m-%d %H:%M:%S")" "$([ "$rc" = 0 ] && echo INFO || echo ERROR)" "$rc"' >>"$log" 2>&1 </dev/null &
echo $! >.cove/run/app.pid
echo "up: started pid $(cat .cove/run/app.pid), log .cove/run/app.log"

wait_s="${WAIT:-600}"
for ((i = 0; i < wait_s; i++)); do
  if healthy; then
    mark SUCCESS "answering on 127.0.0.1:$APP_PORT after $i s"
    echo "up: serving on 127.0.0.1:$APP_PORT after $i s"
    exit 0
  fi
  if ! kill -0 "$(cat .cove/run/app.pid)" 2>/dev/null; then
    mark ERROR "exited before answering $APP_HEALTH"
    echo "up: the app exited; last lines of .cove/run/app.log:" >&2
    tail -n 20 .cove/run/app.log >&2
    rm -f .cove/run/app.pid
    exit 1
  fi
  sleep 1
done
mark WARN "still starting after $wait_s s"
echo "up: still starting after $wait_s s; follow .cove/run/app.log" >&2
exit 1
