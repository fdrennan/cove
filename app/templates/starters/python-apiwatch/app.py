"""API Observatory: a FastAPI service that watches its own traffic.

Five sample endpoints under /sample/ (fast, slow, flaky, cpu, items) are the
API being observed. An ASGI middleware times every request to them, keeps the
record in memory (no database) and checks each response body against the
pydantic model the route declares - the "contract". The page at / reads
GET /api/stream (Server-Sent Events, a snapshot twice a second) and drives
GET /api/load, a load generator that streams its own progress the same way.

Change first: SAMPLES and the /sample/ routes - put the endpoints your service
serves there, declare their response models in CONTRACTS, and the
observatory, the contract check and /docs all pick them up.
"""
import asyncio
import json
import logging
import math
import os
import platform
import random
import sys
import time
from collections import Counter, deque
from datetime import datetime, timezone
from importlib.metadata import version
from pathlib import Path
from typing import Literal

import httpx
import numpy as np
from fastapi import Body, FastAPI, HTTPException, Query, Request
from fastapi.responses import FileResponse, JSONResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field, ValidationError

from log import log

HERE = Path(__file__).parent
STARTED = datetime.now(timezone.utc)
T0 = time.monotonic()
IMPORTS = ["fastapi", "pydantic", "starlette", "uvicorn", "numpy", "httpx"]


# ---- the contracts: what each sample endpoint promises to answer ------------------------

class Pong(BaseModel):
    ok: bool
    server_time: float = Field(description="Unix seconds")


class Slept(BaseModel):
    slept_ms: float = Field(ge=0)
    median_ms: float


class Flaky(BaseModel):
    ok: bool
    failure_rate: float = Field(ge=0, le=1)


class Problem(BaseModel):
    detail: str


class Primes(BaseModel):
    n: int
    count: int = Field(description="primes <= n")
    largest: int


class Star(BaseModel):
    id: int
    name: str
    constellation: str
    magnitude: float


class StarPage(BaseModel):
    page: int
    size: int
    total: int
    pages: int
    items: list[Star]


SAMPLES = {   # key -> (path, label); the page and the load generator read this
    "fast": ("/sample/fast", "fast"),
    "slow": ("/sample/slow", "slow (random latency)"),
    "flaky": ("/sample/flaky", "flaky"),
    "cpu": ("/sample/cpu", "cpu (prime sieve)"),
    "items": ("/sample/items", "items (paginated)"),
}
CONTRACTS = {   # path -> {status: model}; a status not listed is not checked
    "/sample/fast": {200: Pong},
    "/sample/slow": {200: Slept},
    "/sample/flaky": {200: Flaky, 503: Problem},
    "/sample/cpu": {200: Primes},
    "/sample/items": {200: StarPage},
}


class Settings(BaseModel):
    """What the sample API does; the page's sliders write these."""
    flaky_rate: float = Field(0.2, ge=0, le=1, description="share of /sample/flaky calls answered 503")
    slow_median_ms: float = Field(120, ge=1, le=3000, description="median of /sample/slow's log-normal sleep")
    slow_sigma: float = Field(0.6, ge=0, le=2, description="log-normal spread of that sleep")
    drift: float = Field(0, ge=0, le=1, description="share of /sample/items pages that break their contract")
    window_s: int = Field(60, ge=5, le=600, description="sliding window the observatory summarises")


SETTINGS = Settings()


# ---- the record ---------------------------------------------------------------------------

PATHS = [*CONTRACTS, "(unmatched)"]   # a request's path is stored as its index here


class Ring:
    """The last CAP requests as columns: numpy arrays written in a circle.

    Columns, not a list of tuples, because the page asks for a snapshot twice a
    second: a window of tens of thousands of requests is then a few vector
    operations, not a Python loop that holds the event loop. Bounded so a long
    load run cannot grow memory without limit.
    """
    CAP = 200_000

    def __init__(self):
        self.t = np.zeros(self.CAP)                       # monotonic s at completion
        self.path = np.zeros(self.CAP, np.int16)          # index into PATHS
        self.status = np.zeros(self.CAP, np.int16)
        self.ms = np.zeros(self.CAP)                      # latency
        self.contract = np.zeros(self.CAP, np.int8)       # 1 valid, 0 broken, -1 not checked
        self.n = 0                                        # written since reset
        self.reset_at = T0

    def add(self, t, path, status, ms, contract):
        i = self.n % self.CAP
        self.t[i], self.path[i], self.status[i], self.ms[i], self.contract[i] = t, path, status, ms, contract
        self.n += 1

    def clear(self):
        self.n = 0
        self.reset_at = time.monotonic()

    def window(self, seconds: float, path: str | None = None) -> dict:
        """Columns of the requests newer than now - seconds, oldest first."""
        k = min(self.n, self.CAP)
        idx = (np.arange(k) + (self.n - k)) % self.CAP
        keep = self.t[idx] >= time.monotonic() - seconds
        if path is not None:
            keep &= self.path[idx] == PATHS.index(path)
        idx = idx[keep]
        return {c: getattr(self, c)[idx] for c in ("t", "path", "status", "ms", "contract")}

    def last(self, count: int) -> list[int]:
        """Positions of the newest `count` requests, newest first."""
        return [(self.n - 1 - j) % self.CAP for j in range(min(count, self.n, self.CAP))]


