# Pattern Lab: Gray-Scott reaction-diffusion computed in Julia on every core
# and sent to the page as one byte per cell, which the page colours.
#
#   ∂u/∂t = Du ∇²u - u v² + F (1 - u)        u: the substrate, fed in at rate F
#   ∂v/∂t = Dv ∇²v + u v² - (F + k) v        v: the catalyst, removed at rate F + k
#
# Pearson, J. E. (1993), "Complex patterns in a simple system", Science
# 261(5118): 189-192. Forward Euler, a five-point Laplacian, periodic edges and
# a unit time step, as in the paper; its Du = 2e-5 and Dv = 1e-5 on a 2.5-wide
# square of 256 cells become Du = 0.2097, Dv = 0.105 per cell.
#
#   GET  /            the page (static/index.html, read on every request)
#   POST /api/frame   {id, w, h, f, k, du, dv, steps, reset?, seed?, drops?, r?}
#                     -> w*h bytes, v scaled to 0-255, row by row from the top;
#                        headers X-Step-Ms, X-Threads, X-Size, X-Steps
#   GET  /api/info    {threads, presets, du, dv, max_cells, max_sims}
#   GET  /health      "ok"
#
# The grid lives on the server, one per page, keyed by an id the page makes.
# A frame is one byte per cell, while the state is two Float32 per cell: a
# stateless server would need eight times the frame sent back up on every
# frame. The table holds at most MAX_SIMS grids; a new page evicts the one
# idle longest, which then restarts from a fresh seed on its next frame.

using HTTP, JSON3, Random

const ROOT = @__DIR__
# run.sh sets PORT from app.env's APP_PORT; a second default here would
# serve on a port no other part of the project knows.
const PORT = parse(Int, ENV["PORT"])
const MAX_CELLS = 300_000      # about 1.2 MB of state per grid on each side of the swap
const MAX_SIMS = 16
const DU, DV = 0.2097, 0.105

# Feed and kill rates for the classic regimes, as catalogued from Pearson's
# (F, k) map by Munafo and Sims. Worms is the exception: Sims's 0.078/0.061
# dies out to an empty dish on Pearson's stencil (measured 2026-10-26, from
# 20- and 40-cell seeds), so it takes 0.050/0.065 from the worm region near
# Pearson's kappa, which grows long worms from the same seeds.
const PRESETS = [
    (name = "Mitosis",  f = 0.0367, k = 0.0649),
    (name = "Coral",    f = 0.0545, k = 0.0620),
    (name = "Worms",    f = 0.0500, k = 0.0650),
    (name = "Maze",     f = 0.0290, k = 0.0570),
    (name = "Solitons", f = 0.0300, k = 0.0620),
]

mutable struct Sim
    u::Matrix{Float32}       # indexed [x, y]: x runs fastest, so memory is row by row
    v::Matrix{Float32}
    u2::Matrix{Float32}      # the next step is written here, then the pairs swap
    v2::Matrix{Float32}
    steps::Int
    seen::Float64
    lock::ReentrantLock
end

function Sim(w, h)
    Sim(ones(Float32, w, h), zeros(Float32, w, h), Matrix{Float32}(undef, w, h), Matrix{Float32}(undef, w, h),
        0, time(), ReentrantLock())
end

# Pearson's start: u = 1/2, v = 1/4 inside a square, u = 1, v = 0 outside,
# all of it perturbed by ±1% noise to break the square's symmetry. "pearson"
# places his one 20-cell square in the middle; "scatter" places one square per
# few thousand cells, so a wide screen fills in seconds instead of minutes;
# "blank" leaves the dish empty for the pointer.
function reset!(s::Sim, pattern::AbstractString, seed::Int)
    w, h = size(s.u)
    rng = Xoshiro(seed)
    fill!(s.u, 1f0); fill!(s.v, 0f0)
    square!(x, y, side) = for j in y:y+side-1, i in x:x+side-1
        s.u[mod1(i, w), mod1(j, h)] = 0.5f0
        s.v[mod1(i, w), mod1(j, h)] = 0.25f0
    end
    if pattern == "pearson"
        square!(w ÷ 2 - 9, h ÷ 2 - 9, 20)
    elseif pattern == "scatter"
        for _ in 1:max(3, (w * h) ÷ 5000)
            square!(rand(rng, 1:w), rand(rng, 1:h), rand(rng, 6:16))
        end
    end
    if pattern != "blank"
        @inbounds for i in eachindex(s.u)
            s.u[i] *= 1f0 + 0.01f0 * (2rand(rng, Float32) - 1)
            s.v[i] *= 1f0 + 0.01f0 * (2rand(rng, Float32) - 1)
        end
    end
    s.steps = 0
    s
