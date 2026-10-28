# Adaptive Trial Designer: a Bayesian response-adaptive multi-arm trial with a
# binary outcome, simulated in Julia and set against the fixed design that
# randomises everyone equally and looks once, at the end.
#
# Arm 1 is the control. Each arm's response rate has a Beta(1, 1) prior, so
# after x responses in n patients its posterior is Beta(1 + x, 1 + n - x).
# After a burn-in of equal allocation, patients go where the posterior says
# the best arm is:
#
#   thompson   each patient draws one rate per arm from its posterior and joins
#              the arm with the highest draw, so arm j is chosen with
#              probability P(j is best) (Thompson 1933)
#   pbest      at each interim look, allocation proportional to P(j is best)^c
#              with c = n / (2 n_max), cautious early and greedier as data
#              accrue (Thall and Wathen 2007)
#
# Every `every` patients after the burn-in there is a look: an experimental
# arm with P(best) > eff wins and the trial stops for efficacy; an
# experimental arm with P(best) < fut is dropped, and when every experimental
# arm is dropped the trial stops for futility. The control is never dropped.
# P(best) is integrated on a 500-point grid, not sampled, so it carries no
# Monte Carlo noise.
#
# The fixed design puts n_max / K patients on every arm and tests each
# experimental arm against control with a one-sided two-proportion z-test at
# alpha / (K - 1) (Bonferroni), so its type-I error sits at or a little under alpha.
#
#   GET  /            the page (static/index.html, read on every request)
#   POST /api/trial   {rates, n, burnin, every, rule, eff, fut, alpha, seed}
#                     -> one trial, patient by patient: {arm, y, alloc,
#                        looks: [{n, pbest, active after the look}], stop: {n, reason, winner},
#                        fixed: {per, x, z, crit, winner}}
#   POST /api/oc      {same, sims}
#                     -> operating characteristics: {null, alt} x {adaptive,
#                        fixed}, each {reject, correct, ess, best_n, best_frac,
#                        responders, select, stop, nhist}; millis, threads
#   GET  /health      "ok"

using HTTP, JSON3, Distributions, Random, Statistics

const ROOT = @__DIR__
# run.sh sets PORT from app.env's APP_PORT; a second default here would
# serve on a port no other part of the project knows.
const PORT = parse(Int, ENV["PORT"])

# The grid P(best) is integrated on: midpoints of 500 cells over (0, 1). A
# posterior after 2000 patients has sd near 0.011, over five cells, and the
# midpoint rule on a smooth density that well resolved agrees with a
# 1000-cell grid to 0.001 (measured 2026-10-26) at under half the cost.
const G = 500
const XS = [(i - 0.5) / G for i in 1:G]
const LX = log.(XS)
const L1X = log1p.(.-XS)

struct Design
    rates::Vector{Float64}
    nmax::Int
    burnin::Int
    every::Int
    rule::Symbol
    eff::Float64
    fut::Float64
    alpha::Float64
end

# Per-task work space for pbest!, so thousands of simulated trials on every
# thread allocate nothing per look.
struct Scratch
    f::Matrix{Float64}
    F::Matrix{Float64}
    P::Vector{Float64}
end
Scratch(k) = Scratch(zeros(G, k), zeros(G, k), zeros(G))

"""
    pbest!(out, a, b, active, s)

P(arm j has the highest rate) for the active arms, under independent
Beta(a[j], b[j]) posteriors: the sum over the grid of arm j's density times
every other active arm's CDF. Inactive arms get 0.
"""
function pbest!(out, a, b, active, s::Scratch)
    K = length(a)
    fill!(out, 0.0)
    fill!(s.P, 1.0)
    @inbounds for j in 1:K
        active[j] || continue
        m = -Inf
        for i in 1:G
            v = (a[j] - 1) * LX[i] + (b[j] - 1) * L1X[i]
            s.f[i, j] = v
            m = max(m, v)
        end
        tot = 0.0
        for i in 1:G
            # Below e^-40 of the peak a cell adds nothing a Float64 sum can hold.
            d = s.f[i, j] - m
            e = d < -40 ? 0.0 : exp(d)
            s.f[i, j] = e
            tot += e
        end
        c = 0.0
        for i in 1:G
            p = s.f[i, j] / tot
            s.f[i, j] = p
            # Half the cell's own mass: the CDF at the cell's midpoint.
            s.F[i, j] = c + p / 2
            c += p
            s.P[i] *= s.F[i, j]
        end
    end
    # P holds every active arm's CDF multiplied; dividing out arm j's own
    # leaves the others, one pass per arm instead of K. F is zero only where
    # f is zero too, so those cells add nothing.
    @inbounds for j in 1:K
        active[j] || continue
        acc = 0.0
        for i in 1:G
            F = s.F[i, j]
            F > 0 && (acc += s.f[i, j] * s.P[i] / F)
        end
        out[j] = acc
    end
    out
