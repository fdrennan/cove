# Fractal Explorer: Mandelbrot and Julia sets computed in Julia on every core
# and sent to the page as raw RGBA pixels.
#
#   GET /          the page (static/index.html, read on every request)
#   GET /render    ?kind=mandelbrot|julia&cx&cy&scale&w&h&iter&palette&jr&ji
#                  -> w*h*4 bytes of RGBA; headers X-Render-Ms, X-Threads
#   GET /info      {threads, palettes}
#   GET /health    "ok"
#
# `scale` is the width of one pixel in the complex plane; (cx, cy) is the
# centre of the image.

include(joinpath(@__DIR__, "log.jl"))
using HTTP, JSON3

const ROOT = @__DIR__
# run.sh sets PORT from app.env's APP_PORT; a second default here would
# serve on a port no other part of the project knows.
const PORT = parse(Int, ENV["PORT"])
const LUT = 1024

# Cosine palettes, colour(t) = a + b cos(2π(c t + d)) per channel (Inigo
# Quilez's form): four vectors make a smooth, cyclic gradient.
const PALETTE_SPECS = [
    "aurora" => ((0.50, 0.50, 0.50), (0.50, 0.50, 0.50), (1.0, 1.0, 1.0), (0.30, 0.20, 0.20)),
    "ember"  => ((0.50, 0.30, 0.20), (0.50, 0.35, 0.25), (1.0, 0.9, 0.7), (0.00, 0.10, 0.20)),
    "ocean"  => ((0.20, 0.40, 0.55), (0.25, 0.35, 0.40), (1.0, 1.0, 1.0), (0.55, 0.45, 0.35)),
    "neon"   => ((0.50, 0.50, 0.50), (0.50, 0.50, 0.50), (2.0, 1.0, 0.0), (0.50, 0.20, 0.25)),
    "ivory"  => ((0.62, 0.58, 0.52), (0.38, 0.38, 0.40), (1.0, 1.0, 1.0), (0.00, 0.03, 0.08)),
]

function cosine_palette(a, b, c, d)
    map(0:LUT-1) do i
        t = i / LUT
        ntuple(k -> round(UInt8, 255 * clamp(a[k] + b[k] * cospi(2 * (c[k] * t + d[k])), 0, 1)), 3)
    end
end

const PALETTES = Dict(name => cosine_palette(spec...) for (name, spec) in PALETTE_SPECS)
const PALETTE_NAMES = first.(PALETTE_SPECS)

# Escape time with a smooth (fractional) count, so bands blend instead of
# stepping. Returns -1 for a point that never escaped: the set itself.
@inline function escape(zr, zi, cr, ci, maxit)
    r2 = zr * zr; i2 = zi * zi
    n = 0
    while n < maxit && r2 + i2 <= 65536.0
        zi = 2 * zr * zi + ci
        zr = r2 - i2 + cr
        r2 = zr * zr; i2 = zi * zi
        n += 1
    end
    n == maxit && return -1.0
    n + 1 - log2(log(sqrt(r2 + i2)))
end

# The main cardioid and the period-2 bulb hold most of the Mandelbrot set's
# interior; skipping them saves maxit iterations per pixel there.
@inline function in_bulbs(cr, ci)
    q = (cr - 0.25)^2 + ci^2
    q * (q + (cr - 0.25)) <= 0.25 * ci^2 || (cr + 1)^2 + ci^2 <= 0.0625
end

function render(julia::Bool, cx, cy, scale, w, h, maxit, pal, jr, ji)
    buf = Vector{UInt8}(undef, 4w * h)
    density = 2048 / sqrt(maxit)      # how fast colour cycles with the count
    Threads.@threads :dynamic for row in 1:h
        y = cy + (h / 2 - row + 0.5) * scale
        o = 4w * (row - 1)
        @inbounds for col in 1:w
            x = cx + (col - 0.5 - w / 2) * scale
            ν = julia ? escape(x, y, jr, ji, maxit) :
                in_bulbs(x, y) ? -1.0 : escape(0.0, 0.0, x, y, maxit)
            k = o + 4col - 3
            if ν < 0
                buf[k] = 0x03; buf[k+1] = 0x04; buf[k+2] = 0x0a
            else
                r, g, b = pal[mod(floor(Int, ν * density / 16), LUT) + 1]
                buf[k] = r; buf[k+1] = g; buf[k+2] = b
            end
            buf[k+3] = 0xff
        end
    end
    buf
