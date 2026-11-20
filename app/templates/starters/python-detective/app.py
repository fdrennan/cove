"""Dataset Detective: exploratory data analysis with pandas on built-in datasets.

DATASETS is the whole menu: four tables that ship inside scikit-learn (no
network). The server does the statistics with pandas, numpy and scipy and
answers JSON; the page draws every chart itself on canvas, so a new dataset
added here appears there with its heatmap, histograms and scatter.

  GET /api/menu                         datasets, their size and source
  GET /api/profile?dataset=&missing=&z=&iqr=&seed=
                                        per-column summary, correlation matrix,
                                        findings (strongest pairs, outliers,
                                        skew, missing, what separates groups)
  GET /api/column?dataset=&col=&bins=&by=...
                                        histogram (split by a category), density,
                                        box statistics
  GET /api/scatter?dataset=&x=&y=&color=...
                                        points, least-squares line, Pearson and
                                        Spearman

`missing` knocks a share of feature cells out at random (seeded), because the
built-in tables are complete and a detective should see what gaps do to the
numbers: every statistic here skips missing values pairwise, as pandas does.
"""
import os
import time
from functools import lru_cache
from pathlib import Path

import numpy as np
import pandas as pd
from fastapi import FastAPI, HTTPException, Query
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from scipy import stats
from sklearn import datasets as skd

from log import log as applog   # /api/column has a `log` parameter (log scale)

HERE = Path(__file__).parent
# Dark categorical slots 1-3 of the validated reference palette: they pass every
# colour-blind check on all pairs against the page's #0b1020 surface, and no
# dataset here has more than three groups.
GROUP_COLORS = ["#3987e5", "#d95926", "#199e70"]


# ---- the menu --------------------------------------------------------------------------

def iris():
    b = skd.load_iris(as_frame=True)
    df = b.frame.rename(columns=lambda c: c.replace(" (cm)", "").replace(" ", "_"))
    df["species"] = pd.Categorical.from_codes(df.pop("target"), list(b.target_names))
    return df


def wine():
    b = skd.load_wine(as_frame=True)
    df = b.frame.rename(columns={"od280/od315_of_diluted_wines": "od280_od315"})
    df["cultivar"] = pd.Categorical.from_codes(df.pop("target"), ["cultivar 1", "cultivar 2", "cultivar 3"])
    return df


def breast_cancer():
    b = skd.load_breast_cancer(as_frame=True)
    df = b.frame.rename(columns=lambda c: c.replace(" ", "_"))
    df["diagnosis"] = pd.Categorical.from_codes(df.pop("target"), ["malignant", "benign"])
    return df


def diabetes():
    # scaled=False: the raw clinical units (years, kg/m², mm Hg, mg/dL), not the
    # mean-centred copy, so summaries read as a clinician would.
    b = skd.load_diabetes(as_frame=True, scaled=False)
    df = b.frame.rename(columns={"s1": "tc", "s2": "ldl", "s3": "hdl", "s4": "tch", "s5": "ltg", "s6": "glucose",
                                 "target": "progression"})
    df["sex"] = pd.Categorical(df["sex"].map({1.0: "sex 1", 2.0: "sex 2"}))
    return df


DATASETS = {
    "iris": {"label": "Iris", "make": iris, "target": "species",
             "about": "150 irises, 3 species, petal and sepal size (Fisher 1936)"},
    "wine": {"label": "Wine", "make": wine, "target": "cultivar",
             "about": "178 Italian wines, 3 cultivars, 13 chemical assays (Forina et al.)"},
    "cancer": {"label": "Breast cancer", "make": breast_cancer, "target": "diagnosis",
               "about": "569 fine-needle aspirates, 30 cell-nucleus measures (Wisconsin, Street et al. 1993)"},
    "diabetes": {"label": "Diabetes", "make": diabetes, "target": "progression",
                 "about": "442 patients, 10 baseline measures, disease progression a year on (Efron et al. 2004)"},
}


@lru_cache(maxsize=64)
def table(name: str, missing: float, seed: int) -> pd.DataFrame:
    """The dataset with `missing` of its feature cells blanked (target untouched)."""
    if name not in DATASETS:
        raise HTTPException(422, f"dataset must be one of {list(DATASETS)}")
    df = DATASETS[name]["make"]()
    if missing > 0:
        rng = np.random.default_rng(seed)
        target = DATASETS[name]["target"]
        for c in df.columns:
            if c != target:
                df.loc[rng.random(len(df)) < missing, c] = np.nan if df[c].dtype.kind == "f" else None
    applog.info("loaded %s: %d rows x %d columns, %.0f%% of feature cells blanked (seed %d)",
                name, len(df), df.shape[1], missing * 100, seed)
    return df


