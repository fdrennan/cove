"""Machine-learning playground: scikit-learn on 2-D datasets.

MODELS and DATASETS are the whole menu: the page builds its controls from
GET /api/menu, so a model added here appears there with its sliders. POST
/api/train fits on a stratified split and answers with train/test accuracy and
the decision surface, drawn by matplotlib on the server, as a PNG data URI.
"""
import base64
import io
import time
import warnings
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import numpy as np
from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from matplotlib.backends.backend_agg import FigureCanvasAgg
from matplotlib.colors import to_rgb
from matplotlib.figure import Figure
from pydantic import BaseModel, Field
from sklearn import datasets as skd
from sklearn.ensemble import RandomForestClassifier
from sklearn.exceptions import ConvergenceWarning
from sklearn.linear_model import LogisticRegression
from sklearn.model_selection import train_test_split
from sklearn.neighbors import KNeighborsClassifier
from sklearn.neural_network import MLPClassifier
from sklearn.pipeline import make_pipeline
from sklearn.preprocessing import StandardScaler
from sklearn.svm import SVC

HERE = Path(__file__).parent
BG = "#0b1020"
CLASS_COLORS = ["#14a0be", "#d6702a", "#9a7be6"]   # validated together on a dark surface


# ---- the menu ------------------------------------------------------------------------

def p(label, lo, hi, step, default, log=False, integer=False):
    return {"label": label, "min": lo, "max": hi, "step": step, "default": default, "log": log, "integer": integer}


MODELS = {
    "logreg": {"label": "Logistic regression",
               "params": {"C": p("C", -3, 3, 0.1, 0, log=True)},
               "make": lambda q: LogisticRegression(C=q["C"], max_iter=2000)},
    "knn": {"label": "k-nearest neighbours",
            "params": {"k": p("k", 1, 50, 1, 7, integer=True)},
            "make": lambda q: KNeighborsClassifier(n_neighbors=int(q["k"]))},
    "svm": {"label": "SVM (RBF kernel)",
            "params": {"C": p("C", -2, 3, 0.1, 0, log=True), "gamma": p("γ", -2, 2, 0.1, 0, log=True)},
            "make": lambda q: SVC(C=q["C"], gamma=q["gamma"], kernel="rbf")},
    "forest": {"label": "Random forest",
               "params": {"trees": p("trees", 1, 300, 1, 100, integer=True),
                          "depth": p("max depth", 1, 20, 1, 8, integer=True)},
               "make": lambda q: RandomForestClassifier(n_estimators=int(q["trees"]), max_depth=int(q["depth"]),
                                                        random_state=0, n_jobs=1)},
    "mlp": {"label": "Neural network (MLP)",
            "params": {"width": p("units per layer", 2, 128, 1, 32, integer=True),
                       "layers": p("layers", 1, 4, 1, 2, integer=True),
                       "alpha": p("L2 α", -6, 0, 0.1, -4, log=True)},
            "make": lambda q: MLPClassifier(hidden_layer_sizes=(int(q["width"]),) * int(q["layers"]),
                                            alpha=q["alpha"], max_iter=800, random_state=0)},
}


def spiral(n, noise, rng):
    k = n // 2
    t = np.sqrt(rng.uniform(0, 1, k)) * 3 * np.pi
    a = np.column_stack([t * np.cos(t), t * np.sin(t)]) / 3
    X = np.vstack([a, -a]) + rng.normal(0, noise * 1.2, (2 * k, 2))
    return X, np.repeat([0, 1], k)


def xor(n, noise, rng):
    X = rng.uniform(-1, 1, (n, 2))
    y = (X[:, 0] * X[:, 1] > 0).astype(int)
    return X + rng.normal(0, noise, X.shape), y


DATASETS = {
    "moons": {"label": "Moons", "make": lambda n, s, r: skd.make_moons(n, noise=s, random_state=r)},
    "circles": {"label": "Circles", "make": lambda n, s, r: skd.make_circles(n, noise=s, factor=0.45, random_state=r)},
    "blobs": {"label": "Blobs (3 classes)", "make": lambda n, s, r: skd.make_blobs(n, centers=3, cluster_std=0.5 + 3 * s, random_state=r)},
    "spiral": {"label": "Spiral", "make": lambda n, s, r: spiral(n, s, np.random.default_rng(r))},
    "xor": {"label": "XOR", "make": lambda n, s, r: xor(n, s, np.random.default_rng(r))},
}


class Train(BaseModel):
    dataset: str = "moons"
    model: str = "svm"
    params: dict[str, float] = Field(default_factory=dict, description="slider values; log params as log10")
    n: int = Field(400, ge=40, le=2000)
    noise: float = Field(0.2, ge=0, le=0.6)
    test: float = Field(0.3, ge=0.1, le=0.6, description="share held out for testing")
    seed: int = Field(0, ge=0, le=10_000)


