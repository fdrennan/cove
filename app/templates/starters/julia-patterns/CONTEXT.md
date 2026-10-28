# Pattern Lab · __HOST__
Gray-Scott reaction-diffusion computed in Julia on every core
(`Threads.@threads`, one task per row, a branch-free vectorised interior) and
streamed to the page as one byte per cell, which the page colours and paints
full screen. Five presets set the feed and kill rates of the classic regimes
(mitosis, coral, worms, maze, solitons) and reseed the dish; sliders move
feed, kill, the two diffusion rates, steps per frame and the brush; click or
drag on the dish to drop new catalyst. The milliseconds per step and the
thread count come back with every frame.

## Files
- server.jl - HTTP.jl: POST /api/frame ({id, w, h, f, k, du, dv, steps, reset?, seed?, drops?, r?} -> w·h bytes of v; headers X-Step-Ms, X-Threads, X-Size, X-Steps), GET /api/info (threads, presets, diffusion defaults, caps), GET /health, GET / (the page). Each page's grid lives on the server under the page's id, at most 16 grids and 300,000 cells each; the one idle longest is dropped first
- static/index.html - the page: the full-window canvas, the glass card that folds (on a phone it starts folded), five colour maps; no external scripts
- run.sh - loads Julia 1.11.7 and its packages from the ndexr stack, then serves with one thread per core (JULIA_NUM_THREADS=auto)
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - precompile the packages server.jl loads;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Julia from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
The first start precompiles HTTP and JSON3 and can take a few minutes. A
640x360 grid steps in 0.8-1.5 ms on 24 threads (measured 2026-10-26 with the
box at load average 60).

## Built on
Pearson, J. E. (1993), "Complex patterns in a simple system", Science
261(5118): 189-192. Forward Euler, a five-point Laplacian, periodic edges and
his Du = 2e-5, Dv = 1e-5 on a 2.5-wide square of 256 cells (0.2097 and 0.105
per cell).

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
