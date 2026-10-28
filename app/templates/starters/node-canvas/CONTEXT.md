# Project Canvas · __HOST__
A Node app, started from Cove's template: a drag-and-drop task board for a
research project, kept live across every open tab. Four columns (Backlog,
Running, Analysing, Written up), cards with a title, owner, tags, due date and
checklist; drag a card between columns or within one (mouse, or a long press
on a phone), filter by owner, tag or text, and a burndown and throughput chart
in SVG follows the filter. It opens on a sample board: a cell-culture lab four
weeks into a HIF-1α hypoxia experiment. Open the page in two browsers: a move
in one shows in the other within a frame, and survives a server restart.
Change first: `sampleBoard()` in server.js (your own columns and cards), or
edit the board in the page and keep it - it lives in data/board.json.

What it shows off: Node's event loop holds every open tab's Server-Sent Events
stream as one idle socket, and one change fans out to all of them from a single
callback; React's components, hooks and a reducer keep the board as
"confirmed by the server + operations still in flight", which is what makes
optimistic updates safe to undo.

## Files
- server.js - node:http, standard library only (no npm, no node_modules). The
  board in memory, `commit()` (apply, version, persist, broadcast: the one path
  every change takes), atomic writes to data/board.json (temp file, fsync,
  rename; coalesced while one is in flight), and the routes:
  GET /health; GET /api/board; PUT /api/board (replace, validated);
  POST /api/board/reset (the sample again); POST /api/cards
  (`{"title","owner","tags","due","checklist","column","before"}`);
  PATCH /api/cards/:id (any card fields, and/or `{"column","before"}` to move:
  `before` is the card it lands in front of, null for the end);
  DELETE /api/cards/:id; GET /api/events (SSE: `hello` with the board,
  `change` with the operation, who sent it and the new board, `presence` with
  the number of open streams); GET /api/env (runtime, stack module, imports,
  vendored libraries with live sha256, start time, CPUs)
- static/board-ops.js - the board's data structure and its four operations
  (create, update, move, delete), run by both the server and the page, so an
  optimistic change and the confirmed one cannot disagree
- static/app.js - React 18 with htm (JSX-like templates, no build): the
  reducer, the live stream, drag and drop (HTML5 and touch), filters, the
  chart, the card editor and the Environment panel
- static/index.html, static/style.css - the page; dark glass panels, one
  swiped column per screen on a phone
- static/vendor/ - React, ReactDOM, htm; laid down from the shared Node folder
  with their checksums (its README.md)
- run.sh - loads Node from the ndexr stack and runs server.js
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - `node --check` every script and verify
  the vendored checksums; start in the background and wait for /health (pid
  and log in .run/); stop (the server finishes its last write on TERM); load
  nodejs from the ndexr stack
- data/board.json - the live board (gitignored); delete it to start again
  from the sample

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
Node.js comes from the stack (nodejs/22.22.2); the browser libraries are the
vendored files; nothing is installed and nothing is fetched at run time.
Watch the live stream from a shell: `curl -N 127.0.0.1:$APP_PORT/api/events`.

## Built on
- Little, J. D. C. (1961), "A Proof for the Queuing Formula L = λW",
  Operations Research 9(3):383-387 - work in progress = throughput × time in
  the system; the chart's two panels are L and λ.
- Anderson, D. J. (2010), "Kanban: Successful Evolutionary Change for Your
  Technology Business", Blue Hole Press - the pull board with visible columns.
- WHATWG HTML Living Standard, §9.2 "Server-sent events" - the EventSource
  protocol `/api/events` speaks, with reconnection built into the browser.

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it. Every reader can edit: there are no per-card permissions.

## Open
- (none logged)
