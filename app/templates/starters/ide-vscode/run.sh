#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground:
# VS Code (code-server), as you, opened on this project. up.sh runs this in
# the background.
#
# Your VS Code is config/: settings.json, keybindings.json and snippets/ are
# linked into its user folder, so what you change in its Settings is written
# back here and versioned with the project. Its state - logs, caches, extensions - is .run/vscode, never
# committed.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

here="$(pwd -P)"
st="$here/.run/vscode"
mkdir -p "$st/data/User" "$st/extensions"
chmod 700 "$st"
for f in settings.json keybindings.json snippets; do
  [ -e "$here/config/$f" ] && ln -sfn "$here/config/$f" "$st/data/User/$f"
done
# The stack's extensions (R, Python, Quarto: the code-server-extensions
# module, built by ndexr - never a marketplace), each installed once, in the
# order the module gives. Matched by name and version: REditorSupport.r is not
# REditorSupport.r-syntax.
vsix="${EBROOTCODEMINSERVERMINEXTENSIONS:-}/vsix"
if [ -r "$vsix/install-order.txt" ]; then
  while read -r f; do
    [ -n "$f" ] || continue
    name="$(printf '%s' "${f%.vsix}" | sed -E 's/-[0-9][^-]*(-linux-[a-z0-9]+)?$//')"
    ls "$st/extensions" 2>/dev/null | grep -qiE "^${name//./\\.}-[0-9]" && continue
    code-server --extensions-dir "$st/extensions" --user-data-dir "$st/data" --install-extension "$vsix/$f" >/dev/null 2>&1 || echo "run: could not install $f" >&2
  done <"$vsix/install-order.txt"
fi

printf '%s INFO    [run] __HOST__: code-server (VS Code) starting on 127.0.0.1:%s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$PORT"
exec code-server \
  --bind-addr "127.0.0.1:$PORT" \
  --auth none \
  --user-data-dir "$st/data" \
  --extensions-dir "$st/extensions" \
  --disable-telemetry \
  --disable-update-check \
  --disable-workspace-trust \
  "$here"
