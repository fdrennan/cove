# Stack knowledge

What the field has learned about building and delivering scientific software,
set beside what the ndexr stack does. For the people who use Cove and for the
agents that work in it. Every "ndexr" line is a measurement or a file you can
open; nothing here is a plan written as a fact. Added 2026-10-21 by the
operator ("stack knowledge into the system for the user and yourself").

## Sources

- **CC** - Boissonneault, Oldeman, Taylor. *Providing a Unified Software
  Environment for Canada's National Advanced Computing Centers.* PEARC'19.
- **EESSI** - Dröge, Holanda Rusu, Hoste, van Leeuwen, O'Cais, Röblitz.
  *EESSI: A cross-platform ready-to-use optimised scientific software stack.*
  Software: Practice and Experience 53(1), 176-210, 2023.
  doi:10.1002/spe.3075 (open access).

- **REF** - the operator's research reference *HPC and scientific software,
  centered on EESSI* (2026-10-21): 17 papers, the MultiXscale D1.3 / D1.5 /
  D5.3 reports and project documentation, from CC (2019) and EESSI (2023)
  through RISC-V (2024), portable ESPResSo testing (2025) and
  ESPResSo-waLBerla (2026), with EasyBuild, Lmod, CernVM-FS, Spack, Guix,
  Singularity/Apptainer, ReFrame, FAIR software and floating-point
  reproducibility. The `.docx` itself is not on this machine; this file keeps
  its synthesis, set beside ndexr.

## The model both papers share

Three layers over what the machine itself provides (kernel, drivers,
anything privileged), each with one job:

