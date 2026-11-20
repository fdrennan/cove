# Optimizer Race: six optimizers start from the same point on a classic test
# surface and race to its minimum. Every derivative comes from ForwardDiff
# (forward-mode automatic differentiation through dual numbers); no gradient
# or Hessian in this file is derived by hand.
#
#   GET  /              the page (static/index.html, read on every request)
#   GET  /api/surfaces  [{id, label, src, xlim, ylim, minima, start}]
#   GET  /api/surface   ?id&n -> {id, n, xlim, ylim, z (log10(1+f), row-major,
#                       y up), levels, contours: [[x1,y1,x2,y2,...] per level]}
#   POST /api/race      {surface, x, y, iters, scale}
#                       -> {start: {f, grad, hess, eig, fd_err}, runs: [{name,
#                          steps, path: [[x, y, step], ...], x, y, f, gnorm,
#                          status, nearest, dist, fcalls, gcalls, hcalls, micros}],
#                          millis, threads}
#   GET  /health        "ok"

include(joinpath(@__DIR__, "log.jl"))
using HTTP, JSON3, Optim, ForwardDiff, LinearAlgebra

const ROOT = @__DIR__
# run.sh sets PORT from app.env's APP_PORT; a second default here would
# serve on a port no other part of the project knows.
const PORT = parse(Int, ENV["PORT"])

# Each surface is written once, as Julia source text: the page shows that text
# and the server parses it into the function it differentiates, so what the
# visitor reads is what runs. `lr` is the fixed step for gradient descent and
# momentum, `alr` Adam's; both scale with the step slider.
const SPECS = [
    (id = "rosenbrock", label = "Rosenbrock",
     src = "(1 - x)^2 + 100 * (y - x^2)^2",
     xlim = (-2.0, 2.0), ylim = (-1.0, 3.0), start = (-1.2, 1.0), lr = 2e-4, alr = 0.02,
     minima = [(1.0, 1.0)]),
    (id = "himmelblau", label = "Himmelblau",
     src = "(x^2 + y - 11)^2 + (x + y^2 - 7)^2",
     xlim = (-5.0, 5.0), ylim = (-5.0, 5.0), start = (-0.3, -0.9), lr = 2e-3, alr = 0.05,
     minima = [(3.0, 2.0), (-2.805118, 3.131312), (-3.779310, -3.283186), (3.584428, -1.848126)]),
    (id = "rastrigin", label = "Rastrigin",
     src = "20 + x^2 + y^2 - 10 * (cos(2pi * x) + cos(2pi * y))",
     xlim = (-5.12, 5.12), ylim = (-5.12, 5.12), start = (2.3, -1.6), lr = 2e-3, alr = 0.05,
     minima = [(0.0, 0.0)]),
    (id = "beale", label = "Beale",
     src = "(1.5 - x + x*y)^2 + (2.25 - x + x*y^2)^2 + (2.625 - x + x*y^3)^2",
     xlim = (-4.5, 4.5), ylim = (-4.5, 4.5), start = (1.0, 1.4), lr = 1e-3, alr = 0.05,
     minima = [(3.0, 0.5)]),
]

# All four surfaces live in one callable type, so Optim and ForwardDiff
# compile their machinery once rather than once per surface (a closure per
# surface cost a two-minute start). The body is spliced in from `src` with
# @eval, and stays generic: it runs on Float64 and on ForwardDiff's dual
# numbers alike.
struct Surface <: Function
    k::Int
end
let chain = foldr((i, rest) -> :(s.k == $i ? $(Meta.parse(SPECS[i].src)) : $rest), eachindex(SPECS);
                  init = :(throw(ArgumentError("unknown surface"))))
    @eval (s::Surface)(v) = (x = v[1]; y = v[2]; $chain)
end
const FUNS = Dict(s.id => Surface(i) for (i, s) in enumerate(SPECS))
const SPEC = Dict(s.id => s for s in SPECS)

# --- the surface as a picture ----------------------------------------------

