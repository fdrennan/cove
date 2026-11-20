# Heat Lab: 2-D heat diffusion on a 100 mm square plate, stepped in Julia
# across every core with the explicit (FTCS) finite-difference scheme and sent
# to the page as raw RGBA frames.
#
#   GET  /           the page (static/index.html, read on every request)
#   POST /api/step   {sid, grid, alpha, dt, steps, hold, bc, edge, paint, scene}
#                    -> grid*grid*4 bytes of RGBA; headers X-Dt, X-Dt-Max,
#                    X-R, X-Clipped, X-Time, X-Steps, X-Step-Ms, X-Mean,
#                    X-Min, X-Max, X-Grid, X-Tasks, X-Threads
#   GET  /api/check  ?alpha&grid -> JSON: a Gaussian blob's variance against
#                    var0 + 2*alpha*t, stepped by the same kernel
#   GET  /info       {threads, materials, grids}
#   GET  /health     "ok"
#
# Units: millimetres, seconds, degrees Celsius. Diffusivity alpha in mm^2/s.
# u_t = alpha (u_xx + u_yy); on a grid of spacing h the scheme
#   u' = u + r * (sum of the 4 neighbours - 4u),   r = alpha dt / h^2
# is stable only for r <= 1/4 (von Neumann: the checkerboard mode is
# multiplied by 1 - 8r each step, which leaves [-1, 1] once r > 1/4).

include(joinpath(@__DIR__, "log.jl"))
using HTTP, JSON3

const ROOT = @__DIR__
# run.sh sets PORT from app.env's APP_PORT; a second default here would
# serve on a port the project does not own.
const PORT = parse(Int, ENV["PORT"])
const PLATE_MM = 100.0
const AMBIENT = 20.0
const HOT = 100.0
const COLD = 0.0
const GRIDS = (128, 256, 384)
# Thermal diffusivities near room temperature, mm^2/s (CRC Handbook values,
# rounded): what the material buttons on the page set alpha to.
const MATERIALS = [
    (name = "Copper", alpha = 111.0), (name = "Aluminium", alpha = 97.0),
    (name = "Iron", alpha = 23.0), (name = "Steel", alpha = 4.2),
    (name = "Glass", alpha = 0.34), (name = "Water", alpha = 0.143),
]
# The clip target sits a hair under the limit: at exactly r = 1/4 the
# checkerboard mode neither grows nor decays, so noise painted in never fades.
const CFL_SAFETY = 0.98
# Work cap per request (cell updates), so one visitor cannot hold every core.
const MAX_WORK = 60_000_000

# What each cell is. FREE cells follow the heat equation; a WALL neither
# changes nor passes heat; a HELD cell keeps the temperature it was painted
# with (a heater or a chiller) and exchanges heat with its neighbours. One
# matrix of kinds, so a cell cannot be two of them at once.
const FREE = 0x00
const WALL = 0x01
const HELD = 0x02

mutable struct Sim
    n::Int
    u::Matrix{Float64}
    cell::Matrix{UInt8}
    t::Float64
    steps::Int
    seen::Float64
    lock::ReentrantLock
end

spacing(n) = PLATE_MM / n
dt_limit(n, alpha) = spacing(n)^2 / (4alpha)

# One explicit step from u into v, columns 2:end-1 (rows 2:end-1 likewise),
# on a grid padded by one ghost cell a side. k is a conductance mask (0 for a
# wall, 1 otherwise) and g says whether a cell may change (1 only for FREE).
# A cell's change is g r Σ k_nb (u_nb - u), so a wall's neighbours see an
# insulated face, a held cell feeds its neighbours and stays put, a ghost with
# k = 1 holding the edge temperature is a fixed (Dirichlet) edge, and a ghost
# with k = 0 an insulated (Neumann) one. No branches, so the inner loop
# vectorises; a missing neighbour only lowers the diagonal, so r <= 1/4 stays
# sufficient. Grid index (i, j) = (x, y): a matrix column is an image row.
function step!(v, u, k, g, r)
    m, w = size(u)
    @inbounds for j in 2:w-1
        @simd for i in 2:m-1
            c = u[i, j]
            f = k[i-1, j] * (u[i-1, j] - c) + k[i+1, j] * (u[i+1, j] - c) +
                k[i, j-1] * (u[i, j-1] - c) + k[i, j+1] * (u[i, j+1] - c)
            v[i, j] = c + r * g[i, j] * f
        end
    end
    v
