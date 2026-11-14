# Blank · __HOST__
A blank Jupyter notebook started in Cove, published as a page, as Posit
Connect publishes one: notebook.ipynb runs from the top and the page shows
its text, code and results. Whatever this project is told to become replaces
it. Once it has a purpose, change this file's first line from "Blank" to its
name and rewrite this paragraph.

## Files
- notebook.ipynb - the notebook; edit it in Cove or any Jupyter
- app.py - runs the notebook whenever it changes (nbclient, in memory: the
  file on disk is never written) and draws it as HTML (markdown through
  mistune; text, HTML, PNG and SVG results; errors)
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
Each save of notebook.ipynb runs it again, and the open page reloads when the
run is done. A notebook that fails keeps the last good page and shows the
error above it. `./build.sh` runs it once, so a failing cell stops the build.

## The kernel
The notebook runs on the stack's Python (env/r4.6_py3.13-release), as the
kernel named `python3`. env.sh writes that kernel's spec into .run/jupyter
and points JUPYTER_PATH at it: the stack's Python set registers none that
Jupyter's own listing can see.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