end

# What a trial records when it is drawn on the page, not only counted.
struct Trace
    arm::Vector{Int}
    y::Vector{Int}
    alloc::Vector{Vector{Float32}}
    looks::Vector{Any}
end
Trace() = Trace(Int[], Int[], Vector{Float32}[], Any[])

# How one simulated trial ended. `top` is the highest P(best) any
# experimental arm reached at any look: the trial rejects at threshold e
# exactly when top > e, which is what lets oc calibrate e to alpha.
struct Outcome
    n::Int
    winner::Int
    reason::Symbol
    per_arm::Vector{Int}
    resp::Int
    top::Float64
end

"""
    adaptive(rng, d, s, trace)

One response-adaptive trial, as an Outcome.
"""
function adaptive(rng, d::Design, s::Scratch, trace::Union{Nothing,Trace} = nothing)
    K = length(d.rates)
    a, b = ones(K), ones(K)
    n = zeros(Int, K)
    active = trues(K)
    w = fill(1 / K, K)          # pbest's allocation, fixed between looks
    pb, probs = zeros(K), zeros(K)
    block = Int[]
    resp, N, winner, reason, top = 0, 0, 0, :max, 0.0
    for i in 1:d.nmax
        if i <= d.burnin
            # Permuted blocks keep the burn-in balanced, not only balanced on average.
            isempty(block) && append!(block, randperm(rng, K))
            arm = pop!(block)
            trace === nothing || (probs .= active ./ count(active))
        elseif d.rule === :thompson
            arm, high = 0, -1.0
            for j in 1:K
                active[j] || continue
                v = rand(rng, Beta(a[j], b[j]))
                v > high && ((arm, high) = (j, v))
            end
            trace === nothing || pbest!(probs, a, b, active, s)
        else
            u, arm = rand(rng), K
            for j in 1:K
                u -= w[j]
                u < 0 && (arm = j; break)
            end
            trace === nothing || (probs .= w)
        end
        y = rand(rng) < d.rates[arm]
        n[arm] += 1
        a[arm] += y
        b[arm] += !y
        resp += y
        N = i
        if trace !== nothing
            push!(trace.arm, arm); push!(trace.y, y); push!(trace.alloc, Float32.(probs))
        end

        i >= max(d.burnin, 1) && (i % d.every == 0 || i == d.nmax) || continue
        pbest!(pb, a, b, active, s)
        best = 1 + argmax(@view pb[2:K])
        top = max(top, pb[best])
        efficacy = pb[best] > d.eff
        if !efficacy && i < d.nmax
            for j in 2:K
                active[j] && pb[j] < d.fut && (active[j] = false)
            end
        end
        # The arms still enrolling after this look's drops.
        trace === nothing || push!(trace.looks, (n = i, pbest = copy(pb), active = collect(active)))
        if efficacy
            winner, reason = best, :efficacy
            break
        end
        i == d.nmax && break
        if !any(@view active[2:K])
            reason = :futility
            break
        end
        if d.rule === :pbest
            c = i / (2d.nmax)
            for j in 1:K
                w[j] = active[j] ? pb[j]^c : 0.0
            end
            w ./= sum(w)
        end
    end
    Outcome(N, winner, reason, n, resp, top)
end

