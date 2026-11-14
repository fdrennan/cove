# Ideas, expression and licences: what Cove may take from Inlet and RStudio

Written 2026-10-29 for the operator ("what is/isn't available to us through
building cove.ndexr.io, which ideas came from vs inlet.ndexr.io which is more
closely related; do software licenses manage anything related to the essence
of an idea? Where have software licenses failed to keep up with the AI age?").
A plan, not legal advice: every "counsel" line below is a question for a
lawyer, not a conclusion.

## 1. The short answer: licences govern expression, not ideas

A software licence is permission to do things copyright would otherwise
forbid: copy, modify, distribute, perform. Copyright protects **expression**.
It does not protect "any idea, procedure, process, system, method of
operation, concept, principle, or discovery" (US 17 U.S.C. §102(b); the same
idea/expression line runs through Berne, TRIPS art. 9(2) and the EU Software
Directive art. 1(2)). So the AGPL on RStudio reaches RStudio's code, text,
icons and styles. It does not reach the idea of a four-pane IDE, an
environment browser or a console beside an editor.

What *can* touch an idea is a different instrument:

| Instrument | Protects | Reaches an idea? | For Cove |
|---|---|---|---|
| Copyright (and every software licence built on it) | Expression: code, text, images, CSS, docs, translations of them | No (§102(b), *Baker v. Selden* 1879) | The line Cove must not cross: copying |
| Patent | A claimed invention, including a software method | **Yes**, the implemented method, however written | Independent of any licence; a reimplementation gets no patent grant from the AGPL |
| Trademark | Names, logos, trade dress that signal origin | No, but the name an idea is sold under | Why the fork is Inlet and no ndexr host carries a Posit mark |
| Trade secret / contract | Undisclosed information; terms you agreed to | Only what was kept secret, or what you promised | RStudio is public; nothing here unless an agreement with Posit is signed |

Courts have drawn the copyright line for software at function, not at
appearance of effort:

- *Lotus v. Borland* (1st Cir. 1995, affirmed by an equally divided Supreme
  Court 1996): a menu command hierarchy is a "method of operation", not
  protectable expression.
- *Computer Associates v. Altai* (2d Cir. 1992): abstraction, filtration,
  comparison. Strip out ideas, efficiency-dictated choices and public-domain
  elements, then compare what is left.
- *Google v. Oracle* (US 2021): reimplementing the Java API's declaring code
  was fair use. The Court decided fair use and left copyrightability open.

Taken together, behaviour, layout, workflow and interfaces sit mostly on the
idea side. Code, strings, icons, stylesheets and documentation sit on the
expression side.

## 2. What is available to Cove, and what is not

Cove is MIT (LICENSE) and is "not a derivative of RStudio" because of a
measurement, not a belief (CONTEXT.md, 2026-10-28). That claim holds only
while the measurement does.

### Available: ideas and behaviour, from either source
- RStudio's and Inlet's **features as behaviour**: several live sessions per
  person, a project opened from the URL, an Lmod module picker, the console
  showing the loaded module, live module swaps, reproducibility locks,
  pinning a project's module. Cove may reimplement any of these from a
  description of what they do.
- **Layout and workflow conventions**: panes, an environment browser, a
  console beside an editor, a help pane. These are methods of operation
  (*Lotus*), and most are common to every IDE (scènes à faire).
- **Interfaces and protocols**: talking to R through its C API or a kernel,
  reading the same files (`.Rproj`, `renv.lock`), the shape of a command or
  keyboard map as function.
- **Facts**: versions, checksums, licences, which patch changes which file.
  The atlas already shows Inlet's patches *as data*. Pointing to and
  displaying a published AGPL file with its licence is distribution of that
  file under the AGPL, which is allowed, and does not make Cove a derivative.
- **ndexr's own separable work in Inlet**: anything ndexr wrote that contains
  no RStudio expression. Examples: the Inlet mark (the SVG in `InletMark.java`),
  the record pages, the build recipe, `rstudio-session.sh`. As copyright owner,
  ndexr may also license these under MIT. (Counsel: confirm ownership, see §4.)

