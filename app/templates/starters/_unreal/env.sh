# Sourced by build.sh and run.sh: app.env, the Unreal Engine, the GPU and
# Node. Unreal is not in the ndexr stack: its licence (the Unreal Engine EULA)
# does not let it be redistributed, so each machine installs its own copy -
# from source after linking a GitHub account to an Epic account, or Epic's
# Linux binary build - and this finds it.
. ./app.env
# Settings › Variables (UE_ROOT among them): Cove exports them for Up only,
# and Build needs the engine too.
if [ -f .cove/app.vars ]; then set -a; . .cove/app.vars; set +a; fi

# UE_ROOT names the engine's folder (the one holding Engine/); else the usual
# places, first this machine's host_injections, where `ndexr stack inject pull
# UnrealEngine/<version>` lays the platform's private copy.
if [ -z "${UE_ROOT:-}" ]; then
  for d in "${NDEXR_HOST_INJECTIONS:-/opt/ndexr/host_injections}/eb/software/UnrealEngine/"* "$HOME/UnrealEngine" "$HOME/Linux_Unreal_Engine"* /opt/UnrealEngine* /opt/unreal/*; do
    [ -x "$d/Engine/Build/BatchFiles/RunUAT.sh" ] && { UE_ROOT="$d"; break; }
  done
fi
[ -n "${UE_ROOT:-}" ] && [ -x "$UE_ROOT/Engine/Build/BatchFiles/RunUAT.sh" ] || {
  echo "no Unreal Engine here: run ndexr stack inject pull UnrealEngine/5.5, or install Unreal Engine 5 (source or Epic's Linux build) and set UE_ROOT in .cove/app.vars to its folder" >&2
  exit 127
}
export UE_ROOT

# The engine's version picks the Pixel Streaming servers that match it (Epic
# keeps one branch per release: UE5.5, UE5.6, ...).
UE_VERSION="$(sed -n 's/.*"MajorVersion": *\([0-9]*\).*/\1/p' "$UE_ROOT/Engine/Build/Build.version").$(sed -n 's/.*"MinorVersion": *\([0-9]*\).*/\1/p' "$UE_ROOT/Engine/Build/Build.version")"
export UE_VERSION

# Node for the signalling server, from the stack when it is here; setup.sh
# fetches its own into the servers' folder when none fits.
if ! command -v node >/dev/null && [ -r /opt/ndexr/stack/use.sh ]; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1 && ml nodejs >/dev/null 2>&1
  set -u
fi

# Where the build leaves the game, the servers, and the port the game talks
# to them on (the player's page is APP_PORT). Cove gives a new project both
# ports; a project switched to this type in the Type menu has only APP_PORT,
# and APP_PORT + 1000 / + 2000 (outside the app range) stand in until set.
export PORT="${PORT:-$APP_PORT}"
export STREAMER_PORT="${APP_STREAMER_PORT:-$((PORT + 1000))}"
export SFU_PORT="${APP_SFU_PORT:-$((PORT + 2000))}"
export PS_DIR="$PWD/.cove/unreal/ps"
export PACKAGED="$PWD/Saved/Packaged/Linux"
