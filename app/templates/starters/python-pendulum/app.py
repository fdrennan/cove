"""Double Pendulum Lab: a fan of double pendulums integrated with scipy.

Every pendulum shares the masses and lengths; their first angles differ by a
hair, fanned evenly across `spread`. POST /api/run integrates all of them as
one system with solve_ivp and answers with the angles at 30 frames a second
(float32, base64: the page interpolates between frames), how far each tip has
wandered from the first pendulum's tip, a Lyapunov-style growth rate fitted to
the nearest neighbour, and the energy drift - the integrator's error made
visible, since a frictionless pendulum keeps its energy exactly.
"""
import base64
import os
import time
from pathlib import Path

import numpy as np
from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse, PlainTextResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field
from scipy.integrate import solve_ivp

from log import log

HERE = Path(__file__).parent
G = 9.81
FPS = 30            # samples per simulated second; the page interpolates up to the display rate
FLOOR = 1e-16       # log plots need a floor: double precision cannot resolve less

# The integrators worth comparing. All explicit Runge-Kutta: the double pendulum
# is not stiff, so an implicit method (Radau, BDF) would spend its Jacobians on nothing.
METHODS = {
    "DOP853": "Dormand-Prince 8(5,3)",
    "RK45": "Dormand-Prince 5(4)",
    "RK23": "Bogacki-Shampine 3(2)",
}

# What the page builds its sliders from, so a limit changed here is changed there.
LIMITS = {
    "m1": {"label": "mass 1 (kg)", "min": 0.1, "max": 5, "step": 0.1, "default": 1.0},
    "m2": {"label": "mass 2 (kg)", "min": 0.1, "max": 5, "step": 0.1, "default": 1.0},
    "l1": {"label": "length 1 (m)", "min": 0.2, "max": 2, "step": 0.05, "default": 1.0},
    "l2": {"label": "length 2 (m)", "min": 0.2, "max": 2, "step": 0.05, "default": 1.0},
    "theta1": {"label": "angle 1 (°)", "min": -180, "max": 180, "step": 1, "default": 120},
    "theta2": {"label": "angle 2 (°)", "min": -180, "max": 180, "step": 1, "default": -20},
    "n": {"label": "pendulums", "min": 2, "max": 100, "step": 1, "default": 30},
    "spread": {"label": "spread (rad, log10)", "min": -12, "max": -1, "step": 0.5, "default": -6},
    "rtol": {"label": "tolerance (log10)", "min": -12, "max": -3, "step": 0.5, "default": -9},
    "duration": {"label": "seconds", "min": 5, "max": 30, "step": 1, "default": 20},
}


class Run(BaseModel):
    m1: float = Field(1.0, ge=0.1, le=5)
    m2: float = Field(1.0, ge=0.1, le=5)
    l1: float = Field(1.0, ge=0.2, le=2)
    l2: float = Field(1.0, ge=0.2, le=2)
    theta1: float = Field(120, ge=-180, le=180, description="degrees from straight down")
    theta2: float = Field(-20, ge=-180, le=180, description="degrees from straight down")
    n: int = Field(30, ge=2, le=100)
    spread: float = Field(-6, ge=-12, le=-1, description="log10 of the radians between first and last pendulum")
    rtol: float = Field(-9, ge=-12, le=-3, description="log10 of the relative tolerance; atol sits 2 decades below")
    method: str = "DOP853"
    duration: float = Field(20, ge=5, le=30)


# ---- the physics ------------------------------------------------------------------------

def rhs(m1, m2, l1, l2):
    """Equations of motion, angles from the downward vertical, for N pendulums at once.

    State layout [θ1 (N), θ2 (N), ω1 (N), ω2 (N)]: one solve_ivp call carries the
    whole fan, so every pendulum is stepped with the same step sizes and the
    differences between them come from the physics, not from separate step choices.

    The textbook form calls sin and cos on a, a-2b, 2(a-b) and a-b. Here one sin
    and one cos of [θ1, θ2] feed everything through the angle-sum identities
    (den = 2m1+m2-m2·cos 2Δ = 2(m1 + m2·sin²Δ), sin(θ1-2θ2) = sin(Δ-θ2)): the
    right-hand side runs tens of thousands of times a run, and on arrays this
    small numpy's per-call cost, not the arithmetic, is what takes the time.
    """
    M = m1 + m2

    def f(_t, y):
        n = y.size // 4
        s, c = np.sin(y[:2 * n]), np.cos(y[:2 * n])
        sa, sb, ca, cb = s[:n], s[n:], c[:n], c[n:]
        wa, wb = y[2 * n:3 * n], y[3 * n:]
        sd = sa * cb - ca * sb                    # sin Δ
        cd = ca * cb + sa * sb                    # cos Δ
        den = 2 * (m1 + m2 * sd * sd)
        wa2, wb2 = wa * wa, wb * wb
        out = np.empty_like(y)
        out[:2 * n] = y[2 * n:]
        out[2 * n:3 * n] = (-G * (M + m1) * sa - m2 * G * (sd * cb - cd * sb)
                            - 2 * m2 * sd * (wb2 * l2 + wa2 * l1 * cd)) / (l1 * den)
        out[3 * n:] = 2 * sd * (M * (wa2 * l1 + G * ca) + m2 * l2 * wb2 * cd) / (l2 * den)
        return out
    return f


def energy(y, m1, m2, l1, l2):
    """Total energy per pendulum per sample; y has shape (4N, T)."""
    n = y.shape[0] // 4
    a, b, wa, wb = y[:n], y[n:2 * n], y[2 * n:3 * n], y[3 * n:]
    kinetic = 0.5 * m1 * (l1 * wa) ** 2 + 0.5 * m2 * ((l1 * wa) ** 2 + (l2 * wb) ** 2
                                                      + 2 * l1 * l2 * wa * wb * np.cos(a - b))
    potential = -(m1 + m2) * G * l1 * np.cos(a) - m2 * G * l2 * np.cos(b)
    return kinetic + potential


