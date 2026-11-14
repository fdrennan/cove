# Blank Quarto (R) · __HOST__
A blank Quarto document with R chunks (knitr), started in Cove. Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank Quarto (R)" to its name and rewrite this paragraph.

## Files
- index.qmd - the document
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
`quarto preview` renders it again on every save and reloads the open page.
Packages come from the stack: env/r4.6_py3.13-release (`ml env/r4.6_py3.13-release`); APP_NEEDS in app.env names them, and env.sh refuses to start without them.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