end

# Threads, with few meetings. One barrier per step costs more than the step
# (measured 2026-10-26 on a 24-core box at load 70: 6.7 ms per 256² step on
# 24 threads against 0.11 ms on one). So each task copies its band of rows
# plus HALO rows on each side, steps HALO times on its private copy, and the
# tasks meet once per HALO steps. Errors from a cut edge travel one row per
# step, so after HALO steps the band itself is still exact; the price is
# redundant work on the halos (ghost-zone, or temporal, blocking).
const HALO = 16

# How many bands each grid size runs with, chosen by measurement at start
# (calibrate!): bands of at least 64 rows keep the halo overhead at or under
# 50%, but on a box already full of other work one task can beat a team.
const TASKS = Dict{Int,Int}()
team(n) = clamp(n ÷ 64, 1, Threads.nthreads())

function advance!(u, cell, r, fixed::Bool, edge, steps; ntasks = get(TASKS, size(u, 1), 1))
    steps == 0 && return u
    n = size(u, 1)
    P = fill(fixed ? edge : 0.0, n + 2, n + 2)
    K = fill(fixed ? 1.0 : 0.0, n + 2, n + 2)
    P[2:n+1, 2:n+1] .= u
    K[2:n+1, 2:n+1] .= cell .!= WALL
    G = zeros(n + 2, n + 2)
    G[2:n+1, 2:n+1] .= cell .== FREE
    Q = copy(P)
    # Each task's columns and buffers are fixed for the whole call: allocating
    # inside the loop would call the GC, which must wait for every thread.
    span = map(1:ntasks) do t
        lo = 2 + (t - 1) * n ÷ ntasks; hi = 1 + t * n ÷ ntasks
        jlo = max(1, lo - HALO); jhi = min(n + 2, hi + HALO)
        (lo, hi, jlo, jhi)
    end
    As = [P[:, jlo:jhi] for (_, _, jlo, jhi) in span]
    Bs = copy.(As)
    Ks = [K[:, jlo:jhi] for (_, _, jlo, jhi) in span]
    Gs = [G[:, jlo:jhi] for (_, _, jlo, jhi) in span]
    left = steps
    while left > 0
        b = min(HALO, left)
        Threads.@threads :static for t in 1:ntasks
            lo, hi, jlo, jhi = span[t]
            A = As[t]; B = Bs[t]
            copyto!(A, @view P[:, jlo:jhi])
            for _ in 1:b
                step!(B, A, Ks[t], Gs[t], r)
                A, B = B, A
            end
            Q[:, lo:hi] .= @view A[:, lo-jlo+1:hi-jlo+1]
        end
        P, Q = Q, P
        left -= b
    end
    u .= @view P[2:n+1, 2:n+1]
end

function calibrate!()
    for n in GRIDS
        u = fill(AMBIENT, n, n); u[n÷2, n÷2] = HOT
        cell = fill(FREE, n, n)
        best = map(unique((1, team(n)))) do nt
            advance!(u, cell, 0.2, true, AMBIENT, HALO; ntasks = nt)
            minimum(@elapsed(advance!(u, cell, 0.2, true, AMBIENT, 4HALO; ntasks = nt)) for _ in 1:3) => nt
        end
        TASKS[n] = last(minimum(best))
        Log.info("$(n)² grid: " * join(["$(nt) task$(nt == 1 ? "" : "s") $(round(t / 4HALO * 1e6; digits = 1)) µs/step" for (t, nt) in best], ", ") * " -> $(TASKS[n])")
    end
end

