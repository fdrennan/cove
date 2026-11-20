"""Cell Atlas: a single-cell RNA-seq analysis, one visible step at a time.

The cells are simulated here, so the truth is known and every step can be
checked against it: a blood-forming lineage (stem cells, progenitors, then an
erythroid and a monocyte branch) plus a rare plasmacytoid dendritic cell
population, with damaged cells, empty droplets and doublets mixed in.

The pipeline is the one in Luecken & Theis (2019): quality control, library
size normalisation + log1p, highly variable genes, PCA, a k-nearest-neighbour
graph, a 2-D embedding, clustering, and marker genes by Wilcoxon rank-sum test.
POST /api/run runs it and answers with what each step saw; POST /api/gene
answers one gene's expression per kept cell. Every stage is cached on exactly
the settings it depends on, so moving a clustering slider does not redo the
embedding.
"""
import os
import time
from functools import lru_cache
from pathlib import Path

# A 3,000 x 500 matrix gains nothing from a BLAS thread per core, and on a
# shared, busy machine those threads queue behind each other: randomized PCA
# took 49 s with the default and well under one with four. Set before numpy loads.
for var in ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS"):
    os.environ.setdefault(var, "4")

import numpy as np
from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field
from scipy import sparse
from scipy.stats import false_discovery_control, norm
from sklearn.cluster import AgglomerativeClustering, KMeans, SpectralClustering
from sklearn.decomposition import PCA
from sklearn.manifold import TSNE
from sklearn.metrics import adjusted_rand_score
from sklearn.neighbors import kneighbors_graph

from log import log

HERE = Path(__file__).parent
N_CELLS = 3000
N_PCS_MAX = 50

# ---- the biology being simulated ------------------------------------------------------
#
# Each cell type is an expression programme: log fold changes over the gene
# baseline. Cells on the lineage sit between their own type and a neighbour on
# the path, so clusters touch the way real differentiation does; the rare type
# stands apart. Named markers are real human genes for those cell types.

TYPES = ["HSC", "Progenitor", "Early erythroid", "Late erythroid", "Monocyte", "pDC"]
SHARE = np.array([0.14, 0.22, 0.17, 0.17, 0.275, 0.025])      # pDC is the rare population
PREV = {1: [0], 2: [1], 3: [2], 4: [1]}                         # lineage: HSC > Prog > Ery E > Ery L
NEXT = {0: [1], 1: [2, 4], 2: [3]}                              #                  Prog > Monocyte

# (gene, {type: log fold change}) - several genes mark more than one stage, as in life
NAMED = [
    ("CD34", {0: 3.0, 1: 2.2}), ("CRHBP", {0: 3.2}), ("HLF", {0: 2.8}), ("AVP", {0: 3.4}), ("MECOM", {0: 2.6}),
    ("SPINK2", {1: 3.0, 0: 1.0}), ("CDK6", {1: 2.6}), ("FLT3", {1: 2.8}), ("MKI67", {1: 2.4, 2: 1.2}), ("TOP2A", {1: 2.4, 2: 1.0}),
    ("GATA1", {2: 3.0, 3: 1.6}), ("KLF1", {2: 3.0, 3: 1.4}), ("TFRC", {2: 2.8, 3: 1.8}), ("CA1", {2: 2.6, 3: 2.0}), ("APOC1", {2: 2.4}),
    ("HBB", {2: 1.5, 3: 4.2}), ("HBA1", {2: 1.2, 3: 4.0}), ("ALAS2", {3: 3.2}), ("GYPA", {3: 3.0}), ("SLC4A1", {3: 2.8}),
    ("LYZ", {4: 4.0, 1: 0.8}), ("CD14", {4: 3.0}), ("S100A8", {4: 3.6}), ("S100A9", {4: 3.6}), ("CSF1R", {4: 2.6}), ("FCN1", {4: 2.8}),
    ("LILRA4", {5: 3.4}), ("IRF7", {5: 2.8}), ("CLEC4C", {5: 3.2}), ("IL3RA", {5: 2.8}), ("TCF4", {5: 2.6}), ("GZMB", {5: 2.6}),
]
MITO = ["MT-ND1", "MT-ND2", "MT-CO1", "MT-CO2", "MT-ATP8", "MT-ATP6", "MT-CO3", "MT-ND3", "MT-ND4L", "MT-ND4",
        "MT-ND5", "MT-ND6", "MT-CYB"]