def lyapunov(t, d, d_sat):
    """Growth rate (1/s) of ln d while d is small: separation grows like e^(λt) until it
    is as large as the pendulum itself, after which it saturates and says nothing more.

    Fitted from the first sample to the first one past 1% of the saturation scale.
    A regular (non-chaotic) motion never gets there and fits a rate near zero.
    """
    stop = np.argmax(d > 0.01 * d_sat) if np.any(d > 0.01 * d_sat) else len(d)
    if stop < 8:            # saturated in under 8 frames: too few to fit, the spread was too wide
        return None
    slope, intercept = np.polyfit(t[:stop], np.log(np.maximum(d[:stop], FLOOR)), 1)
    return float(slope), float(intercept), float(t[stop - 1])


def b64(a: np.ndarray) -> str:
    return base64.b64encode(np.ascontiguousarray(a, dtype="<f4").tobytes()).decode()


# ---- routes -----------------------------------------------------------------------------

app = FastAPI(title="Double Pendulum Lab · __HOST__", docs_url=None, redoc_url=None)
log.info("__HOST__: Double Pendulum Lab starting on 127.0.0.1:%s", os.environ.get("PORT"))


@app.get("/health", response_class=PlainTextResponse)
def health() -> str:
    """Answers once the app is serving (app.env APP_HEALTH; up.sh waits on it)."""
    return "ok"


app.mount("/static", StaticFiles(directory=HERE / "static"), name="static")


@app.get("/")
def index():
    return FileResponse(HERE / "static" / "index.html")


@app.get("/api/menu")
def menu():
    return {"limits": LIMITS, "methods": METHODS, "fps": FPS}


@app.post("/api/run")
def run(r: Run):
    if r.method not in METHODS:
        raise HTTPException(422, f"method must be one of {list(METHODS)}")
    n = r.n
    offsets = np.linspace(0.0, 10 ** r.spread, n)      # pendulum 0 is the reference, the rest fan out from it
    y0 = np.concatenate([np.radians(r.theta1) + offsets,
                         np.full(n, np.radians(r.theta2)),
                         np.zeros(n), np.zeros(n)])
    frames = int(round(r.duration * FPS)) + 1
    t_eval = np.linspace(0.0, r.duration, frames)
    rtol = 10 ** r.rtol
    t0 = time.perf_counter()
    sol = solve_ivp(rhs(r.m1, r.m2, r.l1, r.l2), (0.0, r.duration), y0, method=r.method,
                    t_eval=t_eval, rtol=rtol, atol=rtol * 1e-2)
    solve_ms = (time.perf_counter() - t0) * 1000
    if not sol.success:
        log.error("integration failed (%s, %d pendulums, rtol %g): %s", r.method, n, rtol, sol.message)
        raise HTTPException(500, f"integration failed: {sol.message}")

    y = sol.y
    a, b = y[:n], y[n:2 * n]
    tip_x = r.l1 * np.sin(a) + r.l2 * np.sin(b)
    tip_y = -r.l1 * np.cos(a) - r.l2 * np.cos(b)
    # distance of each tip from the reference tip, pendulums 1..n-1, shape (n-1, frames)
    dist = np.hypot(tip_x[1:] - tip_x[0], tip_y[1:] - tip_y[0])
    log_d = np.log10(np.maximum(dist, FLOOR))

    # Energy drift relative to the pendulum's own scale, (m1+m2) g l1 + m2 g l2 - the
    # depth of its potential well. Relative to E0 would blow up whenever E0 is near zero,
    # which it is for a pendulum released level with its pivot.
    scale = (r.m1 + r.m2) * G * r.l1 + r.m2 * G * r.l2
    e = energy(y, r.m1, r.m2, r.l1, r.l2)
    drift = np.abs(e - e[:, :1]) / scale
    worst = drift.max(axis=0)

    fit = lyapunov(t_eval, dist[0], r.l1 + r.l2)
    log.info("integrated %d pendulums over %g s with %s in %.0f ms (%d evaluations), energy drift %.1e",
             n, r.duration, r.method, solve_ms, sol.nfev, float(worst.max()))
    return {
        "n": n, "frames": frames, "fps": FPS, "duration": r.duration,
        "l1": r.l1, "l2": r.l2, "m1": r.m1, "m2": r.m2,
        # angles frame-major, [frame][pendulum][θ1, θ2], so frame k is one contiguous slice
        "angles": b64(np.stack([a.T, b.T], axis=2)),
        "divergence": {
            "near": np.round(log_d[0], 4).tolist(),                 # pendulum 1, the closest start
            "median": np.round(np.median(log_d, axis=0), 4).tolist(),
            "far": np.round(log_d[-1], 4).tolist(),                 # the last pendulum, the widest start
        },
        # the fit is ln d = intercept + λ t over [0, until]; the page draws it as a dashed line
        "lyapunov": None if fit is None else {"lambda": round(fit[0], 4), "intercept": round(fit[1], 4),
                                              "until": round(fit[2], 3),
                                              # below 0.05/s the "doubling time" outlasts any run: the motion is regular, not chaotic
                                              "doubling": round(float(np.log(2) / fit[0]), 4) if fit[0] > 0.05 else None},
        "energy": {
            "log_drift": np.round(np.log10(np.maximum(worst, FLOOR)), 4).tolist(),
            "max_drift": float(worst.max()),
            "e0": round(float(e[0, 0]), 6),
        },
        "method": r.method, "rtol": rtol, "nfev": int(sol.nfev),
        "solve_ms": round(solve_ms, 1),
    }