RING = Ring()
VIOLATIONS: deque = deque(maxlen=20)   # (monotonic s, path, status, first pydantic error)


def check(path: str, status: int, body: bytes):
    """Validate one response body against its declared model: (True|False|None, error)."""
    model = CONTRACTS.get(path, {}).get(status)
    if model is None:
        return None, ""
    try:
        model.model_validate_json(body)
        return True, ""
    except ValidationError as e:
        err = e.errors()[0]
        return False, f"{model.__name__}.{'.'.join(map(str, err['loc']))}: {err['msg']}"


class Recorder:
    """Pure ASGI middleware: time /sample/ requests from first byte in to last byte out."""

    def __init__(self, app):
        self.app = app

    async def __call__(self, scope, receive, send):
        if scope["type"] != "http" or not scope["path"].startswith("/sample/"):
            return await self.app(scope, receive, send)
        t0 = time.perf_counter()
        status, body = 500, bytearray()

        async def capture(msg):
            nonlocal status
            if msg["type"] == "http.response.start":
                status = msg["status"]
            elif msg["type"] == "http.response.body":
                body.extend(msg.get("body", b""))
            await send(msg)

        try:
            await self.app(scope, receive, capture)
        finally:
            ms = (time.perf_counter() - t0) * 1000
            path = scope["path"] if scope["path"] in CONTRACTS else "(unmatched)"
            ok, err = check(path, status, bytes(body))
            now = time.monotonic()
            RING.add(now, PATHS.index(path), status, ms, -1 if ok is None else int(ok))
            if ok is False:
                VIOLATIONS.append((now, path, status, err))


# ---- statistics ---------------------------------------------------------------------------

def percentile(xs, p: float) -> float | None:
    """Linear interpolation between closest ranks (Hyndman & Fan type 7, numpy's default).

    Written out rather than delegated so the proof can hold it against numpy.percentile.
    xs must be sorted.
    """
    if len(xs) == 0:
        return None
    h = (len(xs) - 1) * p / 100
    lo = math.floor(h)
    hi = min(lo + 1, len(xs) - 1)
    return float(xs[lo] + (h - lo) * (xs[hi] - xs[lo]))


# log-spaced latency bins, 8 per decade from 0.03 ms to 30 s
EDGES = np.logspace(-1.5, 4.5, 49)


def summary(w: dict, span: float) -> dict:
    lat = np.sort(w["ms"])
    n = len(lat)
    errors = int((w["status"] >= 500).sum())
    return {
        "n": n,
        "rps": n / span if span > 0 else 0.0,
        "err_rate": errors / n if n else None,
        "errors": errors,
        "contract_fail": int((w["contract"] == 0).sum()),
        "contract_checked": int((w["contract"] >= 0).sum()),
        "p50": percentile(lat, 50), "p95": percentile(lat, 95), "p99": percentile(lat, 99),
        "mean": float(lat.mean()) if n else None, "max": float(lat[-1]) if n else None,
    }