HOUSEKEEPING = ["ACTB", "GAPDH", "B2M", "MALAT1", "TMSB4X", "EEF1A1", "RPL13", "RPS27"]
N_GENES = 2000
GENES = MITO + HOUSEKEEPING + [g for g, _ in NAMED]
GENES += [f"G{i:04d}" for i in range(N_GENES - len(GENES))]
GENE_INDEX = {g: i for i, g in enumerate(GENES)}
IS_MITO = np.array([g.startswith("MT-") for g in GENES])

# what each true category is called in the payload: the six types, then the two artefacts
TRUTH = TYPES + ["doublet", "damaged / empty"]


def programmes(rng) -> tuple[np.ndarray, list[list[int]]]:
    """Type x gene log fold changes, and the genes planted as each type's markers.

    Beyond the named markers: 15 unnamed genes per type, three shared lineage
    modules (stemness, erythroid, myeloid) and a block of genes the late
    erythroid cells switch off, so the structure is wider than a marker list.
    """
    lfc = np.zeros((len(TYPES), N_GENES))
    for g, effects in NAMED:
        for t, v in effects.items():
            lfc[t, GENE_INDEX[g]] = v
    free = list(rng.permutation(np.arange(len(MITO) + len(HOUSEKEEPING) + len(NAMED), N_GENES)))
    take = lambda n: [free.pop() for _ in range(n)]
    for t in range(len(TYPES)):
        lfc[t, take(15)] = rng.uniform(1.2, 2.6, 15)
    stem, ery, mye, off = take(20), take(25), take(20), take(30)
    lfc[0, stem] += 2.0; lfc[1, stem] += 1.2
    lfc[2, ery] += 1.5; lfc[3, ery] += 2.4
    lfc[1, mye] += 0.6; lfc[4, mye] += 2.0
    lfc[3, off] -= 1.8
    planted = [[int(i) for i in np.argsort(-lfc[t]) if lfc[t, i] >= 1.2 and lfc[t, i] - np.delete(lfc[:, i], t).max() >= 0.8]
               for t in range(len(TYPES))]
    return lfc, planted