"""
    grid(f, xlim, ylim, n)

log10(1 + f) on an n x n grid, row 1 at the top (largest y). The log turns
Rosenbrock's 0..2500 range into something a colour map can show.
"""
function grid(f, xlim, ylim, n)
    xs = range(xlim..., length = n); ys = range(ylim[2], ylim[1], length = n)
    z = Matrix{Float64}(undef, n, n)
    Threads.@threads for i in 1:n
        for j in 1:n
            z[i, j] = log10(1 + f((xs[j], ys[i])))
        end
    end
    z, xs, ys
end

"""
    contours(z, xs, ys, levels)

Marching squares: for each level, the line segments where the field crosses
it, flattened to [x1, y1, x2, y2, ...] so the page draws them in one path.
Levels run in parallel, one task each.
"""
function contours(z, xs, ys, levels)
    n, m = size(z)
    out = Vector{Vector{Float64}}(undef, length(levels))
    Threads.@threads for li in eachindex(levels)
        lv = levels[li]
        seg = Float64[]
        # Where the level crosses the edge from corner p (value zp) to corner q.
        at(zp, zq, x1, y1, x2, y2) = (t = (lv - zp) / (zq - zp); (x1 + t * (x2 - x1), y1 + t * (y2 - y1)))
        for i in 1:n-1, j in 1:m-1
            a, b, c, d = z[i, j], z[i, j+1], z[i+1, j+1], z[i+1, j]   # tl, tr, br, bl
            x1, x2, y1, y2 = xs[j], xs[j+1], ys[i], ys[i+1]
            k = 0
            for (zp, zq, px, py, qx, qy) in ((a, b, x1, y1, x2, y1), (b, c, x2, y1, x2, y2),
                                             (c, d, x2, y2, x1, y2), (d, a, x1, y2, x1, y1))
                (zp < lv) == (zq < lv) && continue
                cx, cy = at(zp, zq, px, py, qx, qy)
                push!(seg, round(cx; digits = 3), round(cy; digits = 3))
                k += 1
            end
            # A saddle cell crosses all four edges: two segments, already paired
            # in order. An odd count cannot happen; drop it if rounding made one.
            isodd(k) && resize!(seg, length(seg) - 2)
        end
        out[li] = seg
    end
    out
end

function surface(req::HTTP.Request)
    q = HTTP.queryparams(HTTP.URI(req.target))
    s = get(SPEC, get(q, "id", "rosenbrock"), nothing)
    s === nothing && throw(ArgumentError("unknown surface"))
    n = clamp(something(tryparse(Int, get(q, "n", "200")), 200), 32, 600)
    z, xs, ys = grid(FUNS[s.id], s.xlim, s.ylim, n)
    lo, hi = extrema(z)
    levels = collect(range(lo, hi, length = 16)[2:end-1])
    json((id = s.id, n = n, xlim = s.xlim, ylim = s.ylim, zmin = lo, zmax = hi,
          z = round.(vec(permutedims(z)); digits = 3), levels = levels,
          contours = contours(z, xs, ys, levels)))
end

surfaces(_) = json([(id = s.id, label = s.label, src = s.src, xlim = s.xlim, ylim = s.ylim,
                     start = s.start, minima = [(x, y, FUNS[s.id]([x, y])) for (x, y) in s.minima])
                    for s in SPECS])

# --- the racers --------------------------------------------------------------

const GTOL = 1e-6        # stop once the gradient is this small
const FAR = 1e6          # a run this far out has diverged

"""
    firstorder(f, x0, iters, rule)

The three fixed-step methods share one loop: ForwardDiff gives the gradient,
`rule` turns it into a step. Returns the path (one column per step) and how
the run ended.
"""
function firstorder(f, x0, iters, rule)
    x = copy(x0); g = similar(x)
    cfg = ForwardDiff.GradientConfig(f, x)
    path = [copy(x)]
    state = rule(nothing, g)
    status = "max steps"
    for k in 1:iters
        ForwardDiff.gradient!(g, f, x, cfg)
        if norm(g) < GTOL
            status = "converged"; break
        end
        x .-= rule(state, g)
        push!(path, copy(x))
        if !all(isfinite, x) || norm(x) > FAR
            status = "diverged"; break
        end
    end
    path, status, length(path) - 1
