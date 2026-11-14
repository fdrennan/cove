#!/usr/bin/env python3
"""Cove's third-party notices, from Cargo.lock.

MIT, BSD, Apache, Zlib, ISC, Unicode, OFL and the Ubuntu font licence all ask
that their notices travel with a binary built from the code. Cove ships
binaries (the server, the browser UI, .deb/.dmg/Windows/Linux desktops), so
this file is how that duty is met - and the check that it still is.

  third-party-notices.py METADATA.json            write app/THIRD-PARTY-NOTICES.txt
  third-party-notices.py METADATA.json --check    exit 1 if a crate has no allowed
                                                  licence or the file is stale
  ... --crate DIR --name NAME                     another crate (the ndexr CLI): its
                                                  Cargo.lock in, DIR/THIRD-PARTY-NOTICES.txt out

METADATA.json is `cargo metadata --format-version 1 --locked --offline` for
app/ (onprem/run.sh and onprem/compliance.sh produce it). Only crates linked
or built into Cove count (normal and build dependencies, every target);
dev-dependencies never ship.
"""
import glob
import hashlib
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TEXTS = os.path.join(HERE, "licence-texts")


def arg(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


# One generator for every Rust product ndexr ships (licenses.ndexr.io asks for
# exactly that): Cove's app/ by default, any crate with --crate (the ndexr CLI:
# ndexr.ndexr.io/crate/tools/third-party-notices.sh).
APP = os.path.abspath(arg("--crate", os.path.join(os.path.dirname(HERE), "app")))
NAME = arg("--name", "Cove")
OUT = os.path.join(APP, "THIRD-PARTY-NOTICES.txt")

# Allowed licences, most preferred first: where a crate offers a choice, Cove
# takes the first of these it offers. No copyleft option is ever chosen, so
# nothing here can reach Cove's own code.
PREFER = ["MIT", "Apache-2.0", "BSD-3-Clause", "BSD-2-Clause", "ISC", "Zlib", "0BSD", "Unlicense",
          "BSL-1.0", "Unicode-3.0", "Unicode-DFS-2016", "CDLA-Permissive-2.0", "OFL-1.1", "Ubuntu-font-1.0",
          "Apache-2.0 WITH LLVM-exception", "CC0-1.0",
          # File-level copyleft, last resort: allowed in any binary, with the duty
          # (MPL-2.0 s3.2(a)) to tell recipients where the covered files' source
          # is - written into the notices for each such crate (render()).
          "MPL-2.0"]
NOTICE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE|OFL|UFL)", re.I)


def tokens(expr):
    expr = expr.replace("/", " OR ")
    expr = re.sub(r"\bWITH\s+(\S+)", r"WITH_\1", expr)       # keep "X WITH exception" one token
    return re.findall(r"\(|\)|[^\s()]+", expr)


def choose(expr):
    """The licences Cove relies on for an SPDX expression, or None if no
    allowed combination exists. OR takes the most preferred allowed side; AND
    needs every side."""
    toks = tokens(expr)
    pos = [0]

    def atom():
        t = toks[pos[0]]; pos[0] += 1
        if t == "(":
            v = disj(); pos[0] += 1          # ")"
            return v
        name = t.replace("WITH_", "WITH ") if "WITH_" in t else t
        name = re.sub(r"(\S)WITH ", r"\1 WITH ", name)
        return [name] if name in PREFER else None

    def conj():
        v = atom()
        while pos[0] < len(toks) and toks[pos[0]] == "AND":
            pos[0] += 1
            w = atom()
            v = v + w if v is not None and w is not None else None
        return v

    def disj():
        opts = [conj()]
        while pos[0] < len(toks) and toks[pos[0]] == "OR":
            pos[0] += 1
            opts.append(conj())
        ok = [o for o in opts if o is not None]
        return min(ok, key=lambda o: max(PREFER.index(x) for x in o)) if ok else None

    try:
        return disj()
    except (IndexError, ValueError):
        return None


def deps_sha256(lock_text):
    """sha256 of the third-party packages in a Cargo.lock: one line
    "name version source" per [[package]] with a `source`, sorted. A workspace
    crate has no source, so its own version bump (every ndexr release) does
    not make the notices stale; any change to a dependency does. ndexr's
    dist::deps_sha256 computes the same."""
    lines = []
    for block in lock_text.split("[[package]]")[1:]:
        f = dict(re.findall(r'^(name|version|source) = "([^"]*)"', block, re.M))
        if "source" in f:
            lines.append(f"{f['name']} {f['version']} {f['source']}")
    return hashlib.sha256("\n".join(sorted(lines)).encode()).hexdigest()


def shipped(meta):
    pk = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]
    seen, stack = set(), [root]
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k["kind"] in (None, "build") for k in d["dep_kinds"]):
                stack.append(d["pkg"])
    return sorted((pk[i] for i in seen if i != root), key=lambda p: (p["name"], p["version"]))


