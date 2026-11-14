# Deployment records (proposal, 2026-10-28)

Status: **proposed, nothing built.** Closes stack-knowledge.md gap 6 ("No
execution record") for deployed apps, and starts on gap 7 (evaluation).

## The question it answers

For any app running on a Cove domain, a stranger with read access can answer,
without trusting Cove, the operator or an agent:

1. Which source is running? (commit, and the hash of every file)
2. On what? (stack revision, modules, env set, toolchain, lockfiles)
3. Which of that came as a binary built elsewhere? (provenance)
4. Who put it there, when, and who can reach it? (deployer, access gate)
5. What was checked, by which command, with what result? (checks)

Today each answer exists somewhere - the S3 git archive, the CVMFS mount's
xattrs, `/view/provenance.json`, the vhost's gate - but nothing ties them to
one deployment. The record is that tie.

## Principle: evidence, not verdicts

A record holds facts and commands, never a judgement. "Licenses OK" is not a
field; `{"check": "licenses", "command": "...", "exit": 0, "output_sha256":
"..."}` is. A check counts only if anyone can rerun its command against the
record and get the same output. An agent may write checks; an agent's opinion
is never evidence. This keeps the evaluator replaceable: anyone may write and
run a different check over the same record.

## Where records live

In the project itself: `.cove/deployments/<id>.json`, one file per
deployment, never edited after it is written.

- The 30-minute autosave (`ndexr-autosave.timer` → `s3://ndexr-git-dr/git/
  repos/<slug>.git`) versions them with the code, so the history of what ran
  is kept, versioned and restorable like the source. No new store.
- `<id>` is the sha256 of the record's canonical JSON (keys sorted, no
  whitespace) without the `id` field. A changed record is a different id.
- `.cove/deployments/current` names the id that is live.

## When one is written

| Event | Code | Record |
|---|---|---|
| Rust app deploy | `app/assets/deploy-app.sh`, after step 2 (serve) answers | yes |
| Domain publish | host.rs `publish_as` → site_provision.r `provision_start` | yes, `kind: "publish"` (routing + access only) |
| Shiny module change | app.r hot reload of `module.r` | **open question** - see below |
| Access change | target.r `set_domain_target` (gate) | yes, a new record reusing the source/runtime blocks |

## The record

```json
{
  "schema": "cove.deployment/1",
  "id": "sha256:…",
  "domain": "lan.ndexr.io",
  "owner": "drennanfreddy",
  "kind": "rust-app",
  "deployed_by": "drennanfreddy",
  "deployed_at": "2026-10-28T18:02:11Z",

  "source": {
    "archive": "s3://ndexr-git-dr/git/repos/drennanfreddy__lan.ndexr.io.git",
    "commit": "228b624c…",
    "dirty": false,
    "files": { "src/main.rs": "sha256:…", "Cargo.lock": "sha256:…" }
  },

  "runtime": {
    "triple": "x86_64-linux",
    "stack": { "repo": "stack.ndexr.io", "revision": 1234, "root_hash": "…" },
    "modules": ["env/R/4.6.1-bioc3.23"],
    "env_set": { "name": "r4.6_py3.13-release", "current_json_sha256": "…" },
    "toolchain": { "rustc": "1.9x.y", "wasm_bindgen": "0.2.x" },
    "locks": { "Cargo.lock": "sha256:…", "renv.lock": null, "requirements.txt": null }
  },

  "provenance": {
    "external": [
      { "language": "Python", "name": "pyarrow", "how": "external",
        "evidence": "wheel tag manylinux_2_28_x86_64", "accepted": "wheel-exceptions.tsv" }
    ]
  },

  "build": {
    "host": "ubuntu-System-Product-Name",
    "started": "…", "finished": "…",
    "artifacts": { "server": "sha256:…", "web/app_bg.wasm": "sha256:…" },
    "log_sha256": "sha256:…"
  },

  "serve": { "unit": "cove-app-lan.ndexr.io.service", "user": "drennanfreddy", "port": 8821 },
  "access": { "gate": "owner" },

  "checks": [
    { "check": "rebuild", "command": "cove verify rebuild <id>",
      "exit": 0, "output_sha256": "…", "ran_at": "…", "ran_by": "…" }
  ]
}
```