@lru_cache(maxsize=3)
def simulate(seed: int) -> dict:
    """Raw UMI counts, cells x genes, drawn negative-binomial (gamma-Poisson).

    The mix: 90% healthy singlets, 5% doublets (two cells in one droplet: more
    counts, two programmes), 3% damaged cells (cytoplasmic RNA lost, so few
    counts and a high mitochondrial share) and 2% empty droplets (ambient RNA
    only). Library sizes vary log-normally, as capture efficiency does.
    """
    rng = np.random.default_rng(seed)
    lfc, planted = programmes(rng)
    lfc = lfc.astype(np.float32)                                  # float32 throughout: half the memory traffic
    n_dbl, n_dmg, n_empty = int(0.05 * N_CELLS), int(0.03 * N_CELLS), int(0.02 * N_CELLS)
    n_single = N_CELLS - n_dbl - n_dmg - n_empty

    # gene baselines: most genes low, a few high; marker and module genes sit low so they read as on/off
    base = rng.normal(0, 1.3, N_GENES)
    programmed = np.abs(lfc).max(0) > 0
    base[programmed] = rng.normal(-0.6, 0.6, programmed.sum())
    base[[GENE_INDEX[g] for g in HOUSEKEEPING]] = rng.uniform(3.0, 4.5, len(HOUSEKEEPING))
    base[IS_MITO] = rng.normal(0, 0.4, IS_MITO.sum())
    base = base.astype(np.float32)
    phi = np.exp(rng.normal(np.log(0.25), 0.4, N_GENES)).astype(np.float32)   # per-gene overdispersion

    def cells(n):
        """n singlets: type, and log fold changes blended towards a lineage neighbour."""
        t = rng.choice(len(TYPES), n, p=SHARE / SHARE.sum())
        offset = rng.uniform(-0.5, 0.5, n)                       # where in its stage each cell is
        w = 0.45 * (2 * np.abs(offset)) ** 2                      # 0 mid-stage, 0.45 at the edge
        nb = t.copy()                                             # no neighbour on that side: blend with itself
        pick = rng.random(n)
        for links, side in ((PREV, offset < 0), (NEXT, offset >= 0)):
            for src, dst in links.items():
                m = side & (t == src)
                nb[m] = np.asarray(dst)[(pick[m] * len(dst)).astype(int)]
        return t, (1 - w)[:, None] * lfc[t] + w[:, None] * lfc[nb]

    def expected(prof, lib, mito_frac):
        """Mean counts: library split between nuclear genes (by programme) and mitochondrial ones."""
        logit = base + prof
        logit[:, IS_MITO] = -np.inf
        p = np.exp(logit - logit.max(1, keepdims=True))
        p /= p.sum(1, keepdims=True)
        q = np.where(IS_MITO, np.exp(base), 0)
        q /= q.sum()
        lib, f = lib.astype(np.float32)[:, None], mito_frac.astype(np.float32)[:, None]
        return lib * ((1 - f) * p + f * q[None, :])

    t1, p1 = cells(n_single)
    mu = [expected(p1, rng.lognormal(np.log(4000), 0.35, n_single), rng.beta(3, 55, n_single))]
    # doublets: two cells of different types sharing a droplet
    ta, pa = cells(n_dbl)
    tb, pb = cells(n_dbl)
    pb[ta == tb] = lfc[(ta[ta == tb] + 1) % len(TYPES)]
    mu.append(expected(np.log((np.exp(pa) + np.exp(pb)) / 2), rng.lognormal(np.log(7600), 0.3, n_dbl),
                       rng.beta(3, 55, n_dbl)))
    _, pd_ = cells(n_dmg)
    mu.append(expected(pd_, rng.lognormal(np.log(1300), 0.45, n_dmg), rng.uniform(0.18, 0.6, n_dmg)))
    ambient = np.log(np.exp(lfc).T @ SHARE)[None, :].repeat(n_empty, 0)
    mu.append(expected(ambient, rng.lognormal(np.log(260), 0.4, n_empty), rng.beta(3, 40, n_empty)))
    mu = np.vstack(mu)

    # gamma-Poisson: each cell's gene rate wobbles around its mean (variance mu + phi mu^2)
    rate = mu * rng.standard_gamma(np.broadcast_to(1 / phi, mu.shape), dtype=np.float32) * phi
    counts = rng.poisson(rate).astype(np.int32)
    truth = np.concatenate([t1, np.full(n_dbl, 6), np.full(n_dmg + n_empty, 7)])
    order = rng.permutation(N_CELLS)                              # droplets come in no order
    counts, truth = counts[order], truth[order]
    total = counts.sum(1)
    return {
        "counts": counts, "truth": truth, "planted": planted,
        "n_counts": total, "n_genes": (counts > 0).sum(1),
        "pct_mito": 100 * counts[:, IS_MITO].sum(1) / np.maximum(total, 1),
    }


# ---- the pipeline, stage by stage ------------------------------------------------------
#
# Each stage takes the settings it depends on (and its upstream's, as a tuple)
# and is cached on them; four entries per stage bounds memory to a few
# hundred MB at 3,000 x 2,000.

@lru_cache(maxsize=4)
def qc(seed, min_counts, max_counts, min_genes, max_mito) -> np.ndarray:
    """Cells kept: enough counts and genes (not empty/damaged), not too many
    counts (a crude doublet guard), mitochondrial share under the ceiling."""
    s = simulate(seed)
    return np.flatnonzero((s["n_counts"] >= min_counts) & (s["n_counts"] <= max_counts) &
                          (s["n_genes"] >= min_genes) & (s["pct_mito"] <= max_mito))


@lru_cache(maxsize=4)
def normalise(seed, qck, target) -> np.ndarray:
    """Scale every cell to the same total (removes library size), then log1p
    (tames the mean-variance relationship of counts, so a highly expressed
    gene does not dominate every distance)."""
    x = simulate(seed)["counts"][qc(seed, *qck)].astype(np.float32)
    x *= target / x.sum(1, keepdims=True)
    return np.log1p(x)


