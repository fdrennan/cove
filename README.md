# From Source to Scientific Application

### A living model of reproducible scientific computing

**System:** ndexr and Cove · **Implementation:** Rust and WebAssembly over Gentoo Prefix, EasyBuild, Lmod and CernVM-FS
**Read it running:** [ndexr.io](https://ndexr.io), Cove's public viewer, no account · **Use it:** [cove.ndexr.io](https://cove.ndexr.io) · **Licence:** MIT

---

## Abstract

A scientific result depends on far more software than its own source code: a
language runtime, compiled packages, numerical libraries, a compiler
toolchain, system libraries, and the machine that runs them. Whether the
result can be rebuilt, moved to another computer, or questioned later depends
on whether that whole stack can be named and inspected.

This repository holds Cove, the interface to a software stack built on the
model Compute Canada published in 2019 [1] and EESSI later generalised [2]. A
compatibility layer (Gentoo Prefix) frees the software from the host
distribution. EasyBuild builds the scientific software, Lmod selects it, and
CernVM-FS delivers it read-only to every machine that mounts it. ndexr adds a
fourth layer, *language sets*: R, Python, Julia and Rust package libraries,
each pinned in its own manifest and loaded by one module.

Cove is one Rust program that runs natively and, compiled to WebAssembly, in
the browser. In it a person writes and runs an application beside an agent,
on that stack, and the application goes live at its own address. The same
program, read-only and with no account, is the public viewer at
[ndexr.io](https://ndexr.io). That viewer is this paper's evidence: its four
views are the figures below, read live from the stack's own records. The
figures change as the stack is built, and the paper is about the state they
show.

## 1. Introduction

Consider an analysis written in R. It runs on one R release, against compiled
packages, which link C and Fortran libraries, built by one compiler, for one
processor architecture. Reproducing it means answering two questions:

1. Can the environment it ran in be rebuilt?
2. Can the relationship between that environment and the result be inspected?

Shared research computing has answered the first with centrally built
software and environment modules. Compute Canada showed that one stack,
delivered over CernVM-FS, can serve many heterogeneous national clusters [1].
EESSI took the same design across institutions and processor families [2].

This work asks what happens when that design reaches past the cluster to the
person writing an application. That person needs one place where the stack is
something to look at, a tree of builds whose provenance can be followed, as
well as something to use.

## 2. The computing model

**[Figure 1 · The atlas](https://ndexr.io/view/?tab=atlas).** The whole stack,
from the kernel up: every package, language set and build, drawn from the
stack's own install records, with each package's dependencies as typed edges.
How each installed package arrived is marked on it: compiled here, or
downloaded as a binary (amber), with the reason when one is recorded.

Five responsibilities, each with one job:

| Layer | Job | ndexr |
|---|---|---|
| Host | kernel, drivers, anything privileged | yours; the stack's glibc sets the floor at Linux ≥ 3.2 on x86-64, ≥ 3.7 on arm64 |
| Compatibility | its own C library and OS tools, so nothing depends on the host | Gentoo Prefix, one per triple: `x86_64-pc-linux-gnu`, `aarch64-unknown-linux-gnu` |
| Software | compilers, libraries and applications, as environment modules | EasyBuild with Lmod; toolchains `GCCcore-13.3.0` and `foss-2024a` |
| Language sets | each language's packages, pinned, loaded by one module | `env/R/…`, `env/Python/…`, `env/Julia/…`, `env/Rust/…`, and `env/r4.6_py3.13-release`-style pairs |
| Distribution | one build to every machine, read-only, on demand | CernVM-FS repository `stack.ndexr.io`, mounted at `/opt/ndexr/stack` |

Distribution is not a layer a program runs in. It spans the others: what is
published is the whole tree, at one revision, under one content hash.

## 3. From source to execution

**[Figure 2 · Builds](https://ndexr.io/view/?tab=builds).** How the stack got
built and what is building now: cumulative curves per machine, the timeline,
and the queue.

1. **Build.** Four machines build the same specification, one per lane:
   Intel and AMD (x86-64), Arm (aarch64) and a GPU machine (x86-64 with CUDA).
   `ndexr stack up` builds the Prefix and the EasyBuild layer on any Linux
   machine without root.
2. **Publish.** Each lane is staged to S3 under its run's timestamp
   (`stack up --publish`).
3. **Distribute.** The staged trees are published into the CernVM-FS
   repository (`stack cvmfs publish --from-s3`). Clients fetch only the files
   a process opens and keep them in a local cache.
4. **Select.** On any machine, one line enters the stack and Lmod loads what a
   program needs:

   ```bash
   . /opt/ndexr/stack/use.sh          # this machine's architecture, and ml
   ml env/r4.6_py3.13-release         # R 4.6 + Python 3.13 and their libraries
   ```

5. **Execute.** In Cove, a project is source plus three scripts, `.cove/build.sh`,
   `.cove/up.sh` and `.cove/down.sh`, run in the stack. It is live at its own
   address from the moment it is made.

**[Figure 3 · Packages](https://ndexr.io/view/?tab=packages).** Every package
the stack builds, searchable: its versions, the architectures it is built
for, its dependencies, and whether each build succeeded.

**[Figure 4 · Logs](https://ndexr.io/view/?tab=logs).** Each build machine's
log, live as it builds.

## 4. Cove

Cove is one Rust program (egui). The same code runs as a desktop application
and, compiled to WebAssembly, in the browser. It serves itself: `cove serve`
for one person, `cove multiuser` for many behind a reverse proxy, and `/view/`
for anyone, read-only.

- **Work beside the code.** Source, files, R consoles with Python inside them
  through reticulate, a terminal, plots, and an agent in the same window.
- **Start from a working application.** 65 starters across R (Shiny), Python
  (FastAPI, Flask, Streamlit, Dash, Bokeh, Panel), Julia, Rust, React, Node,
  TypeScript and C++, and VS Code in the browser as a site of its own. Most
  are small scientific instruments: the Lorenz system, an SIR model, a
  Bayesian adaptive trial, single-cell clustering, reaction-diffusion, Monte
  Carlo pricing and more.
- **Every project at its own address,** under the person's own domain, and
  every version kept: projects are saved every 30 minutes, any version can be
  restored, and history only grows.
- **See what it stands on.** The atlas, packages, builds and logs above are
  Cove's own views. The public viewer is the same program with everything that
  changes anything left out.

## 5. Measured state

On 2026-11-14 the stack stood as follows. The figures above show it as it is
now.

| Quantity | Value |
|---|---|
| CernVM-FS revision | 58 (root hash `1b1de07e66b7b23239916fb37f609b5dd3754d05`) |
| Pieces of software in the catalog | 3,890: Gentoo Prefix 324 · EasyBuild 374 · R 819 · Python 579 · Julia 791 · Rust 1,003 |
| Lmod modules (x86-64) | 551, of which 102 are environment modules |
| Build machines | 4 (Intel, AMD, Arm, GPU) |
| Build runs / steps recorded | 202 / 943,575 |

## 6. Limitations

These are measured and recorded in [`docs/stack-knowledge.md`](docs/stack-knowledge.md),
set beside CC, EESSI and the reproducibility literature.

- **Generic binaries.** One tree per instruction set, built
  `--optarch=GENERIC`. EESSI measured GROMACS 57% faster built for AVX-512
  than generically [2]; ndexr pays that cost. Five of EESSI's CPU targets are
  hardware ndexr's build machines already are (`docs/targets.tsv`).
- **Builds are named by where they were built.** A build's record holds the
  machine, the run and the package, but no hash of its specification and no
  hash of what it produced. Two builds of the same recipe cannot be shown to
  be the same bytes.
- **No staging repository.** Publication writes straight into the production
  repository. Compute Canada tests in a development repository first [1].
- **Some binaries are downloaded, not built.** Some Python wheels (PyTorch,
  the NVIDIA libraries and others) arrive prebuilt. Each one is recorded and
  shown, and most are on an accepted list with a reason.
- **Only construction is recorded.** Of the three records a platform keeps
  (construction, execution and evaluation), only construction exists. Nothing
  records which environment one run of an analysis used, or why a result was
  accepted.
- **No layer-wide test suite.** EasyBuild's sanity checks run at build; there
  is no smoke or performance suite (ReFrame or similar) over the layers.
- **Cove assumes its host.** Parts of Cove expect the stack at
  `/opt/ndexr/stack`, the platform checkout at `/home/ubuntu/ndexr`, and
  Postgres for messages and sharing. Compiling Cove does not make those tabs
  work on another machine.

## 7. Discussion and future work

The design treats an application as the product of a named software
environment, not as a standalone executable. What the viewer already makes
visible (what was built, where, from what, and how each installed package
arrived) is the construction record. Two steps would close most of the
limitations above:

- **Two identities per build.** A *spec id* hashes the recipe, its
  dependencies' spec ids, the target and the Prefix pin, and is known before
  anything compiles. An *artifact id* hashes the installed tree's paths, modes
  and contents. Same spec id with different artifact ids is a build that does
  not reproduce. A result that names its artifact ids names exactly what it
  ran.
- **An execution record.** Cove already knows a session's environment set and
  loaded modules while it runs; keeping them would let a result name its
  environment.

The figures are live views today. They are not yet experiments. Proposed and
not built: select an architecture and environment and see which layers a
program uses; compare two stack revisions; trace one package from recipe to
log to artifact to the applications that load it. Each would read the same
records the viewer reads now.

## Building Cove

Rust 1.96 or later.

```bash
cd app
cargo build --release          # the native program, target/release/cove
./build-web.sh                 # the browser bundle, dist/web (needs wasm-bindgen-cli at Cargo.lock's version)
target/release/cove serve --listen 127.0.0.1:8796 --web-dir dist/web
cargo test --release           # tests; the live ones are #[ignore] and need a database
```

| path | what it is |
|---|---|
| `app/src/` | the program: UI, server (`server.rs`, `multiuser.rs`), host actions (`host.rs`), atlas (`atlas.rs`, `atlas/`), the public viewer (`view.rs`) |
| `app/templates/starters/` | the project starters, compiled in by `build.rs` |
| `app/assets/` | fonts, backdrops, framework marks, helper scripts |
| `app/packaging/` | Linux, macOS and Windows packaging |
| `onprem/` | the on-prem service: build-on-save (`run.sh`), systemd units, session launchers |
| `docs/` | the stack's model against the literature, targets, compliance notes |

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

## References

1. M. Boissonneault, B. E. Oldeman, R. P. Taylor. *Providing a Unified
   Software Environment for Canada's National Advanced Computing Centers.*
   PEARC '19. doi:[10.1145/3332186.3332210](https://doi.org/10.1145/3332186.3332210)
2. B. Dröge, V. Holanda Rusu, K. Hoste, C. van Leeuwen, A. O'Cais, T. Röblitz.
   *EESSI: A cross-platform ready-to-use optimised scientific software stack.*
   Software: Practice and Experience 53(1), 176-210, 2023.
   doi:[10.1002/spe.3075](https://doi.org/10.1002/spe.3075)
3. [EasyBuild](https://docs.easybuild.io/) · [Lmod](https://lmod.readthedocs.io/) ·
   [CernVM-FS](https://cvmfs.readthedocs.io/) · [Gentoo Prefix](https://wiki.gentoo.org/wiki/Project:Prefix)

## Licence

MIT, see `LICENSE`. Third-party fonts, libraries and marks keep their own
licences, listed there.
