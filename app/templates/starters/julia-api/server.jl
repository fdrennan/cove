# Scientific API: statistics, probability distributions, regression and a
# differential equation, answered by Julia over HTTP with JSON in and out.
#
#   GET  /                        the page that calls every endpoint live
#   GET  /api                     the list of endpoints
#   POST /stats                   {numbers:[...]}            -> summary, quantiles, histogram
#   GET  /distributions/{name}    ?params                    -> pdf/pmf and cdf over the support
#   POST /fit/linear              {x:[...], y:[...], degree} -> coefficients, R², confidence band
#   GET  /ode/lotka-volterra      ?alpha&beta&gamma&delta&prey&predator&tmax -> the series
#   GET  /health                  "ok"

include(joinpath(@__DIR__, "log.jl"))
using HTTP, JSON3
using Statistics, StatsBase, Distributions, GLM, OrdinaryDiffEq

const ROOT = @__DIR__
# run.sh sets PORT from app.env's APP_PORT; a second default here would
# serve on a port no other part of the project knows.
const PORT = parse(Int, ENV["PORT"])

struct BadRequest <: Exception
    msg::String
end
Base.showerror(io::IO, e::BadRequest) = print(io, e.msg)
bad(msg) = throw(BadRequest(msg))

# JSON has no NaN or Inf: a statistic that is undefined goes out as null.
clean(x::Integer) = x
clean(x::Real) = isfinite(x) ? Float64(x) : nothing
clean(x::Union{AbstractArray,NamedTuple}) = map(clean, x)
clean(x::AbstractDict) = Dict(k => clean(v) for (k, v) in x)
clean(x) = x

# 0.05 -> "5", 0.025 -> "2.5": the key a client looks a quantile up by.
pct(p) = (v = round(100p; digits = 6); isinteger(v) ? string(Int(v)) : string(v))

json(x; status = 200) = HTTP.Response(status, ["Content-Type" => "application/json", "Cache-Control" => "no-store"], JSON3.write(x))

body(req) = isempty(req.body) ? JSON3.Object() : JSON3.read(req.body)

function numbers(v, name)
    v isa AbstractVector || bad("$name must be an array of numbers")
    all(x -> x isa Real && isfinite(x), v) || bad("$name must hold only finite numbers")
    Float64.(v)
end

query(req) = HTTP.queryparams(HTTP.URI(req.target))

function qnum(q, key, default; lo = -Inf, hi = Inf)
    s = get(q, key, nothing)
    v = s === nothing ? Float64(default) : something(tryparse(Float64, s), NaN)
    isfinite(v) || bad("$key must be a number")
    lo <= v <= hi || bad("$key must lie in [$lo, $hi]")
    v
end

# ---------------------------------------------------------------- /stats

function stats(req)
    x = numbers(get(body(req), :numbers, nothing), "numbers")
    length(x) >= 2 || bad("numbers needs at least two values")
    length(x) <= 1_000_000 || bad("at most 1,000,000 numbers")
    probs = [0.05, 0.25, 0.5, 0.75, 0.95]
    h = fit(Histogram, x; nbins = clamp(round(Int, sqrt(length(x))), 5, 40))
    json(clean((
        n = length(x),
        mean = mean(x), std = std(x), var = var(x), sem = sem(x),
        median = median(x), min = minimum(x), max = maximum(x),
        mad = mad(x; normalize = true), iqr = iqr(x),
        skewness = skewness(x), kurtosis = kurtosis(x),
        quantiles = Dict(pct(p) => q for (p, q) in zip(probs, quantile(x, probs))),
        histogram = (edges = collect(h.edges[1]), counts = h.weights),
    )))
end

# ---------------------------------------------------------- /distributions

# name => (constructor, [(parameter, default)...]). Every parameter is a query
# string value; the order is the constructor's argument order.
const DISTRIBUTIONS = Dict(
    "normal"      => (Normal,           [("mu", 0.0), ("sigma", 1.0)]),
    "lognormal"   => (LogNormal,        [("mu", 0.0), ("sigma", 0.5)]),
    "gamma"       => (Gamma,            [("shape", 2.0), ("scale", 2.0)]),
    "beta"        => (Beta,             [("alpha", 2.0), ("beta", 5.0)]),
    "exponential" => (Exponential,      [("scale", 1.0)]),
    "studentt"    => (TDist,            [("nu", 4.0)]),
    "chisq"       => (Chisq,            [("k", 3.0)]),
    "cauchy"      => (Cauchy,           [("location", 0.0), ("scale", 1.0)]),
    "weibull"     => (Weibull,          [("shape", 1.5), ("scale", 1.0)]),
    "uniform"     => (Uniform,          [("a", 0.0), ("b", 1.0)]),
    "poisson"     => (Poisson,          [("lambda", 4.0)]),
    "binomial"    => (Binomial,         [("n", 20.0), ("p", 0.3)]),
    "geometric"   => (Geometric,        [("p", 0.2)]),
    "negbinomial" => (NegativeBinomial, [("r", 5.0), ("p", 0.5)]),
)

