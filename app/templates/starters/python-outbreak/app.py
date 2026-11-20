"""Outbreak Simulator: an agent-based epidemic, vectorised with numpy.

Every person is a dot on a 100 x 100 map. They sleep in households, and by day
some go to a hub (a school or a market) while the rest wander near home.
Infection passes by proximity: each tick, every infectious-susceptible pair
closer than CONTACT_RADIUS transmits with probability `p_contact`, found with a
scipy cKDTree. An infected person incubates (E), is infectious (I), then
recovers (R); vaccination moves the susceptible straight to immune (V).

Interventions change mid-run and act from the next tick: distancing (people
wander less and stand further apart at hubs), closing schools or markets, a
daily vaccination rate, and daily testing that sends the detected home to
isolate, where they can still infect their household.

Every infection records who caused it, so the reproduction number is read off
the transmission tree, not assumed. All randomness comes from one generator
seeded per run: the same seed with the same interventions at the same ticks
replays the same epidemic, however the page batches its steps.
"""
import base64
import logging
import os
import re
import threading
import time
from collections import OrderedDict
from pathlib import Path

import numpy as np
from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field
from scipy.spatial import cKDTree

from log import log

HERE = Path(__file__).parent

WORLD = 100.0          # map side, arbitrary units
TICKS_PER_DAY = 12     # 0-3 night at home, 4-9 out, 10-11 evening near home
DAY_TICKS = range(4, 10)
NIGHT_TICKS = range(0, 4)
CONTACT_RADIUS = 1.0   # closer than this, a pair is a contact for that tick
HOUSEHOLD_R = 0.35     # members sleep this close: inside one contact radius
HUB_R = 3.2            # people at a hub spread over this radius
WANDER = 4.0           # how far a stay-at-home person drifts from the door
SPEED = 0.4            # share of the way to the target covered per tick
JITTER = 0.15          # step noise, so nobody stands still
SCHOOL, MARKET = 0, 1
STATES = "SEIRV"       # index = state code sent to the page
S, E, I, R, V = range(5)

# Each simulation is about 30 small arrays of n entries. One per client, capped,
# least recently used dropped first: an abandoned tab must not hold memory for
# the life of the server, and nobody can grow it by opening tabs.
MAX_SIMS = 24
SIMS: "OrderedDict[str, Sim]" = OrderedDict()
SIMS_LOCK = threading.Lock()
CLIENT_ID = re.compile(r"^[A-Za-z0-9_-]{8,64}$")

# Neutral for the susceptible majority, four hues for the states that matter.
# Validated on the dark surface: E/I sit in the CVD warn band, so the map also
# encodes state by shape and size and the curve labels every line directly.
COLORS = {"S": "#5d6680", "E": "#b58f00", "I": "#e66767", "R": "#199e70", "V": "#3987e5"}


# ---- the menu ------------------------------------------------------------------------

def p(label, lo, hi, step, default, unit="", integer=False):
    return {"label": label, "min": lo, "max": hi, "step": step, "default": default, "unit": unit, "integer": integer}


DISEASE = {
    "n": p("people", 200, 5000, 100, 2000, integer=True),
    "p_contact": p("transmission per contact-tick", 0.001, 0.05, 0.001, 0.01),
    "incubation": p("incubation", 0.5, 10, 0.5, 3, unit="days"),
    "infectious": p("infectious", 1, 14, 0.5, 5, unit="days"),
    "initial": p("first cases", 1, 50, 1, 5, integer=True),
}

POLICY = {
    "distancing": p("distancing", 0, 1, 0.05, 0, unit="%") | {"short": "distancing"},
    "vaccination": p("vaccinated per day", 0, 0.05, 0.0025, 0, unit="%") | {"short": "vaccinating"},
    "testing": p("infectious found per day", 0, 1, 0.05, 0, unit="%") | {"short": "testing"},
}