@lru_cache(maxsize=4)
def hvg(seed, qck, target, n_top) -> dict:
    """Highly variable genes, the Seurat way (Satija et al. 2015): dispersion
    (variance / mean) of each gene, z-scored within 20 bins of mean
    expression, so a gene counts as variable relative to genes as abundant."""
    x = np.expm1(normalise(seed, qck, target))
    mean = x.mean(0)
    var = x.var(0, ddof=1)
    expressed = mean > 1e-6
    disp = np.zeros_like(mean)
    disp[expressed] = np.log(var[expressed] / mean[expressed])
    logm = np.log1p(mean)
    edges = np.quantile(logm[expressed], np.linspace(0, 1, 21))
    bins = np.clip(np.searchsorted(edges, logm, side="right") - 1, 0, 19)
    z = np.full_like(mean, -np.inf)
    for b in range(20):
        m = expressed & (bins == b)
        if m.sum() > 1:
            sd = disp[m].std(ddof=1)
            z[m] = (disp[m] - disp[m].mean()) / (sd if sd > 0 else 1)
    top = np.argsort(-z)[:n_top]
    return {"log_mean": np.log10(mean + 1e-4), "z": np.where(np.isfinite(z), z, np.nan), "top": np.sort(top)}


@lru_cache(maxsize=4)
def pca(seed, qck, target, n_top) -> tuple[np.ndarray, np.ndarray]:
    """PCA on the variable genes, each scaled to unit variance (clipped at 10
    so a handful of extreme cells cannot own a component)."""
    x = normalise(seed, qck, target)[:, hvg(seed, qck, target, n_top)["top"]]
    sd = x.std(0)
    z = np.clip((x - x.mean(0)) / np.where(sd > 0, sd, 1), -10, 10)
    p = PCA(n_components=N_PCS_MAX, svd_solver="randomized", random_state=0)
    return p.fit_transform(z).astype(np.float32), p.explained_variance_ratio_


@lru_cache(maxsize=4)
def knn(seed, pk, n_pcs, k) -> sparse.csr_matrix:
    """The k-nearest-neighbour graph in PCA space, made symmetric: the
    structure both graph clustering and the graph embedding work on."""
    g = kneighbors_graph(pca(seed, *pk)[0][:, :n_pcs], k, mode="connectivity", include_self=False)
    return ((g + g.T) > 0).astype(np.float32).tocsr()


def graph_layout(g: sparse.csr_matrix, init: np.ndarray, seed: int, epochs=250, negatives=10, repulsion=2.5) -> np.ndarray:
    """Neighbour embedding on the kNN graph, the way UMAP optimises: pull every
    edge together, push randomly sampled pairs apart, with the heavy-tailed
    t-SNE kernel 1/(1+d^2). Böhm, Berens & Kobak (2022) place this between
    t-SNE and UMAP on one attraction-repulsion spectrum. Each cell's pull is averaged over its
    neighbours and its push over a few random cells, so an epoch costs
    O(edges), not O(cells^2) as exact t-SNE does.
    Started from PCA so the global layout keeps PC1/PC2's arrangement (Kobak &
    Linderman 2021)."""
    rng = np.random.default_rng(seed)
    n = g.shape[0]
    coo = sparse.triu(g, 1).tocoo()
    i, j = coo.row, coo.col
    deg = np.maximum(np.bincount(i, minlength=n) + np.bincount(j, minlength=n), 1)
    y = init[:, :2] / np.abs(init[:, :2]).max() * 10
    src = np.repeat(np.arange(n), negatives)
    for e in range(epochs):
        d = y[i] - y[j]
        att = (-2 / (1 + (d ** 2).sum(1)))[:, None] * d           # pulls each edge's ends together
        dn = y[src] - y[rng.integers(0, n, len(src))]             # random pairs: almost all non-neighbours
        d2 = (dn ** 2).sum(1)
        rep = np.clip((2 / ((0.001 + d2) * (1 + d2)))[:, None] * dn, -4, 4)
        grad = np.empty_like(y)
        for c in range(2):                                         # bincount is numpy's fast scatter-add
            pull = np.bincount(i, att[:, c], n) - np.bincount(j, att[:, c], n)
            push = np.bincount(src, rep[:, c], n)
            grad[:, c] = pull / deg + repulsion * push / negatives
        y += (1.0 - e / epochs) * np.clip(4 * grad, -4, 4)
    return y


