# Your site · __HOST__
This person's own site, and the first thing they make in Cove. It is a React page
served by a small Node server. They are watching the App pane as you work: every
file saved in static/ shows there within a second, so the first change they see
should arrive quickly.

## How to work here
- Once they have said what this is, change this file's first line from "Your site"
  to its name, and rewrite the paragraph above. Until then Cove keeps opening here
  and asking what it should be.
- Whatever they ask for replaces the starter page. The dunes, the headline and the
  colour button are a starting point: remove them unless they fit.
- Make a visible change first, then refine it. Save often: each save is on screen.
- When they leave out details, choose sensible ones and write believable text
  instead of asking. Ask only when a wrong guess would waste their time.
- Finish a turn in a sentence or two, saying what changed and what they might ask
  for next.
- No npm, no bundler, no CDN. React 18 and htm are in static/vendor/. Other
  libraries go there too, as files.

## Files
- static/app.js - the page, as React components written with htm
- static/style.css - its styles
- static/index.html - loads the libraries, then app.js
- static/live.js - reloads the page when static/ changes. Keep it in index.html.
- server.js - Node standard library only. Serves static/, and the ROUTES table is
  where an API goes: one line per endpoint. Restarts itself when saved.
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1. Serve assets from static/ at the top level or under /static/.

## Logging
log.js (`const log = require('./log')`), one format across languages; LOG_LEVEL=DEBUG for more.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx
in front decides who reaches it.