end

# A drop of catalyst where the pointer touched: a disc of Pearson's seed values.
function drop!(s::Sim, fx, fy, r)
    w, h = size(s.u)
    cx, cy = round(Int, fx * w), round(Int, fy * h)
    ri = ceil(Int, r)
    for dy in -ri:ri, dx in -ri:ri
        dx^2 + dy^2 <= r^2 || continue
        i, j = mod1(cx + dx, w), mod1(cy + dy, h)
        s.u[i, j] = 0.5f0
        s.v[i, j] = 0.25f0
    end
end

# One cell of one Euler step, from (u, v) into (u2, v2).
@inline function cell!(u2, v2, u, v, x, xm, xp, y, ym, yp, f, k, du, dv)
    @inbounds begin
        a = u[x, y]; b = v[x, y]
        lu = u[xm, y] + u[xp, y] + u[x, ym] + u[x, yp] - 4a
        lv = v[xm, y] + v[xp, y] + v[x, ym] + v[x, yp] - 4b
        abb = a * b * b
        u2[x, y] = a + du * lu - abb + f * (1 - a)
        v2[x, y] = b + dv * lv + abb - (f + k) * b
    end
end

# One step over the grid. Each task takes whole rows; a row reads the rows
# above and below and writes only its own. The two edge cells wrap around and
# are done apart, so the interior loop has no branch and vectorises (measured
# 2026-10-26 at 640x360: 0.52 ms a step on one thread against 1.0 with the wrap
# test in the loop). :dynamic lets an idle thread take rows from one that the
# operating system has paused.
function kernel!(u2, v2, u, v, f::Float32, k::Float32, du::Float32, dv::Float32)
    w, h = size(u)
    Threads.@threads :dynamic for y in 1:h
        ym = y == 1 ? h : y - 1
        yp = y == h ? 1 : y + 1
        cell!(u2, v2, u, v, 1, w, 2, y, ym, yp, f, k, du, dv)
        @simd for x in 2:w-1
            cell!(u2, v2, u, v, x, x - 1, x + 1, y, ym, yp, f, k, du, dv)
        end
        cell!(u2, v2, u, v, w, w - 1, 1, y, ym, yp, f, k, du, dv)
    end
end

function advance!(s::Sim, n, f, k, du, dv)
    for _ in 1:n
        kernel!(s.u2, s.v2, s.u, s.v, Float32(f), Float32(k), Float32(du), Float32(dv))
        s.u, s.u2 = s.u2, s.u
        s.v, s.v2 = s.v2, s.v
    end
    s.steps += n
    s
end

# v rarely passes 0.4 in any regime; scaling by 2.5 spends the byte's range
# on the values that occur.
frame(s::Sim) = [round(UInt8, 255 * clamp(2.5f0 * x, 0f0, 1f0)) for x in s.v]

const SIMS = Dict{String,Sim}()
const SIMS_LOCK = ReentrantLock()

# The page's grid, made (or remade, when the window changed size) on demand.
function sim_for(id, w, h)
    lock(SIMS_LOCK) do
        s = get(SIMS, id, nothing)
        if s === nothing || size(s.u) != (w, h)
            if s === nothing && length(SIMS) >= MAX_SIMS
                delete!(SIMS, argmin(k -> SIMS[k].seen, keys(SIMS)))
            end
            s = SIMS[id] = reset!(Sim(w, h), "scatter", 1)
        end
        s.seen = time()
        s
    end
end

function num(body, key, default, lo, hi)
    v = get(body, key, default)
    v isa Real && isfinite(v) || throw(ArgumentError("$key must be a number"))
    clamp(Float64(v), lo, hi)
