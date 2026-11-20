# Nature's Laboratory · __HOST__
A Shiny site, started from Cove's template, after Veera Rajagopal's essay
"Nature's laboratory" (Works in Progress, 27 November 2025): people born with a
gene switched off are experiments no drug developer could run, and their
genomes say what a drug against that gene will do - its effect, the disease it
prevents and the harm it causes - before the first trial. Every cohort is
simulated, so the truth is known and every finding can be checked against it.
Change first: `TARGETS` - add a gene with its allele frequencies by ancestry and
its true effects, and every branch reads it.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Four branches, in the
  essay's order, each a `ui_`/`server_` pair:
  - `trial` - a natural trial: pick a target (PCSK9, CCR5, HSD17B13, APOC3,
    HAO1, DGAT1, GPR75, ANGPTL7), sequence a cohort, compare people with 0, 1
    or 2 broken copies - the biomarker by copies, the disease's odds ratio, and
    a PheWAS across 29 traits that shows the side effects a trial finds too late
    (DGAT1's albumin and triglycerides)
  - `pipeline` - nine in ten drugs fail; a portfolio run through the BIO
    phase-success rates, with genetically supported targets approved a chosen
    multiple as often (2.6 by default), the gain placed in phases II and III
  - `ancestry` - power to find a variant's effect against people sequenced, one
    curve per ancestry: PCSK9 takes ~850 people of African ancestry and ~17,000
    Europeans at p < 5e-8
  - `knockouts` - genes with a human knockout against cohort size, with
    first-cousin marriage and community endogamy as inbreeding F; the essay's
    Pakistan cohorts drawn as points
  Each visual fills the window under the site's header, its controls in a glass
  card that folds to its title (a `<details>`; on a phone it starts folded).
  Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

Logging: r/log.r (log$log_info, log_fn in every ui_/server_), one format across languages; LOG_LEVEL=DEBUG for more.

## The models
- Genotypes under inbreeding: a knockout's odds are q^2 + F q (1 - q); first
  cousins' children have F = 1/16.
- `GENE_Q`, 18,000 genes' pooled loss-of-function frequencies, is fitted to the
  essay's gene counts: at 60% first-cousin marriage plus 0.01 endogamy (F =
  0.0475) it gives 1,370 genes knocked out in 10,000 people (reported 1,317)
  and 5,596 in 80,000 (reported more than 5,000); matching the 80,000 takes
  35 million people with unrelated parents.
- Disease by logistic risk on copies carried; biomarkers normal with the
  carriers' shift; PheWAS by closed-form least squares, Bonferroni across traits.
- Pipeline: phase I 52%, II 29%, III 58%, review 91% (overall 8%); supported
  targets get one odds multiplier on phases II and III, solved so overall
  approval is the chosen multiple.
Allele frequencies and effects are the orders of magnitude the cited studies
report, not their exact estimates.

## Built on
Base R and Shiny only, from the stack's R; no dataset, key or database.
Rajagopal, V. (2025), "Nature's laboratory", Works in Progress.
Cohen, J. C. et al. (2006), "Sequence variations in PCSK9, low LDL, and
protection against coronary heart disease", NEJM 354. Samson, M. et al. (1996),
CCR5, Nature 382. Abul-Husn, N. S. et al. (2018), HSD17B13, NEJM 378.
Saleheen, D. et al. (2017), "Human knockouts and phenotypic analysis in a cohort
with a high rate of consanguinity", Nature 544. King, E. A. et al. (2019) and
Minikel, E. V. et al. (2024), genetic support and drug approval, PLoS Genet 15 /
Nature 629. BIO, Informa and QLS (2021), "Clinical Development Success Rates
2011-2020".

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public).

## Open
- The essay's "over 11 million" outbred people counts knockout genotypes. Under
  this independent-gene model it needs mean F near 0.10 alongside the gene
  counts, above what 60% cousin marriage gives, so the app fits the gene counts
  and computes the outbred equivalent in genes.
