#!/usr/bin/env python3
"""Cove's container builder, brokered: anyone may ask for an image of their
own project, nobody builds it in their own account.

    broker.py serve            the service (cove-container.service, as ubuntu)
    broker.py build <project>  ask for one, as the account running this
    broker.py list  <project>
    broker.py fetch <project> <stamp> <file> > out

A Unix socket at /run/cove-container/broker.sock, like ndexr vault: the kernel
names the caller (SO_PEERCRED), so there is no token, and an account reaches
only its own projects - src/domains/<account>/<project>. uid 0 and ubuntu may
pass "as". One build at a time: each compiles a whole project.

Every request is one JSON line; every reply is one JSON line, and a fetch
reply is followed by the file's bytes.

The builds are build.sh's (beside this file); this decides who may ask and
keeps each build's state in <out>/<account>/<project>/<stamp>/status.json:
queued -> running -> done | failed (with the reason, the log's last line).
"""
import json
import os
import pwd
import queue
import re
import socket
import struct
import subprocess
import sys
import threading
import time

SOCKET = os.environ.get("COVE_CONTAINER_SOCKET", "/run/cove-container/broker.sock")
OUT = os.environ.get("COVE_CONTAINER_OUT", "/srv/ndexr/containers")
DOMAINS = os.environ.get("COVE_CONTAINER_DOMAINS", "/home/ubuntu/ndexr/src/domains")
BUILD = os.path.join(os.path.dirname(os.path.abspath(__file__)), "build.sh")
PRIVILEGED = {0, pwd.getpwnam("ubuntu").pw_uid}

PROJECT = re.compile(r"^[a-z0-9][a-z0-9-]*(\.[a-z0-9-]+)+$")
ACCOUNT = re.compile(r"^[a-z0-9][a-z0-9_-]*$")
STAMP = re.compile(r"^\d{8}T\d{6}Z$")
FILES = {"manifest.json", "build.log", "RUN.txt"}

jobs = queue.Queue()
pending = set()  # (account, project) queued or running: one at a time each
lock = threading.Lock()


def now():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def write_status(d, **kv):
    path = os.path.join(d, "status.json")
    try:
        with open(path) as f:
            st = json.load(f)
    except (OSError, ValueError):
        st = {}
    st.update(kv)
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(st, f)
    os.replace(tmp, path)


def project_dir(account, project):
    if not ACCOUNT.match(account or "") or not PROJECT.match(project or ""):
        raise ValueError("no such project")
    d = os.path.join(DOMAINS, account, project)
    if not os.path.isfile(os.path.join(d, ".cove", "app.env")):
        raise ValueError(f"{account} has no Cove project {project}")
    return d


def account_of(uid, req):
    asked = req.get("as")
    if asked:
        if uid not in PRIVILEGED:
            raise PermissionError("only the platform may build for another account")
        return asked
    return pwd.getpwuid(uid).pw_name


def builds(account, project, limit=20):
    root = os.path.join(OUT, account, project)
    try:
        stamps = sorted((s for s in os.listdir(root) if STAMP.match(s)), reverse=True)[:limit]
    except FileNotFoundError:
        return []
    out = []
    for s in stamps:
        d = os.path.join(root, s)
        b = {"stamp": s}
        try:
            with open(os.path.join(d, "status.json")) as f:
                b.update(json.load(f))
        except (OSError, ValueError):
            b["state"] = "unknown"
        try:
            with open(os.path.join(d, "manifest.json")) as f:
                b["manifest"] = json.load(f)
        except (OSError, ValueError):
            pass
        out.append(b)
    return out


def worker():
    while True:
        account, project, stamp = jobs.get()
        d = os.path.join(OUT, account, project, stamp)
        write_status(d, state="running", started=now())
        env = dict(os.environ, CONTAINER_STAMP=stamp)
        try:
            r = subprocess.run([BUILD, project_dir(account, project), OUT], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=3 * 3600)
            ok = r.returncode == 0
        except Exception as e:  # a vanished project or a hung build
            ok = False
            with open(os.path.join(d, "build.log"), "a") as f:
                f.write(f"container: {e}\n")
        reason = None
        if not ok:
            try:
                with open(os.path.join(d, "build.log"), errors="replace") as f:
                    lines = [l.rstrip() for l in f if l.strip()]
                said = [l for l in lines if l.startswith("container: ")]
                reason = (said or lines or ["the build stopped without a word"])[-1]
            except OSError:
                reason = "the build left no log"
        write_status(d, state="done" if ok else "failed", finished=now(), error=reason)
        print(f"container: {account}/{project} {stamp} -> {'done' if ok else reason}", flush=True)
        with lock:
            pending.discard((account, project))


