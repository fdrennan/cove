# Monte Carlo Pricer: a European option priced by simulating the stock in
# Julia, and checked against the Black-Scholes formula.
#
#   dS = r S dt + sigma S dW      S_T = S_0 exp((r - sigma^2 / 2) T + sigma sqrt(T) Z)
#
# Risk-neutral: the stock drifts at the risk-free rate r, the payoff is
# discounted by exp(-r T). Black, F. and Scholes, M. (1973), "The Pricing of
# Options and Corporate Liabilities", J. Political Economy 81(3).
#
#   GET  /            the page (static/index.html, read on every request)
#   POST /api/price   {s, k, r, sigma, t, n, type: "call"|"put", seed}
#                     -> {mc, se, bs, itm, millis, threads,
#                         paths: [m paths of steps+1 prices], times,
#                         converge: [[n, mean, se], ...],
#                         ends: {edges, density, theory}}
#   GET  /health      "ok"

include(joinpath(@__DIR__, "log.jl"))
using HTTP, JSON3, Distributions, Random, Statistics

const ROOT = @__DIR__
# run.sh sets PORT from app.env's APP_PORT; a second default here would
# serve on a port no other part of the project knows.
const PORT = parse(Int, ENV["PORT"])

"""
    black_scholes(s, k, r, σ, t, type)

The closed-form price of a European call or put.
"""
function black_scholes(s, k, r, σ, t, type::Symbol)
    d1 = (log(s / k) + (r + σ^2 / 2) * t) / (σ * sqrt(t))
    d2 = d1 - σ * sqrt(t)
    N(x) = cdf(Normal(), x)
    type === :call ? s * N(d1) - k * exp(-r * t) * N(d2) : k * exp(-r * t) * N(-d2) - s * N(-d1)
end

payoff(st, k, type::Symbol) = type === :call ? max(st - k, 0.0) : max(k - st, 0.0)

"""
    terminal!(out, seed, s, r, σ, t)

Fill `out` with prices at expiry, drawn exactly: one normal each, no time
steps. Split across threads, each chunk with its own generator seeded
`seed + chunk`.
"""
function terminal!(out::Vector{Float64}, seed::Int, s, r, σ, t)
    drift, vol = (r - σ^2 / 2) * t, σ * sqrt(t)
    chunks = collect(Iterators.partition(eachindex(out), cld(length(out), Threads.nthreads())))
    Threads.@threads for c in eachindex(chunks)
        rng = Xoshiro(seed + c)
        @inbounds for i in chunks[c]
            out[i] = s * exp(drift + vol * randn(rng))
        end
    end
    out
end

"""
    paths(m, steps, seed, s, r, σ, t)

`m` whole paths of `steps` steps, for drawing: a matrix, one path per column.
"""
function paths(m, steps, seed, s, r, σ, t)
    rng = Xoshiro(seed)
    dt = t / steps
    p = Matrix{Float64}(undef, steps + 1, m)
    for j in 1:m
        x = s
        p[1, j] = x
        for i in 1:steps
            x *= exp((r - σ^2 / 2) * dt + σ * sqrt(dt) * randn(rng))
            p[i + 1, j] = x
        end
    end
    p
end

num(body, key, default, lo, hi) = clamp(Float64(get(body, key, default)), lo, hi)

json(x; status = 200) = HTTP.Response(status, ["Content-Type" => "application/json", "Cache-Control" => "no-store"], JSON3.write(x))

function price(req::HTTP.Request)
    body = isempty(req.body) ? JSON3.Object() : JSON3.read(req.body)
    s = num(body, :s, 100, 1, 1e4)
    k = num(body, :k, 105, 1, 1e4)
    r = num(body, :r, 0.03, 0, 0.5)
    σ = num(body, :sigma, 0.25, 0.01, 3)
    t = num(body, :t, 1, 0.01, 30)
    n = round(Int, num(body, :n, 200_000, 1000, 2_000_000))
    seed = round(Int, num(body, :seed, 1, 0, 1e9))
    type = get(body, :type, "call") == "put" ? :put : :call

    elapsed = @elapsed begin
        st = terminal!(Vector{Float64}(undef, n), seed, s, r, σ, t)
        disc = exp(-r * t) .* payoff.(st, k, type)
    end
    mc, se = mean(disc), std(disc) / sqrt(n)

    # The running estimate at 200 points from 100 paths to all of them.
    cs, cq = cumsum(disc), cumsum(disc .^ 2)
    at = unique(round.(Int, exp.(range(log(100), log(n), length = 200))))
    converge = [(m = cs[i] / i; (i, m, sqrt(max(cq[i] / i - m^2, 0) / i))) for i in at]

    # Where the stock ends: a histogram as a density, and the lognormal theory gives.
    lo, hi = quantile(st, 0.001), quantile(st, 0.995)
    edges = collect(range(lo, hi, length = 81))
    counts = zeros(Int, 80)
    for x in st
        lo <= x < hi && (counts[min(80, 1 + floor(Int, (x - lo) / (hi - lo) * 80))] += 1)
    end
    width = (hi - lo) / 80
    mids = (edges[1:end-1] .+ edges[2:end]) ./ 2
    theory = pdf.(LogNormal(log(s) + (r - σ^2 / 2) * t, σ * sqrt(t)), mids)

    drawn = paths(220, 160, seed + 10_000, s, r, σ, t)
    json((mc = mc, se = se, bs = black_scholes(s, k, r, σ, t, type), itm = mean(disc .> 0),
          millis = round(Int, 1000elapsed), threads = Threads.nthreads(), n = n,
          paths = [Float32.(drawn[:, j]) for j in 1:size(drawn, 2)], times = collect(range(0, t, length = 161)),
          converge = converge, ends = (edges = edges, density = counts ./ (n * width), theory = theory)))
end

function page(::HTTP.Request)
    HTTP.Response(200, ["Content-Type" => "text/html; charset=utf-8", "Cache-Control" => "no-store"],
                  read(joinpath(ROOT, "static", "index.html")))
end

const router = HTTP.Router()
HTTP.register!(router, "GET", "/", page)
HTTP.register!(router, "POST", "/api/price", price)
HTTP.register!(router, "GET", "/health", _ -> HTTP.Response(200, "ok"))

message(err) = replace(first(split(sprint(showerror, err), "\n\n")), r"\s*\n\s*" => " ")

function guarded(handler)
    return function (req)
        try
            handler(req)
        catch err
            err isa ArgumentError || err isa MethodError || err isa InexactError ||
                Log.error("$(req.method) $(req.target) failed: $(message(err))")
            json((error = message(err),); status = 400)
        end
    end
end

const WARM = time()
# Answer one request in-process so the first visitor does not wait for the compiler.
guarded(router)(HTTP.Request("POST", "/api/price", ["Content-Type" => "application/json"], "{\"n\":2000}"))

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

Log.info("__HOST__: Monte Carlo Pricer serving on 127.0.0.1:$PORT with $(Threads.nthreads()) threads")
HTTP.serve(logged(guarded(router)), "127.0.0.1", PORT)
