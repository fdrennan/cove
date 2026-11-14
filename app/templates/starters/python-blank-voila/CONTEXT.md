# Blank · __HOST__
A blank Voilà app started in Cove: app.ipynb run afresh for each visitor, its
code hidden and its widgets (ipywidgets) live, as Posit Connect serves a
Voilà notebook. Whatever this project is told to become replaces it. Once it
has a purpose, change this file's first line from "Blank" to its name and
rewrite this paragraph.

## Files
- app.ipynb - the notebook Voilà runs; edit it in Cove or any Jupyter
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
Voilà runs the notebook for each visit: save, then reload the page.
`./build.sh` runs it once, so a failing cell stops the build.

## State (2026-11-07)
Voilà does not start on the stack's Python set yet: its install dropped the
packages' shared data (share/jupyter), so Voilà finds no `lab` template.
Referred to ndexr.ndexr.io. The kernel spec env.sh writes into .run/jupyter
is for the same kind of gap: the set registers no `python3` kernel.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
