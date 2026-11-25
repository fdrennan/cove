# Cove's code, by system

`src/` is laid out by what each part of Cove is and which side of the
UI ↔ machine boundary it runs on, so the tree is the map. Reorganised
2026-11-25 from 77 files in one folder (12 more in `atlas/` and `software/`);
no behaviour changed in the move.

One binary, four ways to run it (`main.rs`): the desktop app driving this
machine, the desktop app driving a server (`--connect`), `cove serve` (the UI as
WebAssembly plus `/api`), and `cove multiuser` (a `serve` per person). The
browser build (wasm32) is the same UI driving its server.

## The tree

```
src/
  main.rs        entry: the command line, then the desktop window or a server
  api.rs         the contract: every Query and every view, the same on both sides
  backend.rs     the one boundary: Local (this process), Http (a server), Cell
  host.rs        machine side: answers a Query by looking at this machine
  host/            what it answers with, one subsystem a file
                   kernel runner envs stack apps record storage limits patches
                   pkgflow boxes recap brief messages lsp boxcomplete zippack
                   vitals localagent account shares update git
  server.rs      `cove serve`: the UI and /api over HTTP (Unix)
  server/          what builds on it: multiuser (a serve per person), localserve
  desktop/       the desktop window's native pieces: webview, desk (hand-over)
  project/       what a project is, both sides: dotcove (.cove/), starters, find
    diff/          its diffs as data: patch (parse, write), words, model (rows,
                   hunks, folds, pairings)
  catalog/       what the stack holds, both sides: manifest, names, source
  ui/            everything drawn, desktop and browser alike
    mod.rs
    app.rs         the frame: header, modes, menus, dialogs, panes
    kit/           the toolkit: theme icons bar backdrop md ansi frameworks
                   device paste links pick shots
    ide.rs         Workspace mode: the windows and their tabs
    ide/             its panes and parts: dock tabs term db gitview
                     settings_tab account_tab access localstack
      gitview/         the Git window's drawing: render (the page), ruler, tree
    editor/        the editor's language knowledge: syntax edcomplete editkeys
    agent.rs       the agent tab and its conversation
    agent/           voice, spoken (where each word came from), drafts
    atlas.rs       Atlas mode   atlas/  inspect owners patched provenance serving
    software.rs    Builds mode  software/  builds branches brush draft install
                                           logs manifests
    manage.rs      Connections mode       inbox.rs    Messages
    recents.rs     the Recent menu        release.rs  a newer Cove, offered
    setup.rs       first start            tour.rs     the walkthroughs
    front/         the public face: showcase (slides), view (Visualization
                   mode, browser only), stage
```

## Systems and their boundaries

| Directory | Responsibility | Compiles for |
|---|---|---|
| root (`main`, `api`, `backend`) | entry, and the contract both sides share | all |
| `host` | everything that touches this machine: processes, files, the stack, accounts, self-update | native |
| `server` | serving the UI and `/api` over HTTP, alone or one per person | Unix native |
| `desktop` | the native window's embedded browser and its hand-over | native, by feature |
| `project` | the shape of a project, read by both the UI and the host | all |
| `catalog` | the stack's catalogue: manifests, names, sources | all (fetching halves `cfg`-gated) |
| `ui` | every pixel; talks to a machine only through `backend` | all |

The test of where code goes is the target it must compile for. Code that must
also run in the browser can't live under `host`, `server` or `desktop`.

## Which way dependencies go

```
main ──> ui ──> backend ──> api
          │        └─(native)──> host ──> project, catalog, api
          └──> project, catalog, ui::kit
server ──> host ──> api
```

- `ui` reaches a machine through `backend` and the `api` types. It calls
  `host` directly only in `#[cfg(not(target_arch = "wasm32"))]` blocks (desktop
  conveniences: the account, the self-update, the stack setup).
- `host` and `server` never draw. `project` and `catalog` never draw.
- `ui::kit` depends on nothing else in `ui`. Every surface may use it.

Edges that run the wrong way, measured 2026-11-25 (`crate::` references across
systems), are listed under "Open" below. Don't add new ones.

## Where new work goes