# Colour map, temperature -> RGB: icy at 0 °C, near-black at the 20 °C
# ambient, through magenta and orange to white-hot at 100 °C, so both hot
# and cold paint stand out against an untouched plate.
const STOPS = [
    (0.0, (150, 235, 255)), (8.0, (50, 120, 230)), (20.0, (10, 12, 32)),
    (42.0, (110, 28, 120)), (62.0, (215, 55, 60)), (82.0, (255, 160, 40)),
    (100.0, (255, 250, 225)),
]
const LUT_N = 1024
const LUT = map(0:LUT_N-1) do k
    T = 100 * k / (LUT_N - 1)
    a = findlast(s -> s[1] <= T, STOPS)
    b = min(a + 1, length(STOPS))
    f = a == b ? 0.0 : (T - STOPS[a][1]) / (STOPS[b][1] - STOPS[a][1])
    ntuple(c -> round(UInt8, STOPS[a][2][c] + f * (STOPS[b][2][c] - STOPS[a][2][c])), 3)
end

function frame(sim::Sim)
    n = sim.n
    buf = Vector{UInt8}(undef, 4n * n)
    u, cell = sim.u, sim.cell
    Threads.@threads :static for j in 1:n
        @inbounds for i in 1:n
            k = 4 * ((j - 1) * n + i - 1) + 1
            if cell[i, j] == WALL
                # Hatched light grey, so a wall reads as material, not as a temperature.
                g = mod(i + j, 6) < 2 ? 0xd8 : 0xa8
                buf[k] = g; buf[k+1] = g; buf[k+2] = g + 0x10
            else
                T = u[i, j]
                r, g, b = LUT[clamp(round(Int, T / 100 * (LUT_N - 1)), 0, LUT_N - 1) + 1]
                # Isotherms every 10 °C: a faint line where a cell and its left or
                # lower neighbour fall in different 10-degree bands.
                band = floor(Int, T / 10)
                # Held cells carry a dot grid, so a heater reads apart from a hot spot.
                if (i > 1 && cell[i-1, j] != WALL && floor(Int, u[i-1, j] / 10) != band) ||
                   (j > 1 && cell[i, j-1] != WALL && floor(Int, u[i, j-1] / 10) != band) ||
                   (cell[i, j] == HELD && mod(i, 4) == 0 && mod(j, 4) == 0)
                    r = UInt8(min(255, r + 40)); g = UInt8(min(255, g + 40)); b = UInt8(min(255, b + 40))
                end
                buf[k] = r; buf[k+1] = g; buf[k+2] = b
            end
            buf[k+3] = 0xff
        end
    end
    buf
end

# Scenes are drawn in millimetres so every grid size gets the same plate.
function scene!(sim::Sim, name)
    n, h = sim.n, spacing(sim.n)
    fill!(sim.u, AMBIENT); fill!(sim.cell, FREE)
    sim.t = 0.0; sim.steps = 0
    if name == "gaussian"
        # One blob, the shape the variance check uses: sigma 6 mm, 80 degrees above ambient.
        for j in 1:n, i in 1:n
            x = (i - 0.5) * h - 50; y = (j - 0.5) * h - 50
            sim.u[i, j] = AMBIENT + 80 * exp(-(x^2 + y^2) / (2 * 6.0^2))
        end
    elseif name == "sample"
        # A held heater inside an insulating ring with one gap, a held cold
        # block, a baffle with a slot between them, and a hot bar that is not
        # held and fades: walls steer the heat on its way to a steady state.
        for j in 1:n, i in 1:n
            x = (i - 0.5) * h; y = (j - 0.5) * h
            d = hypot(x - 32, y - 36)
            ang = atan(y - 36, x - 32)
            if 15 <= d <= 18 && !(abs(ang) < 0.35)
                sim.cell[i, j] = WALL
            elseif d <= 7
                sim.u[i, j] = HOT; sim.cell[i, j] = HELD
            end
            if 66 <= x <= 84 && 18 <= y <= 32
                sim.u[i, j] = COLD; sim.cell[i, j] = HELD
            end
            if 60 <= x <= 63 && 40 <= y <= 92 && !(62 <= y <= 70)
                sim.cell[i, j] = WALL
            end
            if 70 <= x <= 90 && 70 <= y <= 76
                sim.u[i, j] = HOT
            end
        end
    end
    sim
