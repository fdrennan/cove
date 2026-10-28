"""Digit Lab: draw a digit, scikit-learn reads it.

At startup three classifiers are fitted on sklearn's built-in handwritten
digits (load_digits: 1,797 8x8 images, no download) and scored on a held-out
quarter. The page sends the drawing as a 64x64 ink grid; to_8x8() turns it into
the same kind of 8x8 image the models were trained on, following how that
dataset was made, and every model answers with its probabilities for 0-9.

MODELS is the whole menu: add an entry and it is trained, scored and shown.
"""
import os

# BLAS and OpenMP default to one thread per core. On a many-core box the
# threads fight over these tiny problems: logistic regression took 78 s with
# the default and 0.2 s pinned to one thread. Set before numpy is imported.
for _v in ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS"):
    os.environ.setdefault(_v, "1")

import time
import warnings
from contextlib import asynccontextmanager
from pathlib import Path

import numpy as np
from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field
from sklearn.calibration import CalibratedClassifierCV
from sklearn.datasets import load_digits
from sklearn.exceptions import ConvergenceWarning
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import confusion_matrix
from sklearn.model_selection import train_test_split
from sklearn.neighbors import KNeighborsClassifier
from sklearn.svm import SVC

HERE = Path(__file__).parent
GRID = 64          # side of the ink grid the page sends
FRAME = 32         # optdigits bitmaps are 32x32 before 4x4 block counting
TEST_SHARE = 0.25
SEED = 0

# Inputs are scaled to 0-1 (pixel / 16) before fitting; the hyperparameters
# below are tuned for that scale.
MODELS = {
    # SVC has no probabilities of its own; sigmoid calibration on internal
    # folds (Platt scaling) gives them, and is the replacement sklearn 1.9
    # names for the deprecated SVC(probability=True).
    "svc": {"label": "SVM (RBF)",
            "make": lambda: CalibratedClassifierCV(SVC(C=10, gamma=0.05), method="sigmoid", cv=3,
                                                 ensemble=False)},
    "logreg": {"label": "Logistic regression",
               "make": lambda: LogisticRegression(C=1.0, max_iter=3000)},
    "knn": {"label": "k-NN (k=5)",
            "make": lambda: KNeighborsClassifier(n_neighbors=5, weights="distance")},
}

STATE: dict = {}


def train_all() -> None:
    d = load_digits()
    # Raw 0-16 images ride along in the split so the Sample button can serve
    # held-out digits the models never saw.
    Xtr, Xte, ytr, yte, _, img_te = train_test_split(d.data / 16.0, d.target, d.data.astype(int),
                                                     test_size=TEST_SHARE, random_state=SEED,
                                                     stratify=d.target)
    fitted, report = {}, {}
    t_all = time.perf_counter()
    for key, spec in MODELS.items():
        m = spec["make"]()
        t0 = time.perf_counter()
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", ConvergenceWarning)
            m.fit(Xtr, ytr)
        fit_ms = (time.perf_counter() - t0) * 1000
        pred = m.predict(Xte)
        fitted[key] = m
        report[key] = {
            "label": spec["label"],
            "train_acc": float(m.score(Xtr, ytr)),
            "test_acc": float((pred == yte).mean()),
            "fit_ms": round(fit_ms, 1),
            "confusion": confusion_matrix(yte, pred, labels=range(10)).tolist(),
        }
    STATE.update(models=fitted, report=report, test_images=img_te, test_labels=yte,
                 n_train=len(ytr), n_test=len(yte),
                 train_s=round(time.perf_counter() - t_all, 2))


