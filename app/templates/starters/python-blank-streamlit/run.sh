#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

exec python3 -m streamlit run app.py --server.address 127.0.0.1 --server.port "$PORT" --server.headless true \
  --server.runOnSave true --server.fileWatcherType poll --browser.gatherUsageStats false \
  --server.enableCORS false --server.enableXsrfProtection false --global.developmentMode false
