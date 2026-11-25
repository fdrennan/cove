#!/usr/bin/env python3
"""Licence compliance for Cove and Inlet, as evidence rather than intent.

Each check states a duty, tests it against what is on disk or served, and
records what it saw. The report lands in docs/compliance/ (latest.md,
latest.json) and one line per run is appended to docs/compliance/history.tsv,
so the record of having been compliant accumulates in git.

  compliance.py --metadata M.json           every check
  compliance.py --metadata M.json --gate    only the checks that block a Cove
                                            release (onprem/run.sh); no network
  ... --out DIR                             report somewhere other than docs/compliance
                                            (run.sh: its state dir, so every build does
                                            not add a line to the history in git)
Exit 1 when a check fails. A check that cannot run says "skip" and why - it
never passes silently.

Educational engineering, not legal advice: these test the duties written in
docs/ideas-and-licences.md, they do not establish legal compliance.
"""
import datetime
import glob
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
COVE = os.path.dirname(HERE)
APP = os.path.join(COVE, "app")
OUTDIR = os.path.join(COVE, "docs", "compliance")
RSRC = os.path.join(COVE, ".rstudio-src")
NGINX = "/home/ubuntu/ndexr/nginx/sites-enabled"
KIT_INLET = "/home/ubuntu/ndexr/src/domains/ndexr.ndexr.io/crate/assets/stack-kit/easybuild-suite/easyconfigs/i/Inlet"
INLET_VER = "2026.04.0+526-foss-2024a-R-4.6.1"
TRIPLE = "x86_64-pc-linux-gnu"
INSTALL = f"/opt/ndexr/stack/{TRIPLE}/eb/software/Inlet/{INLET_VER}"
RECORDS = "https://ndexr-stack-public.s3.us-east-2.amazonaws.com/stack/records/Inlet/" + INLET_VER.replace("+", "%2B")
MARKS = re.compile(r"(rstudio|posit|positron|shiny)", re.I)

results = []


def check(id, tool, duty, gate=False):
    def wrap(fn):
        details = {}
        try:
            out = fn()
            status, evidence = out[0], out[1]
            details = out[2] if len(out) > 2 else {}
        except Exception as e:                          # a check that crashed is a failure, said so
            status, evidence = "fail", f"check raised {type(e).__name__}: {e}"
        # id/tool/duty/evidence for the report; check/product/summary/details are the
        # fields licenses.ndexr.io (the hub for outward-facing compliance) reads.
        product = {"Cove": "cove", "Inlet": "inlet", "Positron": "positron"}.get(tool, "platform")
        results.append({"id": id, "tool": tool, "duty": duty, "gate": gate, "status": status, "evidence": evidence,
                        "check": id, "product": product, "summary": evidence, "details": details})
        return fn
    return wrap


def fetch(url, limit=None):
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "cove-compliance"}), timeout=60) as r:
        return r.status, r.read(limit) if limit else r.read()


def sha(b):
    return hashlib.sha256(b).hexdigest()


