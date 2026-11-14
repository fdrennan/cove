# Blank Plumber · __HOST__
A blank Plumber API started in Cove: annotated R functions as routes, docs at /__docs__/. Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank Plumber" to its name and rewrite this paragraph.

## Files
- plumber.R - the routes: `#* @get /path` above a function makes it one
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
Plumber does not reload itself; run.sh restarts it whenever an .R file changes.
Packages come from the stack: env/r4.6_py3.13-release (`ml env/r4.6_py3.13-release`); APP_NEEDS in app.env names them, and env.sh refuses to start without them.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
