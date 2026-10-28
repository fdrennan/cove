# __HOST__ · __HOST__
A Rust app, started from Cove's template (a minimal Cove: egui UI in the
browser and on the desktop, a small Rust server). It opens on Chaos Lab - the
Lorenz system solved in Rust every frame, two runs a hair apart parting before
your eyes - then Dependency Universe, an atlas of the ndexr stack read live
from the stack's catalog, and a media tool over the stack's FFmpeg. Both are starting points: describe what this
site is for here, and make the first screen yours.

## Files
- Cargo.toml, src/ - the app. One binary: `app serve` (the site), `app` (desktop window)
- src/api.rs - what the UI can ask; add a question here first
- src/media.rs - the media work (FFmpeg from the stack): Clean audio, Cut, Extract audio
- src/server.rs - HTTP on 127.0.0.1: /api/q, /api/upload, /files/, the UI
- src/app.rs - the UI: the tabs, and the Media tab. Chaos Lab and the Dependency Universe fill the window; the tabs float as a pill and each visual's controls as a glass card that folds (`float_card`); H hides them all, Space pauses Chaos Lab
- src/chaos.rs - Chaos Lab, the first tab: Lorenz equations, RK4, sliders, the gap plot, "Built with Cove"
- src/atlas.rs - Dependency Universe: every package of the ndexr stack as a star, by layer
- src/catalog.rs - reads the stack's catalog for the atlas ($NDEXR_CATALOG, /srv/ndexr/software/catalog.json, or cove.ndexr.io/view/software.json)
- web/index.html, build-web.sh - the browser build into dist/web
- run.sh - build and serve on 127.0.0.1:$PORT (default 8800)
- media/ (your files), out/ (results) - on disk, not in git

## Auth
The server listens on 127.0.0.1 only; nginx in front decides who reaches it.

## Serving it
There is no module.r: like cove.ndexr.io, this domain is served by its own
process. exec runs `bash scripts/new-rust-app.sh __HOST__` from the platform
repo: it builds, keeps `app serve <port>` running (user unit
`app-__HOST__.service`), and points the domain's `location /` at it behind the
owner gate. Rerun it to put edits live.

## Open
- (none logged)