def snapshot(win: float) -> dict:
    now = time.monotonic()
    w = RING.window(win)
    span = min(win, now - RING.reset_at)
    counts, _ = np.histogram(np.clip(w["ms"], EDGES[0], EDGES[-1]), bins=EDGES)
    codes, code_n = np.unique(w["status"], return_counts=True)
    label = {p: lab for p, lab in SAMPLES.values()}
    shown = [p for p, _ in SAMPLES.values()] + (["(unmatched)"] if (w["path"] == len(PATHS) - 1).any() else [])
    # last 60 s, one bucket per second, oldest first: answered < 500 and >= 500
    m = RING.window(60)
    bucket = 59 - np.minimum(59, (now - m["t"]).astype(int))
    bad = m["status"] >= 500
    return {
        "window_s": win,
        "span_s": span,
        "uptime_s": now - T0,
        **summary(w, span),
        "hist": {"edges": EDGES.tolist(), "counts": counts.tolist()},
        "status": {str(c): int(k) for c, k in zip(codes, code_n)},
        "endpoints": [{"path": p, "label": label.get(p, p),
                       **summary({c: v[w["path"] == PATHS.index(p)] for c, v in w.items()}, span)} for p in shown],
        "timeline": {"ok": np.bincount(bucket[~bad], minlength=60).tolist(),
                     "err": np.bincount(bucket[bad], minlength=60).tolist()},
        "recent": [{"path": PATHS[RING.path[i]], "status": int(RING.status[i]), "ms": float(RING.ms[i]),
                    "contract": None if RING.contract[i] < 0 else bool(RING.contract[i]),
                    "age_s": now - float(RING.t[i])} for i in RING.last(25)],
        "violations": [{"path": v[1], "status": v[2], "error": v[3], "age_s": now - v[0]}
                       for v in reversed(VIOLATIONS)],
        "settings": SETTINGS.model_dump(),
        "load_running": LOAD.locked(),
    }


def sse(event: str, data: dict) -> str:
    return f"event: {event}\ndata: {json.dumps(data, separators=(',', ':'))}\n\n"


def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float]:
    """95% Wilson score interval for a proportion k/n: where the true error rate lies."""
    if n == 0:
        return 0.0, 1.0
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return max(0.0, c - h), min(1.0, c + h)


# ---- app ----------------------------------------------------------------------------------

app = FastAPI(
    title="API Observatory · __HOST__",
    description="Sample endpoints under /sample/ and the observatory that watches them.",
    openapi_tags=[{"name": "sample", "description": "the API being observed"},
                  {"name": "observatory", "description": "what the page reads"}],
)
app.add_middleware(Recorder)
log.info("__HOST__: API Observatory starting on 127.0.0.1:%s, watching %d sample endpoints",
         os.environ.get("PORT"), len(SAMPLES))

BURSTS = 0   # load and contract runs in progress: their own /sample/ calls


class BurstAtDebug(logging.Filter):
    """A load run sends up to 200,000 requests to /sample/: while one runs, those
    access lines are DEBUG (the observatory records each one), the rest INFO."""
    def filter(self, r):
        if BURSTS and r.args and str(r.args[2]).startswith("/sample/"):
            r.levelno, r.levelname = logging.DEBUG, "DEBUG"
            return logging.getLogger().isEnabledFor(logging.DEBUG)
        return True


logging.getLogger("uvicorn.access").addFilter(BurstAtDebug())
logging.getLogger("httpx").setLevel(logging.WARNING)   # it logs every request the load generator sends


@app.get("/health", include_in_schema=False)
def health() -> dict:
    """Answers once the app is serving (app.env APP_HEALTH; up.sh waits on it)."""
    return {"ok": True}


app.mount("/static", StaticFiles(directory=HERE / "static"), name="static")


@app.get("/", include_in_schema=False)
def index():
    return FileResponse(HERE / "static" / "index.html")


# ---- the sample API -------------------------------------------------------------------------
# Routes answer plain dicts on purpose: the declared models document the contract in
# /docs, and the middleware - not FastAPI's own response filtering - checks what
# went out on the wire, so a drifting response shows up instead of being masked.

@app.get("/sample/fast", tags=["sample"], responses={200: {"model": Pong}})
async def sample_fast():
    return {"ok": True, "server_time": time.time()}


@app.get("/sample/slow", tags=["sample"], responses={200: {"model": Slept}})
async def sample_slow():
    """Sleeps a log-normal time: median slow_median_ms, spread slow_sigma (asyncio, so it holds no thread)."""
    ms = random.lognormvariate(math.log(SETTINGS.slow_median_ms), SETTINGS.slow_sigma)
    ms = min(ms, 10_000)
    await asyncio.sleep(ms / 1000)
    return {"slept_ms": ms, "median_ms": SETTINGS.slow_median_ms}


@app.get("/sample/flaky", tags=["sample"], responses={200: {"model": Flaky}, 503: {"model": Problem}})
async def sample_flaky():
    """Fails with 503 for a flaky_rate share of calls, independently each time."""
    if random.random() < SETTINGS.flaky_rate:
        return JSONResponse({"detail": "flaky by design"}, status_code=503)
    return {"ok": True, "failure_rate": SETTINGS.flaky_rate}


