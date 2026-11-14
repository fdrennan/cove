#!/usr/bin/env bash
# Build: the game, cooked and packaged for Linux with Pixel Streaming, and the
# signalling server that hands it to a browser. up never compiles; it starts
# what this built. A first build compiles the project's C++ and cooks the
# content - tens of minutes; later ones only what changed.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

start=$SECONDS
echo "build: Unreal Engine $UE_VERSION at $UE_ROOT"
"$UE_ROOT/Engine/Build/BatchFiles/RunUAT.sh" BuildCookRun \
  -project="$PWD/CoveGame.uproject" -platform=Linux -clientconfig=Development \
  -build -cook -stage -pak -archive -archivedirectory="$PWD/Saved/Packaged" \
  -unattended -utf8output -nop4

# Epic's Pixel Streaming servers for this engine release, once; setup.sh
# builds the player page and the signalling server (and fetches Node into
# their folder if none fits) - never with sudo.
if [ ! -d "$PS_DIR/.git" ]; then
  git clone -q --depth 1 --branch "UE$UE_VERSION" https://github.com/EpicGames/PixelStreamingInfrastructure.git "$PS_DIR" \
    || { echo "build: no Pixel Streaming servers for UE$UE_VERSION on GitHub - is this engine release supported?" >&2; exit 1; }
fi
# The player page puts its stylesheets under css/ and pictures under images/,
# which the platform's nginx serves itself on every domain (static_dirs.conf):
# through https://<domain>/ they would never reach this app. They go under
# static/ instead, and an older build made before this is rebuilt.
cfg="$PS_DIR/Frontend/implementations/typescript/webpack.base.js"
sed -i "s#'css/\[name\]#'static/css/[name]#; s#'images/\[name\]#'static/images/[name]#" "$cfg"
grep -q "'static/css/\[name\]" "$cfg" && grep -q "'static/images/\[name\]" "$cfg" \
  || { echo "build: $cfg no longer names css/ and images/ as it did in UE5.5 - move the player's assets off nginx's reserved paths by hand" >&2; exit 1; }
[ -d "$PS_DIR/SignallingWebServer/www/css" ] && rm -rf "$PS_DIR/SignallingWebServer/www"
( cd "$PS_DIR/SignallingWebServer/platform_scripts/bash" && chmod +x ./*.sh && ./setup.sh --nosudo )
echo "build: done in $((SECONDS - start)) s"
