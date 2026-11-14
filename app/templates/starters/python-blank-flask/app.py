"""__HOST__: a blank Flask app. Flask reloads it on every save."""
import time
from pathlib import Path

from flask import Flask, jsonify

HERE = Path(__file__).parent
STARTED = time.time()

app = Flask(__name__)


def version():
    # What live.js watches: a restart, or any file in static/ changing.
    files = [p.stat().st_mtime for p in (HERE / "static").rglob("*") if p.is_file()]
    return f"{STARTED}-{len(files)}-{max(files, default=0)}"


@app.get("/health")
def health():
    return jsonify(ok=True)


@app.get("/__live")
def live():
    return jsonify(version=version())


@app.get("/")
def home():
    return app.send_static_file("index.html")


if __name__ == "__main__":
    import os

    # debug=False: Werkzeug's debugger runs code typed in the browser, and nginx
    # puts this on the web. The "stat" reloader checks the files the app
    # imported; the default (watchdog, in the stack) put an inotify watch on
    # every folder of the site library - 14,474 of the host's 65,536 for one
    # blank app (2026-11-07), which stopped an Inlet build on the same host.
    app.run(host="127.0.0.1", port=int(os.environ["PORT"]), debug=False, use_reloader=True, reloader_type="stat")
