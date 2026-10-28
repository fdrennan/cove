# Orbital · __HOST__
A Node app with a React page, started from Cove's template: our own Solar
System, placed from JPL's Keplerian orbital elements. The eight planets, Pluto,
Ceres, Vesta and the comets Halley and Encke move on a full-screen canvas that
you rotate (drag), zoom (wheel or pinch) and pan (shift-drag or two fingers);
Kepler's equation is solved for every body on every frame. A date scrubber runs
1800 to 2100, the speed slider 0.1 to 10 000 days a second either way; tapping a
body opens its card (period, a/e/i, distance from the Sun and from Earth, light
time, heliocentric x, y, z and longitude). "Line up" asks the server for the
next dates two bodies share a heliocentric longitude - Mars's oppositions,
Venus's inferior conjunctions, a Jupiter-Saturn great conjunction - and shows
their angle apart on Earth's sky on the shown date. The orbital maths is one
file, static/orbits.js, loaded by the server with require() and by the browser
with a script tag, so the API and the animation cannot disagree; the header
shows the measured gap between them. A starting point: describe what this site
is for here, and make the first screen yours.

Node is here for one language on both sides of the wire: the same module runs
in V8 on the server and in the browser, the server is the standard library
alone (no npm, no node_modules), and React's components and hooks keep a
60 frames-a-second canvas apart from panels that re-render 8 times a second.

## Files
- server.js - node:http, standard library only: GET / and static/, GET /health,
  GET /api/env (runtime, stack module, packages, start time, CPUs - read from
  the process), GET /api/bodies (elements, periods, sources), GET
  /api/positions?date= (every body's heliocentric ecliptic x, y, z, r,
  longitude, latitude, anomalies, distance and light time from Earth; date as
  `2026-10-26`, `2026-10-26T18:00` or a Julian date, default now), GET
  /api/lineup?a=earth&b=mars&from=&count= (next equal-longitude dates, synodic
  period, mean gap), GET /api/separation?a=&b=&date= (angle apart seen from
  Earth and from the Sun)
- static/orbits.js - the elements and the maths: Kepler's equation by Newton,
  the rotation to the ecliptic, orbit rings, the lineup search
- static/index.html, static/app.js, static/style.css - the page: React
  components (Stage, Title, Info, Controls, Lineup, Env) written with htm, no
  compile step; glass panels that fold on a phone
- check.js - `node check.js`: the known values below, exit 1 on a miss
- run.sh - loads Node from the ndexr stack and serves
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - syntax-check every script and verify the
  vendored libraries; start in the background and wait for /health (pid and
  log in .run/); stop; load Node (ml nodejs) from the ndexr stack
- static/vendor/ - React 18.3.1, ReactDOM, htm 3.1.1, checksummed (its README)

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background. Node comes from the stack
(nodejs/22.22.2); nothing is installed. Measured 2026-10-26 on a loaded
24-core box: /api/positions for all 13 bodies in 5 ms round trip; the next
four Mars oppositions in under 1 ms of search (14 ms round trip); the slowest
pair, Neptune-Pluto, scans 400 years in 80 ms. The page's own positions agree
with the server's to 1e-10 AU, rounding only. Checked against known values (check.js): Earth at
J2000.0 at x -0.1772, y 0.9672 AU, r 0.9833 AU (Horizons: -0.1771, 0.9672);
Earth's distance through 2000 between 0.9833 and 1.0167 AU; Mars's twelve
oppositions 2003-2027 each on its published date, mean gap 779.6 days against a
synodic period of 779.94.

Planet positions are good to arcminutes (inner planets) and under a degree
(outer) from 1800 to 2050, the span Table 1 was fitted over; outside it the
date shows amber. Ceres, Vesta and the comets move two-body on fixed elements
with no planetary perturbations: Halley's next perihelion lands in 2061 within
months, not days.

## Built on
Standish, E. M. and Williams, J. G., "Keplerian Elements for Approximate
Positions of the Major Planets", JPL Solar System Dynamics
(ssd.jpl.nasa.gov/planets/approx_pos.html), Table 1, fitted to the DE405
ephemeris. Kepler, J. (1609), Astronomia Nova. Meeus, J. (1998), Astronomical
Algorithms, 2nd ed., Willmann-Bell, chapters 30 (Kepler's equation) and 33.
Small-body elements: JPL Small-Body Database, rounded.

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