def answer(uid, req, conn):
    op = req.get("op")
    account = account_of(uid, req)
    project = req.get("project", "")
    project_dir(account, project)  # refuses what is not this account's
    if op == "build":
        with lock:
            if (account, project) in pending:
                raise ValueError(f"{project} is already building - one at a time")
            pending.add((account, project))
        stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
        d = os.path.join(OUT, account, project, stamp)
        os.makedirs(d, exist_ok=True)
        write_status(d, state="queued", queued=now(), by=pwd.getpwuid(uid).pw_name)
        jobs.put((account, project, stamp))
        return {"stamp": stamp, "ahead": jobs.qsize() - 1}, None
    if op == "list":
        return {"builds": builds(account, project)}, None
    if op == "fetch":
        stamp, name = req.get("stamp", ""), req.get("file", "")
        if not STAMP.match(stamp) or name not in FILES | {f"{project}.sif"}:
            raise ValueError("no such file")
        path = os.path.join(OUT, account, project, stamp, name)
        if not os.path.isfile(path):
            raise ValueError(f"{name} is not in build {stamp}")
        return {"file": name, "bytes": os.path.getsize(path)}, path
    raise ValueError(f"{op}: not a container operation")


def serve_one(conn):
    with conn:
        cred = conn.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i"))
        _, uid, _ = struct.unpack("3i", cred)
        line = conn.makefile("rb").readline(64 * 1024)
        send = None
        try:
            req = json.loads(line)
            reply, send = answer(uid, req, conn)
            reply["ok"] = True
            desc = f"uid={uid} op={req.get('op')} project={req.get('project')}"
        except Exception as e:
            reply = {"ok": False, "error": str(e).strip("'\"")}
            desc = f"uid={uid} refused: {reply['error']}"
        print(f"container: {desc}", flush=True)
        conn.sendall((json.dumps(reply) + "\n").encode())
        if send:
            with open(send, "rb") as f:
                conn.sendfile(f)


def recover():
    """A build the service was running when it stopped did not finish: say so.
    One still queued is queued again."""
    for account in os.listdir(OUT) if os.path.isdir(OUT) else []:
        for project in os.listdir(os.path.join(OUT, account)):
            for b in builds(account, project, limit=1000):
                d = os.path.join(OUT, account, project, b["stamp"])
                if b.get("state") == "running":
                    write_status(d, state="failed", finished=now(), error="container: the builder restarted mid-build - ask again")
                elif b.get("state") == "queued":
                    pending.add((account, project))
                    jobs.put((account, project, b["stamp"]))


def serve():
    os.makedirs(os.path.dirname(SOCKET), exist_ok=True)
    try:
        os.unlink(SOCKET)
    except FileNotFoundError:
        pass
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(SOCKET)
    # Anyone may connect; the peer's uid decides what they reach.
    os.chmod(SOCKET, 0o666)
    s.listen(64)
    recover()
    threading.Thread(target=worker, daemon=True).start()
    print(f"container: listening on {SOCKET}", flush=True)
    while True:
        conn, _ = s.accept()
        threading.Thread(target=serve_one, args=(conn,), daemon=True).start()


def call(req, out=None):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        s.connect(SOCKET)
    except OSError as e:
        sys.exit(f"container: the builder is not running here ({SOCKET}: {e})")
    s.sendall((json.dumps(req) + "\n").encode())
    f = s.makefile("rb")
    reply = json.loads(f.readline())
    if not reply.get("ok"):
        sys.exit(f"container: {reply.get('error')}")
    if out is not None:
        left = reply["bytes"]
        while left:
            chunk = f.read(min(left, 1 << 20))
            if not chunk:
                sys.exit("container: the file was cut short")
            out.write(chunk)
            left -= len(chunk)
    return reply


def main(argv):
    if argv[:1] == ["serve"]:
        serve()
    elif len(argv) == 2 and argv[0] in ("build", "list"):
        print(json.dumps(call({"op": argv[0], "project": argv[1]}), indent=2))
    elif len(argv) == 4 and argv[0] == "fetch":
        call({"op": "fetch", "project": argv[1], "stamp": argv[2], "file": argv[3]}, sys.stdout.buffer)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
