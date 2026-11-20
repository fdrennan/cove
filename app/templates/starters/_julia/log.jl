# Logging for __HOST__, Julia's standard library only. Every line goes to
# stdout, flushed, in the one format every Cove starter writes:
#
#   2026-11-18 12:04:31 INFO    [app] serving on 127.0.0.1:8834
#
# so .cove/run/app.log (Cove's Logs tab) reads the same in Julia, R, Python,
# Node and Rust. Times are UTC. Levels, shown when at or above LOG_LEVEL
# (default INFO): TRACE DEBUG INFO SUCCESS WARN ERROR.
#
#   include("log.jl")
#   Log.info("serving on 127.0.0.1:$PORT")
#   Log.info("loaded $(n) rows"; name = "db")      # [db] on the line
#   Log.request(req, status, t0)                    # GET /path 200 3 ms
module Log

using Dates

const LEVELS = Dict("TRACE" => 0, "DEBUG" => 1, "INFO" => 2, "SUCCESS" => 2, "WARN" => 3, "ERROR" => 4)
threshold() = get(LEVELS, uppercase(get(ENV, "LOG_LEVEL", "INFO")), 2)
const NAME = get(ENV, "LOG_NAME", "app")

function line(level::String, text; name = NAME)
    LEVELS[level] < threshold() && return nothing
    stamp = Dates.format(now(UTC), "yyyy-mm-dd HH:MM:SS")
    println(stdout, stamp, " ", rpad(level, 7), " [", name, "] ", text)
    flush(stdout)
    nothing
end

trace(t; name = NAME) = line("TRACE", t; name)
debug(t; name = NAME) = line("DEBUG", t; name)
info(t; name = NAME) = line("INFO", t; name)
success(t; name = NAME) = line("SUCCESS", t; name)
warn(t; name = NAME) = line("WARN", t; name)
error(t; name = NAME) = line("ERROR", t; name)

"""One line per request: method, path, status and the time since `t0`
(`time()` when it came in). 5xx are ERROR, 4xx WARN, Cove's health probes
TRACE."""
function request(req, status::Integer, t0::Float64; name = NAME)
    path = first(split(req.target, '?'))
    level = status >= 500 ? "ERROR" : status >= 400 ? "WARN" : path in ("/health", "/__live") ? "TRACE" : "INFO"
    line(level, "$(req.method) $path $status $(round(Int, (time() - t0) * 1000)) ms"; name)
end

end