def numeric(df):
    return [c for c in df.columns if df[c].dtype.kind in "if"]


def categorical(df):
    return [c for c in df.columns if isinstance(df[c].dtype, pd.CategoricalDtype)]


def fnum(x, nd=4):
    """JSON-safe float: NaN/inf -> None, rounded to nd significant figures."""
    if x is None or not np.isfinite(x):
        return None
    return float(f"{x:.{nd}g}")


# ---- the work --------------------------------------------------------------------------

def column_summary(s: pd.Series, z: float, k: float) -> dict:
    out = {"name": s.name, "missing": int(s.isna().sum()), "n": int(s.notna().sum())}
    if isinstance(s.dtype, pd.CategoricalDtype):
        vc = s.value_counts(sort=False)
        return out | {"kind": "category", "dtype": "category", "levels": list(map(str, vc.index)),
                      "counts": vc.astype(int).tolist()}
    x = s.dropna()
    q1, med, q3 = x.quantile([0.25, 0.5, 0.75])
    iqr = q3 - q1
    zs = (x - x.mean()) / x.std()
    return out | {
        "kind": "numeric", "dtype": str(s.dtype),
        "mean": fnum(x.mean()), "std": fnum(x.std()), "min": fnum(x.min()), "q1": fnum(q1),
        "median": fnum(med), "q3": fnum(q3), "max": fnum(x.max()),
        "skew": fnum(x.skew(), 3), "kurtosis": fnum(x.kurt(), 3), "unique": int(x.nunique()),
        "out_z": int((zs.abs() > z).sum()),
        "out_iqr": int(((x < q1 - k * iqr) | (x > q3 + k * iqr)).sum()),
        "spark": np.histogram(x, bins=16)[0].astype(int).tolist(),
    }


def eta_squared(x: pd.Series, g: pd.Series) -> float:
    """Share of x's variance explained by the group (one-way ANOVA effect size)."""
    d = pd.DataFrame({"x": x, "g": g}).dropna()
    total = ((d.x - d.x.mean()) ** 2).sum()
    between = d.groupby("g", observed=True).x.agg(lambda v: len(v) * (v.mean() - d.x.mean()) ** 2).sum()
    return float(between / total) if total > 0 else 0.0