end

const SIMS = Dict{String,Sim}()
const SIMS_LOCK = ReentrantLock()
const MAX_SIMS = 32

function session(sid, n)
    lock(SIMS_LOCK) do
        now = time()
        # Idle plates go after 15 minutes; past MAX_SIMS the stalest goes first.
        for (k, s) in collect(SIMS)
            now - s.seen > 900 && delete!(SIMS, k)
        end
        sim = get(SIMS, sid, nothing)
        if sim === nothing || sim.n != n
            if sim === nothing && length(SIMS) >= MAX_SIMS
                delete!(SIMS, argmin(k -> SIMS[k].seen, collect(keys(SIMS))))
            end
            sim = Sim(n, fill(AMBIENT, n, n), fill(FREE, n, n), 0.0, 0, now, ReentrantLock())
            scene!(sim, "sample")
            SIMS[sid] = sim
        end
        sim.seen = now
        sim
    end
end

# A stroke is a capsule: every cell within `r` mm of the segment (x0,y0)-(x1,y1).
# Hot and cold paint is held (a heater, a chiller) when `hold` is true.
function paint!(sim::Sim, s, hold::Bool)
    n, h = sim.n, spacing(sim.n)
    kind = string(get(s, :kind, "hot"))
    x0 = Float64(s[:x0]); y0 = Float64(s[:y0]); x1 = Float64(get(s, :x1, x0)); y1 = Float64(get(s, :y1, y0))
    rad = clamp(Float64(get(s, :r, 3.0)), 0.3, 30.0)
    all(isfinite, (x0, y0, x1, y1)) || throw(ArgumentError("paint coordinates must be numbers"))
    dx, dy = x1 - x0, y1 - y0
    L2 = dx^2 + dy^2
    ilo = max(1, floor(Int, (min(x0, x1) - rad) / h)); ihi = min(n, ceil(Int, (max(x0, x1) + rad) / h) + 1)
    jlo = max(1, floor(Int, (min(y0, y1) - rad) / h)); jhi = min(n, ceil(Int, (max(y0, y1) + rad) / h) + 1)
    for j in jlo:jhi, i in ilo:ihi
        x = (i - 0.5) * h; y = (j - 0.5) * h
        τ = L2 == 0 ? 0.0 : clamp(((x - x0) * dx + (y - y0) * dy) / L2, 0, 1)
        hypot(x - x0 - τ * dx, y - y0 - τ * dy) <= rad || continue
        if kind == "hot" || kind == "cold"
            sim.u[i, j] = kind == "hot" ? HOT : COLD
            sim.cell[i, j] = hold ? HELD : FREE
        elseif kind == "wall"
            sim.cell[i, j] = WALL
        elseif kind == "erase"
            sim.cell[i, j] == WALL && (sim.u[i, j] = AMBIENT)
            sim.cell[i, j] = FREE
        else
            throw(ArgumentError("paint kind must be hot, cold, wall or erase"))
        end
    end
end

num(b, key, default, lo, hi) = begin
    v = get(b, key, default)
    v isa Number && isfinite(v) || throw(ArgumentError("$key must be a number"))
    clamp(Float64(v), lo, hi)
end