"""
    fixed(rng, d)

The fixed design: n_max / K patients per arm, each experimental arm tested
against control at alpha / (K - 1); the winner is the best observed among the
significant arms, 0 if none.
"""
function fixed(rng, d::Design)
    K = length(d.rates)
    per = d.nmax ÷ K
    x = [rand(rng, Binomial(per, p)) for p in d.rates]
    crit = quantile(Normal(), 1 - d.alpha / (K - 1))
    z = zeros(K)
    for j in 2:K
        pool = (x[j] + x[1]) / 2per
        se = sqrt(pool * (1 - pool) * 2 / per)
        z[j] = se > 0 ? (x[j] - x[1]) / per / se : 0.0
    end
    winner = 0
    for j in 2:K
        z[j] > crit && (winner == 0 || x[j] > x[winner]) && (winner = j)
    end
    (per = per, x = x, z = z, crit = crit, winner = winner)
end

num(body, key, default, lo, hi) = clamp(Float64(get(body, key, default)), lo, hi)

function design(body)
    rates = Float64.(collect(get(body, :rates, [0.3, 0.3, 0.45])))
    2 <= length(rates) <= 4 || throw(ArgumentError("rates needs 2 to 4 arms, control first"))
    all(r -> 0 < r < 1, rates) || throw(ArgumentError("every rate must lie strictly between 0 and 1"))
    nmax = round(Int, num(body, :n, 300, 20, 2000))
    rule = get(body, :rule, "thompson")
    rule in ("thompson", "pbest") || throw(ArgumentError("rule must be thompson or pbest"))
    Design(rates, nmax, round(Int, num(body, :burnin, 60, 0, nmax)), round(Int, num(body, :every, 30, 5, nmax)),
           Symbol(rule), num(body, :eff, 0.99, 0.5, 0.99999), num(body, :fut, 0.01, 0, 0.2), num(body, :alpha, 0.05, 0.001, 0.2))
end

with_eff(d::Design, rates, eff) = Design(rates, d.nmax, d.burnin, d.every, d.rule, eff, d.fut, d.alpha)

body_of(req) = isempty(req.body) ? JSON3.Object() : JSON3.read(req.body)

json(x; status = 200) = HTTP.Response(status, ["Content-Type" => "application/json", "Cache-Control" => "no-store"], JSON3.write(x))

function trial(req::HTTP.Request)
    body = body_of(req)
    d = design(body)
    seed = round(Int, num(body, :seed, 1, 0, 1e9))
    t = Trace()
    o = adaptive(Xoshiro(seed), d, Scratch(length(d.rates)), t)
    json((arm = t.arm, y = t.y, alloc = t.alloc, looks = t.looks, rates = d.rates, nmax = d.nmax, burnin = d.burnin,
          eff = d.eff, fut = d.fut, stop = (n = o.n, reason = o.reason, winner = o.winner, per_arm = o.per_arm, responders = o.resp),
          fixed = fixed(Xoshiro(seed + 7919), d)))
end

"""
    simulate(d, sims, seed)

`sims` adaptive and `sims` fixed trials of design `d`, split across threads,
each chunk with its own generator and scratch. The same seed gives the same
patients, so a design run twice differs only where its rules differ.
"""
function simulate(d::Design, sims, seed)
    K = length(d.rates)
    A = Vector{Outcome}(undef, sims)
    F = Vector{Outcome}(undef, sims)
    chunks = collect(Iterators.partition(1:sims, cld(sims, 4Threads.nthreads())))
    Threads.@threads :dynamic for c in eachindex(chunks)
        rng, s = Xoshiro(seed + c), Scratch(K)
        for i in chunks[c]
            A[i] = adaptive(rng, d, s)
            f = fixed(rng, d)
            F[i] = Outcome(K * f.per, f.winner, :max, fill(f.per, K), sum(f.x), 0.0)
        end
    end
    A, F
end

