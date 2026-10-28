"""Topic map: the themes in a corpus of documents, found by scikit-learn.

corpus.json is the corpus (title, field, text); the field is never shown to a
model, only used afterwards to say how well the topics recover it. Three
cached stages, each keyed by the settings it depends on, so moving the k
slider refits topics without redrawing the map, and a paste reuses both:

  features   counts and TF-IDF over one shared vocabulary (n-grams, stop
             words, min/max document frequency)
  topics     NMF on TF-IDF or LDA on counts: top words per topic, each
             document's topic mix, coherence (UMass, NPMI) per topic
  layout     TruncatedSVD to 30 dimensions, then t-SNE to 2

POST /api/model answers with all of it; /api/similar and /api/place answer
"more like this" for a document or for pasted text; /api/sweep fits every k
so coherence can choose it.
"""
import os

# One thread for BLAS and OpenMP, set before numpy and scikit-learn load them.
# The matrices are tiny, and on a shared box every extra thread spins waiting
# for a core: t-SNE on these 91 documents took 25-112 s with 24 OpenMP threads
# at load 70, and under 2 s with one. threadpoolctl is not enough: OpenMP's
# limit is per thread, and FastAPI runs each request on a worker thread.
for _v in ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS"):
    os.environ.setdefault(_v, "1")

import json
import time
from functools import lru_cache
from pathlib import Path
from typing import Literal

import numpy as np
from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field
from sklearn.decomposition import NMF, LatentDirichletAllocation, TruncatedSVD
from sklearn.feature_extraction.text import CountVectorizer, TfidfTransformer
from sklearn.manifold import TSNE
from sklearn.metrics import adjusted_mutual_info_score
from sklearn.preprocessing import normalize

HERE = Path(__file__).parent
CORPUS = json.loads((HERE / "corpus.json").read_text(encoding="utf-8"))
# The title is part of the document: it is the densest statement of the theme.
TEXTS = [f"{d['title']}. {d['text']}" for d in CORPUS]
FIELDS = list(dict.fromkeys(d["field"] for d in CORPUS))
FIELD_OF = np.array([FIELDS.index(d["field"]) for d in CORPUS])

# One colour per topic, in fixed order. Eight is the most a scatter can carry
# by hue, and even these share lightness, so each topic also has its own marker
# shape on the map (static/app.js SHAPES): hue is never the only cue.
TOPIC_COLORS = ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767"]
K_MAX = len(TOPIC_COLORS)
TOP_N = 10          # words per topic, and the words coherence is scored on
SVD_DIMS = 30       # LSA dimensions t-SNE starts from


class Settings(BaseModel):
    method: Literal["nmf", "lda"] = "nmf"
    k: int = Field(6, ge=2, le=K_MAX, description="number of topics")
    ngram: int = Field(1, ge=1, le=3, description="longest n-gram counted")
    min_df: int = Field(2, ge=1, le=10, description="drop terms in fewer documents than this")
    max_df: float = Field(0.5, ge=0.1, le=1.0, description="drop terms in more than this share of documents")
    perplexity: float = Field(15, ge=5, le=40, description="t-SNE neighbourhood size")
    seed: int = Field(0, ge=0, le=10_000)

    def fkey(self):
        return (self.ngram, self.min_df, self.max_df)


class Similar(Settings):
    doc: int = Field(0, ge=0)
    n: int = Field(6, ge=1, le=30)


class Place(Settings):
    text: str = Field(..., min_length=1, max_length=20_000)
    n: int = Field(6, ge=1, le=30)


# ---- the work --------------------------------------------------------------------------

class Features:
    def __init__(self, ngram, min_df, max_df):
        # Letters only, two or more: numbers ("1.6", "2023") are noise for themes.
        self.vec = CountVectorizer(stop_words="english", ngram_range=(1, ngram), min_df=min_df, max_df=max_df,
                                   token_pattern=r"(?u)\b[^\W\d_]{2,}\b")
        self.counts = self.vec.fit_transform(TEXTS)
        # sublinear tf: a word said five times is not five times the evidence.
        self.tfidf_t = TfidfTransformer(sublinear_tf=True).fit(self.counts)
        self.tfidf = self.tfidf_t.transform(self.counts)          # rows are unit length
        self.terms = self.vec.get_feature_names_out()
        self.binary = (self.counts > 0).astype(np.float64).tocsc()

    def embed(self, texts):
        c = self.vec.transform(texts)
        return c, self.tfidf_t.transform(c)


@lru_cache(maxsize=16)
def features(ngram, min_df, max_df) -> Features:
    try:
        return Features(ngram, min_df, max_df)
    except ValueError as e:      # min_df above max_df, or nothing left after pruning
        raise HTTPException(422, f"no vocabulary with these settings: {e}") from None


