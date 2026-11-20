# __HOST__: a blank Julia app on HTTP.jl. run.sh restarts it when this file is saved;
# files in static/ are read on each request.
using HTTP, JSON3
include(joinpath(@__DIR__, "log.jl"))

const ROOT = @__DIR__
const STATIC = joinpath(ROOT, "static")
const PORT = parse(Int, ENV["PORT"])
const STARTED = time()
const TYPES = Dict(".html" => "text/html; charset=utf-8", ".js" => "text/javascript; charset=utf-8", ".css" => "text/css; charset=utf-8", ".json" => "application/json", ".svg" => "image/svg+xml", ".png" => "image/png")

json(x; status = 200) = HTTP.Response(status, ["Content-Type" => "application/json", "Cache-Control" => "no-cache"], JSON3.write(x))

function version()
    times = [mtime(joinpath(d, f)) for (d, _, fs) in walkdir(STATIC) for f in fs]
    "$(STARTED)-$(length(times))-$(maximum(times; init = 0.0))"
end

function serve_static(path)
    rel = path == "/" ? "index.html" : replace(lstrip(path, '/'), r"^static/" => "")
    file = normpath(joinpath(STATIC, rel))
    (startswith(file, STATIC * "/") && isfile(file)) || return json((; error = "not found"); status = 404)
    HTTP.Response(200, ["Content-Type" => get(TYPES, splitext(file)[2], "application/octet-stream"), "Cache-Control" => "no-cache"], read(file))
end

function route(req::HTTP.Request)
    path = HTTP.URI(req.target).path
    path == "/health" && return json((; ok = true))
    path == "/__live" && return json((; version = version()))
    serve_static(HTTP.URIs.unescapeuri(path))
end

# One log line per request (GET /path 200 3 ms); an error is logged and
# answered 500 with {error}.
function handle(req::HTTP.Request)
    t0 = time()
    res = try
        route(req)
    catch err
        Log.error("$(req.method) $(req.target) failed: $(sprint(showerror, err))")
        json((; error = sprint(showerror, err)); status = 500)
    end
    Log.request(req, res.status, t0)
    res
end

Log.info("__HOST__: blank Julia app serving on 127.0.0.1:$PORT")
HTTP.serve(handle, "127.0.0.1", PORT)