function summarise(rows::Vector{Outcome}, rates, nmax)
    K = length(rates)
    best = findall(==(maximum(rates)), rates)
    # Power is only defined when one experimental arm is strictly best.
    target = length(best) == 1 && best[1] != 1 ? best[1] : 0
    hi = argmax(rates)
    Ns = [r.n for r in rows]
    edges = collect(range(0, nmax, length = 26))
    hist = zeros(Int, 25)
    for v in Ns
        hist[clamp(1 + floor(Int, v / nmax * 25), 1, 25)] += 1
    end
    (reject = mean(r.winner > 0 for r in rows),
     correct = target == 0 ? nothing : mean(r.winner == target for r in rows),
     target = target,
     ess = mean(Ns),
     best_n = mean(r.per_arm[hi] for r in rows),
     best_frac = mean(r.per_arm[hi] / r.n for r in rows),
     responders = mean(r.resp for r in rows),
     select = [mean(r.winner == j for r in rows) for j in 1:K],
     stop = (efficacy = mean(r.reason === :efficacy for r in rows), futility = mean(r.reason === :futility for r in rows),
             max = mean(r.reason === :max for r in rows)),
     nhist = (edges = edges, counts = hist))
end

function oc(req::HTTP.Request)
    body = body_of(req)
    d = design(body)
    sims = round(Int, num(body, :sims, 4000, 100, 20_000))
    seed = round(Int, num(body, :seed, 1, 0, 1e9))
    calibrate = get(body, :calibrate, true) === true
    # The null scenario: every arm at the control's rate. Its reject rate is the type-I error.
    flat = fill(d.rates[1], length(d.rates))
    elapsed = @elapsed begin
        if calibrate
            # A null run that never stops for efficacy records each trial's
            # highest P(best). Up to its first crossing a trial does not depend
            # on the threshold, so the threshold that a fraction alpha of
            # those maxima exceed is the one that rejects a fraction alpha of
            # null trials. The null run below measures it again on fresh trials.
            probe, _ = simulate(with_eff(d, flat, 1.0), sims, seed + 2_000_029)
            d = with_eff(d, d.rates, clamp(quantile([r.top for r in probe], 1 - d.alpha), 0.5, 0.99999))
        end
        An, Fn = simulate(with_eff(d, flat, d.eff), sims, seed)
        Aa, Fa = simulate(d, sims, seed + 1_000_003)
    end
    json((sims = sims, millis = round(Int, 1000elapsed), threads = Threads.nthreads(), rates = d.rates,
          eff = d.eff, calibrated = calibrate,
          null = (adaptive = summarise(An, flat, d.nmax), fixed = summarise(Fn, flat, d.nmax)),
          alt = (adaptive = summarise(Aa, d.rates, d.nmax), fixed = summarise(Fa, d.rates, d.nmax))))
end

function page(::HTTP.Request)
    HTTP.Response(200, ["Content-Type" => "text/html; charset=utf-8", "Cache-Control" => "no-store"],
                  read(joinpath(ROOT, "static", "index.html")))
end

const router = HTTP.Router()
HTTP.register!(router, "GET", "/", page)
HTTP.register!(router, "POST", "/api/trial", trial)
HTTP.register!(router, "POST", "/api/oc", oc)
HTTP.register!(router, "GET", "/health", _ -> HTTP.Response(200, "ok"))

message(err) = replace(first(split(sprint(showerror, err), "\n\n")), r"\s*\n\s*" => " ")

function guarded(handler)
    return function (req)
        try
            handler(req)
        catch err
            err isa ArgumentError || err isa MethodError || err isa InexactError ||
                @error "request failed" exception = (err, catch_backtrace())
            json((error = message(err),); status = 400)
        end
    end
end

# Answer one request of each kind in-process so the first visitor does not wait for the compiler.
for (path, rule) in (("/api/trial", "thompson"), ("/api/trial", "pbest"), ("/api/oc", "thompson"), ("/api/oc", "pbest"))
    guarded(router)(HTTP.Request("POST", path, ["Content-Type" => "application/json"], "{\"rule\":\"$rule\",\"sims\":100}"))
end

println("Adaptive Trial Designer listening on http://127.0.0.1:$PORT with $(Threads.nthreads()) threads")
flush(stdout)
HTTP.serve(guarded(router), "127.0.0.1", PORT)