def resize_area(a: np.ndarray, h: int, w: int) -> np.ndarray:
    """Box-filter resample: each output pixel is the mean of the input pixels it covers."""
    H, W = a.shape
    r = np.minimum((np.arange(h + 1) * H) // h, H)
    c = np.minimum((np.arange(w + 1) * W) // w, W)
    out = np.empty((h, w))
    for i in range(h):
        r0, r1 = r[i], max(r[i + 1], r[i] + 1)
        for j in range(w):
            c0, c1 = c[j], max(c[j + 1], c[j] + 1)
            out[i, j] = a[r0:r1, c0:c1].mean()
    return out


def to_8x8(ink: np.ndarray) -> np.ndarray:
    """A drawing -> an 8x8 image made the way load_digits' images were made.

    The UCI optdigits set (Alpaydin & Kaynak) normalised each scanned digit
    into a 32x32 bitmap and counted the 'on' pixels in each 4x4 block, so a
    value is 0-16. Its digits always fill the full height (all 1,797 images
    span the 8 rows) with the aspect kept, so: crop to the ink, scale to 32
    rows (or 32 columns for a very wide stroke), centre the columns on the
    ink's centre of mass, threshold, count.
    """
    on = ink > 0.2
    if not on.any():
        return np.zeros((8, 8), dtype=int)
    rows, cols = np.where(on.any(1))[0], np.where(on.any(0))[0]
    crop = ink[rows[0]:rows[-1] + 1, cols[0]:cols[-1] + 1]
    h, w = crop.shape
    s = FRAME / max(h, w)  # height wins unless the stroke is wider than tall
    nh, nw = max(1, min(FRAME, round(h * s))), max(1, min(FRAME, round(w * s)))
    patch = resize_area(crop, nh, nw)
    frame = np.zeros((FRAME, FRAME))
    mass = patch.sum()
    cy = (patch.sum(1) * np.arange(nh)).sum() / mass
    cx = (patch.sum(0) * np.arange(nw)).sum() / mass
    y0 = int(np.clip(round(FRAME / 2 - 0.5 - cy), 0, FRAME - nh))
    x0 = int(np.clip(round(FRAME / 2 - 0.5 - cx), 0, FRAME - nw))
    frame[y0:y0 + nh, x0:x0 + nw] = patch
    bits = frame >= 0.5
    return bits.reshape(8, 4, 8, 4).sum(axis=(1, 3))


# ---- routes ----------------------------------------------------------------------------

@asynccontextmanager
async def lifespan(_app):
    # Fit here, not at import, so build.sh's `import app` stays a quick check.
    train_all()
    # One prediction before /health answers: the first one paid 570 ms of
    # lazy imports against 50 ms after, and it landed on the first stroke.
    warm = np.zeros((GRID, GRID), dtype=int)
    warm[16:48, 30:34] = 255
    predict(Drawing(ink=warm.flatten().tolist()))
    yield


app = FastAPI(title="Digit Lab · __HOST__", docs_url=None, redoc_url=None, lifespan=lifespan)


@app.get("/health")
def health() -> dict:
    """Answers once the models are fitted and the app is serving (app.env APP_HEALTH)."""
    return {"ok": True, "trained": bool(STATE.get("models"))}


app.mount("/static", StaticFiles(directory=HERE / "static"), name="static")


@app.get("/")
def index():
    return FileResponse(HERE / "static" / "index.html")


@app.get("/api/models")
def models():
    return {"models": STATE["report"], "n_train": STATE["n_train"], "n_test": STATE["n_test"],
            "train_s": STATE["train_s"]}


@app.get("/api/sample")
def sample(i: int | None = None):
    """A held-out digit (0-16 per pixel) and its true label; random unless i is given."""
    n = len(STATE["test_labels"])
    if i is None:
        i = int(np.random.default_rng().integers(n))
    if not 0 <= i < n:
        raise HTTPException(422, f"i must be in 0..{n - 1}")
    return {"i": i, "label": int(STATE["test_labels"][i]), "pixels": STATE["test_images"][i].tolist()}


class Drawing(BaseModel):
    ink: list[int] = Field(..., min_length=GRID * GRID, max_length=GRID * GRID,
                           description=f"{GRID}x{GRID} row-major ink, 0-255")


@app.post("/api/predict")
def predict(d: Drawing):
    t0 = time.perf_counter()
    ink = np.asarray(d.ink, dtype=float).reshape(GRID, GRID) / 255.0
    seen = to_8x8(ink)
    out = {}
    if seen.any():
        x = seen.reshape(1, 64) / 16.0
        for key, m in STATE["models"].items():
            p = m.predict_proba(x)[0]
            out[key] = {"digit": int(p.argmax()), "probs": [round(float(v), 4) for v in p]}
    return {"seen": seen.flatten().tolist(), "models": out,
            "ms": round((time.perf_counter() - t0) * 1000, 1)}
