#!/usr/bin/env bash
# Install and start cove.ndexr.io's on-prem server as ubuntu's user unit.
# Then cove.ndexr.io's nginx vhost proxies / to 127.0.0.1:8796 (see CLAUDE.md).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p ~/.config/systemd/user
install -m 644 "$here/cove-onprem.service" ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now cove-onprem.service
systemctl --user --no-pager status cove-onprem.service | head -5
echo "follow: journalctl --user -fu cove-onprem"
