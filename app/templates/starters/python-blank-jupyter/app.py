"""__HOST__: a Jupyter notebook published as a page.

notebook.ipynb is run from the top (nbclient, the stack's Python kernel)
whenever it changes, and drawn as HTML here: markdown through mistune, then
each code cell and what it printed or showed (text, HTML, PNG, SVG, errors).
The run happens in memory; the notebook on disk is never written. A notebook
that fails keeps the last good page and shows the error above it.
"""
import os
import re
import threading
import time
import traceback
from html import escape
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import mistune
import nbformat
from nbclient import NotebookClient

from log import log

HERE = Path(__file__).parent
NOTEBOOK = HERE / "notebook.ipynb"
ANSI = re.compile(r"\x1b\[[0-9;]*m")
STYLE = """<style>
body { max-width: 52rem; margin: 2rem auto; padding: 0 1rem; font: 16px/1.5 system-ui, sans-serif; }
pre { background: #f6f6f6; padding: .75em; overflow-x: auto; border-radius: 4px; }
pre.out { background: none; border-left: 3px solid #ddd; }
pre.err { background: #fee; color: #900; white-space: pre-wrap; }
img, svg { max-width: 100%; }
@media (prefers-color-scheme: dark) { body { background: #111; color: #ddd; } pre { background: #1d1d1d; } }
</style>"""
LIVE = """<script>
// Reloads the page when the notebook has run again (GET /__live).
(() => { let seen = null; setInterval(async () => { if (document.hidden) return;
  try { const v = await (await fetch('/__live', {cache: 'no-store'})).text();
        if (seen !== null && v !== seen) location.reload(); seen = v; } catch {} }, 1000); })();
</script>"""

state = {"body": "<p>Running the notebook for the first time...</p>", "error": "", "version": "0", "ran": None}
lock = threading.Lock()


def text(x):
    return "".join(x) if isinstance(x, list) else x


def output(o):
    kind = o.get("output_type")
    if kind == "stream":
        return f'<pre class="out">{escape(text(o["text"]))}</pre>'
    if kind == "error":
        return f'<pre class="err">{escape(ANSI.sub("", chr(10).join(o["traceback"])))}</pre>'
    data = o.get("data", {})
    if "image/png" in data:
        return f'<img alt="" src="data:image/png;base64,{text(data["image/png"]).strip()}">'
    if "image/svg+xml" in data:
        return text(data["image/svg+xml"])
    if "text/html" in data:
        return text(data["text/html"])
    if "text/markdown" in data:
        return mistune.html(text(data["text/markdown"]))
    if "text/plain" in data:
        return f'<pre class="out">{escape(text(data["text/plain"]))}</pre>'
    return ""


def draw(nb):
    parts = []
    for c in nb.cells:
        if c.cell_type == "markdown":
            parts.append(mistune.html(c.source))
        elif c.cell_type == "code":
            parts.append(f"<pre><code>{escape(c.source)}</code></pre>")
            parts.extend(output(o) for o in c.get("outputs", []))
    return "\n".join(parts)


def run():
    nb = nbformat.read(NOTEBOOK, as_version=4)
    NotebookClient(nb, kernel_name="python3", timeout=600, resources={"metadata": {"path": str(HERE)}}).execute()
    return draw(nb)


def watch():
    while True:
        try:
            mtime = NOTEBOOK.stat().st_mtime
        except FileNotFoundError:
            mtime = None
        if mtime != state["ran"]:
            state["ran"] = mtime
            started = time.time()
            try:
                body, error = run(), ""
                log.success("ran notebook.ipynb in %.1f s", time.time() - started)
            except Exception:
                body, error = None, ANSI.sub("", traceback.format_exc(limit=3))
                log.exception("notebook.ipynb failed; the page keeps its last good run")
            with lock:
                if body is not None:
                    state["body"] = body
                state["error"] = error
                state["version"] = str(time.time())
        time.sleep(1)


def page():
    with lock:
        body, error = state["body"], state["error"]
    banner = f'<pre class="err">{escape(error)}</pre>' if error else ""
    return f'<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>__HOST__</title>{STYLE}</head><body>{banner}{body}{LIVE}</body></html>'


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/health":
            self.reply(200, "text/plain", "ok")
        elif path == "/__live":
            self.reply(200, "text/plain", state["version"])
        elif path == "/":
            self.reply(200, "text/html; charset=utf-8", page())
        else:
            self.reply(404, "text/plain", "not found")

    def reply(self, code, kind, body):
        data = body.encode()
        self.send_response(code)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, fmt, *args):
        # The open page asks /__live once a second; those lines would bury the rest.
        if not self.path.startswith("/__live"):
            log.info("%s %s", self.address_string(), fmt % args)


if __name__ == "__main__":
    log.info("__HOST__: Jupyter notebook page starting on 127.0.0.1:%s", os.environ.get("PORT"))
    threading.Thread(target=watch, daemon=True).start()
    ThreadingHTTPServer(("127.0.0.1", int(os.environ["PORT"])), Handler).serve_forever()