function distribution(req)
    name = lowercase(HTTP.getparams(req)["name"])
    haskey(DISTRIBUTIONS, name) || bad("unknown distribution '$name'; known: " * join(sort(collect(keys(DISTRIBUTIONS))), ", "))
    ctor, spec = DISTRIBUTIONS[name]
    q = query(req)
    args = Any[qnum(q, k, d) for (k, d) in spec]
    ctor === Binomial && (args[1] = round(Int, args[1]))  # n trials is a count
    d = try
        ctor(args...)
    catch err
        bad("invalid parameters for $name: " * message(err))
    end
    params = Dict(k => a for ((k, _), a) in zip(spec, args))
    points = round(Int, qnum(q, "points", 200; lo = 10, hi = 5000))
    # The plotted range holds the middle 99.8% of the mass (95% for the Cauchy,
    # whose tails would otherwise squeeze the peak to a line).
    tail = d isa Cauchy ? 0.025 : 0.001
    lo, hi = quantile(d, tail), quantile(d, 1 - tail)
    discrete = d isa DiscreteDistribution
    x = discrete ? collect(Float64, lo:hi) : collect(range(lo, hi; length = points))
    density = discrete ? pdf.(d, round.(Int, x)) : pdf.(d, x)
    json(clean((
        name = name, params = params, discrete = discrete,
        mean = mean(d), var = var(d), median = median(d),
        quantiles = Dict(pct(p) => quantile(d, p) for p in (0.025, 0.25, 0.5, 0.75, 0.975)),
        x = x, density = density, cdf = cdf.(d, x),
    )))
end

# ------------------------------------------------------------ /fit/linear

function fit_linear(req)
    b = body(req)
    x = numbers(get(b, :x, nothing), "x")
    y = numbers(get(b, :y, nothing), "y")
    length(x) == length(y) || bad("x and y must be the same length")
    degree = Int(clamp(get(b, :degree, 1), 1, 5))
    length(x) > degree + 1 || bad("need more than $(degree + 1) points for degree $degree")
    design(v) = hcat((v .^ k for k in 0:degree)...)
    m = lm(design(x), y)
    ct = coeftable(m)
    grid = collect(range(minimum(x), maximum(x); length = 100))
    band = predict(m, design(grid); interval = :confidence, level = 0.95)
    terms = ["intercept"; ["x" * (k == 1 ? "" : "^$k") for k in 1:degree]]
    json(clean((
        n = length(x), degree = degree,
        coefficients = [(term = terms[i], estimate = ct.cols[1][i], std_error = ct.cols[2][i],
                         t = ct.cols[3][i], p = ct.cols[4][i], lower95 = ct.cols[5][i], upper95 = ct.cols[6][i])
                        for i in eachindex(terms)],
        r2 = r2(m), adj_r2 = adjr2(m), sigma = sqrt(deviance(m) / dof_residual(m)),
        aic = aic(m), bic = bic(m),
        fit = (x = grid, y = band.prediction, lower = band.lower, upper = band.upper),
        residuals = residuals(m),
    )))
end

# ---------------------------------------------------- /ode/lotka-volterra

function lotka_volterra!(du, u, p, t)
    α, β, γ, δ = p
    du[1] = α * u[1] - β * u[1] * u[2]
    du[2] = δ * u[1] * u[2] - γ * u[2]
end

