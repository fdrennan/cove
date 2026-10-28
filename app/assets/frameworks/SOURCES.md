# Framework icons

The real marks of the languages and frameworks Cove's projects are built with,
shown beside a project so a person sees what kind of app is open (operator,
2026-10-28). 96 x 96 PNG, transparent, rasterized from each project's own SVG
with librsvg (`ml librsvg`, `rsvg-convert -w 192`) and ImageMagick (resize to
96, centred). Used to name the framework (nominative use); each mark belongs
to its project.

| file | mark | source | licence of the file |
|---|---|---|---|
| r.png | R | devicon v2.16.0 `icons/r/r-original.svg` | MIT (devicon) |
| shiny.png | Shiny (Posit) | github.com/rstudio/hex-stickers `PNG/shiny.png` | CC0 1.0 |
| python.png | Python | devicon v2.16.0 `icons/python/python-original.svg` | MIT (devicon) |
| fastapi.png | FastAPI | devicon v2.16.0 `icons/fastapi/fastapi-original.svg` | MIT (devicon) |
| flask.png | Flask | devicon v2.16.0 `icons/flask/flask-original.svg`, black inverted to white | MIT (devicon) |
| streamlit.png | Streamlit | devicon v2.16.0 `icons/streamlit/streamlit-original.svg` | MIT (devicon) |
| react.png | React | devicon v2.16.0 `icons/react/react-original.svg` | MIT (devicon) |
| nodejs.png | Node.js | devicon v2.16.0 `icons/nodejs/nodejs-original.svg` | MIT (devicon) |
| rust.png | Rust | devicon v2.16.0 `icons/rust/rust-original.svg`, black inverted to white | MIT (devicon) |
| wasm.png | WebAssembly | devicon v2.16.0 `icons/wasm/wasm-original.svg` | MIT (devicon) |
| julia.png | Julia | devicon v2.16.0 `icons/julia/julia-original.svg` | MIT (devicon) |

flask.png and rust.png are one-colour marks drawn white; Cove tints them with
the theme's text colour (frameworks.rs `Mark::tint`) so they read on light and
dark looks alike.