def coherence(binary, top) -> tuple[float, float]:
    """Mean pairwise UMass and NPMI of one topic's top terms, from document co-occurrence.

    UMass (Mimno et al. 2011): log (D(wi, wj) + 1) / D(wi), wi ranked above wj;
    0 is the ceiling, more negative is less coherent. NPMI (Bouma 2009) lies in
    [-1, 1]; a pair that never shares a document scores -1. Both are averaged
    over pairs so topics with different numbers of terms compare.
    """
    sub = binary[:, top]
    D = (sub.T @ sub).toarray()
    df = np.diag(D)
    i, j = np.triu_indices(len(top), 1)
    umass = np.log((D[i, j] + 1) / df[i])
    n = binary.shape[0]
    pij, pi, pj = D[i, j] / n, df[i] / n, df[j] / n
    with np.errstate(divide="ignore", invalid="ignore"):
        npmi = np.log(pij / (pi * pj)) / -np.log(pij)
    npmi = np.where(D[i, j] == 0, -1.0, np.where(pij >= 1, 1.0, npmi))
    return float(umass.mean()), float(npmi.mean())


class Topics:
    def __init__(self, f: Features, method, k, seed):
        t0 = time.perf_counter()
        if method == "nmf":
            # NMF on TF-IDF: nndsvda starts from an SVD, so results are stable across seeds.
            self.model = NMF(n_components=k, init="nndsvda", max_iter=600, random_state=seed)
            self.uses_counts = False
            W = self.model.fit_transform(f.tfidf)
        else:
            # LDA is a model of word counts, not of TF-IDF weights. An abstract
            # here keeps about 20 counted words, and with sklearn's default
            # priors (1/k) and batch updates each document locked onto one
            # topic within a few passes and the topics mixed every field
            # (agreement with the fields 0.05, near chance). A flat
            # document prior and online updates keep documents undecided long
            # enough for topics to form: 0.34-0.43 over seeds, the best of
            # some thirty settings tried. NMF reaches 0.91 on the same counts;
            # LDA wants longer documents.
            self.model = LatentDirichletAllocation(n_components=k, doc_topic_prior=1.0, topic_word_prior=0.1,
                                                   learning_method="online", batch_size=16, max_iter=50,
                                                   random_state=seed)
            self.uses_counts = True
            W = self.model.fit_transform(f.counts)
        H = self.model.components_ / self.model.components_.sum(1, keepdims=True)
        mix = self.normalise(W)
        dom = mix.argmax(1)
        # Biggest topic first: colour slot 1 goes to the topic most documents belong to.
        order = np.argsort(-np.bincount(dom, minlength=k), kind="stable")
        self.order = order
        self.H = H[order]
        self.mix = mix[:, order]
        self.dom = self.mix.argmax(1)
        self.topics = []
        for t in range(k):
            top = np.argsort(-self.H[t])[:TOP_N]
            umass, npmi = coherence(f.binary, top)
            members = FIELD_OF[self.dom == t]
            counts = np.bincount(members, minlength=len(FIELDS))
            self.topics.append({
                "id": t,
                "words": [[f.terms[w], round(float(self.H[t, w]), 5)] for w in top],
                "umass": round(umass, 3), "npmi": round(npmi, 3),
                "size": int(len(members)),
                "field": FIELDS[int(counts.argmax())] if len(members) else None,
                "field_share": round(float(counts.max() / len(members)), 3) if len(members) else 0.0,
            })
        self.umass = float(np.mean([t["umass"] for t in self.topics]))
        self.npmi = float(np.mean([t["npmi"] for t in self.topics]))
        # How much the topics agree with the fields, chance-corrected: 1 is a
        # perfect match, 0 is what random assignment would give.
        self.ami = float(adjusted_mutual_info_score(FIELD_OF, self.dom))
        self.fit_ms = (time.perf_counter() - t0) * 1000

    @staticmethod
    def normalise(W):
        W = np.asarray(W, dtype=float) + 1e-12      # a document with no vocabulary gets an even mix
        return W / W.sum(1, keepdims=True)

    def mix_of(self, counts, tfidf):
        return self.normalise(self.model.transform(counts if self.uses_counts else tfidf))[:, self.order]


@lru_cache(maxsize=64)
def topics(fkey, method, k, seed) -> Topics:
    return Topics(features(*fkey), method, k, seed)


