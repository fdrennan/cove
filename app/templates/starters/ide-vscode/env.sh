# Sourced by run.sh, build.sh and recipe.sh: app.env, and VS Code (code-server)
# from the ndexr stack, in this one shell. Your own build (rebuild.sh) comes
# first when there is one.
. ./app.env

if [ -r /opt/ndexr/stack/use.sh ]; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  mine="${COVE_EB_PREFIX:-$HOME/.local/easybuild/$(basename "$(pwd -P)")}/modules/all"
  [ -d "$mine" ] && module use "$mine"
  ml code-server code-server-extensions >/dev/null 2>&1 || ml code-server >/dev/null 2>&1 || true
  # The stack's R, with its site library, for the R extension and the
  # terminal (request #193, 2026-11-07): without it the extension found
  # whatever R was first on PATH, with no {languageserver}, and offered to
  # install it from CRAN. languageserver is kept on every box (SiteLibrary).
  ml env/R/4.6.1-bioc3.23 >/dev/null 2>&1 || ml R/4.6.1-foss-2024a >/dev/null 2>&1 || true
  set -u
fi
command -v code-server >/dev/null || { echo "no code-server: run where /opt/ndexr/stack is mounted (ml code-server)" >&2; exit 127; }
export PORT="${PORT:-$APP_PORT}"