end

# Each rule is called once with `nothing` to build its state, then once per
# step with that state. Plain gradient descent keeps none: an empty tuple.
gd(lr) = (st, g) -> st === nothing ? () : lr .* g

function momentum(lr, β = 0.9)            # Polyak's heavy ball
    (st, g) -> st === nothing ? zeros(length(g)) : (st .= β .* st .+ lr .* g; st)
end

function adam(lr, β1 = 0.9, β2 = 0.999, ϵ = 1e-8)   # Kingma and Ba (2015)
    (st, g) -> begin
        st === nothing && return (m = zeros(length(g)), v = zeros(length(g)), t = Ref(0))
        st.t[] += 1; t = st.t[]
        st.m .= β1 .* st.m .+ (1 - β1) .* g
        st.v .= β2 .* st.v .+ (1 - β2) .* g .^ 2
        lr .* (st.m ./ (1 - β1^t)) ./ (sqrt.(st.v ./ (1 - β2^t)) .+ ϵ)
    end
end

"""
    optim_run(f, x0, iters, method)

Nelder-Mead, BFGS and Newton from Optim.jl. The gradient and Hessian Optim
asks for are ForwardDiff's; the trace gives the path (for Nelder-Mead, the
simplex centroid at each step, then the best vertex it ends on).
"""
function optim_run(f, x0, iters, method)
    opts = Optim.Options(store_trace = true, extended_trace = true, iterations = iters, g_tol = GTOL * GTOL)
    if method isa NelderMead
        r = optimize(f, x0, method, opts)
        path = [[x0]; [Vector{Float64}(t.metadata["centroid"]) for t in Optim.trace(r)[2:end]]; [Optim.minimizer(r)]]
    else
        g!(G, x) = ForwardDiff.gradient!(G, f, x)
        h!(H, x) = ForwardDiff.hessian!(H, f, x)
        r = optimize(TwiceDifferentiable(f, g!, h!, x0), x0, method, opts)
        path = [[x0]; [Vector{Float64}(t.metadata["x"]) for t in Optim.trace(r)[2:end]]]
    end
    x = Optim.minimizer(r)
    status = !all(isfinite, x) || norm(x) > FAR ? "diverged" : Optim.converged(r) ? "converged" : "max steps"
    path, status, Optim.iterations(r), (Optim.f_calls(r), Optim.g_calls(r), Optim.h_calls(r))
end

"""Keep every early step (where the action is) and evenly spaced later ones."""
function thin(path, maxpts = 700)
    n = length(path)
    keep = n <= maxpts ? (1:n) : sort!(unique!([1:min(n, 200); round.(Int, range(1, n, length = maxpts - 200)); n]))
    [(round(path[k][1]; sigdigits = 7), round(path[k][2]; sigdigits = 7), k - 1) for k in keep]
end

