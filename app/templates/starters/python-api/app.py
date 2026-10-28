"""Scientific API: statistics, fits, distributions and ODEs over HTTP.

numpy and scipy do the work; FastAPI validates the input, answers in JSON and
writes the interactive reference at /docs. Every answer carries `ms`, the
server's own compute time.
"""
import time
from functools import wraps
from pathlib import Path

import numpy as np
import scipy
from fastapi import FastAPI, HTTPException, Query
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field, FiniteFloat
from scipy import stats
from scipy.integrate import solve_ivp

HERE = Path(__file__).parent

app = FastAPI(
    title="Scientific API · __HOST__",
    version="1.0.0",
    description="Statistics, least-squares fits, probability distributions and ODEs, computed with numpy and scipy.",
)
app.mount("/static", StaticFiles(directory=HERE / "static"), name="static")


def timed(fn):
    """Add the compute time in milliseconds to the answer."""
    @wraps(fn)
    def inner(*a, **k):
        t0 = time.perf_counter()
        out = fn(*a, **k)
        out["ms"] = round((time.perf_counter() - t0) * 1000, 3)
        return out
    return inner


def clean(a) -> list:
    return [round(float(v), 10) for v in np.asarray(a)]


# ---- models ------------------------------------------------------------------

class Numbers(BaseModel):
    values: list[FiniteFloat] = Field(..., min_length=1, max_length=1_000_000,
                                      examples=[[2.1, 3.4, 1.9, 5.6, 4.4, 3.3, 2.8]])
    probs: list[float] = Field([0.05, 0.25, 0.5, 0.75, 0.95], description="quantiles to report, each in [0, 1]")


class XY(BaseModel):
    x: list[FiniteFloat] = Field(..., min_length=3, examples=[[1, 2, 3, 4, 5, 6]])
    y: list[FiniteFloat] = Field(..., min_length=3, examples=[[2.2, 4.1, 6.3, 7.9, 10.2, 11.8]])


# ---- routes --------------------------------------------------------------------

@app.get("/", include_in_schema=False)
def index():
    return FileResponse(HERE / "static" / "index.html")


@app.get("/health", tags=["meta"])
def health():
    return {"ok": True, "numpy": np.__version__, "scipy": scipy.__version__}


@app.post("/stats/describe", tags=["statistics"])
@timed
def describe(body: Numbers):
    """Summary statistics of a sample: centre, spread, shape, quantiles and a 95% CI for the mean."""
    if any(not 0 <= p <= 1 for p in body.probs):
        raise HTTPException(422, "every prob must be in [0, 1]")
    v = np.asarray(body.values, dtype=float)
    n = v.size
    sd = float(v.std(ddof=1)) if n > 1 else None
    se = sd / np.sqrt(n) if sd is not None else None
    ci = None
    if se is not None:
        h = float(stats.t.ppf(0.975, n - 1)) * se
        ci = [float(v.mean()) - h, float(v.mean()) + h]
    spread = n > 2 and np.ptp(v) > 0
    return {
        "n": n,
        "mean": float(v.mean()),
        "sd": sd,
        "se": se,
        "ci95_mean": ci,
        "min": float(v.min()),
        "max": float(v.max()),
        "median": float(np.median(v)),
        "iqr": float(np.subtract(*np.quantile(v, [0.75, 0.25]))),
        "skewness": float(stats.skew(v)) if spread else None,
        "kurtosis": float(stats.kurtosis(v)) if spread else None,
        "quantiles": {str(p): float(q) for p, q in zip(body.probs, np.quantile(v, body.probs))},
    }


@app.post("/fit/linear", tags=["models"])
@timed
def fit_linear(body: XY):
    """Ordinary least squares y = intercept + slope·x, with standard errors and R²."""
    if len(body.x) != len(body.y):
        raise HTTPException(422, "x and y must be the same length")
    x, y = np.asarray(body.x), np.asarray(body.y)
    if np.ptp(x) == 0:
        raise HTTPException(422, "x must not be constant")
    X = np.column_stack([np.ones_like(x), x])
    coef, *_ = np.linalg.lstsq(X, y, rcond=None)
    fitted = X @ coef
    resid = y - fitted
    n = x.size
    sse = float(resid @ resid)
    sst = float(((y - y.mean()) ** 2).sum())
    sigma2 = sse / (n - 2)
    cov = sigma2 * np.linalg.inv(X.T @ X)
    se = np.sqrt(np.diag(cov))
    tval = coef[1] / se[1] if se[1] > 0 else float("inf")
    return {
        "n": n,
        "intercept": float(coef[0]),
        "slope": float(coef[1]),
        "se_intercept": float(se[0]),
        "se_slope": float(se[1]),
        "r2": 1 - sse / sst if sst > 0 else 1.0,
        "residual_sd": float(np.sqrt(sigma2)),
        "p_slope": float(2 * stats.t.sf(abs(tval), n - 2)),
        "fitted": clean(fitted),
    }


# Query floats refuse nan and inf, and stay inside bounds where the arithmetic
# is finite: mu=nan, sigma=inf and K=inf were 500s (2026-10-26); now 422.
@app.get("/distributions/normal", tags=["distributions"])
@timed
def normal(mu: float = Query(0.0, ge=-1e12, le=1e12, allow_inf_nan=False, description="mean"),
           sigma: float = Query(1.0, gt=0, le=1e12, allow_inf_nan=False, description="standard deviation"),
           n: int = Query(201, ge=2, le=10_000, description="points across mu ± 4 sigma")):
    """Density and CDF of Normal(mu, sigma) on a grid spanning four standard deviations."""
    x = np.linspace(mu - 4 * sigma, mu + 4 * sigma, n)
    d = stats.norm(mu, sigma)
    return {"mu": mu, "sigma": sigma, "x": clean(x), "density": clean(d.pdf(x)), "cdf": clean(d.cdf(x)),
            "quantiles": {"0.025": float(d.ppf(0.025)), "0.5": float(d.ppf(0.5)), "0.975": float(d.ppf(0.975))}}


@app.get("/ode/logistic", tags=["ode"])
@timed
def logistic(r: float = Query(0.8, gt=0, le=50, allow_inf_nan=False, description="growth rate"),
             K: float = Query(100.0, gt=0, le=1e12, allow_inf_nan=False, description="carrying capacity"),
             y0: float = Query(2.0, gt=0, le=1e12, allow_inf_nan=False, description="starting population"),
             t_end: float = Query(20.0, gt=0, le=1000, allow_inf_nan=False),
             n: int = Query(101, ge=2, le=5000, description="output points")):
    """dy/dt = r·y·(1 − y/K), solved with scipy's solve_ivp (RK45) and checked against the closed form."""
    t = np.linspace(0, t_end, n)
    sol = solve_ivp(lambda _t, y: r * y * (1 - y / K), (0, t_end), [y0], t_eval=t, rtol=1e-8, atol=1e-10)
    if not sol.success:
        raise HTTPException(500, sol.message)
    exact = K / (1 + (K - y0) / y0 * np.exp(-r * t))
    return {"r": r, "K": K, "y0": y0, "t": clean(sol.t), "y": clean(sol.y[0]),
            "max_abs_error": float(np.abs(sol.y[0] - exact).max()), "evaluations": int(sol.nfev),
            "inflection_t": float(np.log((K - y0) / y0) / r) if K > y0 else None}