@app.get("/sample/cpu", tags=["sample"], responses={200: {"model": Primes}})
def sample_cpu(n: int = Query(1_000_000, ge=10, le=20_000_000, description="count primes up to n")):
    """Sieve of Eratosthenes. A plain def: FastAPI runs it in its thread pool, off the event loop."""
    sieve = bytearray([1]) * (n + 1)
    sieve[0:2] = b"\x00\x00"
    for i in range(2, math.isqrt(n) + 1):
        if sieve[i]:
            sieve[i * i::i] = bytes(len(range(i * i, n + 1, i)))
    return {"n": n, "count": sieve.count(1), "largest": sieve.rindex(1)}


CONSTELLATIONS = ["Andromeda", "Aquila", "Auriga", "Boötes", "Carina", "Cassiopeia", "Cygnus",
                  "Draco", "Lyra", "Orion", "Perseus", "Scorpius", "Taurus", "Ursa Major"]
_rng = random.Random(7)
CATALOGUE = [{"id": i, "name": f"NDX {1000 + i}", "constellation": _rng.choice(CONSTELLATIONS),
              "magnitude": round(_rng.uniform(-1.5, 6.5), 2)} for i in range(1, 1001)]


@app.get("/sample/items", tags=["sample"], responses={200: {"model": StarPage}})
async def sample_items(page: int = Query(1, ge=1), size: int = Query(20, ge=1, le=100)):
    """A built-in catalogue of 1,000 synthetic stars, one page at a time."""
    pages = math.ceil(len(CATALOGUE) / size)
    items = CATALOGUE[(page - 1) * size: page * size]
    if items and random.random() < SETTINGS.drift:
        # schema drift: one item loses its number - the kind of change that breaks clients
        items = [*items]
        items[0] = {**items[0], "magnitude": "n/a"}
    return {"page": page, "size": size, "total": len(CATALOGUE), "pages": pages, "items": items}


# ---- the observatory -----------------------------------------------------------------------

@app.get("/api/stats", tags=["observatory"])
def stats(window_s: float | None = Query(None, ge=1, le=3600)):
    """One snapshot of the sliding window (window_s defaults to the setting)."""
    return snapshot(window_s or SETTINGS.window_s)


@app.get("/api/samples", tags=["observatory"])
def samples(window_s: float = Query(60, ge=1, le=3600), path: str | None = None):
    """The raw latencies (ms, oldest first) behind a snapshot - for checking the percentiles elsewhere."""
    if path is not None and path not in PATHS:
        raise HTTPException(422, f"path must be one of {PATHS}")
    w = RING.window(window_s, path)
    return {"n": len(w["ms"]), "ms": w["ms"].tolist(), "status": w["status"].tolist()}


@app.get("/api/stream", tags=["observatory"])
async def stream(request: Request):
    """Server-Sent Events: a snapshot event twice a second until the page goes away."""
    async def gen():
        while not await request.is_disconnected():
            yield sse("snapshot", snapshot(SETTINGS.window_s))
            await asyncio.sleep(0.5)
    return StreamingResponse(gen(), media_type="text/event-stream", headers={"Cache-Control": "no-store", "X-Accel-Buffering": "no"})


@app.get("/api/settings", tags=["observatory"], response_model=Settings)
def settings_get():
    return SETTINGS


@app.post("/api/settings", tags=["observatory"], response_model=Settings)
def settings_set(patch: dict = Body(..., examples=[{"flaky_rate": 0.3}])):
    """Change any subset of the settings; the rest keep their values."""
    global SETTINGS
    try:
        SETTINGS = Settings.model_validate(SETTINGS.model_dump() | patch)
    except ValidationError as e:
        raise HTTPException(422, e.errors(include_url=False, include_context=False))
    return SETTINGS


@app.post("/api/reset", tags=["observatory"])
def reset():
    """Forget every recorded request."""
    RING.clear()
    VIOLATIONS.clear()
    return {"ok": True}


LOAD = asyncio.Lock()   # one load run at a time: two would measure each other