function race(req::HTTP.Request)
    body = isempty(req.body) ? JSON3.Object() : JSON3.read(req.body)
    s = get(SPEC, string(get(body, :surface, "rosenbrock")), nothing)
    s === nothing && throw(ArgumentError("unknown surface"))
    f = FUNS[s.id]
    num(key, d, lo, hi) = (v = Float64(get(body, key, d)); isfinite(v) || throw(ArgumentError("$key must be a number")); clamp(v, lo, hi))
    x0 = [num(:x, s.start[1], s.xlim...), num(:y, s.start[2], s.ylim...)]
    iters = round(Int, num(:iters, 5000, 10, 50_000))
    scale = num(:scale, 1, 0.01, 100)

    # What automatic differentiation says at the start: exact derivatives, set
    # against a central difference to show the gap a hand-rolled one leaves.
    gr = ForwardDiff.gradient(f, x0)
    H = ForwardDiff.hessian(f, x0)
    h = 1e-5
    fd = [(f(x0 .+ h .* e) - f(x0 .- h .* e)) / 2h for e in ([1.0, 0.0], [0.0, 1.0])]

    racers = [("Gradient descent", () -> firstorder(f, x0, iters, gd(s.lr * scale))),
              ("Momentum", () -> firstorder(f, x0, iters, momentum(s.lr * scale))),
              ("Adam", () -> firstorder(f, x0, iters, adam(s.alr * scale))),
              ("Nelder-Mead", () -> optim_run(f, x0, iters, NelderMead())),
              ("BFGS", () -> optim_run(f, x0, iters, BFGS())),
              ("Newton", () -> optim_run(f, x0, iters, Newton()))]
    t0 = time_ns()
    # One racer per thread: the six runs are independent.
    runs = Vector{Any}(undef, length(racers))
    Threads.@threads for i in eachindex(racers)
        name, go = racers[i]
        t = time_ns()
        out = go()
        micros = round(Int, (time_ns() - t) / 1000)
        path, status, steps = out[1], out[2], out[3]
        x = path[end]
        calls = length(out) == 4 ? out[4] : (0, steps + (status == "converged"), 0)
        fx = f(x)
        gn = all(isfinite, x) ? norm(ForwardDiff.gradient(f, x)) : NaN
        d, k = all(isfinite, x) ? findmin(m -> hypot(x[1] - m[1], x[2] - m[2]), s.minima) : (Inf, 1)
        runs[i] = (name = name, steps = steps, path = thin(path), x = x[1], y = x[2],
                   f = isfinite(fx) ? fx : nothing, gnorm = isfinite(gn) ? gn : nothing, status = status,
                   nearest = k, dist = isfinite(d) ? d : nothing,
                   fcalls = calls[1], gcalls = calls[2], hcalls = calls[3], micros = micros)
    end
    json((start = (x = x0, f = f(x0), grad = gr, hess = [H[1, :], H[2, :]], eig = eigvals(Symmetric(H)),
                   fd_err = maximum(abs.(fd .- gr))),
          runs = runs, millis = round((time_ns() - t0) / 1e6; digits = 2), threads = Threads.nthreads()))
end

json(x; status = 200) = HTTP.Response(status, ["Content-Type" => "application/json", "Cache-Control" => "no-store"], JSON3.write(x))

page(_) = HTTP.Response(200, ["Content-Type" => "text/html; charset=utf-8", "Cache-Control" => "no-store"],
                        read(joinpath(ROOT, "static", "index.html")))

const router = HTTP.Router()
HTTP.register!(router, "GET", "/", page)
HTTP.register!(router, "GET", "/api/surfaces", surfaces)
HTTP.register!(router, "GET", "/api/surface", surface)
HTTP.register!(router, "POST", "/api/race", race)
HTTP.register!(router, "GET", "/health", _ -> HTTP.Response(200, "ok"))

message(err) = replace(first(split(sprint(showerror, err), "\n\n")), r"\s*\n\s*" => " ")

function guarded(handler)
    return function (req)
        try
            handler(req)
        catch err
            bad = err isa ArgumentError || err isa MethodError || err isa InexactError || err isa KeyError
            bad || Log.error("$(req.method) $(req.target) failed: $(message(err))")
            json((error = message(err),); status = bad ? 400 : 500)
        end
    end
end

const app = guarded(router)

const WARM = time()
# Draw and race once in-process so the first visitor does not wait for the
# compiler; one surface warms them all, since they share the Surface type.
app(HTTP.Request("GET", "/api/surface?id=rosenbrock&n=40"))
app(HTTP.Request("POST", "/api/race", ["Content-Type" => "application/json"], "{\"iters\":50}"))

Log.info("compiled the handlers in $(round(time() - WARM; digits = 1)) s")

# One log line per request: method, path, status, milliseconds.
function logged(handler)
    return function (req)
        t0 = time()
        res = handler(req)
        Log.request(req, res.status, t0)
        res
    end
end

Log.info("__HOST__: Optimizer Race serving on 127.0.0.1:$PORT with $(Threads.nthreads()) threads")
HTTP.serve(logged(app), "127.0.0.1", PORT)