function step_handler(req::HTTP.Request)
    b = JSON3.read(req.body)
    sid = string(get(b, :sid, ""))
    occursin(r"^[A-Za-z0-9_-]{8,64}$", sid) || throw(ArgumentError("sid must be 8-64 letters, digits, - or _"))
    n = Int(num(b, :grid, 256, 64, 512))
    n in GRIDS || throw(ArgumentError("grid must be one of $(join(GRIDS, ", "))"))
    alpha = num(b, :alpha, 97.0, 0.01, 500.0)
    dt_req = num(b, :dt, 1e-3, 1e-7, 1e3)
    bc = string(get(b, :bc, "fixed"))
    bc in ("fixed", "insulated") || throw(ArgumentError("bc must be fixed or insulated"))
    edge = num(b, :edge, AMBIENT, COLD, HOT)
    hold = get(b, :hold, true)
    hold isa Bool || throw(ArgumentError("hold must be true or false"))
    k = Int(num(b, :steps, 50, 0, 5000))
    k = min(k, MAX_WORK ÷ (n * n))

    lim = dt_limit(n, alpha)
    clipped = dt_req > lim
    dt = clipped ? CFL_SAFETY * lim : dt_req
    r = alpha * dt / spacing(n)^2

    sim = session(sid, n)
    ms = lock(sim.lock) do
        sc = get(b, :scene, nothing)
        sc === nothing || scene!(sim, string(sc))
        for s in get(b, :paint, ())
            paint!(sim, s, hold)
        end
        t0 = time_ns()
        advance!(sim.u, sim.cell, r, bc == "fixed", edge, k)
        sim.t += k * dt; sim.steps += k
        (time_ns() - t0) / 1e6
    end
    buf, lo, hi, mean = lock(sim.lock) do
        open = sim.cell .!= WALL
        cells = count(open)
        vals = sim.u[open]
        frame(sim), (cells == 0 ? AMBIENT : minimum(vals)), (cells == 0 ? AMBIENT : maximum(vals)),
            (cells == 0 ? AMBIENT : sum(vals) / cells)
    end
    HTTP.Response(200, ["Content-Type" => "application/octet-stream", "Cache-Control" => "no-store",
        "X-Dt" => string(dt), "X-Dt-Req" => string(dt_req), "X-Dt-Max" => string(lim), "X-R" => string(r),
        "X-Clipped" => clipped ? "1" : "0", "X-Time" => string(sim.t), "X-Steps" => string(k),
        "X-Total-Steps" => string(sim.steps), "X-Step-Ms" => string(round(ms; digits = 2)),
        "X-Mean" => string(round(mean; digits = 6)), "X-Min" => string(round(lo; digits = 3)),
        "X-Max" => string(round(hi; digits = 3)), "X-Grid" => string(n), "X-Tasks" => string(get(TASKS, n, 1)), "X-H" => string(spacing(n)),
        "X-Threads" => string(Threads.nthreads())], buf)
end

