"""Logging for __HOST__, standard library only. Every line goes to stdout,
flushed, in the one format every Cove starter writes:

    2026-11-18 12:04:31 INFO    [app] serving on 127.0.0.1:8834

so .cove/run/app.log (Cove's Logs tab) reads the same in Python, R, Node,
Julia and Rust. Times are UTC. Levels, shown when at or above LOG_LEVEL
(default INFO): TRACE DEBUG INFO SUCCESS WARN ERROR.

    from log import log, get
    log.info("serving on 127.0.0.1:%s", port)
    db = get("db")              # [db] on its lines
    db.success("loaded %d rows", n)
    log.exception("render failed")   # ERROR with the traceback

Importing it configures the root logger, so the libraries that log through
`logging` (werkzeug, uvicorn, dash, panel) write in the same format.
"""
import logging
import os
import re
import sys
import time

TRACE, SUCCESS = 5, 25
logging.addLevelName(TRACE, "TRACE")
logging.addLevelName(SUCCESS, "SUCCESS")
logging.addLevelName(logging.WARNING, "WARN")


class _Logger(logging.Logger):
    def trace(self, msg, *args, **kw):
        if self.isEnabledFor(TRACE):
            self._log(TRACE, msg, args, **kw)

    def success(self, msg, *args, **kw):
        if self.isEnabledFor(SUCCESS):
            self._log(SUCCESS, msg, args, **kw)


class _Flushing(logging.StreamHandler):
    # uvicorn's "uvicorn.error" carries its ordinary lines too ("Started server
    # process"), and Cove's health probes every few seconds would bury the rest:
    # [uvicorn] and [request], and /health and /__live at TRACE, as in Node.
    NAMES = {"uvicorn.error": "uvicorn", "uvicorn.access": "request", "werkzeug": "request"}

    PROBES = ("GET /health ", "GET /__live ", "/_reload-hash ")
    ANSI = re.compile(r"\x1b\[[0-9;]*m")

    def emit(self, record):
        record.name = self.NAMES.get(record.name, record.name)
        # werkzeug colours its lines; the log is a file, not a terminal.
        text = record.getMessage()
        if "\x1b[" in text:
            text = self.ANSI.sub("", text)
            record.msg, record.args = text, None
        if record.name == "request" and record.levelno == logging.INFO and any(p in text for p in self.PROBES):
            if logging.getLogger().getEffectiveLevel() > TRACE:
                return
            record.levelno, record.levelname = TRACE, "TRACE"
        super().emit(record)
        self.flush()


def _setup():
    logging.setLoggerClass(_Logger)
    fmt = logging.Formatter("%(asctime)s %(levelname)-7s [%(name)s] %(message)s", "%Y-%m-%d %H:%M:%S")
    fmt.converter = time.gmtime
    handler = _Flushing(sys.stdout)
    handler.setFormatter(fmt)
    root = logging.getLogger()
    root.handlers[:] = [handler]
    level = os.environ.get("LOG_LEVEL", "INFO").upper()
    root.setLevel({"WARN": logging.WARNING, "TRACE": TRACE, "SUCCESS": SUCCESS}.get(level, getattr(logging, level, logging.INFO)))
    # uvicorn gives its loggers handlers of their own and stops them passing
    # lines up, so its access lines kept uvicorn's format (`INFO:     127.0.0.1
    # - "GET / ..."`). It has configured them by the time it imports the app,
    # which is when this runs: hand them to the root's handler.
    for name in ("uvicorn", "uvicorn.error", "uvicorn.access"):
        lg = logging.getLogger(name)
        lg.handlers[:] = []
        lg.propagate = True


def get(name="app"):
    """A logger whose lines say [name]."""
    return logging.getLogger(name)


_setup()
log = get(os.environ.get("LOG_NAME", "app"))
