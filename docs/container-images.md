# Project container images: Cove ↔ ndexr contract

Every Cove project can be turned into a versioned container image that its
owner can pull and run, stored in Amazon ECR and managed entirely by ndexr
(operator, 2026-11-01). This file is the contract between the two sides:

- **ndexr** (the CLI, ndexr.ndexr.io's crate) inspects the project, builds,
  validates, publishes to ECR, keeps every version, and hands out
  short-lived pull access. One implementation for every project type.
- **Cove** shows it: Settings › Container image, per project. It never builds
  or pushes anything itself. It calls `ndexr image …` (app/src/host.rs
  `image_at`) as the platform account, behind the same ownership check as
  Versions (multiuser.rs).

## What a project gives the builder

Every project has the same contract in `.cove/` (dotcove.rs):

| file | used for |
|---|---|
| `.cove/app.env` | `APP_LANGUAGE` (r, python, julia, node, rust), `APP_KIND` (module, port), `APP_PORT`, `APP_HEALTH` |
| `.cove/build.sh` | run once in the image build, as Build runs it |
| `.cove/run.sh` | the image's entry point, as Up runs it (port apps) |
| `.cove/<language>/env.sh` | the modules it loads (`ml ...`) - read to record them |
| `.cove/app.vars` | **never** copied into an image: it holds the person's variables, some secret. They are passed at run time (`--env-file`) |

The builder reads these; it does not guess a runtime from file extensions.

## Commands Cove calls

All take the project's folder and `--owner <ndexr username>`, and print
JSON with `--json`. A CLI without `image` makes Cove say so: nothing is faked.

### `ndexr image status <dir> --owner <u> --json`

```json
{
  "repository": "<acct>.dkr.ecr.us-east-2.amazonaws.com/cove/<owner>/<host>",
  "building": false,
  "versions": [
    {
      "tag": "20261101T221500Z-1a2b3c4",
      "digest": "sha256:…",
      "built_at": "2026-11-01T22:15:00Z",
      "source": "1a2b3c4",
      "size_bytes": 412345678,
      "validated": true
    }
  ],
  "changed_since": true,
  "needs": {
    "stack": true,
    "stack_revision": "37",
    "triple": "x86_64-pc-linux-gnu",
    "modules": ["env/r4.6_py3.13-release"],
    "platform_r": true,
    "gpu": false,
    "services": [],
    "arch": "x86_64",
    "binds": ["/opt/ndexr/stack"]
  },
  "port": "8080",
  "health": "/"
}
```

- `versions`: newest first, every version ever published (the repository is
  IMMUTABLE; nothing is overwritten or retagged). No `latest` tag exists.
- `changed_since`: the project's files differ from the newest version's
  `source`, so Rebuild would publish something new.
- `needs`: **measured at build time, not assumed from the type.**
  - `stack`: true when anything shipped names `/opt/ndexr/stack` (the
    `readelf` interpreter/RPATH audit of `hpc.ndexr.io/scripts/lib/elf_audit.sh`
    over every ELF file, plus any `ml` in env.sh). A binary built with the
    stack's toolchain runs on the stack's own loader, so most images need it.
  - `platform_r`: an R module project imports the platform's `src/r`
    (shell, inputs). The image carries a copy at the revision built from.
  - `gpu`, `services`: drivers or outside services the app needs (a database,
    an API key). Recorded from what the build saw; listed to the person.

### `ndexr image build <dir> --owner <u>`

Runs to the end, printing progress (Cove keeps it as a job and shows the
log). Steps:

1. Read `.cove/`; refuse a project without `app.env`.
2. Build with the stack mounted read-only at `/opt/ndexr/stack`
   (`podman build -v /opt/ndexr/stack:/opt/ndexr/stack:ro`). The image holds
   the project (less `.git`, `.cove/run`, `app.vars`, `target/`,
   `node_modules/` caches), its build output, and for `APP_KIND=module` a
   runner plus the platform's `src/r`. It never holds the stack.
3. Audit what ships (above) and write `needs`.
4. Validate: run it with the stack bound, wait for `APP_HEALTH` on
   `APP_PORT` (a module app: the runner answering). An image that fails is
   not pushed; its log says why.
5. Push to `cove/<owner>/<host>` (created IMMUTABLE with scan-on-push by
   `ndexr aws ecr create`), tag `<UTC stamp>-<source>`, and record the digest.
6. Labels on every image:
   `org.opencontainers.image.{created,revision,source,title}` and
   `io.ndexr.{owner,host,language,kind,port,health,stack.revision,stack.triple,modules,needs.stack,needs.platform_r}`.

Rebuild is the same command; it always makes a new version.

### `ndexr image pull-login <dir> --owner <u> --json`

```json
{"registry": "<acct>.dkr.ecr.us-east-2.amazonaws.com", "username": "AWS",
 "password": "…", "expires_at": "2026-11-02T10:15:00Z",
 "repository": "<registry>/cove/<owner>/<host>"}
```

A registry login made from credentials that can pull **only**
`cove/<owner>/*`: a federated STS session (`sts:GetFederationToken`) whose
session policy names those repositories, turned into an ECR password. Never the platform's own keys and never a password that
outlives `expires_at`. One person cannot pull another's images, because the
credentials do not allow it, not because Cove hides a button.

## What the person is told

Cove shows exactly what `needs` says, never "portable" on its own:

- `needs.stack`: the image runs only where the ndexr stack is mounted at
  `/opt/ndexr/stack` (CVMFS `stack.ndexr.io`, at least the recorded revision).
  That is on ndexr, in an account cell, or on any Linux host with the CVMFS
  client and that repository mounted.
- `needs.platform_r`: it carries ndexr's own R code at the recorded revision.
- `gpu`, `services`: named, each one.
- Only when all of these are empty does Cove say it runs on its own.

Run commands Cove prints (with the digest, never a tag):

```bash
# Apptainer (shares the host's network: the app is on APP_PORT)
export APPTAINER_DOCKER_USERNAME=AWS APPTAINER_DOCKER_PASSWORD='<from pull-login>'
PORT=<port> apptainer run --writable-tmpfs --bind <needs.binds, comma-separated> docker://<repository>@<digest>

# Podman or Docker
echo '<password>' | podman login --username AWS --password-stdin <registry>
podman run --rm --network host -e PORT=<port> -v <bind>:<bind>:ro … <repository>@<digest>
```

`needs.binds` is every host folder the image needed, read-only, to answer its
health check, measured by ndexr starting it with nothing bound, then the
stack, then everything its build touched (`/opt/ndexr/site-library` when the
env/ modules point there). The first that answered is the list. An older
ndexr without `binds`: the stack alone when `needs.stack`.

The host network, not `-p <port>:<port>`: the starters listen on 127.0.0.1,
which a published port never reaches (measured 2026-11-01, game.fdrennan:
`-p` got no answer, `--network host -e PORT` answered in 4 s). `--writable-tmpfs`
because an image is read-only under Apptainer and apps write beside
themselves (scores, caches).
An image is built for one architecture (`needs.arch`); an arm64 host needs an
arm64 build.

## Not decided here

- Where builds run. Podman on the platform host competes with everything
  else for memory (earlyoom has killed Cove's own builds, referral #717); a
  build box or a cell may be the right place. ndexr's call.
- Deleting versions. Every version is kept; a person removing one is a later
  command (`ndexr image delete`), never a lifecycle rule that quietly expires
  something someone may be running.