- **A new Query:** its type in `api.rs`, its answer in `host.rs` (or a
  `host/` file when it is a subsystem of its own), and its call in the UI.
- **A new surface** (a mode, a window, a tab): `ui/<name>.rs`. When it grows
  parts of its own, they go in `ui/<name>/`, the way `atlas` and `software` have.
- **A drawing helper more than one surface uses:** `ui/kit/`. Glyphs go
  in `ui/kit/icons.rs`. Colours and type come from `ui/kit/theme.rs` tokens,
  never literals.
- **A control in any bar** (a toolbar, a tab strip, the top bar): register it
  once in `ui/kit/icons.rs` (`ctl::NAME`: glyph, word, tip) and draw it with
  `ui/kit/bar.rs` - `button` / `toggle` / `menu_item`, laid out by
  `bar::Toolbar`, with its `Weight` (Primary, Secondary, Icon) deciding glyph
  or glyph and word. What a bar has no room for goes in its ⋯ (`bar::more`).
  Never a framed `egui::Button` in a bar, and never a font symbol as an icon:
  `theme::tool("⟳")` draws the registered glyph.
- **A diff or a patch:** git runs in `host/git.rs`; reading and laying one
  out is `project/diff/` (no drawing, tested alone); drawing it is
  `ui/ide/gitview/`; what a click does is `ui/ide/gitview.rs`. A new view of
  a diff is a pairing in `model.rs` and a drawing in `render.rs`, never a
  second parser.
- **Logic both the UI and the host need, with no drawing and no machine
  access:** `project/` when it is about a project, `catalog/` when it is about
  the stack's contents. Anything else that fits neither needs a new boundary,
  discussed first; never a `util/` or `common/`.
- **Anything that runs a process, reads the machine or holds a secret:**
  `host/`.
- **Another way of serving Cove:** `server/`.

**When to make a directory.** Make one when a file has grown parts that only
it uses (`foo.rs` plus `foo/`, the 2018-edition way, as `host`, `ide`, `agent`,
`atlas` and `software` do). Also make one when several files form one system
with its own compile target (`desktop`) or its own subject (`project`,
`catalog`, `ui/kit`). Never make one to lower a file count. A system with one
file stays a file.

**Paths.** A module's path follows its folder: `crate::ui::kit::theme`,
`crate::host::kernel`. Assets are included by
`concat!(env!("CARGO_MANIFEST_DIR"), "/assets/…")`, never by a `../` path that
breaks when a file moves. Children are declared `pub(crate) mod` in their
parent (`mod.rs`, or the parent's own `.rs`), with the `cfg` that says where
they compile.

**Scripts that name source files** (`onprem/run.sh`: `guard` runs the atlas
tests when `ui/atlas*` changed; `server_changed` restarts the root server when
its own files changed) must be updated with any move.

## Open (2026-11-25)

- **`host` reads a few UI modules:**
  - `host.rs` → `ui::ide::{db, term}` for their query handlers;
  - `host.rs` → `ui::software::now_secs`;
  - `host/apps.rs` → `ui::ide::settings_tab` (app settings) and `ui::kit::ansi`;
  - `host/kernel.rs` → `ui::ide::env_consoles` and `ui::kit::theme`.

  Each is a non-drawing piece that belongs in `host`, `project` or `api`. Move
  it there and the UI imports it back.
- **`server/multiuser.rs` reads two UI constants:** `ui::agent::SITE_TITLE` and
  `ui::release::notes`.
- **`catalog/{manifest,source}.rs` carry their fetching halves** (`host::runner`,
  `host::stack`, `host::account`), `cfg`-gated for native. They should be split
  into `catalog/` types and `host/` fetchers.
- **`project/dotcove.rs` → `host::starter_files`:** the starter file list
  belongs in `project/starters.rs`.
- **Size:** `ui/ide.rs` (10.7k lines), `ui/app.rs` (5.1k), `host.rs` (6.2k)
  and `ui/atlas.rs` (5.0k) are each several subsystems in one file. Split them
  into their own children as they are worked on, not in one move.
- The template sources under `templates/` (starters, the rust-app) are their
  own projects and follow their own layout.
