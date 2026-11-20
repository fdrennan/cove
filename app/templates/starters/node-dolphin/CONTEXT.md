# Dolphin Jump · __HOST__
A Node app, started from Cove's template: a side-scrolling ocean game. Jump the
dolphin out of the waves for golden stars, dodge the pink jellyfish; stars in a
row build a combo worth more each, three jellyfish end the run, and the world
speeds up the further you swim. Space, ↑ or a tap jumps, so it plays the same
on a phone. A finished run can go on the site's score board, which every player
of this site shares and which survives a restart.
Change first: the constants at the top of static/app.js (GRAVITY, JUMP, the
speed ramp in `step`), or what spawns in `step` - a third kind of object is a
new `kind` there and a drawing function beside `star` and `jelly`.

What it shows off: React drawing around a canvas that changes sixty times a
second without re-rendering on every frame - the game lives in a ref, React
renders the frame around it from a snapshot ten times a second - and a Node
server that keeps shared state on disk with the standard library alone.

## Files
- server.js - node:http, standard library only (no npm, no node_modules). The
  score board in memory and in data/scores.json (temp file, fsync, rename), and
  the routes: GET /health; GET /api/scores (ten best); POST /api/scores
  (`{"name","score","stars","seconds"}`, refused past 100 000) → rank;
  GET /api/env (runtime, stack module, vendored libraries with live sha256)
- static/app.js - React 18 with htm (JSX-like templates, no build): `step`
  (the physics), the drawing, the page, the score form and the Environment panel
- static/index.html, static/style.css - the page; one column on a phone
- static/vendor/ - React, ReactDOM and htm, checked against README.md's sha256
  by build.sh. Served from /static/: nginx claims /vendor/ on every site
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - the app contract
  (APP-CONTRACT.md); Node from the stack (`ml nodejs`)

## Logging
log.js (`const log = require('./log')`), one format across languages; LOG_LEVEL=DEBUG for more.

## Auth
As published at route.ndexr.io.

## Open
- (none logged)
