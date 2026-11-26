# Sourced by run.sh and build.sh: app.env, and Python from the ndexr stack in
# this one shell (a module loaded in one shell is gone in the next). Packages
# come from the stack's Python set, built on the n-boxes; see APP_NEEDS below.
. ./app.env
# Lines reach .cove/run/app.log as they are printed, not when a buffer fills.
export PYTHONUNBUFFERED=1

# The stack this project's modules come from - the ndexr stack, EESSI or
# Compute Canada's - and stack_here, stack_enter and stack_ml (.cove/stack.sh).
. ./stack.sh

if stack_here; then
  set +u
  stack_enter
  stack_ml env/r4.6_py3.13-release env/Python/3.13.15
  set -u
fi
command -v python3 >/dev/null || { echo "no python3: run where $COVE_STACK_LABEL is mounted (env/Python/3.13.15)" >&2; exit 127; }

# Every package APP_NEEDS names as python:<name> (its PyPI name). A box holds
# only part of the set in its site library (the records marked SiteLibrary);
# what it lacks comes from ndexr's own repository for the set - the wheels the
# n-boxes built, pinned to the set's current.json - into a per-user cache for
# the set. A declared package ndexr has not built comes from PyPI: ndexr's
# index sends pip there for any project it lacks (repo.ndexr.io), and pip
# builds it here if PyPI has no wheel (operator, 2026-11-20: "declared
# packages not available are always pulled or built but nothing more"). A
# project made before APP_NEEDS was FastAPI's.
cove_triple="$(printf '%s' "${NDEXR_ENV_SITE_LIBRARY:-}" | sed -n 's|.*/stack/\([^/]*\)/eb/.*|\1|p')"
cove_set="Python/$(python3 -c 'import sys; print(sys.version.split()[0])')"
export PYTHONUSERBASE="${XDG_CACHE_HOME:-$HOME/.cache}/ndexr/env/${cove_triple:-${COVE_STACK_NAME:-ndexr}-$(uname -m)}/$cove_set"
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
                        "--no-warn-script-location", "--prefer-binary", "--index-url", f"{repo}/python/simple",
                        "--constraint", pins, *gone], stdout=sys.stderr, check=False)
    except Exception as e:  # no network, no repo: the message below says what is missing
        print(f"env: could not fetch from {repo}: {e}", file=sys.stderr)
    import site
    site.addsitedir(site.getusersitepackages())
    gone = missing()
elif gone:
    # Another stack (EESSI, Compute Canada's) or one with no ndexr set: PyPI,
    # into the person's own library for this stack and Python.
    print(f"env: {' '.join(gone)} not in this stack; fetching from PyPI", file=sys.stderr)
    subprocess.run([sys.executable, "-m", "pip", "install", "--user", "--quiet", "--disable-pip-version-check",
                    "--no-warn-script-location", "--prefer-binary", *gone], stdout=sys.stderr, check=False)
    import site
    site.addsitedir(site.getusersitepackages())
    gone = missing()
if gone:
    sys.exit(f"could not install: {', '.join(gone)} - neither ndexr's Python set ({os.path.basename(repo)}) nor PyPI had it for this Python")
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
