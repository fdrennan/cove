"""__HOST__: a blank FastAPI app. uvicorn reloads it on every save."""
import time
from pathlib import Path

from fastapi import FastAPI
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles

HERE = Path(__file__).parent
STARTED = time.time()

app = FastAPI(title="__HOST__")
app.mount("/static", StaticFiles(directory=HERE / "static"), name="static")


def version():
    # What live.js watches: a restart, or any file in static/ changing.
    files = [p.stat().st_mtime for p in (HERE / "static").rglob("*") if p.is_file()]
    return f"{STARTED}-{len(files)}-{max(files, default=0)}"


@app.get("/health")
def health():
    return {"ok": True}


@app.get("/__live", include_in_schema=False)
def live():
    return {"version": version()}


@app.get("/", include_in_schema=False)
def home():
    return FileResponse(HERE / "static" / "index.html")