# ---- the work --------------------------------------------------------------------------

def resolve(spec: dict, given: dict) -> dict:
    """Slider values -> model arguments: defaults filled, clamped, log10 params exponentiated."""
    out = {}
    for name, s in spec.items():
        v = float(min(s["max"], max(s["min"], given.get(name, s["default"]))))
        out[name] = 10 ** v if s["log"] else v
    return out


def scores(model, X) -> np.ndarray:
    """Class membership per row, shape (n, k), rows summing to 1."""
    if hasattr(model, "predict_proba"):
        return model.predict_proba(X)
    d = model.decision_function(X)
    if d.ndim == 1:
        pr = 1 / (1 + np.exp(-d))
        return np.column_stack([1 - pr, pr])
    e = np.exp(d - d.max(axis=1, keepdims=True))
    return e / e.sum(axis=1, keepdims=True)


def surface_png(model, Xtr, ytr, Xte, yte, k) -> str:
    pad = 0.6
    lo, hi = np.vstack([Xtr, Xte]).min(0) - pad, np.vstack([Xtr, Xte]).max(0) + pad
    gx, gy = np.meshgrid(np.linspace(lo[0], hi[0], 180), np.linspace(lo[1], hi[1], 180))
    P = scores(model, np.column_stack([gx.ravel(), gy.ravel()]))
    cols = np.array([to_rgb(c) for c in CLASS_COLORS[:k]])
    conf = P.max(1)
    base = np.array(to_rgb(BG))
    # colour of the winning class, stronger where the model is surer
    w = ((conf - 1 / k) / (1 - 1 / k))[:, None] * 0.36 + 0.07
    img = (base * (1 - w) + cols[P.argmax(1)] * w).reshape(gx.shape + (3,))

    fig = Figure(figsize=(7, 7), dpi=120, facecolor=BG)
    ax = fig.add_axes([0, 0, 1, 1])
    ax.set_facecolor(BG)
    ax.imshow(img, origin="lower", extent=(lo[0], hi[0], lo[1], hi[1]), aspect="auto", interpolation="bilinear")
    if k == 2:
        ax.contour(gx, gy, P[:, 1].reshape(gx.shape), levels=[0.5], colors="white", linewidths=1.6, alpha=0.85)
    for c in range(k):
        ax.scatter(*Xtr[ytr == c].T, s=26, c=CLASS_COLORS[c], edgecolors=BG, linewidths=0.8, zorder=3)
        ax.scatter(*Xte[yte == c].T, s=40, c=CLASS_COLORS[c], marker="D", edgecolors="white", linewidths=1.1, zorder=4)
    ax.set_xlim(lo[0], hi[0]); ax.set_ylim(lo[1], hi[1]); ax.set_axis_off()
    buf = io.BytesIO()
    FigureCanvasAgg(fig).print_png(buf)
    return "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode()


# ---- routes ----------------------------------------------------------------------------

app = FastAPI(title="ML playground · __HOST__", docs_url=None, redoc_url=None)


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
    return {"models": {k: {"label": m["label"], "params": m["params"]} for k, m in MODELS.items()},
            "datasets": {k: d["label"] for k, d in DATASETS.items()},
            "colors": CLASS_COLORS}


@app.post("/api/train")
def train(t: Train):
    if t.model not in MODELS or t.dataset not in DATASETS:
        raise HTTPException(422, f"model must be one of {list(MODELS)}, dataset one of {list(DATASETS)}")
    X, y = DATASETS[t.dataset]["make"](t.n, t.noise, t.seed)
    Xtr, Xte, ytr, yte = train_test_split(X, y, test_size=t.test, random_state=t.seed, stratify=y)
    q = resolve(MODELS[t.model]["params"], t.params)
    model = make_pipeline(StandardScaler(), MODELS[t.model]["make"](q))
    t0 = time.perf_counter()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", ConvergenceWarning)
        model.fit(Xtr, ytr)
    fit_ms = (time.perf_counter() - t0) * 1000
    k = int(y.max()) + 1
    t1 = time.perf_counter()
    png = surface_png(model, Xtr, ytr, Xte, yte, k)
    return {
        "train_acc": float(model.score(Xtr, ytr)),
        "test_acc": float(model.score(Xte, yte)),
        "n_train": len(ytr), "n_test": len(yte), "classes": k,
        "args": {k_: round(v, 6) for k_, v in q.items()},
        "fit_ms": round(fit_ms, 1),
        "draw_ms": round((time.perf_counter() - t1) * 1000, 1),
        "png": png,
    }
