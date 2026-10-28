# Scientific API · __HOST__
A small, fast numerical HTTP service in Rust: summary statistics, least-squares
fits, prime sieves, the Lorenz system and the Mandelbrot set, each answered
with the server's own compute time. The page at / calls every endpoint live.

## Files
- Cargo.toml - one binary, `app`; crates pinned to the stack's Rust set
- src/main.rs - `app serve <port>`
- src/server.rs - HTTP on 127.0.0.1: the routes, query and JSON parsing
- src/num.rs - the numerics: stats, linear fit, sieve, Lorenz RK4, Mandelbrot
- src/page.html - the landing page, compiled into the binary
- run.sh - serve target/release/app on 127.0.0.1:$APP_PORT
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - `cargo build --release`; start in the
  background and wait for /health (pid and log in .run/); stop; cargo 1.95+
  from PATH or the stack's Rust/1.96.0, crates from the stack's vendored set

## Endpoints
- GET  /health
- POST /stats             `[1,2,3]` or `{"x":[...], "probs":[0.1,0.9]}`
- POST /fit/linear        `{"x":[...], "y":[...]}` -> slope, intercept, r2, standard errors
- GET  /primes?n=1000000  count, largest, first and last ten
- GET  /lorenz?steps=5000&dt=0.01&sigma=10&rho=28&beta=2.6667
- GET  /mandelbrot.png?w=640&h=400&cx=-0.6&cy=0&scale=3&iter=256

## Run
`./build.sh` compiles; `./run.sh` then runs `target/release/app serve
$APP_PORT`, the port app.env gives this project (`PORT=<n> ./run.sh` for one
run); `./up.sh` and `./down.sh` start and stop it in the background.

## Built on
Lorenz, E. N. (1963), "Deterministic Nonperiodic Flow", J. Atmospheric
Sciences 20(2).

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