def run(args):
    gate_only = "--gate" in args
    meta = args[args.index("--metadata") + 1] if "--metadata" in args else None

    # ---- Cove: its own licence -----------------------------------------------------------------
    @check("cove.licence", "Cove", "Cove is MIT, and says so where people see it", gate=True)
    def _():
        lic = open(os.path.join(COVE, "LICENSE")).read()
        # The About dialog (ui/app.rs since the 2026-11-25 reorganisation by system).
        about = open(os.path.join(APP, "src", "ui", "app.rs")).read()
        ok = lic.startswith("MIT License") and "Cove is MIT-licensed and contains no RStudio code" in about
        return ("pass" if ok else "fail"), f"LICENSE starts '{lic.splitlines()[0]}'; About states MIT: {'yes' if 'Cove is MIT-licensed' in about else 'no'}"

    # ---- Cove: what it is built from -----------------------------------------------------------
    notices = None
    if meta:
        p = subprocess.run([sys.executable, "-I", os.path.join(HERE, "third-party-notices.py"), meta, "--check"],
                           capture_output=True, text=True)
        notices = json.loads(p.stdout.strip().splitlines()[-1]) if p.stdout.strip() else {"problems": [p.stderr.strip()], "file_current": False}

    @check("cove.dependency-licences", "Cove", "Every crate linked into Cove is usable under a permissive licence (nothing copyleft reaches Cove's code)", gate=True)
    def _():
        if notices is None:
            return "skip", "no cargo metadata given (--metadata)"
        if notices["problems"]:
            return "fail", "; ".join(notices["problems"][:5])
        return "pass", f"{notices['allowed']} of {notices['crates']} crates resolve to an allowed licence"

    @check("cove.notices-current", "Cove", "THIRD-PARTY-NOTICES.txt matches the Cargo.lock being built", gate=True)
    def _():
        if notices is None:
            return "skip", "no cargo metadata given (--metadata)"
        return ("pass" if notices["file_current"] else "fail"), \
            f"dependencies sha256 {notices['dependencies_sha256'][:16]}; file {'current' if notices['file_current'] else 'STALE - run onprem/third-party-notices.py'}; " \
            f"{notices.get('from_own_files', '?')} crates from their own notice files, {notices.get('from_canonical_text', '?')} from canonical texts"

    @check("cove.notices-packaged", "Cove", "Every package Cove ships carries LICENSE and the notices", gate=True)
    def _():
        missing = [f for f in ("build-deb.sh", "build-dmg.sh", "build-linux.sh", "build-windows.sh")
                   if "THIRD-PARTY-NOTICES.txt" not in open(os.path.join(APP, f)).read()]
        return ("fail", "no notices in " + ", ".join(missing)) if missing else ("pass", "deb, dmg, linux and windows builds copy LICENSE + THIRD-PARTY-NOTICES.txt")

    # ---- Cove: not a derivative of RStudio or Inlet ------------------------------------------
    @check("cove.not-derivative", "Cove", "No line of RStudio's or Inlet's code, text or CSS is in Cove (read for behaviour only)", gate=True)
    def _():
        if not os.path.isdir(os.path.join(RSRC, "src")):
            return "skip", ".rstudio-src/ is not checked out (onprem/rstudio-src.sh)"
        norm = lambda s: re.sub(r"\s+", " ", s.strip())
        theirs = set()
        for f in glob.glob(os.path.join(RSRC, "src", "**", "*"), recursive=True):
            if os.path.isfile(f) and re.search(r"\.(java|cpp|hpp|h|c|R|js|ts|css|scss|html?|xml|properties|json|py|sh)$", f):
                for line in open(f, encoding="utf-8", errors="ignore"):
                    l = norm(line)
                    if len(l) >= 20:
                        theirs.add(l)
        for f in glob.glob(os.path.join(KIT_INLET, "*.patch")):        # Inlet's own added lines too
            for line in open(f, encoding="utf-8", errors="ignore"):
                if line.startswith("+") and not line.startswith("+++"):
                    l = norm(line[1:])
                    if len(l) >= 20:
                        theirs.add(l)
        allow_path = os.path.join(OUTDIR, "derivation-allow.txt")
        allow = {norm(l) for l in open(allow_path) if l.strip() and not l.startswith("#")} if os.path.exists(allow_path) else set()
        # What ships: the program, its browser UI and the starter templates it
        # writes into people's projects. onprem/ is ops tooling that never
        # reaches a copy of Cove - and this checker quotes RStudio's strings to
        # look for them, so scanning it would only find itself.
        tracked = subprocess.run(["git", "-C", COVE, "ls-files", "app/src", "app/web", "app/templates"],
                                 capture_output=True, text=True).stdout.split()
        ours, hits = 0, {}
        for rel in tracked:
            if not re.search(r"\.(rs|js|css|html?|r|R|py|sh|toml)$", rel):
                continue
            # A vendor/ directory is a third-party library shipped as-is (the
            # node-game starter's pixi.min.js, 2026-11-06): not Cove's lines,
            # and its generic CSS ("box-sizing: border-box;") matched RStudio's.
            if "/vendor/" in rel:
                continue
            # Tracked but deleted and not committed yet: nothing of it ships
            # (paper.rs, 2026-11-14, held the release with FileNotFoundError).
            if not os.path.exists(os.path.join(COVE, rel)):
                continue
            for n, line in enumerate(open(os.path.join(COVE, rel), encoding="utf-8", errors="ignore"), 1):
                l = norm(line)
                if len(l) < 20:
                    continue
                ours += 1
                if l in theirs and l not in allow:
                    hits.setdefault(l, f"{rel}:{n}")
        ev = f"{ours} Cove lines (>= 20 chars) against {len(theirs)} distinct RStudio + Inlet lines; {len(allow)} allow-listed generic lines"
        details = {"cove_lines": ours, "reference_lines": len(theirs), "allow_listed": len(allow),
                   "matches": [{"line": l, "where": w} for l, w in sorted(hits.items(), key=lambda x: x[1])]}
        if hits:
            return "fail", ev + f"; {len(hits)} NOT allow-listed, first: " + " | ".join(f"{w} `{l[:80]}`" for l, w in list(hits.items())[:3]), details
        return "pass", ev + "; no other match", details

    @check("platform.no-marks-in-hostnames", "Cove + Inlet", "No Posit mark (RStudio, Posit, Positron, Shiny) in any served ndexr hostname", gate=True)
    def _():
        bad = []
        for f in glob.glob(os.path.join(NGINX, "*")):
            for m in re.finditer(r"server_name\s+([^;]+);", open(f, errors="ignore").read()):
                bad += [h for h in m.group(1).split() if MARKS.search(h)]
        return ("fail", "served: " + ", ".join(sorted(set(bad)))) if bad else ("pass", f"{len(glob.glob(os.path.join(NGINX, '*')))} vhosts, none names a Posit mark")

    @check("positron.not-hosted", "Positron", "Positron (Elastic License 2.0) is not offered to others: no enabled vhost routes its sessions", gate=True)
    def _():
        users = [os.path.basename(f) for f in glob.glob(os.path.join(NGINX, "*")) if "positron_sessions.conf" in open(f, errors="ignore").read()]
        return ("fail", "positron_sessions.conf included by: " + ", ".join(users)) if users else ("pass", "no enabled vhost includes positron_sessions.conf")

    if gate_only:
        return

    # ---- Inlet: AGPLv3 duties ------------------------------------------------------------------
    @check("inlet.licence-text", "Inlet", "The installed Inlet carries the AGPLv3 text and RStudio's NOTICE")
    def _():
        if not os.path.isdir(INSTALL):
            return "skip", f"{INSTALL} not mounted here"
        c = open(os.path.join(INSTALL, "COPYING"), errors="ignore").read()
        ok = "GNU AFFERO GENERAL PUBLIC LICENSE" in c and os.path.getsize(os.path.join(INSTALL, "NOTICE")) > 0
        return ("pass" if ok else "fail"), f"COPYING is AGPLv3: {'yes' if 'AFFERO' in c else 'no'}; NOTICE {os.path.getsize(os.path.join(INSTALL, 'NOTICE'))} bytes"

    build = None
    if os.path.isdir(INSTALL):
        logs = glob.glob(os.path.join(INSTALL, "easybuild", "easybuild-Inlet-*.log"))
        m = re.search(r"-(\d{8}\.\d{6})\.log$", logs[0]) if logs else None
        build = m.group(1) if m else None

    @check("inlet.source-offer", "Inlet", "AGPL §13: every session links to the corresponding source of the running version, and the link answers")
    def _():
        src = open(os.path.join(KIT_INLET, "Inlet-0014-name-and-source-offer.patch"), errors="ignore").read()
        url = re.search(r'"(https://[^"]+/records/Inlet/[^"]+/index\.html)"', src)
        if not url:
            return "fail", "no records URL in patch 0014 (InletSource.java)"
        status, body = fetch(url.group(1))
        listed = build is not None and build.encode() in body
        ok = status == 200 and (listed or build is None)
        return ("pass" if ok else "fail"), f"{url.group(1)} -> HTTP {status}; installed build {build or '?'} listed: {'yes' if listed else 'no'}"

    @check("inlet.records-match-install", "Inlet", "The published patches and recipe are byte-for-byte the ones the running Inlet was built with")
    def _():
        if not build:
            return "skip", "no installed Inlet build record on this machine"
        files = sorted(f for f in os.listdir(os.path.join(INSTALL, "easybuild")) if f.endswith((".patch", ".eb")))
        bad = []
        for f in files:
            mine = sha(open(os.path.join(INSTALL, "easybuild", f), "rb").read())
            status, body = fetch(f"{RECORDS}/{TRIPLE}/{build}/{f.replace('+', '%2B')}")
            if status != 200 or sha(body) != mine:
                bad.append(f)
        return ("fail", f"differ or missing: {', '.join(bad)}") if bad else ("pass", f"{len(files)} files (17+ patches and the recipe) of build {build} match the public records")

    @check("inlet.upstream-source", "Inlet", "RStudio's release tarball is public, and its sha256 is the one the recipe pins")
    def _():
        eb = open(os.path.join(KIT_INLET, f"Inlet-{INLET_VER}.eb")).read()
        pinned = re.search(r"'v%\(version\)s\.tar\.gz': '([0-9a-f]{64})'", eb).group(1)
        status, body = fetch(f"https://ndexr-stack-public.s3.us-east-2.amazonaws.com/stack/sources/r/RStudio-Server/v2026.04.0%2B526.tar.gz")
        got = sha(body)
        return ("pass" if status == 200 and got == pinned else "fail"), f"public tarball HTTP {status}, sha256 {got[:16]} vs recipe {pinned[:16]}"

    @check("inlet.no-posit-marks-as-name", "Inlet", "The running Inlet does not present itself with RStudio's name (patches 0014-0018)")
    def _():
        if not os.path.isdir(INSTALL):
            return "skip", f"{INSTALL} not mounted here"
        hits = []
        for f in glob.glob(os.path.join(INSTALL, "www", "rstudio", "*.cache.js")):
            t = open(f, errors="ignore").read()
            hits += [p for p in ("About RStudio...", "Quit RStudio", "Restart RStudio", "RStudio Server Home") if p in t]
        return ("fail", "still in the compiled UI: " + ", ".join(sorted(set(hits)))) if hits else ("pass", "the compiled UI names itself Inlet (no 'About/Quit/Restart RStudio')")

    # ---- Cove served -----------------------------------------------------------------------
    @check("cove.notices-served", "Cove", "The running Cove serves its third-party notices publicly (/view/third-party-notices)")
    def _():
        try:
            status, body = fetch("http://127.0.0.1:8796/view/third-party-notices", 4096)   # the header, which names the Cargo.lock
        except Exception as e:
            return "fail", f"http://127.0.0.1:8796/view/third-party-notices: {e}"
        want = notices["dependencies_sha256"].encode() if notices else b"dependencies-sha256"
        return ("pass" if status == 200 and want in body else "fail"), f"HTTP {status}; carries this Cargo.lock's notices: {'yes' if want in body else 'no (release predates them)'}"


