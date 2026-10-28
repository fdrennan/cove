"""Live system dashboard: psutil sampled every 2 s, kept as a rolling history.

One sampler task fills a ring buffer, so every visitor sees the same samples
and a page opened late still gets the last five minutes at once. Rates (network,
disk) are deltas between consecutive samples divided by the real interval.
"""
import asyncio
import os
import platform
import socket
import time
from collections import deque
from contextlib import asynccontextmanager
from pathlib import Path

import psutil
from fastapi import FastAPI, Query
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles

HERE = Path(__file__).parent
EVERY = 2.0                  # seconds between samples
KEEP = 150                   # samples kept: 5 minutes
history: deque = deque(maxlen=KEEP)
_last = {"t": None, "net": None, "disk": None}

SKIP_FS = {"squashfs", "tmpfs", "devtmpfs", "overlay", "proc", "sysfs", "cgroup", "cgroup2",
           "fuse.cvmfs2", "fuse.snapfuse", "autofs", "nsfs", "efivarfs"}


def disks() -> list[dict]:
    """Mounted filesystems that hold data: no pseudo, loop or snap mounts, one row per device."""
    seen, out = set(), []
    for p in psutil.disk_partitions(all=False):
        if p.fstype in SKIP_FS or p.device in seen or p.device.startswith("/dev/loop") or "/snap/" in p.mountpoint:
            continue
        try:
            u = psutil.disk_usage(p.mountpoint)
        except (PermissionError, OSError):
            continue
        seen.add(p.device)
        out.append({"mount": p.mountpoint, "fs": p.fstype, "total": u.total, "used": u.used, "percent": u.percent})
    return sorted(out, key=lambda d: -d["total"])[:8]


def top_processes(n: int = 8) -> list[dict]:
    rows = []
    for p in psutil.process_iter(["pid", "name", "username", "cpu_percent", "memory_info"]):
        i = p.info
        if i["memory_info"] is None:
            continue
        rows.append({"pid": i["pid"], "name": i["name"] or "?", "user": i["username"] or "",
                     "cpu": round(i["cpu_percent"] or 0.0, 1), "rss": i["memory_info"].rss})
    rows.sort(key=lambda r: (-r["cpu"], -r["rss"]))
    return rows[:n]


def sample() -> dict:
    now = time.time()
    net = psutil.net_io_counters()
    dio = psutil.disk_io_counters()
    dt = now - _last["t"] if _last["t"] else None

    def rate(cur, prev, field):
        if not dt or cur is None or prev is None:
            return 0.0
        return max(0.0, (getattr(cur, field) - getattr(prev, field)) / dt)

    vm, sw = psutil.virtual_memory(), psutil.swap_memory()
    per_core = psutil.cpu_percent(percpu=True)
    s = {
        "t": round(now, 3),
        "cpu": round(sum(per_core) / len(per_core), 1),
        "cores": per_core,
        "load": [round(x, 2) for x in os.getloadavg()],
        "mem": {"total": vm.total, "used": vm.total - vm.available, "percent": vm.percent,
                "swap_total": sw.total, "swap_used": sw.used, "swap_percent": sw.percent},
        "net": {"rx": rate(net, _last["net"], "bytes_recv"), "tx": rate(net, _last["net"], "bytes_sent")},
        "disk_io": {"read": rate(dio, _last["disk"], "read_bytes"), "write": rate(dio, _last["disk"], "write_bytes")},
    }
    _last.update(t=now, net=net, disk=dio)
    return s


async def sampler():
    # The first reading of every counter (cpu, per-process cpu, the byte
    # totals behind the rates) has nothing to diff against: take it, drop it.
    await asyncio.to_thread(sample)
    await asyncio.to_thread(top_processes)
    await asyncio.sleep(EVERY)
    while True:
        try:
            history.append(await asyncio.to_thread(sample))
        except Exception as e:               # one bad read must not end the history
            print("sample failed:", e, flush=True)
        await asyncio.sleep(EVERY)


@asynccontextmanager
async def lifespan(_app):
    task = asyncio.create_task(sampler())
    yield
    task.cancel()


app = FastAPI(title="System dashboard · __HOST__", lifespan=lifespan, docs_url=None, redoc_url=None)


@app.get("/health")
def health() -> dict:
    """Answers once the app is serving (app.env APP_HEALTH; up.sh waits on it)."""
    return {"ok": True}
app.mount("/static", StaticFiles(directory=HERE / "static"), name="static")


@app.get("/")
def index():
    return FileResponse(HERE / "static" / "index.html")


@app.get("/api/info")
def info():
    freq = psutil.cpu_freq()
    return {
        "host": socket.gethostname(),
        "os": f"{platform.system()} {platform.release()}",
        "python": platform.python_version(),
        "psutil": psutil.__version__,
        "cpus_logical": psutil.cpu_count(),
        "cpus_physical": psutil.cpu_count(logical=False),
        "cpu_mhz": round(freq.current) if freq else None,
        "boot": psutil.boot_time(),
        "every": EVERY,
    }


@app.get("/api/stats")
def stats(since: float = Query(0.0, description="only samples newer than this unix time")):
    """The rolling history (newer than `since`), the latest disks and the busiest processes."""
    return {
        "now": time.time(),
        "uptime": time.time() - psutil.boot_time(),
        "samples": [s for s in list(history) if s["t"] > since],
        "disks": disks(),
        "processes": top_processes(),
    }
