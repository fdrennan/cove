#!/usr/bin/env bash
# Serve __HOST__: the signalling server's player page on 127.0.0.1:$PORT and
# the packaged game streaming into it, in the foreground. up.sh runs this in
# the background. The game renders on this machine's GPU off screen and
# Pixel Streaming sends it to the browser as video; keys and mouse come back.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

game="$PACKAGED/CoveGame.sh"
[ -x "$game" ] || { echo "not built yet: run ./build.sh" >&2; exit 1; }
[ -d "$PS_DIR/SignallingWebServer" ] || { echo "no signalling server yet: run ./build.sh" >&2; exit 1; }
# Pixel Streaming encodes with the GPU's own video encoder (NVENC on NVIDIA).
command -v nvidia-smi >/dev/null && nvidia-smi -L >/dev/null 2>&1 || echo "run: no NVIDIA GPU answers here - Pixel Streaming needs one to encode the video" >&2

pids=()
trap 'kill "${pids[@]}" 2>/dev/null; exit 0' TERM INT
# Epic's signalling server, started directly rather than by its start.sh,
# which adds an https redirect and asks the internet for a public IP: nginx
# holds TLS here. --no_config, so its config.json's ports (80, 8888, 8889)
# never apply; every port is this project's own.
ws="$PS_DIR/SignallingWebServer"
[ -x "$ws/platform_scripts/bash/node/bin/node" ] && PATH="$ws/platform_scripts/bash/node/bin:$PATH"
( cd "$ws" && exec node dist/index.js --no_config --serve --http_root "$ws/www" --homepage player.html \
    --player_port "$PORT" --streamer_port "$STREAMER_PORT" --sfu_port "$SFU_PORT" \
    --reverse_proxy --console_messages basic --log_folder "$PWD/.cove/run/ps-logs" ) &
pids+=($!)
sleep 3
"$game" -PixelStreamingURL="ws://127.0.0.1:$STREAMER_PORT" -RenderOffScreen -ForceRes -ResX=1920 -ResY=1080 -Unattended &
pids+=($!)
# Either one ending ends both: up.sh sees it and says so.
wait -n "${pids[@]}"
kill "${pids[@]}" 2>/dev/null || true