### Conditional
- **Inlet's patches as a whole.** A patch hunk changes RStudio's code, so the
  patched file is a derivative of RStudio and stays AGPL. Cove may read a
  patch to learn *what* changed and *why*, then implement the behaviour fresh.
  It may not lift the changed lines.
- **Shortcut maps, menu orders, preference names.** Functional and, after
  *Lotus*, probably unprotected. Lists that copy RStudio's exact wording are
  text, though, and text is expression. Take the function and write our own words.
- **Talking to Inlet at run time.** Cove launching Inlet as its own process
  at `/r/<id>/` is an aggregate, not a combined work. This is the "separate
  program" row of licenses.ndexr.io's diagram. It stays that way only while
  Cove does not link or embed Inlet's code.

### Not available
- RStudio's or Inlet's **code, in any language**. A Rust translation of a
  Java or C++ function is still a derivative (the licences page's second
  misconception).
- **Strings, documentation text, CSS, icons, images, fonts** from
  `.rstudio-src/`.
- **Posit's marks**: RStudio, Posit, Shiny, Positron. They may not appear in
  Cove's names, hosts or logos. They may be used only to refer to Posit's own
  products.
- **Positron**, under the Elastic License 2.0: no hosting it for others
  without Posit's agreement. Cove already starts no Positron.

Code may flow the other way. Cove's MIT code can go into Inlet, because MIT
is compatible with AGPLv3, and Inlet then carries it under the AGPL.

## 3. Where the idea/expression line strains with agents building Cove

Cove is written mostly by agents that can read `.rstudio-src/` (CLAUDE.md
tells them to, "for behaviour"). That is the old clean-room problem at a new
speed. A person who reads a function and writes a similar one later is an
argument. An agent that reads a file and produces a near-copy within the same
session is evidence. Three controls, cheapest first:

1. **Re-run the derivation scan on every release.** CONTEXT's verbatim scan
   (27,478 lines and literals, `grep -F` against `.rstudio-src/src`) was run
   once. Make it a script beside `onprem/run.sh`. A release with a non-generic
   match stops, just as a type error stops one. Add a fuzzy pass (normalised
   token windows) on top: verbatim matching misses renamed variables, and
   *Altai* asks about structure, not spelling.
2. **Split reading from writing.** An agent that reads RStudio or Inlet writes
   a behaviour note in prose, with no code, into `docs/behaviour/<feature>.md`.
   The session that implements the feature in Cove gets the note, not
   `.rstudio-src/`. This is the Phoenix BIOS clean room, with two agent
   sessions in place of two teams. The notes double as the record of where
   each idea came from.
