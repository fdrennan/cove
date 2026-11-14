# Licence compliance evidence: Cove and Inlet

This folder is the proof, kept as it accumulates. `onprem/compliance.py`
writes it. The duties it tests are explained in `docs/ideas-and-licences.md`.
licenses.ndexr.io, the hub for every outward-facing ndexr product, reads
`latest.json` from here.

| File | What it is |
|---|---|
| `latest.md` | The last full run, one row per duty: PASS / FAIL / skip, with what was seen |
| `latest.json` | The same run as data; each check has `check`, `product`, `status`, `summary`, `details` |
| `history.tsv` | One line per run, never rewritten: when, which Cove commit, how many passed |
| `derivation-allow.txt` | Lines Cove shares with RStudio that were reviewed and found generic (each with its reason) |

## When it runs
- **Every Cove build** (`onprem/run.sh`, `licence_gate`): the code-side checks
  run in about 10 s. A failure holds the release, the same way a failed atlas
  test does: the running build stays and nobody moves to the failing one. These
  runs report to `~/.local/state/cove-onprem/compliance/`, not here, so builds
  do not flood the history.
- **Daily at 06:00 UTC** (`cove-compliance.timer`): every check, including
  Inlet's live ones. The result is committed here by path.
- **By hand**: `onprem/compliance.sh` (all checks) or `onprem/compliance.sh --gate`.

## The duties, and the check that proves each

| Duty | Product | Check | Gate |
|---|---|---|---|
| Cove is MIT and says so | Cove | `cove.licence` | yes |
| No crate linked into Cove needs a copyleft licence | Cove | `cove.dependency-licences` | yes |
| The notices of every crate travel with Cove (MIT, BSD, Apache, Zlib, OFL...) | Cove | `cove.notices-current`, `cove.notices-packaged`, `cove.notices-served` | yes, yes, daily |
| No RStudio or Inlet code, text or CSS is in Cove | Cove | `cove.not-derivative` | yes |
| No Posit mark in a served ndexr hostname | platform | `platform.no-marks-in-hostnames` | yes |
| Positron (Elastic 2.0) is not offered to others | Positron | `positron.not-hosted` | yes |
| Inlet carries the AGPLv3 text and RStudio's NOTICE | Inlet | `inlet.licence-text` | daily |
| AGPL §13: every Inlet session links to its corresponding source, and the link answers | Inlet | `inlet.source-offer` | daily |
| The published source is exactly what the running Inlet was built from | Inlet | `inlet.records-match-install` | daily |
| RStudio's release is public, at the sha256 the recipe pins | Inlet | `inlet.upstream-source` | daily |
| Inlet does not present itself under RStudio's name | Inlet | `inlet.no-posit-marks-as-name` | daily |

## When a check fails
- **Fix the cause.** Never relax the check to get past it.
- **`cove.notices-current` failed:** run `onprem/compliance.sh --notices`. `run.sh` does
  this by itself whenever `Cargo.lock` changes.
- **`cove.dependency-licences` failed:** a new crate offers no permissive licence.
  Replace it, or bring the question to the operator. Do not add its licence to
  `PREFER` in `third-party-notices.py` unless the operator decides to accept that
  licence's duties for Cove.
- **`cove.not-derivative` failed:** a Cove line matches RStudio or Inlet.
  - If it is generic (a language idiom, R's own API, boilerplate), add it to
    `derivation-allow.txt` with its reason.
  - Otherwise rewrite it from a behaviour note, not from their source (see
    `docs/ideas-and-licences.md` §3).
- **Inlet checks failed:** the fix is in ndexr.ndexr.io's `i/Inlet/` recipe
  and the stack publish. File it there if you are not exec.

## What this does not prove
These checks show that each duty, as written, is met by what is on disk and
being served. They are not a legal opinion. The open legal questions are in
`docs/ideas-and-licences.md` §6.