def report():
    global OUTDIR
    if "--out" in sys.argv:
        OUTDIR = sys.argv[sys.argv.index("--out") + 1]
    now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    commit = subprocess.run(["git", "-C", COVE, "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    counts = {s: sum(r["status"] == s for r in results) for s in ("pass", "fail", "skip")}
    os.makedirs(OUTDIR, exist_ok=True)
    data = {"at": now, "cove_commit": commit, "counts": counts, "checks": results}
    json.dump(data, open(os.path.join(OUTDIR, "latest.json"), "w"), indent=2)
    mark = {"pass": "PASS", "fail": "FAIL", "skip": "skip"}
    md = [f"# Licence compliance - {now}", "",
          f"Cove commit `{commit}`. {counts['pass']} pass, {counts['fail']} fail, {counts['skip']} skipped. "
          "Written by `onprem/compliance.py`; the duties are explained in `docs/ideas-and-licences.md`. "
          "Engineering evidence, not a legal opinion.", "",
          "| | Check | Tool | Duty | Evidence |", "|---|---|---|---|---|"]
    md += [f"| {mark[r['status']]} | `{r['id']}`{' (gate)' if r['gate'] else ''} | {r['tool']} | {r['duty']} | {r['evidence'].replace('|', '/')} |" for r in results]
    open(os.path.join(OUTDIR, "latest.md"), "w").write("\n".join(md) + "\n")
    hist = os.path.join(OUTDIR, "history.tsv")
    new = not os.path.exists(hist)
    with open(hist, "a") as h:
        if new:
            h.write("at\tcove_commit\tscope\tpass\tfail\tskip\tfailed\n")
        scope = "gate" if "--gate" in sys.argv else "full"
        h.write(f"{now}\t{commit}\t{scope}\t{counts['pass']}\t{counts['fail']}\t{counts['skip']}\t{','.join(r['id'] for r in results if r['status'] == 'fail')}\n")
    for r in results:
        print(f"{mark[r['status']]:4} {r['id']:<32} {r['evidence'][:150]}")
    return counts["fail"] == 0


if __name__ == "__main__":
    run(sys.argv)
    sys.exit(0 if report() else 1)