@lru_cache(maxsize=16)
def layout(fkey, perplexity, seed) -> tuple[np.ndarray, float]:
    """TruncatedSVD (latent semantic analysis) then t-SNE, scaled into [-1, 1], and how long it took."""
    t0 = time.perf_counter()
    f = features(*fkey)
    dims = max(2, min(SVD_DIMS, f.tfidf.shape[1] - 1, f.tfidf.shape[0] - 1))
    lsa = normalize(TruncatedSVD(n_components=dims, random_state=seed).fit_transform(f.tfidf))
    xy = TSNE(n_components=2, perplexity=min(perplexity, len(TEXTS) - 1), init="pca", metric="cosine",
              random_state=seed).fit_transform(lsa)
    xy -= xy.mean(0)
    return xy / np.abs(xy).max(), (time.perf_counter() - t0) * 1000


def fitted(s: Settings):
    """Features, topics and layout for these settings; times are those of the first fit, cached or not."""
    f = features(*s.fkey())
    tp = topics(s.fkey(), s.method, s.k, s.seed)
    xy, map_ms = layout(s.fkey(), s.perplexity, s.seed)
    return f, tp, xy, tp.fit_ms, map_ms


def neighbours(f: Features, tp: Topics, q, n, skip=None):
    """The n documents closest to q by cosine on TF-IDF (rows are unit length, so a dot product)."""
    sims = (f.tfidf @ q.T).toarray().ravel()
    if skip is not None:
        sims[skip] = -np.inf
    idx = np.argsort(-sims)[:n]
    return idx, sims[idx], [{"i": int(i), "title": CORPUS[i]["title"], "field": CORPUS[i]["field"],
                             "topic": int(tp.dom[i]), "score": round(float(s), 4)} for i, s in zip(idx, sims[idx])]


# ---- routes ----------------------------------------------------------------------------

app = FastAPI(title="Topic map · __HOST__", docs_url=None, redoc_url=None)


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
    return {"corpus": [{"title": d["title"], "field": d["field"], "text": d["text"]} for d in CORPUS],
            "fields": FIELDS, "colors": TOPIC_COLORS, "k_max": K_MAX,
            "defaults": Settings().model_dump()}


@app.post("/api/model")
def model(s: Settings):
    f, tp, xy, fit_ms, map_ms = fitted(s)
    return {
        "docs": [{"i": i, "x": round(float(xy[i, 0]), 4), "y": round(float(xy[i, 1]), 4),
                  "topic": int(tp.dom[i]), "mix": [round(float(v), 4) for v in tp.mix[i]]}
                 for i in range(len(CORPUS))],
        "topics": tp.topics,
        "coherence": {"umass": round(tp.umass, 3), "npmi": round(tp.npmi, 3)},
        "ami": round(tp.ami, 3),
        "vocab": int(len(f.terms)),
        "fit_ms": round(fit_ms, 1), "map_ms": round(map_ms, 1),
    }


@app.post("/api/sweep")
def sweep(s: Settings):
    """Every k from 2 to K_MAX with the other settings held: coherence and agreement with the fields."""
    rows = []
    for k in range(2, K_MAX + 1):
        tp = topics(s.fkey(), s.method, k, s.seed)
        rows.append({"k": k, "npmi": round(tp.npmi, 3), "umass": round(tp.umass, 3), "ami": round(tp.ami, 3)})
    return {"rows": rows}


@app.post("/api/similar")
def similar(q: Similar):
    if q.doc >= len(CORPUS):
        raise HTTPException(422, f"doc must be below {len(CORPUS)}")
    f, tp, _, _, _ = fitted(q)
    _, _, rows = neighbours(f, tp, f.tfidf[q.doc], q.n, skip=q.doc)
    return {"doc": q.doc, "neighbours": rows}


@app.post("/api/place")
def place(q: Place):
    f, tp, xy, _, _ = fitted(q)
    counts, vec = f.embed([q.text])
    if vec.nnz == 0:
        raise HTTPException(422, "none of the text's words are in the corpus vocabulary")
    mix = tp.mix_of(counts, vec)[0]
    idx, sims, rows = neighbours(f, tp, vec, q.n)
    # t-SNE has no transform for new points: the text lands at the mean of its
    # nearest documents' positions, weighted by how close each one is.
    w = np.clip(sims, 1e-6, None)
    x, y = (xy[idx] * w[:, None]).sum(0) / w.sum()
    terms = f.terms[vec.indices[np.argsort(-vec.data)][:12]]
    return {"x": round(float(x), 4), "y": round(float(y), 4), "topic": int(mix.argmax()),
            "mix": [round(float(v), 4) for v in mix], "neighbours": rows,
            "terms": [str(t) for t in terms], "known_terms": int(vec.nnz)}
