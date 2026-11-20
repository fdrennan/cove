# Sourced by run.sh and build.sh: app.env, and Python from the ndexr stack in
# this one shell (a module loaded in one shell is gone in the next). Packages
# come from the stack's Python set, built on the n-boxes; see APP_NEEDS below.
. ./app.env
# Lines reach .cove/run/app.log as they are printed, not when a buffer fills.
export PYTHONUNBUFFERED=1

# The app runs in its stack environment or not at all - never on whatever the
# host happens to have (operator, 2026-11-19: "the application must absolutely
# run with its proper ml environment"). stack_ml loads the first of its
# modules that loads and says what is loaded, in the log's own format.
stack_here() {
  [ -r /opt/ndexr/stack/use.sh ] || { echo "env: the ndexr stack is not mounted at /opt/ndexr/stack - set it up in Cove (Stack in the status bar)" >&2; exit 127; }
}
stack_ml() {
  for m in "$@"; do
    if ml "$m" >/dev/null 2>&1; then
      printf '%s INFO    [env] %s from the ndexr stack\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$(ml -t list 2>&1 | grep "^${m%%/*}/" | tail -1)"
      return 0
    fi
  done
  echo "env: none of $* loads from the ndexr stack - try: ml spider ${1%%/*}" >&2
  exit 127
}

if stack_here; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  stack_ml env/r4.6_py3.13-release env/Python/3.13.15
  set -u
fi
command -v python3 >/dev/null || { echo "no python3: run where /opt/ndexr/stack is mounted (env/Python/3.13.15)" >&2; exit 127; }

# Every package APP_NEEDS names as python:<name> (its PyPI name). A box holds
# only part of the set in its site library (the records marked SiteLibrary);
# what it lacks comes from ndexr's own repository for the set - the wheels the
# n-boxes built, pinned to the set's current.json - into a per-user cache for
# the set. Never PyPI or another mirror. A project made before APP_NEEDS was
# FastAPI's.
cove_triple="$(printf '%s' "${NDEXR_ENV_SITE_LIBRARY:-}" | sed -n 's|.*/stack/\([^/]*\)/eb/.*|\1|p')"
cove_set="Python/$(python3 -c 'import sys; print(sys.version.split()[0])')"
export PYTHONUSERBASE="${XDG_CACHE_HOME:-$HOME/.cache}/ndexr/env/${cove_triple:-$(uname -m)}/$cove_set"
python3 - "${NDEXR_PKGS_URL:-https://repo.ndexr.io/env}/$cove_triple/$cove_set" "$cove_triple" ${APP_NEEDS:-python:fastapi python:uvicorn} <<'PY' || exit 127
import importlib, importlib.metadata as m, json, os, subprocess, sys, urllib.request

repo, triple, words = sys.argv[1], sys.argv[2], sys.argv[3:]
want = [w[7:] for w in words if w.startswith("python:")]

def missing():
    importlib.invalidate_caches()
    gone = []
    for name in want:
        try:
            m.distribution(name)
        except m.PackageNotFoundError:
            gone.append(name)
    return gone

gone = missing()
if gone and triple:
    print(f"env: {' '.join(gone)} not on this box; fetching from {repo}", file=sys.stderr)
    try:
        with urllib.request.urlopen(f"{repo}/current.json", timeout=60) as r:
            current = json.load(r)["Python"]
        base = os.environ["PYTHONUSERBASE"]
        os.makedirs(base, exist_ok=True)
        pins = os.path.join(base, "constraints.txt")
        with open(pins, "w") as f:
            f.writelines(f"{p['name']}=={p['version']}\n" for p in current.values())
        subprocess.run([sys.executable, "-m", "pip", "install", "--user", "--quiet", "--disable-pip-version-check",
                        "--no-warn-script-location", "--only-binary", ":all:", "--index-url", f"{repo}/python/simple",
                        "--constraint", pins, *gone], stdout=sys.stderr, check=False)
    except Exception as e:  # no network, no repo: the message below says what is missing
        print(f"env: could not fetch from {repo}: {e}", file=sys.stderr)
    import site
    site.addsitedir(site.getusersitepackages())
    gone = missing()
if gone:
    sys.exit(f"not in the stack's Python set ({os.path.basename(repo)}): {', '.join(gone)} - ndexr builds none of it for this box")
PY
# The notebook's kernel: this Python, named python3. The stack's Python set
# registers none that Jupyter's own listing (and so Quarto) can see (#754 to
# ndexr.ndexr.io), so the project carries its own in .run/ (written each
# start, never saved).
export JUPYTER_PATH="$PWD/.run/jupyter"
mkdir -p "$JUPYTER_PATH/kernels/python3"
printf '{"argv": ["%s", "-m", "ipykernel_launcher", "-f", "{connection_file}"], "display_name": "Python 3", "language": "python"}\n' \
  "$(command -v python3)" >"$JUPYTER_PATH/kernels/python3/kernel.json"
export PORT="${PORT:-$APP_PORT}"