@lru_cache(maxsize=4)
def embed(seed, pk, n_pcs, method, k, perplexity) -> np.ndarray:
    pcs = pca(seed, *pk)[0][:, :n_pcs]
    if method == "tsne":
        return TSNE(perplexity=perplexity, init="pca", max_iter=500, random_state=seed).fit_transform(pcs)
    if method == "pca":
        return pcs[:, :2].copy()
    return graph_layout(knn(seed, pk, n_pcs, k), pcs, seed)


@lru_cache(maxsize=4)
def cluster(seed, pk, n_pcs, k, method, n_clusters) -> np.ndarray:
    """Cluster labels, renumbered largest first so the numbering is stable."""
    pcs = pca(seed, *pk)[0][:, :n_pcs]
    if method == "kmeans":
        lab = KMeans(n_clusters, n_init=4, random_state=seed).fit_predict(pcs)
    elif method == "spectral":
        lab = SpectralClustering(n_clusters, affinity="precomputed", random_state=seed,
                                 assign_labels="cluster_qr").fit_predict(knn(seed, pk, n_pcs, k))
    else:   # Ward linkage, merges allowed only along kNN edges: hierarchical clustering on the graph
        lab = AgglomerativeClustering(n_clusters, linkage="ward",
                                      connectivity=knn(seed, pk, n_pcs, k)).fit_predict(pcs)
    rank = np.argsort(np.argsort(-np.bincount(lab, minlength=n_clusters), kind="stable"))
    return rank[lab]


@lru_cache(maxsize=4)
def ranks(seed, qck, target) -> tuple[np.ndarray, np.ndarray]:
    """Per-gene ranks of every kept cell (ties share their average rank), and
    each gene's tie term sum(t^3 - t) over runs of t equal values: most
    entries are zero, so the tie correction matters. One sort serves both."""
    xt = np.ascontiguousarray(normalise(seed, qck, target).T)    # genes x cells
    g, n = xt.shape
    order = np.argsort(xt, axis=1)                                # ties get their average rank, so any order of equals will do
    srt = np.take_along_axis(xt, order, 1)
    edge = np.ones((g, n + 1), dtype=bool)                        # True where a run of equal values starts
    edge[:, 1:n] = srt[:, 1:] != srt[:, :-1]
    pos = np.arange(n)
    first = np.maximum.accumulate(np.where(edge[:, :n], pos, 0), axis=1)
    last = n - 1 - np.maximum.accumulate(np.where(edge[:, :0:-1], pos, 0), axis=1)[:, ::-1]
    r = np.empty((g, n), dtype=np.float32)
    np.put_along_axis(r, order, ((first + last) / 2 + 1).astype(np.float32), 1)
    t = (last - first + 1).astype(np.float64)                     # each cell's run length
    ties = (t ** 2 - 1).sum(1)                                    # t cells x (t^2 - 1) = t^3 - t per run
    return r.T, ties


@lru_cache(maxsize=4)
def wilcoxon(seed, qck, target, labels_key) -> dict:
    """Each cluster against all other kept cells, every gene: Wilcoxon
    rank-sum z (normal approximation, tie-corrected), Benjamini-Hochberg
    adjusted p, log2 fold change, and the share of cells expressing it."""
    labels = cluster(*labels_key)
    x = normalise(seed, qck, target)
    r, ties = ranks(seed, qck, target)
    n = len(labels)
    k = labels.max() + 1
    onehot = np.eye(k, dtype=np.float32)[labels]                # cells x clusters
    n1 = onehot.sum(0)[:, None]
    n2 = n - n1
    u = onehot.T @ r - n1 * (n1 + 1) / 2
    sigma = np.sqrt(n1 * n2 / 12 * ((n + 1) - ties[None, :] / (n * (n - 1))))
    z = np.where(sigma > 0, (u - n1 * n2 / 2) / np.where(sigma > 0, sigma, 1), 0)
    p = 2 * norm.sf(np.abs(z))
    padj = np.vstack([false_discovery_control(row) for row in p])
    lin = np.expm1(x)
    s_in = onehot.T @ lin
    m_in = s_in / n1
    m_out = (lin.sum(0)[None, :] - s_in) / n2
    on = (x > 0).astype(np.float32)
    frac = onehot.T @ on / n1
    return {"z": z, "padj": padj, "lfc": np.log2((m_in + 1e-9) / (m_out + 1e-9)),
            "frac": frac, "mean": onehot.T @ x / n1}


