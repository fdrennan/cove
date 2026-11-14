# Cove

Cove is a system for building and managing large numbers of scientific
applications, from the kernel to the person using them. It runs R and Python
(through reticulate) consoles, source, files, plots and an agent beside the
code, and draws the software stack underneath as an atlas: every package,
language set and build it was compiled from, read from the stack's own
install records.

It is one Rust program (egui) that runs natively and in the browser
(WebAssembly), and serves itself: `cove serve` for one person, `cove
multiuser` for many behind a reverse proxy.

- Source: https://gitlab.com/ndexr/cove
- Running: https://cove.ndexr.io

## Layout

| path | what it is |
|---|---|
| `app/src/` | the program: UI, server (`server.rs`, `multiuser.rs`), host actions (`host.rs`), atlas (`atlas.rs`, `atlas/`) |
| `app/templates/starters/` | the project starters Cove creates (Shiny, Python, Julia, Rust, Node), compiled in by `build.rs` |
| `app/assets/` | fonts, backdrops, framework marks, helper scripts |
| `app/packaging/` | Linux, macOS and Windows packaging |
| `onprem/` | the on-prem service: build-on-save (`run.sh`), systemd units, session launchers |
| `docs/` | notes on the software stack Cove reads |

## Build

Rust 1.96 or later.

```bash
cd app
cargo build --release          # the native program, target/release/cove
./build-web.sh                 # the browser bundle, dist/web (needs wasm-bindgen-cli at Cargo.lock's version)
target/release/cove serve --listen 127.0.0.1:8796 --web-dir dist/web
cargo test --release           # tests; the live ones are #[ignore] and need a database
```

Cove was built for the ndexr platform, and parts of it assume that host: the
stack at `/opt/ndexr/stack` (Lmod), the platform checkout at
`/home/ubuntu/ndexr`, and Postgres for messages and sharing. The tabs that
read them need that host to work.

## Cove, RStudio and Positron

Cove is its own program and contains no RStudio or Positron code. Its IDE
borrows RStudio's layout (four panes, Global Options) as a model, drawn in its
own Rust, and talks to R through its own kernel. On 2026-10-28 every string
literal and code line in this repository was searched in the source of
RStudio v2026.04.0+526. The only matches were generic: XML namespaces, the
base64 alphabet and viewport tags.

A Cove server can also start RStudio Server and Positron sessions for people
(`onprem/rstudio-session.sh`, `onprem/positron-session.sh`). These are separate
programs, installed from the software stack and reached through a proxy, so
their licences govern them and not this repository's:

- **RStudio Server** is AGPLv3. Whoever runs a *modified* RStudio for other
  people over a network must offer those people its complete source (AGPLv3
  section 13): the upstream release, every patch, and the build recipe.
- **Positron** is under the Elastic License 2.0, which does not allow offering
  it to third parties as a hosted service without an agreement with Posit.
  Leave `positron-launcher` unset unless your Posit licence covers that.

RStudio and Positron are trademarks of Posit Software, PBC. Cove is not
affiliated with or endorsed by Posit.

## Licence

MIT, see `LICENSE`. Third-party fonts, libraries and marks keep their own
licences, listed there.