function lotka(req)
    q = query(req)
    p = (qnum(q, "alpha", 1.1; lo = 0, hi = 10), qnum(q, "beta", 0.4; lo = 0, hi = 10),
         qnum(q, "gamma", 0.4; lo = 0, hi = 10), qnum(q, "delta", 0.1; lo = 0, hi = 10))
    u0 = [qnum(q, "prey", 10.0; lo = 0, hi = 1e4), qnum(q, "predator", 10.0; lo = 0, hi = 1e4)]
    tmax = qnum(q, "tmax", 60.0; lo = 1, hi = 1000)
    points = round(Int, qnum(q, "points", 600; lo = 10, hi = 20_000))
    t = collect(range(0, tmax; length = points))
    sol = solve(ODEProblem(lotka_volterra!, u0, (0.0, tmax), p), Tsit5(); saveat = t, reltol = 1e-8, abstol = 1e-8)
    prey, predator = [u[1] for u in sol.u], [u[2] for u in sol.u]
    json(clean((
        params = (alpha = p[1], beta = p[2], gamma = p[3], delta = p[4]),
        solver = "Tsit5", retcode = string(sol.retcode), steps = length(sol.t),
        equilibrium = (prey = p[3] / p[4], predator = p[1] / p[2]),
        t = sol.t, prey = prey, predator = predator,
    )))
end

# ------------------------------------------------------------------ wiring

const ENDPOINTS = [
    (method = "POST", path = "/stats", body = "{\"numbers\": [..]}", returns = "mean, std, quantiles, skewness, histogram"),
    (method = "GET", path = "/distributions/{name}", body = "query: the distribution's parameters, points", returns = "pdf or pmf, cdf, moments, quantiles"),
    (method = "POST", path = "/fit/linear", body = "{\"x\": [..], \"y\": [..], \"degree\": 1}", returns = "coefficients with p-values, R², 95% band"),
    (method = "GET", path = "/ode/lotka-volterra", body = "query: alpha, beta, gamma, delta, prey, predator, tmax", returns = "prey and predator series (Tsit5)"),
]

page(_) = HTTP.Response(200, ["Content-Type" => "text/html; charset=utf-8", "Cache-Control" => "no-store"],
                        read(joinpath(ROOT, "static", "index.html")))

const router = HTTP.Router()
HTTP.register!(router, "GET", "/", page)
HTTP.register!(router, "GET", "/api", _ -> json((endpoints = ENDPOINTS, distributions = sort(collect(keys(DISTRIBUTIONS))))))
HTTP.register!(router, "GET", "/health", _ -> HTTP.Response(200, "ok"))
HTTP.register!(router, "POST", "/stats", stats)
HTTP.register!(router, "GET", "/distributions/{name}", distribution)
HTTP.register!(router, "POST", "/fit/linear", fit_linear)
HTTP.register!(router, "GET", "/ode/lotka-volterra", lotka)

# Every handler answers JSON, failures included: 400 with {error} for a bad
# request, 500 (and a log line) for a bug.
# The error's first paragraph on one line: enough to act on, without a
# MethodError's list of candidate methods.
message(err) = replace(first(split(sprint(showerror, err), "\n\n")), r"\s*\n\s*" => " ")

function guarded(handler)
    return function (req)
        try
            handler(req)
        catch err
            client = err isa BadRequest || err isa ArgumentError || err isa MethodError ||
                     err isa InexactError || err isa DomainError
            client || Log.error("$(req.method) $(req.target) failed: $(message(err))")
            json((error = message(err),); status = client ? 400 : 500)
        end
    end
end

const app = guarded(router)

const WARM = time()
# Answer each endpoint once in-process before listening: on this host the
# compiler's first pass over them takes about a minute, and a visitor who
# arrives during it would wait as long (measured 2026-10-25: answering lazily
# made the page's first five calls take 47 s each).
Log.info("compiling the endpoints")
# Bodies go in as bytes, the type a request off the socket carries.
let post(path, b) = app(HTTP.Request("POST", path, ["Content-Type" => "application/json"], Vector{UInt8}(JSON3.write(b))))
    # JSON arrays of whole numbers, of decimals and of both parse to three
    # different element types; each compiles its own path.
    for xs in ([1, 2, 3, 5, 8, 13], [0.5, 1.5, 2.25, 4.0, 6.5, 9.0], [1, 2.5, 3, 4.5, 6, 7.5])
        post("/stats", (numbers = xs,))
        post("/fit/linear", (x = xs, y = xs, degree = 1))
        post("/fit/linear", (x = xs, y = reverse(xs), degree = 2))
    end
    app(HTTP.Request("GET", "/ode/lotka-volterra"))
    for name in keys(DISTRIBUTIONS)
        app(HTTP.Request("GET", "/distributions/$name"))
    end
end

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

Log.info("__HOST__: Scientific API serving on 127.0.0.1:$PORT")
HTTP.serve(logged(app), "127.0.0.1", PORT)
