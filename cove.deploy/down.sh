#!/usr/bin/env bash
# Terminate a box started by up.sh and delete its security group.
#
#   cove.deploy/down.sh <name>      # one box
#   cove.deploy/down.sh --all       # every box in cove.deploy/state/
#   cove.deploy/down.sh             # list what is up
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/state"

kill_one() {
  local state="$here/state/$1.env"
  [[ -r "$state" ]] || { echo "✗ no $state" >&2; return 1; }
  local NAME="" AWS_REGION="" SG="" INSTANCE="" IP=""
  # shellcheck disable=SC1090
  source "$state"
  export AWS_REGION
  if [[ -n "$INSTANCE" ]]; then
    echo "  › terminating $NAME ($INSTANCE${IP:+, $IP})"
    aws ec2 terminate-instances --instance-ids "$INSTANCE" >/dev/null
    aws ec2 wait instance-terminated --instance-ids "$INSTANCE"
  fi
  if [[ -n "$SG" ]]; then
    echo "  › deleting security group $SG"
    # The ENI can linger a few seconds after termination.
    for _ in $(seq 1 12); do
      aws ec2 delete-security-group --group-id "$SG" >/dev/null 2>&1 && break
      sleep 5
    done
  fi
  rm -f "$state"
  echo "  ✓ $NAME is gone"
}

case "${1:-}" in
  "")
    shopt -s nullglob; files=("$here"/state/*.env)
    [[ ${#files[@]} -eq 0 ]] && { echo "nothing is up"; exit 0; }
    for f in "${files[@]}"; do
      ( source "$f"; printf '%-32s %-20s %s\n' "$NAME" "${INSTANCE:-?}" "${URL:-?}" )
    done
    echo; echo "kill one:  $0 <name>    all:  $0 --all"
    ;;
  --all)
    shopt -s nullglob
    for f in "$here"/state/*.env; do kill_one "$(basename "$f" .env)"; done
    ;;
  *) kill_one "$1" ;;
esac