@app.get("/api/load", tags=["observatory"])
async def load(request: Request,
               endpoint: Literal["fast", "slow", "flaky", "cpu", "items"] = "flaky",
               concurrency: int = Query(8, ge=1, le=64),
               duration: float = Query(10, ge=1, le=120),
               limit: int | None = Query(None, ge=1, le=200_000, description="stop after this many requests")):
    """Server-Sent Events: `concurrency` workers call the endpoint over loopback HTTP until
    `duration` seconds or `limit` requests; a `tick` event every 250 ms, then `done`."""
    host, port = request.scope["server"]
    url = f"http://{host}:{port}{SAMPLES[endpoint][0]}"

    async def gen():
        global BURSTS
        if LOAD.locked():
            yield sse("busy", {"detail": "a load run is in progress"})
            return
        async with LOAD:
            lat: list[float] = []
            codes: Counter = Counter()
            sent = 0
            t_start = time.perf_counter()
            deadline = t_start + duration

            def progress(final: bool) -> dict:
                done = len(lat)
                errs = sum(v for c, v in codes.items() if c >= 500 or c == 0)
                s = sorted(lat)
                lo, hi = wilson(errs, done)
                el = time.perf_counter() - t_start
                return {"endpoint": endpoint, "elapsed": el, "duration": duration, "done": done,
                        "rps": done / el if el else 0, "errors": errs, "err_rate": errs / done if done else None,
                        "err_ci": [lo, hi], "setting": SETTINGS.flaky_rate if endpoint == "flaky" else None,
                        "p50": percentile(s, 50), "p95": percentile(s, 95), "p99": percentile(s, 99),
                        "codes": {str(c): v for c, v in sorted(codes.items())}, "final": final}

            limits = httpx.Limits(max_connections=concurrency, max_keepalive_connections=concurrency)
            async with httpx.AsyncClient(limits=limits, timeout=30) as client:
                async def worker():
                    nonlocal sent
                    while time.perf_counter() < deadline and (limit is None or sent < limit):
                        sent += 1
                        t = time.perf_counter()
                        try:
                            code = (await client.get(url)).status_code
                        except httpx.HTTPError:
                            code = 0
                        lat.append((time.perf_counter() - t) * 1000)
                        codes[code] += 1

                BURSTS += 1
                tasks = [asyncio.create_task(worker()) for _ in range(concurrency)]
                try:
                    while True:
                        finished, _ = await asyncio.wait(tasks, timeout=0.25)
                        if len(finished) == len(tasks):
                            break
                        yield sse("tick", progress(False))
                finally:
                    BURSTS -= 1
                    for t in tasks:
                        t.cancel()
            final = progress(True)
            log.info("load run on %s: %d requests in %.1f s (%.0f/s, x%d), %d errors, p95 %s ms",
                     SAMPLES[endpoint][0], final["done"], final["elapsed"], final["rps"], concurrency,
                     final["errors"], None if final["p95"] is None else round(final["p95"], 1))
        # sent after the lock is released, so a client that starts its next run on
        # `done` does not find this one still holding it
        yield sse("done", final)

    return StreamingResponse(gen(), media_type="text/event-stream", headers={"Cache-Control": "no-store", "X-Accel-Buffering": "no"})


@app.post("/api/contract", tags=["observatory"])
async def contract(request: Request, calls: int = Query(25, ge=1, le=500)):
    """Call every sample endpoint `calls` times over loopback and validate each answer
    against its declared model - the same check the middleware runs on all traffic."""
    global BURSTS
    host, port = request.scope["server"]
    out = []
    BURSTS += 1
    try:
        async with httpx.AsyncClient(base_url=f"http://{host}:{port}", timeout=30) as client:
            for key, (path, label) in SAMPLES.items():
                params = {"n": 100_000} if key == "cpu" else {"page": 3} if key == "items" else None
                res = await asyncio.gather(*(client.get(path, params=params) for _ in range(calls)))
                verdicts = [check(path, r.status_code, r.content) for r in res]
                failed = [e for ok, e in verdicts if ok is False]
                out.append({"path": path, "label": label, "calls": calls,
                            "checked": sum(ok is not None for ok, _ in verdicts),
                            "passed": sum(ok is True for ok, _ in verdicts),
                            "failed": len(failed), "first_error": failed[0] if failed else None,
                            "models": {str(s): m.__name__ for s, m in CONTRACTS[path].items()},
                            "statuses": dict(Counter(str(r.status_code) for r in res))})
    finally:
        BURSTS -= 1
    log.info("contract check: %d calls to each of %d endpoints, %d answers broke their model",
             calls, len(out), sum(e["failed"] for e in out))
    return {"endpoints": out}


@app.get("/api/env", tags=["observatory"])
def env():
    """What runs, read from this process: runtime, stack module, package versions, host."""
    mods = [m for m in os.environ.get("LOADEDMODULES", "").split(":") if m]
    stack = [m for m in mods if m.startswith("env/")]
    return {
        "runtime": platform.python_implementation(),
        "version": platform.python_version(),
        "executable": sys.executable,
        "stack_module": stack[-1] if stack else None,
        "modules_loaded": len(mods),
        "packages": {p: version(p) for p in IMPORTS},
        "started": STARTED.isoformat(timespec="seconds"),
        "uptime_s": round(time.monotonic() - T0, 1),
        "pid": os.getpid(),
        "cpus": os.cpu_count(),
        "platform": platform.platform(),
    }