def findings(df, cols, corr, name, z, k):
    """The things worth a look, strongest first. Each carries what to show."""
    out = []
    num = numeric(df)
    target = DATASETS[name]["target"]
    feats = [c for c in num if c != target]

    # strongest correlations, one per pair
    m = corr.where(np.triu(np.ones(corr.shape, bool), 1)).stack()
    for (a, b), r in m.reindex(m.abs().sort_values(ascending=False).index).head(4).items():
        out.append({"kind": "correlation", "score": abs(r), "show": {"x": a, "y": b},
                    "title": f"{a} and {b} move together" if r > 0 else f"{b} falls as {a} rises",
                    "detail": f"Pearson r = {r:+.3f} (r² {r * r:.2f}), n = {int(df[[a, b]].dropna().shape[0])}"})

    # what explains the target
    if isinstance(df[target].dtype, pd.CategoricalDtype):
        eta = sorted(((eta_squared(df[c], df[target]), c) for c in feats), reverse=True)[:3]
        for e, c in eta:
            out.append({"kind": "separation", "score": e, "show": {"col": c, "by": target},
                        "title": f"{c} separates {target}",
                        "detail": f"η² = {e:.2f}: {e:.0%} of its variance lies between the groups"})
    else:
        r = df[feats].corrwith(df[target]).dropna()
        for c in r.abs().sort_values(ascending=False).index[:3]:
            out.append({"kind": "separation", "score": abs(r[c]), "show": {"x": c, "y": target},
                        "title": f"{c} predicts {target}",
                        "detail": f"r = {r[c]:+.3f}: alone it explains {r[c] ** 2:.0%} of the variance"})

    for c in cols:
        if c["kind"] != "numeric":
            continue
        if c["skew"] is not None and abs(c["skew"]) >= 1:
            out.append({"kind": "skew", "score": min(abs(c["skew"]) / 3, 1), "show": {"col": c["name"]},
                        "title": f"{c['name']} is {'right' if c['skew'] > 0 else 'left'}-skewed",
                        "detail": f"skew {c['skew']:+.2f}; median {c['median']:g} vs mean {c['mean']:g}"
                                  + ("; try a log scale" if c["skew"] > 0 and c["min"] > 0 else "")})
        if c["out_z"] or c["out_iqr"]:
            share = max(c["out_z"], c["out_iqr"]) / max(c["n"], 1)
            out.append({"kind": "outlier", "score": min(share * 10, 1), "show": {"col": c["name"]},
                        "title": f"{c['name']} has outliers",
                        "detail": f"{c['out_z']} past {z:g}σ, {c['out_iqr']} beyond {k:g}×IQR"})
        if c["missing"]:
            out.append({"kind": "missing", "score": c["missing"] / len(df), "show": {"col": c["name"]},
                        "title": f"{c['name']} is {c['missing'] / len(df):.0%} missing",
                        "detail": f"{c['missing']} of {len(df)} rows blank"})

    cats = [c for c in cols if c["kind"] == "category"]
    for c in cats:
        lo, hi = min(c["counts"]), max(c["counts"])
        if hi >= 1.5 * lo:
            out.append({"kind": "balance", "score": 1 - lo / hi, "show": {"col": feats[0], "by": c["name"]},
                        "title": f"{c['name']} is imbalanced",
                        "detail": ", ".join(f"{lv} {n}" for lv, n in zip(c["levels"], c["counts"]))})

    # Keep every kind on screen: the top two of each, then by strength.
    keep, seen = [], {}
    for f in sorted(out, key=lambda f: -f["score"]):
        seen[f["kind"]] = seen.get(f["kind"], 0) + 1
        if seen[f["kind"]] <= (4 if f["kind"] == "correlation" else 3):
            keep.append(f | {"score": round(f["score"], 3)})
    return keep


# ---- routes ----------------------------------------------------------------------------

app = FastAPI(title="Dataset Detective · __HOST__", docs_url=None, redoc_url=None)
applog.info("__HOST__: Dataset Detective starting on 127.0.0.1:%s, %d datasets", os.environ.get("PORT"), len(DATASETS))


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
    out = {}
    for k, d in DATASETS.items():
        df = table(k, 0.0, 0)
        out[k] = {"label": d["label"], "about": d["about"], "rows": len(df), "cols": df.shape[1],
                  "target": d["target"]}
    return {"datasets": out, "colors": GROUP_COLORS}


Missing = Query(0.0, ge=0, le=0.5, description="share of feature cells blanked at random")
Seed = Query(0, ge=0, le=10_000)


@app.get("/api/profile")
def profile(dataset: str = "iris", missing: float = Missing, seed: int = Seed,
            z: float = Query(3.0, ge=1.5, le=6), iqr: float = Query(1.5, ge=0.5, le=5),
            method: str = Query("pearson", pattern="^(pearson|spearman)$")):
    t0 = time.perf_counter()
    df = table(dataset, missing, seed)
    cols = [column_summary(df[c], z, iqr) for c in df.columns]
    num = numeric(df)
    corr = df[num].corr(method=method)
    found = findings(df, cols, df[num].corr(), dataset, z, iqr)
    return {
        "dataset": dataset, "rows": len(df), "target": DATASETS[dataset]["target"],
        "columns": cols,
        "corr": {"names": num, "method": method, "r": [[fnum(v, 3) for v in row] for row in corr.to_numpy()]},
        "complete_rows": int(df.notna().all(axis=1).sum()),
        "findings": found,
        "ms": round((time.perf_counter() - t0) * 1000, 1),
    }


def check_col(df, c, want_numeric=True):
    if c not in df.columns:
        raise HTTPException(422, f"no column {c!r}; have {list(df.columns)}")
    if want_numeric and c not in numeric(df):
        raise HTTPException(422, f"{c!r} is not numeric")