end

function frame_handler(req::HTTP.Request)
    body = isempty(req.body) ? JSON3.Object() : JSON3.read(req.body)
    id = string(get(body, :id, ""))
    occursin(r"^[A-Za-z0-9_-]{8,64}$", id) || throw(ArgumentError("id must be 8-64 letters, digits, - or _"))
    w = round(Int, num(body, :w, 256, 16, 2048))
    h = round(Int, num(body, :h, 256, 16, 2048))
    w * h <= MAX_CELLS || throw(ArgumentError("w*h must be at most $MAX_CELLS cells"))
    f = num(body, :f, 0.0367, 0, 0.12)
    k = num(body, :k, 0.0649, 0, 0.08)
    # Explicit Euler on this stencil is stable only while D * dt <= 1/4.
    du = num(body, :du, DU, 0.01, 0.25)
    dv = num(body, :dv, DV, 0.01, 0.25)
    n = round(Int, num(body, :steps, 16, 0, 200))
    r = num(body, :r, 4, 1, 40)

    s = sim_for(id, w, h)
    ms = lock(s.lock) do
        reset = get(body, :reset, nothing)
        if reset !== nothing
            reset in ("pearson", "scatter", "blank") || throw(ArgumentError("reset must be pearson, scatter or blank"))
            reset!(s, reset, round(Int, num(body, :seed, 1, 0, 1e9)))
        end
        drops = get(body, :drops, ())
        for d in Iterators.take(drops, 256)
            length(d) == 2 || throw(ArgumentError("each drop is [x, y], fractions of the width and height"))
            drop!(s, clamp(Float64(d[1]), 0, 1), clamp(Float64(d[2]), 0, 1), r)
        end
        t = time_ns()
        advance!(s, n, f, k, du, dv)
        n == 0 ? 0.0 : (time_ns() - t) / 1e6 / n
    end
    HTTP.Response(200, ["Content-Type" => "application/octet-stream", "Cache-Control" => "no-store",
                        "X-Step-Ms" => string(round(ms; digits = 3)), "X-Threads" => string(Threads.nthreads()),
                        "X-Size" => "$(w)x$(h)", "X-Steps" => string(s.steps)], frame(s))
end

json(x; status = 200) = HTTP.Response(status, ["Content-Type" => "application/json", "Cache-Control" => "no-store"], JSON3.write(x))

page(_) = HTTP.Response(200, ["Content-Type" => "text/html; charset=utf-8", "Cache-Control" => "no-store"],
                        read(joinpath(ROOT, "static", "index.html")))

info(_) = json((threads = Threads.nthreads(), presets = PRESETS, du = DU, dv = DV, max_cells = MAX_CELLS, max_sims = MAX_SIMS))

const router = HTTP.Router()
HTTP.register!(router, "GET", "/", page)
HTTP.register!(router, "POST", "/api/frame", frame_handler)
HTTP.register!(router, "GET", "/api/info", info)
HTTP.register!(router, "GET", "/health", _ -> HTTP.Response(200, "ok"))

message(err) = replace(first(split(sprint(showerror, err), "\n\n")), r"\s*\n\s*" => " ")

function guarded(handler)
    return function (req)
        try
            handler(req)
        catch err
            bad = err isa ArgumentError || err isa MethodError || err isa InexactError
            bad || @error "request failed" exception = (err, catch_backtrace())
            json((error = message(err),); status = bad ? 400 : 500)
        end
    end
end

const app = guarded(router)

# Run a small grid through a reset, a drop and a few steps in-process so the
# first visitor does not wait for the compiler, then forget it.
app(HTTP.Request("POST", "/api/frame", ["Content-Type" => "application/json"],
                 """{"id":"warmup-grid","w":64,"h":48,"steps":4,"reset":"pearson","drops":[[0.5,0.5]]}"""))
app(HTTP.Request("GET", "/api/info"))
lock(() -> delete!(SIMS, "warmup-grid"), SIMS_LOCK)

println("Pattern Lab listening on http://127.0.0.1:$PORT with $(Threads.nthreads()) threads")
flush(stdout)
HTTP.serve(app, "127.0.0.1", PORT)
