# Sourced by run.sh and build.sh: app.env, and Python from the ndexr stack in
# this one shell (a module loaded in one shell is gone in the next). Packages
# come from the stack's Python set; nothing here installs any.
. ./app.env

if [ -r /opt/ndexr/stack/use.sh ]; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  ml env/Python/3.13.15 >/dev/null 2>&1 || ml env/r4.6_py3.13-release >/dev/null 2>&1 || true
  set -u
fi
python3 -c 'import fastapi, uvicorn' 2>/dev/null || {
  echo "no fastapi/uvicorn: run where /opt/ndexr/stack is mounted (env/Python/3.13.15 or env/r4.6_py3.13-release)" >&2
  exit 127
}
export PORT="${PORT:-$APP_PORT}"