3. **Keep provenance.** Run transcripts already land in `host_runs`. Tag the
   sessions that read `.rstudio-src/` so a later question ("where did this
   function come from?") has an answer that is not a guess.

## 4. Where software licences have not kept up with AI

Each gap, and what it means for ndexr.

1. **Training is not an act the licences name.** MIT, GPL and AGPL are
   written around copying, modification, distribution and (AGPL) network
   interaction. Whether training on code is copying at all, or fair use or
   text-and-data mining, is decided by courts and statutes, not by the
   licences.
   - The US is litigating it. The GitHub Copilot case (*Doe v. GitHub*, filed
     2022) lost most of its claims at the district court.
   - The EU allows commercial text-and-data mining unless the rightsholder
     opts out (DSM Directive art. 4).
   - **For ndexr:** a licence cannot stop or condition training, and no
     common open-source licence carries a machine-readable opt-out.
2. **Conditions that cannot survive a model.** MIT's single condition is to
   keep the notice. A model that has learned from a million MIT files keeps
   none of them. When output reproduces a licensed passage, the duty falls on
   whoever uses the output, and that person cannot see where it came from.
   - **For ndexr:** agent-written code needs the same derivation scan as in
     §3, run against the public licences we care about, not only RStudio.
3. **Copyleft triggers do not see "learned from".** The GPL attaches when
   code is copied into a covered work. A model that absorbed GPL code and
   writes fresh code is outside every trigger the GPL defines, unless the
   output reproduces protected expression. Copyleft was designed to stop
   closed derivatives, and it has no clause for this route.
4. **Copyright may not exist in purely machine-written code.**
   - The US Copyright Office registers only human authorship (Copyright
     Office report on copyrightability, January 2025; *Thaler v. Perlmutter*,
     D.C. Cir. 2025).
   - An MIT licence grants permissions under copyright. Where no copyright
     exists, it grants nothing and protects nothing.
   - **For ndexr:** Cove's copyright, and so the meaning of its MIT licence
     and of ndexr's patches inside Inlet, rests on human direction,
     selection and editing. The operator's specs, reviews and edits are that
     human contribution. Keep them recorded (requests, CONTEXT decisions,
     commits).
   - **Counsel:** how much of Cove is protectable, and is that enough to
     relicense ndexr's separable Inlet work under MIT?
5. **"Open" models are mostly not open-source licences.**
   - Popular weight licences restrict use, field or scale (usage policies,
     user-count thresholds), which makes them closer to source-available.
   - The OSI's Open Source AI Definition (1.0, October 2024) asks for weights,
     code and enough information about the data, but not the data itself.
   - **For ndexr:** a model added to the stack is a "Read the grant" row on
     licenses.ndexr.io, not a "Keep notices" row, until checked.
6. **The bill of materials stops at code.** Licences and SBOMs track files
   and packages. A model's weights, training-data provenance and fine-tunes
   need their own records: SPDX 3.0 added AI and Dataset profiles in 2024,
   and the EU AI Act asks general-purpose model providers for a training-data
   summary and a copyright policy from August 2025.
   - **For ndexr:** the build records that already publish recipe, patches
     and log per install are the right place for a model's provenance too, if
     the stack ever ships one.
7. **Who accepts the licence?** Click-through EULAs assume a person agreed.
   An agent that installs, accepts and runs software on someone's behalf
   blurs that.
   - **For ndexr:** keep "an agent accepted a licence" a human step. The
     Posit licence key for Positron is already placed by hand
     (`/etc/cove/positron-license.key`).

## 5. The plan

| # | Step | Where | Done when |
|---|---|---|---|
| 1 | Make the derivation scan a release gate (verbatim + fuzzy), with an allow-list of the generic matches | `onprem/derivation-scan.sh`, called by `onprem/run.sh` | A release with a non-generic match does not publish; the 15 known generic matches pass |
| 2 | Behaviour notes as the only bridge from RStudio/Inlet to Cove | `docs/behaviour/` + a CLAUDE.md line | Each Cove feature that echoes Inlet names its note |
| 3 | Mark sessions that read `.rstudio-src/` | host_runs tagging | The question "who read RStudio before writing X" has a query |
| 4 | List ndexr's separable Inlet work and its authorship | `inlet.ndexr.io/CONTEXT.md` | A file-by-file list counsel can sign off |
| 5 | Add §8 "Ideas, expression and AI" to licenses.ndexr.io, from §1 and §4 of this file | `licenses.ndexr.io/static/licenses.js` | The page shows the instrument table and the seven gaps, checked by `check.js` |
| 6 | Counsel questions, in one place | this file, §6 | Answered or explicitly parked |

Status, 2026-10-29:
- **Step 1 is done**, and went further. `onprem/compliance.py` checks every
  duty above for Cove and Inlet. Its code-side checks hold a Cove release (run.sh
  `licence_gate`), and a daily run leaves evidence in `docs/compliance/` (see
  its README).
- **A gap it found, and closed:** Cove shipped binaries without the notices
  its 469 crates' licences require. They are now generated from Cargo.lock
  (`onprem/third-party-notices.py`), put into every package, served publicly at
  `/view/third-party-notices`, and linked from About.
- Steps 2-4 and 6 are open.
- **Step 5** has moved: licenses.ndexr.io became the compliance hub and reads
  `docs/compliance/latest.json`.

## 6. Questions for counsel
- Human authorship: how much of agent-written Cove is protectable, and does
  ndexr's MIT licence on it mean what we intend?
- Whether ndexr may license its separable Inlet contributions (step 4) under
  MIT alongside the AGPL.
- Patents: does Posit hold any patent a behavioural reimplementation in Cove
  could read on? An AGPL grant covers only Posit's own code, and Cove does
  not use it.
- Trademark: is the nominative use on Inlet's About and help pages ("built
  from the open-source RStudio") within Posit's guidelines?