end

function qnum(q, key, default, lo, hi)
    v = something(tryparse(Float64, get(q, key, string(default))), NaN)
    isfinite(v) || throw(ArgumentError("$key must be a number"))
    clamp(v, lo, hi)
end

function render_handler(req::HTTP.Request)
    q = HTTP.queryparams(HTTP.URI(req.target))
    kind = get(q, "kind", "mandelbrot")
    kind in ("mandelbrot", "julia") || throw(ArgumentError("kind must be mandelbrot or julia"))
    w = round(Int, qnum(q, "w", 640, 8, 3840))
    h = round(Int, qnum(q, "h", 400, 8, 2400))
    maxit = round(Int, qnum(q, "iter", 400, 16, 20_000))
    scale = qnum(q, "scale", 3.5 / w, 1e-16, 1.0)
    cx = qnum(q, "cx", -0.6, -4, 4)
    cy = qnum(q, "cy", 0.0, -4, 4)
    jr = qnum(q, "jr", -0.8, -2, 2)
    ji = qnum(q, "ji", 0.156, -2, 2)
    pal = get(PALETTES, get(q, "palette", "aurora"), PALETTES["aurora"])
    t = time_ns()
    buf = render(kind == "julia", cx, cy, scale, w, h, maxit, pal, jr, ji)
    ms = round((time_ns() - t) / 1e6; digits = 1)
    HTTP.Response(200, ["Content-Type" => "application/octet-stream", "Cache-Control" => "no-store",
                        "X-Render-Ms" => string(ms), "X-Threads" => string(Threads.nthreads()),
                        "X-Size" => "$(w)x$(h)"], buf)
end

json(x; status = 200) = HTTP.Response(status, ["Content-Type" => "application/json", "Cache-Control" => "no-store"], JSON3.write(x))

page(_) = HTTP.Response(200, ["Content-Type" => "text/html; charset=utf-8", "Cache-Control" => "no-store"],
                        read(joinpath(ROOT, "static", "index.html")))

const router = HTTP.Router()
HTTP.register!(router, "GET", "/", page)
HTTP.register!(router, "GET", "/render", render_handler)
HTTP.register!(router, "GET", "/info", _ -> json((threads = Threads.nthreads(), palettes = PALETTE_NAMES)))
HTTP.register!(router, "GET", "/health", _ -> HTTP.Response(200, "ok"))

message(err) = replace(first(split(sprint(showerror, err), "\n\n")), r"\s*\n\s*" => " ")

function guarded(handler)
    return function (req)
        try
            handler(req)
        catch err
            err isa ArgumentError || Log.error("$(req.method) $(req.target) failed: $(message(err))")
            json((error = message(err),); status = err isa ArgumentError ? 400 : 500)
        end
    end
end

const app = guarded(router)

const WARM = time()
# Draw one small frame of each kind in-process so the first visitor does not wait for the compiler.
app(HTTP.Request("GET", "/render?kind=mandelbrot&w=64&h=40"))
app(HTTP.Request("GET", "/render?kind=julia&w=64&h=40"))

Log.info("compiled the handlers in $(round(time() - WARM; digits = 1)) s")

# One log line per request. /render answers every pan and zoom,
# so its lines are DEBUG (LOG_LEVEL=DEBUG shows them).
function logged(handler)
    return function (req)
        t0 = time()
        res = handler(req)
        path = first(split(req.target, '?'))
        if res.status < 400 && (path == "/render")
            Log.debug("$(req.method) $path $(res.status) $(round(Int, (time() - t0) * 1000)) ms")
        else
            Log.request(req, res.status, t0)
        end
        res
    end
end

Log.info("__HOST__: Fractal Explorer serving on 127.0.0.1:$PORT with $(Threads.nthreads()) threads")
HTTP.serve(logged(app), "127.0.0.1", PORT)