# ---- routes ----------------------------------------------------------------------------

class Run(BaseModel):
    seed: int = Field(0, ge=0, le=10_000)
    min_counts: int = Field(1000, ge=0, le=20_000)
    max_counts: int = Field(9000, ge=500, le=50_000, description="ceiling: doublets carry about twice a cell's counts")
    min_genes: int = Field(400, ge=0, le=2000)
    max_mito: float = Field(15, ge=1, le=100, description="percent of counts from mitochondrial genes")
    target: float = Field(10_000, ge=100, le=1_000_000, description="counts per cell after normalisation")
    n_top: int = Field(500, ge=50, le=2000, description="highly variable genes kept")
    n_pcs: int = Field(12, ge=2, le=N_PCS_MAX)
    k: int = Field(15, ge=3, le=100, description="neighbours per cell in the graph")
    embedding: str = Field("graph", pattern="^(graph|tsne|pca)$")
    perplexity: float = Field(30, ge=5, le=100, description="t-SNE only")
    method: str = Field("ward", pattern="^(ward|spectral|kmeans)$")
    n_clusters: int = Field(6, ge=2, le=8, description="eight: the categorical palette has eight colours")
    n_markers: int = Field(4, ge=1, le=10, description="markers per cluster in the dot plot")


class Gene(Run):
    gene: str


def keys(r: Run):
    qck = (r.min_counts, r.max_counts, r.min_genes, r.max_mito)
    pk = (qck, r.target, r.n_top)
    return qck, pk, (r.seed, pk, r.n_pcs, r.k, r.method, r.n_clusters)


def r3(a) -> list:
    return np.round(np.asarray(a, dtype=np.float64), 3).tolist()


app = FastAPI(title="Cell Atlas · __HOST__", docs_url=None, redoc_url=None)
log.info("__HOST__: Cell Atlas starting on 127.0.0.1:%s, %d simulated cells", os.environ.get("PORT"), N_CELLS)


@app.get("/health")
def health() -> dict:
    """Answers once the app is serving (app.env APP_HEALTH; up.sh waits on it)."""
    return {"ok": True}
app.mount("/static", StaticFiles(directory=HERE / "static"), name="static")


@app.get("/")
def index():
    return FileResponse(HERE / "static" / "index.html")


@app.get("/api/menu")
def menu():
    """The defaults and bounds of every setting, the gene list for the search
    box, and the true categories with the markers planted in each type."""
    _, planted = programmes(np.random.default_rng(0))
    schema = Run.model_json_schema()["properties"]
    return {
        "settings": {k: {key: v[key] for key in ("default", "minimum", "maximum") if key in v} for k, v in schema.items()},
        "genes": GENES, "truth": TRUTH,
        "named_markers": {TYPES[t]: [g for g, e in NAMED if max(e, key=e.get) == t] for t in range(len(TYPES))},
    }