# The correctness check. A Gaussian of variance s0^2 per axis spreading under
# u_t = alpha ∇²u stays Gaussian with variance s0^2 + 2 alpha t per axis
# (the heat kernel), and its peak falls as s0^2 / (s0^2 + 2 alpha t) in 2-D.
# The blob is stepped by step! itself, with insulated edges so no heat leaves,
# on a plate wide enough (sigma ends at 9 mm, the edge is 50 mm off) that the
# domain behaves as infinite. Variance is measured from the field's second
# moment, so the check fails if the kernel's flux, r, or dt were wrong.
function check(alpha, n)
    h = spacing(n)
    dt = CFL_SAFETY * dt_limit(n, alpha)
    r = alpha * dt / h^2
    s0, s1 = 3.0, 9.0
    tend = (s1^2 - s0^2) / (2alpha)
    nsteps = ceil(Int, tend / dt)
    xs = [(i - 0.5) * h - PLATE_MM / 2 for i in 1:n]
    u = [exp(-(x^2 + y^2) / (2s0^2)) for x in xs, y in xs]
    cell = fill(FREE, n, n)
    function moments(u)
        m = sum(u)
        mx = sum(u .* xs) / m; my = sum(u .* xs') / m
        (mass = m, vx = sum(u .* (xs .- mx) .^ 2) / m, vy = sum(u .* (xs' .- my) .^ 2) / m, peak = maximum(u))
    end
    m0 = moments(u)
    var0 = (m0.vx + m0.vy) / 2
    rows = []
    marks = round.(Int, nsteps .* (0.0:0.2:1.0))
    done = 0
    t0 = time_ns()
    for target in marks
        advance!(u, cell, r, false, 0.0, target - done)
        done = target
        t = done * dt
        m = moments(u)
        pred = var0 + 2alpha * t
        meas = (m.vx + m.vy) / 2
        push!(rows, (t = t, steps = done, var_x = m.vx, var_y = m.vy, predicted = pred,
                     rel_err = abs(meas - pred) / pred,
                     peak = m.peak / m0.peak, peak_predicted = var0 / pred,
                     mass_drift = abs(m.mass - m0.mass) / m0.mass))
    end
    ms = (time_ns() - t0) / 1e6
    worst = maximum(r -> r.rel_err, rows)
    peak_worst = maximum(r -> abs(r.peak - r.peak_predicted) / r.peak_predicted, rows)
    (alpha = alpha, grid = n, h = h, dt = dt, r = r, var0 = var0, steps = nsteps, ms = round(ms; digits = 1),
     rows = rows, max_rel_err = worst, peak_max_rel_err = peak_worst,
     pass = worst < 1e-3 && peak_worst < 2e-2)
end

function check_handler(req::HTTP.Request)
    q = HTTP.queryparams(HTTP.URI(req.target))
    alpha = something(tryparse(Float64, get(q, "alpha", "97")), NaN)
    isfinite(alpha) && 0.01 <= alpha <= 500 || throw(ArgumentError("alpha must be a number in 0.01..500"))
    n = something(tryparse(Int, get(q, "grid", "256")), 0)
    n in GRIDS || throw(ArgumentError("grid must be one of $(join(GRIDS, ", "))"))
    json(check(alpha, n))
end

json(x; status = 200) = HTTP.Response(status, ["Content-Type" => "application/json", "Cache-Control" => "no-store"], JSON3.write(x))

page(_) = HTTP.Response(200, ["Content-Type" => "text/html; charset=utf-8", "Cache-Control" => "no-store"],
                        read(joinpath(ROOT, "static", "index.html")))

const router = HTTP.Router()
HTTP.register!(router, "GET", "/", page)
HTTP.register!(router, "POST", "/api/step", step_handler)
HTTP.register!(router, "GET", "/api/check", check_handler)
HTTP.register!(router, "GET", "/info", _ -> json((threads = Threads.nthreads(), materials = MATERIALS, grids = GRIDS,
                                                  plate_mm = PLATE_MM, cfl_safety = CFL_SAFETY)))
HTTP.register!(router, "GET", "/health", _ -> HTTP.Response(200, "ok"))

message(err) = replace(first(split(sprint(showerror, err), "\n\n")), r"\s*\n\s*" => " ")

function guarded(handler)
    return function (req)
        try
            handler(req)
        catch err
            # A missing field (KeyError) is the caller's mistake too, not the server's.
            bad = err isa ArgumentError || err isa KeyError
            bad || Log.error("$(req.method) $(req.target) failed: $(message(err))")
            json((error = message(err),); status = bad ? 400 : 500)
        end
    end
end

const app = guarded(router)

const WARM = time()
# Warm the compiler in-process (one step, one check) so the first visitor does
# not wait, and log the check so a broken kernel shows in the log at start.
app(HTTP.Request("POST", "/api/step", [], JSON3.write((sid = "warmup00", grid = 128, steps = 2,
    paint = [(kind = "wall", x0 = 10, y0 = 10, x1 = 20, y1 = 20, r = 2)]))))
lock(SIMS_LOCK) do; empty!(SIMS) end
calibrate!()
let c = check(97.0, 128)
    (c.pass ? Log.success : Log.error)("check: variance vs var0 + 2·alpha·t, worst relative error $(round(c.max_rel_err; sigdigits = 3)) ($(c.pass ? "pass" : "FAIL"))")
end

Log.info("compiled the handlers in $(round(time() - WARM; digits = 1)) s")

# One log line per request. /api/step runs every animation frame,
# so its lines are DEBUG (LOG_LEVEL=DEBUG shows them).
function logged(handler)
    return function (req)
        t0 = time()
        res = handler(req)
        path = first(split(req.target, '?'))
        if res.status < 400 && (path == "/api/step")
            Log.debug("$(req.method) $path $(res.status) $(round(Int, (time() - t0) * 1000)) ms")
        else
            Log.request(req, res.status, t0)
        end
        res
    end
end

Log.info("__HOST__: Heat Lab serving on 127.0.0.1:$PORT with $(Threads.nthreads()) threads")
HTTP.serve(logged(app), "127.0.0.1", PORT)
