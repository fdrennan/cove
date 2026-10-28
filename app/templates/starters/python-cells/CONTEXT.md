# Cell Atlas · __HOST__
A Python app, started from Cove's template: a single-cell RNA-seq analysis,
each step visible and tunable. 3,000 droplets x 2,000 genes are simulated on
the server from a seed - negative-binomial counts for a blood-forming lineage
(stem cells, progenitors, early and late erythroid, monocytes) and a rare
plasmacytoid dendritic cell population, with varying library sizes, damaged
cells, empty droplets and doublets - so the truth is known and every step can
be checked against it. The pipeline: quality control (counts, genes,
mitochondrial share), normalisation and log1p, highly variable genes, PCA
with its elbow, a kNN graph, a 2-D embedding, clustering, and marker genes
per cluster by Wilcoxon rank-sum test, drawn as a dot plot. Click a cluster
or type a gene to colour the cells by it; the adjusted Rand index says how
well the clusters recover the simulated types. Change first: `simulate()` in
app.py - return your own integer counts (cells x genes) and the same `truth`
shape, and every step analyses them.

## Files
- app.py - FastAPI, the simulation and the pipeline, one cached function per
  stage (qc, normalise, hvg, pca, knn, embed, cluster, ranks, wilcoxon), each
  keyed on exactly the settings it depends on, so a clustering change does not
  redo the embedding. GET /api/menu (defaults, bounds, gene list, true
  categories); POST /api/run (the `Run` settings -> per-step results, ARI,
  clusters-against-truth table, markers, dot plot, per-step ms); POST
  /api/gene (the same settings plus `gene` -> its log-normalised value per
  kept cell)
- static/index.html, static/style.css, static/app.js - the page: the
  embedding on a canvas, the step charts as SVG, the controls in a glass panel
  whose steps fold to their titles (folded on a phone)
- run.sh - loads Python from the ndexr stack and serves with uvicorn
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - byte-compile and import-check app.py;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Python from the ndexr stack

The embedding: "Graph layout" (default) is a neighbour embedding optimised
the way UMAP does it - attraction along kNN edges with t-SNE's heavy-tailed
kernel, repulsion from sampled cells - written in numpy in `graph_layout`. "t-SNE" is scikit-learn's Barnes-Hut
t-SNE, the reference: on 2,755 cells it took 41 s against the graph layout's
3.4 s on the same (heavily loaded) machine, 2026-10-26. Neither umap-learn nor leidenalg/igraph is in the stack's Python set,
so clustering is Ward linkage constrained to the kNN graph (default),
spectral clustering on the kNN graph, or k-means on the PCs, with the number
of clusters chosen (at most eight, the categorical palette's size).

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
Python, fastapi, uvicorn, numpy, scipy and scikit-learn come from the
stack's env set (env/Python/3.13.15, else env/r4.6_py3.13-release); nothing is
pip-installed.

## Built on
Luecken, M. D. and Theis, F. J. (2019), "Current best practices in
single-cell RNA-seq analysis: a tutorial", Molecular Systems Biology 15(6).
Grün, D., Kester, L. and van Oudenaarden, A. (2014), "Validation of noise
models for single-cell transcriptomics", Nature Methods 11(6).
Satija, R. et al. (2015), "Spatial reconstruction of single-cell gene
expression data", Nature Biotechnology 33(5) - the highly-variable-gene
dispersion.
van der Maaten, L. and Hinton, G. (2008), "Visualizing Data using t-SNE",
J. Machine Learning Research 9.
Böhm, J. N., Berens, P. and Kobak, D. (2022), "Attraction-Repulsion Spectrum
in Neighbor Embeddings", J. Machine Learning Research 23.
Kobak, D. and Linderman, G. C. (2021), "Initialization is critical for
preserving global data structure in both t-SNE and UMAP", Nature
Biotechnology 39.
Ward, J. H. (1963), "Hierarchical Grouping to Optimize an Objective Function",
J. American Statistical Association 58(301).
Mann, H. B. and Whitney, D. R. (1947), "On a Test of Whether one of Two Random
Variables is Stochastically Larger than the Other", Annals of Mathematical
Statistics 18(1).
Benjamini, Y. and Hochberg, Y. (1995), "Controlling the False Discovery Rate",
J. R. Stat. Soc. B 57(1).
Hubert, L. and Arabie, P. (1985), "Comparing partitions", Journal of
Classification 2 - the adjusted Rand index.
Pedregosa, F. et al. (2011), "Scikit-learn: Machine Learning in Python",
J. Machine Learning Research 12.

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
