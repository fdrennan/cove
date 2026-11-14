"""__HOST__: a blank MCP server - tools an AI agent can call - over MCP's
Streamable HTTP transport at /mcp. uvicorn reloads it on every save.

An agent connects with its address, e.g. for Claude Code:

    claude mcp add --transport http __HOST__ https://__HOST__/mcp

A tool is a function in TOOLS: its name, what it does, the JSON Schema of
its arguments, and the Python that answers. This is the protocol itself
(JSON-RPC 2.0 in a POST, one JSON reply), small enough to read; the
official `mcp` package is not in the ndexr stack yet.
"""
import statistics
import time

from fastapi import FastAPI, Request
from fastapi.responses import HTMLResponse, JSONResponse, Response

PROTOCOL = "2025-06-18"
STARTED = time.time()


def describe(numbers: list[float]) -> str:
    if not numbers:
        raise ValueError("numbers is empty")
    sd = statistics.stdev(numbers) if len(numbers) > 1 else 0.0
    return f"n {len(numbers)}, mean {statistics.fmean(numbers):.6g}, sd {sd:.6g}, median {statistics.median(numbers):.6g}"


TOOLS = {
    "describe": {
        "description": "Summary statistics of a list of numbers: count, mean, standard deviation and median.",
        "inputSchema": {
            "type": "object",
            "properties": {"numbers": {"type": "array", "items": {"type": "number"}}},
            "required": ["numbers"],
        },
        "run": lambda args: describe([float(x) for x in args["numbers"]]),
    },
    "uptime": {
        "description": "How long this server has been running, in seconds.",
        "inputSchema": {"type": "object", "properties": {}},
        "run": lambda args: f"{time.time() - STARTED:.0f} s",
    },
}


def answer(method: str, params: dict):
    if method == "initialize":
        return {
            "protocolVersion": params.get("protocolVersion", PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "__HOST__", "version": "0.1.0"},
        }
    if method == "ping":
        return {}
    if method == "tools/list":
        return {"tools": [{"name": n, "description": t["description"], "inputSchema": t["inputSchema"]} for n, t in TOOLS.items()]}
    if method == "tools/call":
        tool = TOOLS.get(params.get("name", ""))
        if tool is None:
            raise LookupError(f"no tool named {params.get('name')!r}")
        try:
            text, failed = tool["run"](params.get("arguments") or {}), False
        except Exception as e:  # the tool's own failure goes back to the agent as its result
            text, failed = f"{type(e).__name__}: {e}", True
        return {"content": [{"type": "text", "text": str(text)}], "isError": failed}
    raise LookupError(f"method not found: {method}")


app = FastAPI(title="__HOST__")


@app.post("/mcp")
async def mcp(request: Request):
    try:
        msg = await request.json()
    except ValueError:
        return JSONResponse({"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "parse error"}}, 400)
    if "id" not in msg:  # a notification (notifications/initialized): nothing to answer
        return Response(status_code=202)
    try:
        return JSONResponse({"jsonrpc": "2.0", "id": msg["id"], "result": answer(msg.get("method", ""), msg.get("params") or {})})
    except LookupError as e:
        return JSONResponse({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32601, "message": str(e)}})


@app.get("/mcp")
def mcp_stream():
    # No server-initiated messages, so no event stream to open.
    return Response(status_code=405, headers={"Allow": "POST"})


@app.get("/health")
def health():
    return {"ok": True}


@app.get("/", response_class=HTMLResponse, include_in_schema=False)
def home():
    rows = "".join(f"<li><b>{n}</b> - {t['description']}</li>" for n, t in TOOLS.items())
    return (
        "<!doctype html><title>__HOST__</title>"
        "<main style='max-width:40rem;margin:3rem auto;font-family:system-ui,sans-serif'>"
        "<h1>__HOST__</h1><p>A blank MCP server, running. An agent connects at <code>/mcp</code>:</p>"
        "<pre>claude mcp add --transport http __HOST__ https://__HOST__/mcp</pre>"
        f"<p>Its tools:</p><ul>{rows}</ul></main>"
    )