class Policy(BaseModel):
    distancing: float = Field(0.0, ge=0, le=1, description="0 none, 1 people barely leave their spot")
    close_schools: bool = False
    close_markets: bool = False
    vaccination: float = Field(0.0, ge=0, le=0.05, description="share of the population vaccinated per day")
    testing: float = Field(0.0, ge=0, le=1, description="chance per day an infectious person is found and isolated")


class New(BaseModel):
    client: str = Field(..., description="the page's id; one simulation per id")
    seed: int = Field(1, ge=0, le=1_000_000)
    n: int = Field(2000, ge=200, le=5000)
    p_contact: float = Field(0.01, ge=0.001, le=0.05)
    incubation: float = Field(3.0, ge=0.5, le=10)
    infectious: float = Field(5.0, ge=1, le=14)
    initial: int = Field(5, ge=1, le=50)
    policy: Policy = Field(default_factory=Policy)


class Step(BaseModel):
    ticks: int = Field(1, ge=1, le=TICKS_PER_DAY * 10)
    since: int = Field(0, ge=0, description="days of history the page already has; only later rows are sent")


# ---- the work --------------------------------------------------------------------------

class Sim:
    def __init__(self, q: New):
        self.q = q
        self.rng = rng = np.random.default_rng(q.seed)
        self.lock = threading.Lock()
        self.policy = q.policy
        self.policy_log = [{"tick": 0, **q.policy.model_dump()}]
        n = self.n = q.n

        # Four neighbourhoods of households, a school and a market in each.
        centres = np.array([[27, 27], [73, 27], [27, 73], [73, 73]], float) + rng.normal(0, 3, (4, 2))
        n_hh = max(1, round(n / 2.7))
        hood = rng.integers(0, 4, n_hh)
        hh_xy = np.clip(centres[hood] + rng.normal(0, 9, (n_hh, 2)), 3, WORLD - 3)
        self.hh = hh = rng.integers(0, n_hh, n)
        self.hh_xy = hh_xy
        r = HOUSEHOLD_R * np.sqrt(rng.random(n))
        a = rng.random(n) * 2 * np.pi
        self.home = hh_xy[hh] + np.column_stack([r * np.cos(a), r * np.sin(a)])
        # school and market on opposite sides of the centre, so their crowds stay apart
        side = np.array([[-6.0, -4.0]])
        self.hubs = np.vstack([centres + side + rng.normal(0, 1.5, (4, 2)), centres - side + rng.normal(0, 1.5, (4, 2))])
        self.hub_kind = np.repeat([SCHOOL, MARKET], 4)

        # A quarter are children at their own neighbourhood's school, nearly every
        # day. The rest shop, most at their own market, some across town: those
        # trips are what carries an outbreak from one neighbourhood to the next.
        my_hood = hood[hh]
        kid = rng.random(n) < 0.25
        away = rng.random(n) < 0.2
        self.hub = np.where(kid, my_hood, 4 + np.where(away, rng.integers(0, 4, n), my_hood))
        self.go_p = np.where(kid, 0.95, 0.45)

        self.pos = self.home.copy()
        self.wander = np.zeros((n, 2))
        self.hub_off = np.zeros((n, 2))
        self.out_today = np.zeros(n, bool)
        self.state = np.full(n, S, np.uint8)
        self.timer = np.zeros(n, np.int32)
        self.infector = np.full(n, -1, np.int32)
        self.inf_tick = np.full(n, -1, np.int32)
        self.gen = np.full(n, -1, np.int32)        # 0 for the first cases, 1 for those they infect, ...
        self.isolated = np.zeros(n, bool)
        self.dose_carry = 0.0
        self.tick = 0
        self.history = []       # one row per completed day
        self.links = []         # infector -> infectee pairs since the last frame

        first = rng.choice(n, size=min(q.initial, n), replace=False)
        self.state[first] = I
        self.timer[first] = self.duration(q.infectious, len(first))
        self.inf_tick[first] = 0
        self.gen[first] = 0

    def duration(self, mean_days: float, k: int) -> np.ndarray:
        """Ticks spent in a state: gamma with shape 4, so most people sit near the mean."""
        return np.maximum(1, np.round(self.rng.gamma(4, mean_days / 4, k) * TICKS_PER_DAY)).astype(np.int32)

    def start_day(self):
        rng, n, pol = self.rng, self.n, self.policy
        self.out_today = rng.random(n) < self.go_p
        d = pol.distancing
        r = HUB_R * (1 + d) * np.sqrt(rng.random(n))      # distancing spreads a crowd out
        a = rng.random(n) * 2 * np.pi
        self.hub_off = np.column_stack([r * np.cos(a), r * np.sin(a)])

        found = (self.state == I) & ~self.isolated & (rng.random(n) < pol.testing)
        self.isolated |= found

        self.dose_carry += pol.vaccination * n
        doses = int(self.dose_carry)
        self.dose_carry -= doses
        sus = np.flatnonzero(self.state == S)
        if doses and sus.size:
            # Doses go to the susceptible only, chosen at random, and protect
            # fully; nobody is tested before a dose, so none are wasted either.
            self.state[rng.choice(sus, size=min(doses, sus.size), replace=False)] = V

    def move(self):
        rng, n, pol = self.rng, self.n, self.policy
        tod = self.tick % TICKS_PER_DAY
        d = pol.distancing
        # Draw every agent's noise every tick, whatever their state, so the
        # stream of random numbers does not depend on who is out.
        self.wander = 0.9 * self.wander + rng.normal(0, 1, (n, 2))
        noise = rng.normal(0, JITTER * (1 - 0.7 * d), (n, 2))

        target = self.home + self.wander * (WANDER * (1 - 0.85 * d) / 4.4)   # 4.4 ~ sd of the AR(1) walk
        if tod in NIGHT_TICKS:
            target = self.home
        hub_open = np.array([not pol.close_schools, not pol.close_markets])
        if tod in DAY_TICKS:
            out = self.out_today & hub_open[self.hub_kind[self.hub]]
            target = np.where(out[:, None], self.hubs[self.hub] + self.hub_off, target)
        target = np.where(self.isolated[:, None], self.home, target)
        self.pos = np.clip(self.pos + SPEED * (target - self.pos) + noise, 0, WORLD)

    def transmit(self):
        inf = np.flatnonzero(self.state == I)
        sus = np.flatnonzero(self.state == S)
        if not inf.size or not sus.size:
            return
        m = cKDTree(self.pos[inf]).sparse_distance_matrix(cKDTree(self.pos[sus]), CONTACT_RADIUS, output_type="ndarray")
        if not m.size:
            return
        src, dst = inf[m["i"]], sus[m["j"]]
        keep = ~self.isolated[src] | (self.hh[src] == self.hh[dst])   # isolating: household only
        src, dst = src[keep], dst[keep]
        hit = self.rng.random(src.size) < self.q.p_contact
        src, dst = src[hit], dst[hit]
        if not dst.size:
            return
        # Several contacts can infect one person in the same tick; credit one,
        # picked at random, so the tree stays a tree.
        order = self.rng.permutation(dst.size)
        dst, first = np.unique(dst[order], return_index=True)
        src = src[order][first]
        self.state[dst] = E
        self.timer[dst] = self.duration(self.q.incubation, dst.size)
        self.infector[dst] = src
        self.inf_tick[dst] = self.tick
        self.gen[dst] = self.gen[src] + 1
        self.links.extend(zip(src.tolist(), dst.tolist()))

    def progress(self):
        live = (self.state == E) | (self.state == I)
        self.timer[live] -= 1
        due = live & (self.timer <= 0)
        onset = np.flatnonzero(due & (self.state == E))
        done = np.flatnonzero(due & (self.state == I))
        self.state[onset] = I
        self.timer[onset] = self.duration(self.q.infectious, onset.size)
        self.state[done] = R
        self.isolated[done] = False

    def counts(self) -> dict:
        c = np.bincount(self.state, minlength=5)
        return {**{k: int(c[i]) for i, k in enumerate(STATES)}, "isolated": int(self.isolated.sum())}

    def step(self):
        if self.tick % TICKS_PER_DAY == 0:
            self.start_day()
        self.move()
        self.transmit()
        self.progress()
        self.tick += 1
        if self.tick % TICKS_PER_DAY == 0:
            day = self.tick // TICKS_PER_DAY
            lo = (day - 1) * TICKS_PER_DAY
            new = int(((self.inf_tick >= lo) & (self.inf_tick < self.tick) & (self.infector >= 0)).sum())
            self.history.append({"day": day, **self.counts(), "new": new})

    def reproduction(self) -> dict:
        """The realised R, from the tree: secondary cases per case whose infection is over.

        A cohort (everyone infected on one day) counts only once all of its
        members have recovered; until then its count of secondaries can still
        grow, and averaging it in would bias R down.
        """
        infected = self.inf_tick >= 0
        if not infected.any():
            return {"r_recent": None, "r_early": None, "gen_days": None, "series": []}
        sec = np.bincount(self.infector[self.infector >= 0], minlength=self.n)
        cohort = np.where(infected, self.inf_tick // TICKS_PER_DAY, -1)
        days = int(cohort.max()) + 1
        total = np.bincount(cohort[infected], minlength=days)
        finished = np.bincount(cohort[infected & (self.state == R)], minlength=days)
        secs = np.bincount(cohort[infected], weights=sec[infected], minlength=days)
        seen = np.flatnonzero(total > 0)
        # Complete cohorts in order, stopping at the first that is still running:
        # a later cohort can finish first, but its R would be read out of order.
        open_ = np.flatnonzero(finished[seen] < total[seen])
        idx = seen[: open_[0]] if open_.size else seen
        series = [{"day": int(d), "r": round(float(secs[d] / total[d]), 3), "cases": int(total[d])} for d in idx]

        def pooled(ix):
            return round(float(secs[ix].sum() / total[ix].sum()), 3) if total[ix].sum() >= 5 else None

        # Early R: the first cases and the people they infected, while nearly
        # everyone was susceptible; the closest the tree comes to R0.
        early = (self.gen >= 0) & (self.gen <= 1)
        early_done = (self.state[early] == R).all()
        kids = np.flatnonzero(self.infector >= 0)
        gen = (self.inf_tick[kids] - self.inf_tick[self.infector[kids]]) / TICKS_PER_DAY
        return {
            "r_recent": pooled(idx[-7:]),
            "r_early": round(float(sec[early].mean()), 3) if early_done else None,
            "gen_days": round(float(gen.mean()), 2) if gen.size else None,
            "series": series,
        }

    def frame(self, step_ms: float = 0.0, since: int = 0) -> dict:
        xy = np.round(self.pos / WORLD * 65535).astype("<u2")
        st = self.state | (self.isolated.astype(np.uint8) << 3)
        links, self.links = self.links[-400:], []
        rep = self.reproduction()
        return {
            "tick": self.tick,
            "day": round(self.tick / TICKS_PER_DAY, 3),
            "xy": base64.b64encode(xy.tobytes()).decode(),
            "st": base64.b64encode(st.tobytes()).decode(),
            "links": links,
            "now": self.counts(),
            "ever": int((self.inf_tick >= 0).sum()),
            "history": self.history[since:],
            "r_recent": rep["r_recent"], "r_early": rep["r_early"], "gen_days": rep["gen_days"],
            "step_ms": round(step_ms, 2),
        }

    def map(self) -> dict:
        homes = self.hh_xy[np.unique(self.hh)]          # households with someone living there
        return {"world": WORLD, "contact_radius": CONTACT_RADIUS, "hub_radius": HUB_R,
                "hubs": [{"x": round(float(x), 2), "y": round(float(y), 2), "kind": ["school", "market"][k]}
                         for (x, y), k in zip(self.hubs, self.hub_kind)],
                "homes": np.round(homes, 1).ravel().tolist()}


def lookup(client: str) -> Sim:
    with SIMS_LOCK:
        sim = SIMS.get(client)
        if sim is None:
            raise HTTPException(404, "no simulation for this client; POST /api/sim to start one")
        SIMS.move_to_end(client)
        return sim


# ---- routes ----------------------------------------------------------------------------

app = FastAPI(title="Outbreak Simulator · __HOST__", docs_url=None, redoc_url=None)
log.info("__HOST__: Outbreak Simulator starting on 127.0.0.1:%s, up to %d simulations", os.environ.get("PORT"), MAX_SIMS)


class StepsAtDebug(logging.Filter):
    """The page posts .../step every animation frame: those access lines are DEBUG, the rest INFO."""
    def filter(self, r):
        if r.args and str(r.args[2]).endswith("/step"):
            r.levelno, r.levelname = logging.DEBUG, "DEBUG"
            return logging.getLogger().isEnabledFor(logging.DEBUG)
        return True


logging.getLogger("uvicorn.access").addFilter(StepsAtDebug())


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
    return {"disease": DISEASE, "policy": POLICY, "colors": COLORS, "states": list(STATES),
            "ticks_per_day": TICKS_PER_DAY, "max_sims": MAX_SIMS}


@app.post("/api/sim")
def new_sim(q: New):
    """Start (or restart) this client's simulation; answers with the map and frame 0."""
    if not CLIENT_ID.match(q.client):
        raise HTTPException(422, "client must be 8-64 of A-Z a-z 0-9 _ -")
    sim = Sim(q)
    with SIMS_LOCK:
        SIMS.pop(q.client, None)
        SIMS[q.client] = sim
        while len(SIMS) > MAX_SIMS:
            SIMS.popitem(last=False)
        live = len(SIMS)
    log.info("new simulation: %d people, %d infected, seed %d (%d live)", q.n, q.initial, q.seed, live)
    return {"map": sim.map(), "frame": sim.frame(), "params": q.model_dump(exclude={"client"})}


@app.post("/api/sim/{client}/step")
def step(client: str, s: Step):
    sim = lookup(client)
    with sim.lock:
        t0 = time.perf_counter()
        for _ in range(s.ticks):
            sim.step()
        ms = (time.perf_counter() - t0) * 1000
        log.debug("stepped %d ticks to tick %d in %.1f ms", s.ticks, sim.tick, ms)
        return sim.frame(ms, s.since)


@app.post("/api/sim/{client}/policy")
def policy(client: str, pol: Policy):
    """Change interventions; they act from the next tick and are logged for the curve."""
    sim = lookup(client)
    with sim.lock:
        sim.policy = pol
        entry = {"tick": sim.tick, **pol.model_dump()}
        if sim.policy_log[-1]["tick"] == sim.tick:
            sim.policy_log[-1] = entry
        else:
            sim.policy_log.append(entry)
        log.info("policy at day %d: %s", sim.tick // TICKS_PER_DAY, pol.model_dump())
        return {"tick": sim.tick, "policy": pol.model_dump()}


@app.get("/api/sim/{client}")
def summary(client: str):
    """The whole run so far: daily counts, R by infection cohort, interventions by tick."""
    sim = lookup(client)
    with sim.lock:
        rep = sim.reproduction()
        peak = max(sim.history, key=lambda h: h["I"], default=None)
        return {"params": sim.q.model_dump(exclude={"client"}), "tick": sim.tick,
                "history": sim.history, "policy_log": sim.policy_log,
                "ever": int((sim.inf_tick >= 0).sum()), "n": sim.n,
                "peak": peak and {"day": peak["day"], "I": peak["I"]}, **rep}
