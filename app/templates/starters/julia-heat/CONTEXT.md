# Heat Lab · __HOST__
Heat spreading through a 100 mm square plate, live: the 2-D heat equation
u_t = α∇²u stepped by the explicit finite-difference (FTCS) scheme in Julia,
one row per thread (`Threads.@threads`), and streamed to a full-screen canvas
as raw RGBA frames with isotherms every 10 °C. Paint hot (100 °C) or cold
(0 °C) spots, held (a heater or chiller, dotted) or left to fade, or
insulating walls, with the mouse or a finger; pick a material
(copper to water) or set α directly, the time step Δt, steps per frame, the
grid, and fixed-temperature or insulated edges. The panel shows the stability
limit Δt ≤ h²/4α; an unstable Δt is clipped to 0.98 of it, and the page says
why (the checkerboard mode would grow by |1 − 8r| per step). The Check button
proves the solver: a Gaussian blob's variance must grow as σ₀² + 2αt.
Julia earns its place here: the stencil is a plain double loop that compiles
to tight machine code, threads with one macro, and moves tens of millions of
cell updates per frame with no vectorised-array tricks.

## Files
- server.jl - HTTP.jl: POST /api/step ({sid, grid, alpha, dt, steps, hold, bc, edge, paint, scene} -> grid·grid·4 bytes RGBA; headers X-Dt, X-Dt-Max, X-R, X-Clipped, X-Time, X-Steps, X-Step-Ms, X-Mean, X-Min, X-Max, X-Grid, X-Tasks, X-Threads), GET /api/check (?alpha, grid -> the Gaussian variance check as JSON), GET /info, GET /health, GET / (the page). Each browser tab owns one plate on the server (idle plates go after 15 minutes, 32 at most).
- static/index.html - the page: the full-window canvas, the glass panel (folds on a phone), the check table; no external scripts
- run.sh - loads Julia 1.11.7 and its packages from the ndexr stack, then serves with one thread per core (JULIA_NUM_THREADS=auto)
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - precompile the packages server.jl loads;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Julia from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background. The first start precompiles
HTTP and JSON3; at start the server times one task against a team of threads
per grid size, keeps the faster, and logs the Gaussian check.

## Built on
- Fourier, J. (1822), *Théorie analytique de la chaleur* - the heat equation.
- Crank, J. and Nicolson, P. (1947), "A practical method for numerical
  evaluation of solutions of partial differential equations of the
  heat-conduction type", Proc. Camb. Phil. Soc. 43(1) - the explicit scheme's
  stability limit and why implicit schemes exist.
- Charney, J. G., Fjørtoft, R. and von Neumann, J. (1950), "Numerical
  integration of the barotropic vorticity equation", Tellus 2(4) - von Neumann
  stability analysis, the source of r = αΔt/h² ≤ 1/4 in 2-D.
- Courant, R., Friedrichs, K. and Lewy, H. (1928), "Über die partiellen
  Differenzengleichungen der mathematischen Physik", Math. Ann. 100.
- Thermal diffusivities: CRC Handbook of Chemistry and Physics, rounded.

## Logging
Logging: log.jl (`Log.info`), one format across languages; LOG_LEVEL=DEBUG for more. /api/step lines are DEBUG.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