def notice_files(p):
    d = os.path.dirname(p["manifest_path"])
    files = [f for f in glob.glob(os.path.join(d, "*")) + glob.glob(os.path.join(d, "*", "*"))
             if os.path.isfile(f) and NOTICE_FILE.match(os.path.basename(f))]
    files += glob.glob(os.path.join(d, "fonts", "*.txt"))      # epaint's font licences
    if p.get("license_file"):
        files.append(os.path.join(d, p["license_file"]))
    return sorted({os.path.realpath(f) for f in files})


def fallback(p, lic):
    """A crate that publishes no licence file: the canonical text of the
    licence relied on, under a copyright line from its own metadata."""
    who = ", ".join(a.split(" <")[0] for a in p.get("authors") or []) or f"the {p['name']} authors"
    where = p.get("repository") or p.get("homepage") or ""
    line = f"Copyright (c) {who}" + (f" ({where})" if where else "")
    path = os.path.join(TEXTS, lic.split(" WITH ")[0] + ".txt")
    if not os.path.exists(path):
        return None
    body = open(path, encoding="utf-8").read()
    head = {"MIT": "MIT License", "Zlib": "zlib License"}.get(lic, "")
    return "\n\n".join(x for x in [head, line if lic in ("MIT", "Zlib", "BSL-1.0", "BSD-3-Clause") else f"{p['name']}: {line}", body] if x)


def build(meta, lock_sha):
    crates, problems, texts = [], [], {}
    for p in shipped(meta):
        expr = p.get("license") or ""
        chosen = choose(expr) if expr else None
        if not chosen:
            problems.append(f"{p['name']} {p['version']}: licence '{expr or p.get('license_file')}' has no allowed option")
            continue
        files = notice_files(p)
        bodies = [open(f, encoding="utf-8", errors="replace").read().strip() for f in files]
        if not bodies:
            made = [fallback(p, lic) for lic in chosen]
            if None in made:
                problems.append(f"{p['name']} {p['version']}: no licence file and no canonical text for {chosen}")
                continue
            bodies = made
        for b in bodies:
            key = hashlib.sha256(b.encode()).hexdigest()
            texts.setdefault(key, {"text": b, "crates": []})["crates"].append(f"{p['name']} {p['version']}")
        crates.append({"name": p["name"], "version": p["version"], "expression": expr, "chosen": " AND ".join(chosen),
                       "repository": p.get("repository") or "", "own_files": bool(files)})
    return crates, problems, texts


def render(crates, texts, lock_sha):
    title = f"Third-party notices for {NAME}"
    out = [
        title,
        "=" * len(title),
        "",
        f"{NAME} is built from the open-source Rust crates below. Each is used under",
        "the licence named in the 'relied on' column - where a crate offers a choice,",
        "the most permissive one it offers. Their copyright and licence notices",
        "follow, each once, with the crates it covers.",
        "",
        f"dependencies-sha256: {lock_sha}",
        f"crates: {len(crates)}",
        "",
        f"{'crate':<38} {'version':<16} relied on",
        f"{'-' * 38} {'-' * 16} {'-' * 30}",
    ]
    out += [f"{c['name']:<38} {c['version']:<16} {c['chosen']}" for c in crates]
    mpl = [c for c in crates if "MPL-2.0" in c["chosen"]]
    if mpl:
        out += ["", "Source Code Form of the MPL-2.0 files (MPL-2.0 section 3.2(a)): each crate's",
                "complete source, as published on crates.io, unmodified:", ""]
        out += [f"  {c['name']} {c['version']}: https://crates.io/crates/{c['name']}/{c['version']}"
                + (f"  ({c['repository']})" if c["repository"] else "") for c in mpl]
    for i, t in enumerate(sorted(texts.values(), key=lambda t: (-len(t["crates"]), t["crates"][0])), 1):
        out += ["", "", "-" * 78, f"Notice {i} - applies to: " + ", ".join(t["crates"]), "-" * 78, "", t["text"]]
    return "\n".join(out) + "\n"


def main():
    meta = json.load(open(sys.argv[1]))
    check = "--check" in sys.argv
    lock = os.path.join(APP, "Cargo.lock")
    lock_sha = deps_sha256(open(lock, encoding="utf-8").read())
    crates, problems, texts = build(meta, lock_sha)
    summary = {"crates": len(crates) + len(problems), "allowed": len(crates), "problems": problems,
               "from_own_files": sum(c["own_files"] for c in crates),
               "from_canonical_text": sum(not c["own_files"] for c in crates),
               "notice_texts": len(texts), "dependencies_sha256": lock_sha}
    if check:
        have = open(OUT, encoding="utf-8").read() if os.path.exists(OUT) else ""
        summary["file_current"] = f"dependencies-sha256: {lock_sha}" in have
        print(json.dumps(summary))
        sys.exit(0 if summary["file_current"] and not problems else 1)
    if problems:
        print("\n".join(problems), file=sys.stderr)
        sys.exit(1)
    open(OUT, "w", encoding="utf-8").write(render(crates, texts, lock_sha))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
