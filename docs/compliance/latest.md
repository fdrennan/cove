# Licence compliance - 2026-11-24T06:03:03Z

Cove commit `5611795`. 13 pass, 0 fail, 0 skipped. Written by `onprem/compliance.py`; the duties are explained in `docs/ideas-and-licences.md`. Engineering evidence, not a legal opinion.

| | Check | Tool | Duty | Evidence |
|---|---|---|---|---|
| PASS | `cove.licence` (gate) | Cove | Cove is MIT, and says so where people see it | LICENSE starts 'MIT License'; About states MIT: yes |
| PASS | `cove.dependency-licences` (gate) | Cove | Every crate linked into Cove is usable under a permissive licence (nothing copyleft reaches Cove's code) | 469 of 469 crates resolve to an allowed licence |
| PASS | `cove.notices-current` (gate) | Cove | THIRD-PARTY-NOTICES.txt matches the Cargo.lock being built | dependencies sha256 9e2f1007cc847a95; file current; 410 crates from their own notice files, 59 from canonical texts |
| PASS | `cove.notices-packaged` (gate) | Cove | Every package Cove ships carries LICENSE and the notices | deb, dmg, linux and windows builds copy LICENSE + THIRD-PARTY-NOTICES.txt |
| PASS | `cove.not-derivative` (gate) | Cove | No line of RStudio's or Inlet's code, text or CSS is in Cove (read for behaviour only) | 71542 Cove lines (>= 20 chars) against 408860 distinct RStudio + Inlet lines; 26 allow-listed generic lines; no other match |
| PASS | `platform.no-marks-in-hostnames` (gate) | Cove + Inlet | No Posit mark (RStudio, Posit, Positron, Shiny) in any served ndexr hostname | 623 vhosts, none names a Posit mark |
| PASS | `positron.not-hosted` (gate) | Positron | Positron (Elastic License 2.0) is not offered to others: no enabled vhost routes its sessions | no enabled vhost includes positron_sessions.conf |
| PASS | `inlet.licence-text` | Inlet | The installed Inlet carries the AGPLv3 text and RStudio's NOTICE | COPYING is AGPLv3: yes; NOTICE 865416 bytes |
| PASS | `inlet.source-offer` | Inlet | AGPL §13: every session links to the corresponding source of the running version, and the link answers | https://ndexr-stack-public.s3.us-east-2.amazonaws.com/stack/records/Inlet/2026.04.0%2B526-foss-2024a-R-4.6.1/index.html -> HTTP 200; installed build 20261108.001341 listed: yes |
| PASS | `inlet.records-match-install` | Inlet | The published patches and recipe are byte-for-byte the ones the running Inlet was built with | 24 files (17+ patches and the recipe) of build 20261108.001341 match the public records |
| PASS | `inlet.upstream-source` | Inlet | RStudio's release tarball is public, and its sha256 is the one the recipe pins | public tarball HTTP 200, sha256 912df414d1ac94e5 vs recipe 912df414d1ac94e5 |
| PASS | `inlet.no-posit-marks-as-name` | Inlet | The running Inlet does not present itself with RStudio's name (patches 0014-0018) | the compiled UI names itself Inlet (no 'About/Quit/Restart RStudio') |
| PASS | `cove.notices-served` | Cove | The running Cove serves its third-party notices publicly (/view/third-party-notices) | HTTP 200; carries this Cargo.lock's notices: yes |