@app.get("/api/column")
def column(dataset: str = "iris", col: str = "petal_length", bins: int = Query(24, ge=4, le=80),
           by: str | None = None, log: bool = False, missing: float = Missing, seed: int = Seed,
           z: float = Query(3.0, ge=1.5, le=6), iqr: float = Query(1.5, ge=0.5, le=5)):
    t0 = time.perf_counter()
    df = table(dataset, missing, seed)
    check_col(df, col)
    if by and by not in categorical(df):
        raise HTTPException(422, f"split by a category: {categorical(df)}")
    x = df[col]
    if log:
        if (x.dropna() <= 0).any():
            raise HTTPException(422, f"{col!r} has values ≤ 0; no log scale")
        x = np.log10(x)
    v = x.dropna()
    edges = np.histogram_bin_edges(v, bins=bins)
    groups = []
    if by:
        for lv, part in x.groupby(df[by], observed=True):
            groups.append({"name": str(lv), "counts": np.histogram(part.dropna(), edges)[0].astype(int).tolist(),
                           "mean": fnum(part.mean())})
    else:
        groups.append({"name": col, "counts": np.histogram(v, edges)[0].astype(int).tolist(), "mean": fnum(v.mean())})
    # Density on the same axis, scaled to counts per bin so it overlays the bars.
    grid = np.linspace(edges[0], edges[-1], 160)
    kde = stats.gaussian_kde(v)(grid) * len(v) * (edges[1] - edges[0]) if v.nunique() > 1 else np.zeros_like(grid)
    q1, med, q3 = v.quantile([0.25, 0.5, 0.75])
    lo, hi = q1 - iqr * (q3 - q1), q3 + iqr * (q3 - q1)
    zs = (v - v.mean()) / v.std()
    out_rows = v[(v < lo) | (v > hi) | (zs.abs() > z)]
    return {
        "col": col, "log": log, "by": by, "edges": [fnum(e, 6) for e in edges], "groups": groups,
        "kde": {"x": [fnum(g, 6) for g in grid], "y": [fnum(y, 4) for y in kde]},
        "box": {"q1": fnum(q1), "median": fnum(med), "q3": fnum(q3),
                "lo": fnum(v[v >= lo].min()), "hi": fnum(v[v <= hi].max()), "mean": fnum(v.mean()),
                "std": fnum(v.std())},
        "outliers": [{"row": int(i), "value": fnum(val), "z": fnum(zs[i], 3)} for i, val in out_rows.items()][:200],
        "normality": {"shapiro_p": fnum(stats.shapiro(v.iloc[:5000]).pvalue, 3)} if len(v) >= 3 else None,
        "ms": round((time.perf_counter() - t0) * 1000, 1),
    }


@app.get("/api/scatter")
def scatter(dataset: str = "iris", x: str = "petal_length", y: str = "petal_width", color: str | None = None,
            missing: float = Missing, seed: int = Seed):
    t0 = time.perf_counter()
    df = table(dataset, missing, seed)
    check_col(df, x)
    check_col(df, y)
    # dict.fromkeys: colouring by y (or plotting x against itself) must not
    # duplicate a column, or d[y] turns into a two-column frame.
    cols = list(dict.fromkeys([x, y] + ([color] if color else [])))
    if color:
        check_col(df, color, want_numeric=False)
    d = df[cols].dropna()
    slope, intercept, r, p, _ = stats.linregress(d[x], d[y])
    rho = stats.spearmanr(d[x], d[y]).statistic
    out = {"x": x, "y": y, "n": len(d), "dropped": len(df) - len(d), "row": d.index.astype(int).tolist(),
           "xs": [fnum(v, 6) for v in d[x]], "ys": [fnum(v, 6) for v in d[y]],
           "fit": {"slope": fnum(slope), "intercept": fnum(intercept), "r": fnum(r), "r2": fnum(r * r),
                   "p": fnum(p, 3), "spearman": fnum(rho)}}
    if color:
        c = d[color]
        if isinstance(c.dtype, pd.CategoricalDtype):
            out["color"] = {"name": color, "kind": "category", "levels": list(map(str, c.cat.categories)),
                            "codes": c.cat.codes.astype(int).tolist()}
            # within each group the slope can differ from, or reverse, the pooled one
            out["groups"] = []
            for lv, part in d.groupby(color, observed=True):
                if len(part) > 2 and part[x].nunique() > 1:
                    g = stats.linregress(part[x], part[y])
                    out["groups"].append({"name": str(lv), "r": fnum(g.rvalue), "slope": fnum(g.slope),
                                          "intercept": fnum(g.intercept), "n": len(part)})
        else:
            out["color"] = {"name": color, "kind": "numeric", "values": [fnum(v, 5) for v in c],
                            "min": fnum(c.min()), "max": fnum(c.max())}
    out["ms"] = round((time.perf_counter() - t0) * 1000, 1)
    return out