@app.post("/api/run")
def run(r: Run):
    if r.min_counts >= r.max_counts:
        raise HTTPException(422, "min_counts must be below max_counts")
    qck, pk, ck = keys(r)
    ms = {}

    def timed(name, f, *a):
        t0 = time.perf_counter()
        out = f(*a)
        ms[name] = round((time.perf_counter() - t0) * 1000, 1)
        return out

    s = timed("simulate", simulate, r.seed)
    kept = timed("qc", qc, r.seed, *qck)
    if len(kept) < 50 * r.n_clusters:
        raise HTTPException(422, f"{len(kept)} cells pass QC: too few to analyse; loosen the thresholds")
    timed("normalise", normalise, r.seed, qck, r.target)
    h = timed("hvg", hvg, r.seed, qck, r.target, r.n_top)
    _, var = timed("pca", pca, r.seed, *pk)
    timed("knn", knn, r.seed, pk, r.n_pcs, r.k)
    y = timed("embed", embed, r.seed, pk, r.n_pcs, r.embedding, r.k, r.perplexity)
    labels = timed("cluster", cluster, *ck)
    w = timed("markers", wilcoxon, r.seed, qck, r.target, ck)

    truth = s["truth"][kept]
    single = truth < len(TYPES)
    ari = adjusted_rand_score(truth[single], labels[single])
    log.info("pipeline: %d of %d cells pass QC, %d clusters (%s), %s embedding, ARI %.3f, %.0f ms",
             len(kept), N_CELLS, r.n_clusters, r.method, r.embedding, ari, sum(ms.values()))
    table = np.zeros((r.n_clusters, len(TRUTH)), dtype=int)
    np.add.at(table, (labels, truth), 1)

    # markers: top genes per cluster by z, up-regulated and significant only
    planted = s["planted"]
    marker_rows, dot_genes = [], []
    for c in range(r.n_clusters):
        order = [g for g in np.argsort(-w["z"][c]) if w["lfc"][c, g] > 0.5 and w["padj"][c, g] < 0.01][:r.n_markers]
        major = int(np.argmax(table[c, :len(TYPES)]))
        marker_rows.append({
            "cluster": c, "size": int((labels == c).sum()), "major_type": TYPES[major],
            "purity": round(float(table[c, major] / max(table[c].sum(), 1)), 3),
            "genes": [{"gene": GENES[g], "z": round(float(w["z"][c, g]), 1), "lfc": round(float(w["lfc"][c, g]), 2),
                       "padj": float(f"{w['padj'][c, g]:.2e}"), "pct_in": round(float(w["frac"][c, g]) * 100, 1),
                       "planted": bool(g in planted[major])} for g in order],
        })
        dot_genes += [g for g in order if g not in dot_genes]
    mean = w["mean"][:, dot_genes]
    span = mean.max(0) - mean.min(0)
    scaled = (mean - mean.min(0)) / np.where(span > 0, span, 1)  # per gene, 0 to 1 across clusters
    in_hvg = set(h["top"].tolist())
    planted_all = sorted({g for p in planted for g in p})

    # JSONResponse skips FastAPI's per-value encoder walk: this payload is ~40k numbers
    return JSONResponse({
        "ms": ms, "ari": round(float(ari), 4), "n_kept": int(len(kept)), "n_cells": N_CELLS,
        "qc": {"n_counts": s["n_counts"].tolist(), "n_genes": s["n_genes"].tolist(), "pct_mito": r3(s["pct_mito"]),
               "truth": s["truth"].tolist(),
               "removed": {name: int(((s["truth"] == t).sum() - (truth == t).sum())) for t, name in enumerate(TRUTH)},
               "total": {name: int((s["truth"] == t).sum()) for t, name in enumerate(TRUTH)}},
        "hvg": {"log_mean": r3(h["log_mean"]), "z": [None if np.isnan(v) else round(float(v), 3) for v in h["z"]],
                "top": h["top"].tolist(), "planted": planted_all,
                "planted_in_hvg": sum(g in in_hvg for g in planted_all)},
        "pca": {"variance_ratio": r3(var)},
        "embedding": {"x": r3(y[:, 0]), "y": r3(y[:, 1]), "cluster": labels.tolist(), "truth": truth.tolist()},
        "table": table.tolist(),
        "markers": marker_rows,
        "dot": {"genes": [GENES[g] for g in dot_genes], "frac": r3(w["frac"][:, dot_genes]), "scaled": r3(scaled)},
    })


@app.post("/api/gene")
def gene(r: Gene):
    """One gene's log-normalised expression in every kept cell, in the order
    /api/run returned the embedding."""
    g = GENE_INDEX.get(r.gene.strip()) if r.gene.strip() in GENE_INDEX else \
        next((i for name, i in GENE_INDEX.items() if name.upper() == r.gene.strip().upper()), None)
    if g is None:
        raise HTTPException(404, f"no gene {r.gene!r}")
    qck, _, _ = keys(r)
    v = normalise(r.seed, qck, r.target)[:, g]
    return JSONResponse({"gene": GENES[g], "values": r3(v), "max": round(float(v.max()), 3),
                         "pct_expressing": round(float((v > 0).mean()) * 100, 1)})
