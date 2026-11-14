#!/usr/bin/env bash
# Install or update Cove's container builder from this folder: the service
# runs a copy, so an edit here goes live only through this script.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd -P)"
sudo install -d -o ubuntu -g ubuntu -m 0755 /srv/ndexr/containers /usr/local/lib/cove-container
sudo install -m 0755 "$here/build.sh" "$here/broker.py" /usr/local/lib/cove-container/
sudo install -m 0644 "$here/cove-container.service" /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable cove-container >/dev/null 2>&1
sudo systemctl restart cove-container
sleep 1
systemctl is-active cove-container