| Layer | Job | CC / EESSI use | ndexr |
|---|---|---|---|
| Filesystem | deliver one build to every machine, read-only, on demand | CernVM-FS: Stratum 0 → Stratum 1 mirrors → Squid → client cache | CernVM-FS repository `stack.ndexr.io`, mounted at `/opt/ndexr/stack` (not `/cvmfs`); revision 36 on 2026-10-21 (`getfattr -n user.revision /opt/ndexr/stack`) |
| Compatibility | its own glibc and OS tools, so nothing depends on the host | Gentoo Prefix, one per ISA, generic binaries | Gentoo Prefix per triple (`x86_64-pc-linux-gnu`, `aarch64-unknown-linux-gnu`); 324 Prefix packages in the catalog; glibc 2.43 sets the floor - Linux ≥ 3.2 on x86-64, ≥ 3.7 on arm64 (Cove's atlas, kernel stars) |
| Software | the scientific applications, as environment modules | EasyBuild + Lmod; EESSI one tree per CPU microarchitecture, picked by `archspec` | EasyBuild + Lmod, 328 modules, toolchains `foss-2024a` / `GCCcore-13.3.0`; one tree per triple, built `--optarch=GENERIC` (see Gaps) |

ndexr adds a fourth that neither paper has as its own layer: **language
sets** - `env/R/<v>`, `env/Python/<v>`, `env/Rust/<v>`, `env/Julia/<v>` -
each one language's packages in their own pinned manifest and site library,
loaded by one module, plus `env/<combo>` pairing an R and a Python set
(`ndexr.ndexr.io/crate/assets/stack-kit`).

## Lessons, and where ndexr stands

**Dependency hell is the problem, not a user error** (EESSI §1, "Why Johnny
can't build"). Scientific packages pull in large trees of externally built
libraries with incompatible ABIs. ndexr's answer is the same as both papers':
build the tree once, centrally, and give people the result.

**Generic binaries cost speed** (EESSI §1.3, §6). GROMACS ran 57% faster built
for AVX-512 than built generically (SSE 4.1); the generic EESSI build was
~25% slower at scale. *ndexr:* the whole software layer is generic
(`--optarch=GENERIC`, stack-kit `prefix-install/README.md`) on purpose -
`-march=native` SIGILLs on any lesser CPU a tree is restored onto. The cost
EESSI measured is the cost ndexr pays today.

**Prebuilt downloads are generic and unaccounted** (EESSI §2.2, §2.4, §7.4.2).
Distribution packages, conda and container images ship generic builds and
pull in their own Python, MPI and OpenSSL. *ndexr:* Cove reads each install
record (host.rs `provenance`, `/view/provenance.json`). On x86-64,
Python/3.13.15, 28 packages arrived as manylinux wheels built elsewhere -
torch, torchaudio, triton, pyarrow, polars-runtime-32, llvmlite, uv,
cuda-bindings, the nvidia-* CUDA wheels and others - against 61 compiled
here; no R package came as a downloaded binary. 26 of the 28 are on the
kit's list of accepted binaries, each with a measured reason
(`stack-kit/manifest/wheel-exceptions.tsv`: no source published - torch,
triton, swat, the NVIDIA libraries - or a toolchain the suite does not build
yet - polars needs nightly Rust, uv needs rustc 1.97, llvmlite needs LLVM 20).
**pypdfium2 and soundfile are not on it**: downloaded with no recorded reason.
The atlas rings each one in amber and its card says which.

**One environment everywhere** (EESSI §4.1, CC). The same modules on a
laptop, a cloud machine and a cluster, so a workflow moves without being
rewritten. *ndexr:* account cells and the build machines mount the same
stack, and `. /opt/ndexr/stack/use.sh && ml …` works the same in each.

**Releases are versioned; old ones stay reachable** (EESSI §3.5, §7.3).
EESSI dates its releases (2021.06); CernVM-FS lets a client mount an older
revision; retired stacks go to containers or tarballs. *ndexr:* CernVM-FS
revisions (36 now); language sets pin every package version in a manifest so
a result can name what it ran in.

**Security updates land in the compatibility layer** (EESSI §7.3): Gentoo's
`glsa-check` scans for vulnerable packages, then tests re-run. *ndexr:*
`glsa-check` is installed in the Prefix
(`prefix/<triple>/usr/lib/python-exec/*/glsa-check`), but nothing in the repo
runs it.

**Integrity: no binary without a trail** (EESSI §7.4.2). Signed CernVM-FS
catalogs, builds in a controlled container, tarballs ingested only after
review, build logs public. *ndexr:* CernVM-FS signs; build logs are in
Cove's Builds view and `/view/buildlog.json`; the external binaries above are
exactly the installs without a build trail here.

**Test every layer** (EESSI §3.7): smoke tests when the compatibility layer
changes, performance tests when software is added, ReFrame for all three.
*ndexr:* no ReFrame or equivalent suite in the repo.

**Swap in the site's own fast libraries at run time** (EESSI §4.2, §7.5,
§7.8). FlexiBLAS for BLAS/LAPACK backends; ABI-compatible MPI overrides;
libfabric/UCX built "fat" for every interconnect. *ndexr:* FlexiBLAS 3.4.4
and OpenBLAS 0.3.27 are in the suite (`easyconfigs/f/FlexiBLAS`,
`easyconfigs/o/OpenBLAS`).

**Accelerators come from the host** (EESSI §7.9, `host_injections`). CUDA
drivers and licensed runtimes stay on the machine; the stack finds them.
*ndexr:* the catalog marks packages built for a `cuda` architecture beside
x86_64 and aarch64 (torch, numpy); the CUDA runtime libraries themselves
arrive as NVIDIA's binary-only wheels (above).

## Three records (REF)

REF organises a platform round three connected records, and says EESSI
supplies much of the first two while the scientific workflow must close the
third:

| Record | Question | ndexr today |
|---|---|---|
| Construction | what produced each artifact | The catalog's build steps per item (`/srv/ndexr/software/catalog.json`, `st`: ok/fail per arch), build logs (`/view/buildlog.json`), each module's easyconfig (host.rs `easyconfig`), and how each installed package arrived (host.rs `provenance`). |
| Execution | what actually took part in one computation | Not recorded. Cove knows a session's env set (`.ndexr-env`) and loaded modules while it runs; nothing keeps a record of a run. |
| Evaluation | why a result was accepted for its use | Not recorded, with one exception: `stack-kit/manifest/wheel-exceptions.tsv` is an acceptance record - why each downloaded binary was taken. |

## The atlas's relationships against REF's list

REF asks for precise relationships rather than one "depends on". The atlas
keeps its edges typed (`atlas.rs Rel`); every one below is **declared** (read
from a recipe, a manifest or a config) unless marked **observed**:

| REF relationship | atlas | Source |
|---|---|---|
| built from / compiled with | not an edge; a module's toolchain is its group (`foss-2024a`, `GCCcore-13.3.0`) | catalog `g` |
| linked against | `Dep` - the catalog's own dependency list (build and run together) | catalog `dep` |
| loaded during execution | `Uses` - what an env set's module loads | easyconfig `dependencies` |
| published in | not an edge; the whole stack is CernVM-FS revision 36, root hash `576f48f915285ac21e3a8b8c76770ecc2e02f4c6` (2026-10-21) | `getfattr` on `/opt/ndexr/stack` |
| selected by | `Member` (package in an env set), `Joins` (combo joins a set), `Works` (domain works in a set) | manifests, combos.txt, `.ndexr-env` |
| injected by a site | none | - |
| executed on | `Runs` (upstream / nginx on the host, host on a kernel floor) - **observed**, live | host.rs `serving` |
| how it was installed | amber ring, card - **observed** | host.rs `provenance` (wheel tags, DESCRIPTION) |
| tested with / generated from / accepted for a use | none | - |

REF also asks that declared and observed evidence sit side by side, each with
its scope and date: the atlas does this only for provenance and serving.

## Statuses kept apart (REF §9)

REF keeps *available*, *executed successfully*, *functionally tested* and
*performance measured* as four statuses. ndexr records one and a half: built
or failed per architecture (catalog `st`), and published (in the CernVM-FS
revision). None of executed, tested or measured exists for any package.

## Repository identity is not retention (REF §7)

A CernVM-FS root hash names one content tree; whether its objects are still
kept is policy. ndexr can name the tree it serves now (revision 36, hash
above) but nothing records which older revisions are retained, so "the
environment a result ran in is still available" cannot yet be shown.

## EESSI's own releases (REF §9, as of 2026-10-21, not re-checked here)

| Environment | Toolchains | Status |
|---|---|---|
| 2023.06 | foss 2022b, 2023a, 2023b | default |
| 2025.06 | foss 2024a, 2025a, 2025b | available |
| 2026.06 | foss 2026.1 | available |

ndexr builds `foss-2024a`, the toolchain EESSI's 2025.06 carries.

## A build's identity is not the machine that built it (operator, 2026-10-24)

CC separates software from the machines that run it. The operator's next step
is to separate it from the machines that build it as well. A build worker is
replaceable. Software is named by what went into it and what came out, not by
the box it was built on. Here are the six steps the operator listed, each set
beside what ndexr does, measured 2026-10-24:

| Step | ndexr |
|---|---|
| 1. Fetch the source and the exact build spec | Partly. The kit rides inside the `ndexr` binary (`assets/stack-kit/SOURCE.txt`: svbt version, file count, digest). Easyconfigs carry source checksums, and the Prefix tree is pinned (`PREFIX_PIN_TREE`). |
| 2. Rebuild the Prefix and EasyBuild anywhere | Yes. `ndexr stack up --dir <path>` builds on any Linux machine with no root; the four build boxes and a client build use the same command. |
| 3. Compile for a declared target | One target per triple, `--optarch=GENERIC` (`docs/targets.tsv`). |
| 4. Test and record the artifact | No. `unibuild_steps` records `server, instance_type, arch, lane, run_id, name, version, status`. `run_id` is a timestamp plus the builder's IP (`20261011T020514Z-ip-10-0-0-1`). There are no tests, no hash of the spec and no hash of the result. |
| 5. Publish to S3 | Yes. `stack up --publish` stages each lane to S3 under its run timestamp. |
| 6. Promote approved artifacts into CVMFS | Not as a step. `stack cvmfs publish --from-s3` writes straight into the production repository `stack.ndexr.io`. There is no development repository and no approval in between (CC tests in a development repository first). |

So the record names software by where it was built. The same easyconfig built
on two boxes gives two rows that nothing links, and nothing can say whether
the two results are the same bytes.

The data structure that fixes this has two identities per build:

- **spec id**: a hash over the easyconfig's bytes, the spec ids of its
  dependencies and toolchain, the target (`GENERIC`, later a
  microarchitecture), the Prefix pin and the kit version. It is the same on
  every machine, and it is known before anything compiles.
- **artifact id**: a hash over the installed tree, made of each file's path,
  mode and content hash in path order. It is what was produced.

The machine, the run and the time become attributes of a build of a spec id.
Two builds with the same spec id and different artifact ids show a build that
does not reproduce. A result that names its artifact ids names exactly what it
ran. CernVM-FS already stores every published file by its content hash, so an
artifact id can be checked against the published tree.

The nearest thing ndexr has to an artifact id cannot be used as one yet.
`stack publish` names each package's upload by a fingerprint
(`crate/src/stack/publish.rs` `fingerprint`, `tree_listing`), and the publisher
fetches only packages whose fingerprint changed. That fingerprint is a hash of
each file's path, mode, size and **mtime**. It reads no file contents. So it is
an identity of one build on one machine at one moment: two bit-identical builds
on two boxes get two fingerprints and upload twice, and a changed file of the
same size and mtime is missed. The upload directory is named
`<UTC time>-<host>` (`new_id`). Hashing contents instead, with mtimes left out,
turns it into the artifact id above and keeps the incremental publish. Measured
2026-10-24.

The language sets (R, Python, Julia, Rust) are the same story one layer up.
Measured 2026-10-24 from each set's `current.json` and Cove's Packages view
(`app/src/pkgflow.rs`):
- **The record names the publisher, not the builder.** `host` is
  `this_host()` when `ndexr pkgs publish` runs (pkgs.rs). Each package has its
  sha256, which is an artifact id. Nothing records which machine compiled it,
  or the toolchain, flags, CPU target or source digest, so no spec id exists to
  compare a rebuild against.
- **x86-64 sets are built on the production host.** 15 of the 16 published
  x86-64 sets name `ubuntu-System-Product-Name`, the on-prem machine that
  serves Cove to everyone; one names the intel EC2 build box. CC's second reason for dedicated build nodes
  (compiling takes the CPU, memory and I/O that people's work needs) applies
  here. All 16 arm64 sets name one EC2 builder.
- **Promotion lags.** `stack.ndexr.io` is at revision 37, and its kit was last
  synced 2026-10-13T07:57Z. The four language sets checked (R/4.6.1-bioc3.23,
  Python/3.13.15, Julia/1.11.7, Rust/1.96.0) were published after that, between
  2026-10-19 and 2026-10-24. None of their `env/<set>` modules is on
  CVMFS, so `ml` still loads the combined `env/r*_py*` sets.
- **Testing at build** exists for the EasyBuild layer: 202 sanity-check lines
  in the easyconfigs, and the RPATH check. For language packages nothing beyond
  the install itself is recorded.

## Gaps, in the papers' terms

1. **No per-microarchitecture trees.** One generic tree per ISA; EESSI ships
   haswell / skylake_avx512 / zen2 / zen3 / graviton2 trees and picks one
   with `archspec`.
   - `docs/targets.tsv` lists every target in EESSI's 2026.06 tables: 18 CPU
     targets and 6 CUDA capabilities. For each it gives what ndexr builds for
     it and which ndexr machine is natively that CPU.
   - Measured 2026-10-24, five targets are hardware ndexr already runs:
     - sapphirerapids: the intel box, m7i;
     - zen4: the amd box, m7a;
     - neoverse_v1: the arm box, m7g / Graviton 3;
     - zen2: the gpu box's EPYC, and this on-prem host;
     - CUDA 8.x: the gpu box's A10G.
   - So the first per-microarchitecture trees need no new machines.
   - CUDA is the other way round from EESSI. The gpu lane builds one binary
     for capabilities 7.0 to 9.0 on the generic CPU, and nothing for 10.0 or
     12.0. EESSI builds each capability for each CPU target.
2. **28 downloaded binaries in the Python sets** (x86-64), not built from
   source; 2 of them (pypdfium2, soundfile) on no list of accepted ones.
3. **No scheduled `glsa-check`** over the Prefix.
4. **No test suite over the layers** (smoke or performance).
5. **No Stratum 1 mirrors in the repo.** Cove's CVMFS settings take a
   client proxy (Squid) but the repo describes no Stratum 1 replica of
   `stack.ndexr.io`.
6. **No execution record** (REF's second record): nothing keeps which
   modules, env set, libraries, inputs and machine a run used. Proposed for
   deployed apps in `docs/deployment-record.md` (2026-10-28).
7. **No evaluation record** (REF's third), beyond the accepted-binaries list.
8. **One status, not four** (REF §9): built/failed only - nothing marks a
   package executed, functionally tested or performance-measured.
9. **No retention record** for CernVM-FS revisions (REF §7): the current tree
   is identified; older ones are not known to be kept.

Each is a measurement from 2026-10-21; re-measure before quoting it.
