# Blank Quarto (Python) · __HOST__
A blank Quarto document with Python chunks, started in Cove. Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank Quarto (Python)" to its name and rewrite this paragraph.

## Files
- index.qmd - the document
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
`quarto preview` renders it again on every save and reloads the open page. The notebook kernel is this Python, written by env.sh into .run/jupyter (the stack registers none Quarto can see).
Packages come from the stack: env/r4.6_py3.13-release, which also loads Quarto; APP_NEEDS in app.env names them.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
