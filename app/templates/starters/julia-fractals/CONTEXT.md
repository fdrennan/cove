# Fractal Explorer · __HOST__
The Mandelbrot set and its Julia sets, computed in Julia on every core
(`Threads.@threads`, one task per row, smooth escape-time colouring) and sent
to the page as raw RGBA pixels painted full screen. Click to zoom in, drag a
box to zoom to it, scroll to zoom where you point; choose a palette and the
iteration depth. In Julia mode a small Mandelbrot picks the constant c (drag
across it to watch the Julia set change); Alt-click or a long press on the
Mandelbrot view opens that point's Julia set. Each view arrives at a quarter
resolution first, then in full, with the render time and thread count shown.

## Files
- server.jl - HTTP.jl: GET /render (?kind, cx, cy, scale, w, h, iter, palette, jr, ji -> w·h·4 bytes RGBA; headers X-Render-Ms, X-Threads, X-Size), GET /info, GET /health, GET / (the page)
- static/index.html - the page: the full-window canvas, the glass panel, the picker; no external scripts
- run.sh - loads Julia 1.11.7 and its packages from the ndexr stack, then serves with one thread per core (JULIA_NUM_THREADS=auto)
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - precompile the packages server.jl loads;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Julia from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
The first start precompiles HTTP and JSON3 and can take a few minutes.

## Built on
Mandelbrot, B. B. (1980), "Fractal aspects of the iteration of z -> lambda
z(1 - z) for complex lambda and z", Annals of the New York Academy of
Sciences 357. Bezanson, J. et al. (2017), "Julia: A Fresh Approach to
Numerical Computing", SIAM Review 59(1).

## Logging
Logging: log.jl (`Log.info`), one format across languages; LOG_LEVEL=DEBUG for more. /render lines are DEBUG.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
