#!/usr/bin/env bash
# Licence compliance for Cove and Inlet: compliance.py with Cove's cargo
# metadata. The report is docs/compliance/latest.md; history.tsv keeps every run.
#
#   onprem/compliance.sh           every check (cove-compliance.timer, daily)
#   onprem/compliance.sh --gate    the code-side checks run.sh needs before a release
#   onprem/compliance.sh --notices regenerate app/THIRD-PARTY-NOTICES.txt from Cargo.lock
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
web_rust="$HOME/.local/share/cove-rust"
meta="$(mktemp -p "${XDG_RUNTIME_DIR:-$HOME/tmp}" cove-meta.XXXXXX.json)"
trap 'rm -f "$meta"' EXIT
# The same toolchain run.sh builds with; --offline: the registry cache is what the build used.
RUSTUP_HOME="$web_rust/rustup" CARGO_HOME="$web_rust/cargo" PATH="$web_rust/cargo/bin:$PATH" \
  cargo metadata --manifest-path "$here/../app/Cargo.toml" --format-version 1 --locked --offline >"$meta"
if [[ "${1:-}" == --notices ]]; then
  python3 -I "$here/third-party-notices.py" "$meta"
  exit
fi
python3 -I "$here/compliance.py" --metadata "$meta" "$@"
