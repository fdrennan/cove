#!/usr/bin/env bash
# Build: run notebook.ipynb from the top, as the server will, so a cell that
# fails stops here with its error rather than on the page.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

start=$SECONDS
python3 -m compileall -q app.py
python3 - <<'PY'
import nbformat
from nbclient import NotebookClient
nb = nbformat.read("notebook.ipynb", as_version=4)
NotebookClient(nb, kernel_name="python3", timeout=600, resources={"metadata": {"path": "."}}).execute()
print(f"build: {sum(c.cell_type == 'code' for c in nb.cells)} code cells ran")
PY
echo "build: done in $((SECONDS - start)) s ($(python3 --version))"
