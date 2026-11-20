# __HOST__ · Unreal Engine 5, streamed to the browser

A C++ Unreal Engine 5 project. The game runs on this machine's GPU, off
screen, and Pixel Streaming sends it to a browser as video over WebRTC; the
keyboard and mouse come back the same way. In the web's terms, Unreal is the
framework (as React or Shiny is), and Pixel Streaming is how it is served on
a port.

## What it is

- `CoveGame.uproject` - the project; the PixelStreaming plugin is on.
- `Source/CoveGame/CoveGameMode.cpp` - the game mode: a flying camera
  (`ADefaultPawn`: WASD, the mouse, E and Q) and a scene built in C++ at the
  start - a sun, a sky, Lumen lighting, a floor and a ring of turning shapes.
  There are no binary assets: the map is Epic's own empty `/Engine/Maps/Entry`
  and the shapes are the engine's `/Engine/BasicShapes`. Your own maps,
  meshes and Blueprints come from the Unreal Editor, saved under `Content/`.
- `Config/` - the start map and game mode, the renderer (Lumen, virtual
  shadow maps), Vulkan SM6 on Linux, and what to cook.
- `.cove/unreal/build.sh` - Unreal's `RunUAT BuildCookRun`: compile, cook
  and package for Linux into `Saved/Packaged/Linux`, then fetch and build
  Epic's Pixel Streaming servers for this engine release
  (github.com/EpicGames/PixelStreamingInfrastructure, branch `UE<version>`)
  into `.cove/unreal/ps`.
  The player page's `css/` and `images/` are built under `static/`: the
  platform's nginx serves `/css/` and `/images/` itself on every domain.
- `.cove/unreal/run.sh` - Epic's signalling server (started directly with
  `--no_config`, not through its `start.sh`) with the player page on
  `APP_PORT`, the game connected to it on `APP_STREAMER_PORT`, and Epic's SFU
  port on `APP_SFU_PORT`. Cove gave the project all three in `.cove/app.env`;
  only `APP_PORT` is on the web.

## What it needs - none of it comes with the project

- **Unreal Engine 5.5 or newer, installed on this machine.** It is not in the
  ndexr stack: Epic's licence (the Unreal Engine EULA) does not allow the
  engine to be redistributed. Install it from source (link a GitHub account
  to an Epic Games account, then clone EpicGames/UnrealEngine) or from Epic's
  Linux binary build, and set `UE_ROOT` in Settings › Variables to its folder
  if it is not in `~/UnrealEngine` or `/opt/UnrealEngine*`. A source build
  needs well over 100 GB of disk.
- **An NVIDIA GPU with its driver.** Pixel Streaming encodes the video with
  the GPU's hardware encoder (NVENC); without one there is nothing to stream.
- **A network path for WebRTC's media.** The player page and signalling go
  over HTTP and WebSocket, which nginx can carry. The video itself is WebRTC
  over UDP: it reaches a browser on the same network directly, but from the
  internet it needs a TURN relay (coturn, ideally on TCP 443) - a TCP-only
  path such as an SSH tunnel will not carry it.
- **Unreal's own licence terms** apply to what is built with it (royalties
  above Epic's revenue threshold), not ndexr's.

## Build, Up, Down

`.cove/build.sh` compiles, cooks and packages (tens of minutes the first
time); `.cove/up.sh` starts both servers and waits for the player page;
`.cove/down.sh` stops them. Open the app's address and click the page once to
start the stream.

## Logging
run.sh writes a start line for the signalling server and the game in the one format across languages (`2026-11-18 12:04:31 INFO    [run] ...`); their own output follows unchanged.

## State

Written to Unreal Engine 5.5's conventions (BuildSettingsVersion.V5, the
PixelStreaming plugin, `-PixelStreamingURL`, the UE5.5 signalling server's
`setup.sh` / `start.sh --player_port --streamer_port`) on a machine without
Unreal installed: it has not been compiled or run yet. The first build on a
machine with Unreal is the first test.
