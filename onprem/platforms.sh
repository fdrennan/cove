#!/usr/bin/env bash
# Where every platform's Cove stands against the version in app/Cargo.toml and
# its branch on GitHub (operator, 2026-11-26: "github repo branch must always
# equal current version of software for ALL platforms across NDEXR").
#
#   onprem/platforms.sh          the table; exit 1 when any platform is behind
#   onprem/platforms.sh --json   the same as JSON (written to
#                                ~/.local/state/cove-onprem/platforms.json by
#                                public-push.sh after every release)
#
# Platforms, and what moves each one:
#   github main, v<version>   public-push.sh, every release
#   server                    run.sh, every save (cove.ndexr.io, every cell)
#   linux-x86_64, linux-aarch64, windows-x86_64
#                             desktop-push.sh feed, on a version bump
#   darwin                    built on neo (macOS); behind while neo is away
#   cvmfs                     the stack's cove module, carried by the ndexr CLI
#                             (ndexr.ndexr.io crate/src/stack/cove.rs) and built
#                             on the four boxes: a new CLI release, then a stack
#                             run and `ndexr stack cvmfs publish`
set -uo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
want="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$here/app/Cargo.toml" | head -1)"
remote="${COVE_PUBLIC_REMOTE:-git@github.com:fdrennan/cove.git}"
feed="https://repo.ndexr.io/ndexr/cove"

rows=()
row() { rows+=("$1|$2|$3"); } # platform | version | detail

refs="$(GIT_SSH_COMMAND="ssh -o BatchMode=yes -o ConnectTimeout=10" git ls-remote "$remote" main "v$want" 2>/dev/null)"
main_sha="$(awk '$2=="refs/heads/main"{print substr($1,1,7)}' <<<"$refs")"
ver_sha="$(awk -v b="refs/heads/v$want" '$2==b{print substr($1,1,7)}' <<<"$refs")"
row "github v$want" "$([[ -n "$ver_sha" ]] && echo "$want" || echo -)" "${ver_sha:-no branch v$want}"
row "github main" "$([[ -n "$main_sha" && "$main_sha" == "$ver_sha" ]] && echo "$want" || echo "?")" "${main_sha:-unreachable}$([[ -n "$main_sha" && "$main_sha" != "$ver_sha" ]] && echo " (not v$want's head)")"

srv="$(/usr/local/lib/cove/current/cove --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
row "server" "${srv:--}" "$(basename "$(readlink -f /usr/local/lib/cove/current 2>/dev/null)")"

man="$(curl -fsS --max-time 15 "$feed/manifest.json" 2>/dev/null)"
for p in linux-x86_64 linux-aarch64 windows-x86_64 darwin; do
  v="$(python3 -I -c 'import json,sys; m=json.loads(sys.argv[1] or "{}").get("platforms",{}).get(sys.argv[2],{}); print(m.get("version","-"), m.get("published_at","")[:16])' "$man" "$p" 2>/dev/null)"
  row "$p" "${v%% *}" "${v#* }"
done

cv="$(ls -1 /opt/ndexr/stack/x86_64-pc-linux-gnu/eb/software/cove 2>/dev/null | sed 's/-GCCcore.*//' | sort -V | tail -1)"
ca="$(ls -1 /opt/ndexr/stack/aarch64-unknown-linux-gnu/eb/software/cove 2>/dev/null | sed 's/-GCCcore.*//' | sort -V | tail -1)"
row "cvmfs x86_64" "${cv:--}" "ml cove"
row "cvmfs aarch64" "${ca:--}" "ml cove"

behind=0
for r in "${rows[@]}"; do [[ "$(cut -d'|' -f2 <<<"$r")" == "$want" ]] || behind=$((behind + 1)); done

if [[ "${1:-}" == --json ]]; then
  python3 -I -c '
import json, sys
want, behind, rows = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
print(json.dumps({"version": want, "behind": behind, "platforms": [dict(zip(("platform", "version", "detail"), r.split("|", 2))) | {"current": r.split("|")[1] == want} for r in rows]}, indent=1))
' "$want" "$behind" "${rows[@]}"
else
  printf 'Cove %s\n' "$want"
  for r in "${rows[@]}"; do
    IFS='|' read -r p v d <<<"$r"
    printf '  %-2s %-18s %-8s %s\n' "$([[ "$v" == "$want" ]] && echo ok || echo '!!')" "$p" "$v" "$d"
  done
  printf '%d of %d behind\n' "$behind" "${#rows[@]}"
fi
[[ $behind -eq 0 ]]
