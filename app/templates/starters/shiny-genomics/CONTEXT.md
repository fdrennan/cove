# Expression Explorer · __HOST__
A Shiny site, started from Cove's template: a differential-expression study
of 2,000 genes, simulated so the truth is known and every discovery can be
checked against it. Change first: `simulate_study()` - return a real
expression matrix in its place (genes x samples, control columns first) and
both views analyse it.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Two branches, each a
  `ui_`/`server_` pair:
  - `volcano` - a Welch t-test per gene, Benjamini-Hochberg false discovery
    rate control across all of them, and the volcano plot: effect against
    evidence. Click a gene to see its samples.
  - `overview` - the MA plot (change against average expression) and a
    heatmap of the strongest genes across every sample
  Each visual fills the window under the site's header, its controls in a
  glass card that folds to its title (a `<details>`; on a phone it starts
  folded). Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

The data: each gene has its own baseline (log2 expression, 2 to 14) and its
own noise (gene-to-gene spread, as in real arrays); a chosen share of genes
is truly changed in the treated group by about the chosen log2 fold change,
up or down. Same settings and same dataset number, same data.

## Built on
Base R and Shiny only, from the stack's R; no dataset, key or database.
Welch, B. L. (1947), "The generalization of 'Student's' problem when several
different population variances are involved", Biometrika 34(1-2).
Benjamini, Y. and Hochberg, Y. (1995), "Controlling the False Discovery Rate",
J. R. Stat. Soc. B 57(1).

## Auth
Login-gated.

## Open
- (none logged)