Where each field comes from, all of it already read somewhere:

| Field | Source |
|---|---|
| `source.commit`, `files` | the project's git; deploy-app.sh does not commit, so `dirty` is true when the tree differs from HEAD, and `files` hashes what was built either way |
| `runtime.stack` | the CVMFS mount's `user.*` xattrs (what `StackView` reads) |
| `runtime.env_set` | `repo.ndexr.io/env/<triple>/<set>/current.json` |
| `runtime.toolchain` | `/usr/local/lib/cove/rust` as deploy-app.sh resolves it |
| `provenance.external` | `/view/provenance.json` (host.rs `provenance`), filtered to the packages the app loads |
| `build.artifacts` | sha256 of what deploy-app.sh installs |
| `serve`, `access` | the unit deploy-app.sh writes; the gate passed to `set_domain_target` |

`checks` is the one part written after the fact: appending a check writes a new
record (new id) whose `checks` grows. History only grows, like Versions.

## Two levels of claim, kept apart

A deployment page shows exactly one of these, and never rounds up:

- **Recorded:** every field above is filled from the system. This proves what
  was deployed and from where. Cove can do this today.
- **Rebuilt identical:** `cove verify rebuild <id>` rebuilt from `source` +
  `runtime` on another machine and every `build.artifacts` hash matched. This
  needs reproducible builds, and the stack is not there: generic-only trees,
  28 downloaded Python binaries, 2 unaccepted (stack-knowledge.md gaps 1-2).
  Until it is, Cove claims "recorded" and nothing more.

## Interoperability: Connect's `manifest.json`

Posit Connect deployments carry an open `manifest.json` (written by rsconnect /
rsconnect-python). Reading and writing it means an existing Connect workflow
can deploy to Cove, and a Cove record can be handed to Connect. Mapping, as
the format is publicly documented (confirm field names against Posit's docs
before building):

| `manifest.json` | Deployment record |
|---|---|
| `metadata.appmode`, `metadata.entrypoint` | `kind`, entry point |
| `platform` (R version), `python.version` | `runtime.modules` (the env module that provides it) |
| `packages.<name>` (Source, Repository, description) | `runtime.env_set` + `locks` |
| `python.package_manager.package_file` | `locks."requirements.txt"` |
| `files.<path>.checksum` (md5) | `source.files` (sha256; recompute, do not trust md5) |
| *(none)* | `provenance`, `build`, `access`, `checks` |

The bottom row is the difference: Connect's manifest describes what to
install; the record also says where every binary came from, what came out,
who can reach it and what was checked.

Import path: a bundle with a `manifest.json` → resolve its R/Python versions
and packages to an env module and set (refuse, saying which package, when the
stack lacks one) → deploy → write the record with `"imported_from":
"rsconnect-manifest/1"`.

## Build order

1. Write the record in deploy-app.sh (Rust apps). Smallest surface, fully
   scripted, one person's account.
2. `cove verify show <id>`: print a record with every hash recomputed against
   the live system (source files, artifacts, the running unit's binary).
   Drift is the first useful check.
3. The App tab shows `current`: recorded / drifted / rebuilt identical.
4. Records for `publish_as` and gate changes.
5. `manifest.json` import.
6. `cove verify rebuild`, once one env set builds bit-identical twice.

## Open questions

- **Shiny domains have no deploy event.** `module.r` goes live when app.r's
  hot reload sees it change, so the running code is the working tree, not a
  commit. Options: (a) write a record at each autosave commit that touches
  module code; (b) app.r writes one when it reloads a module. (b) is exact but
  lives in the shared router, which is exec's lane, not Cove's.
- **Shared `src/r` code** runs under every Shiny domain and changes outside the
  domain's repo. The record has to name the platform commit too, or it does
  not say what ran.
- **Who signs a record.** Content-addressed ids prove a record was not edited;
  they do not prove who wrote it. Signing by the deploying account (the vault
  broker already keys identity by peer uid) is the next step.
