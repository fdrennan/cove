// Orbital elements and Kepler's equation, shared by the server (require) and the
// browser (<script>): one file, so the page animates with exactly the equations
// GET /api/positions answers with.
//
// Planets: E. M. Standish, "Keplerian Elements for Approximate Positions of the
// Major Planets", JPL Solar System Dynamics, Table 1 (fit to DE405 over
// 1800-2050 AD; mean ecliptic and equinox of J2000). Each element is
// value + rate per Julian century from J2000.0. Earth is the Earth-Moon barycentre.
// Small bodies: rounded osculating elements (JPL Small-Body Database), held fixed
// and moved two-body from their perihelion date - no planetary perturbations.
(function (root, factory) {
  if (typeof module === "object" && module.exports) module.exports = factory();
  else root.Orbits = factory();
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  const DEG = Math.PI / 180;
  const J2000 = 2451545.0;                 // 2000-01-01 12:00 TT
  const DAY_MS = 86400000;
  const UNIX_JD = 2440587.5;               // JD at 1970-01-01 00:00 UTC
  const GAUSS_K = 0.01720209895;           // Gaussian gravitational constant, rad/day for a = 1 AU
  const AU_KM = 149597870.7;
  const LIGHT_MIN_PER_AU = 499.004784 / 60;
  const VALID = [2378496.5, 2469807.5];    // 1800-01-01 .. 2050-12-31, Table 1's fit span

  // [a (AU), e, I (deg), L (deg), varpi (deg), Omega (deg)], then rates per century.
  const PLANETS = [
    ["mercury", "Mercury", "#b9b3a8", 2.4, [0.38709927, 0.20563593, 7.00497902, 252.25032350, 77.45779628, 48.33076593],
      [0.00000037, 0.00001906, -0.00594749, 149472.67411175, 0.16047689, -0.12534081]],
    ["venus", "Venus", "#f1d39a", 3.6, [0.72333566, 0.00677672, 3.39467605, 181.97909950, 131.60246718, 76.67984255],
      [0.00000390, -0.00004107, -0.00078890, 58517.81538729, 0.00268329, -0.27769418]],
    ["earth", "Earth", "#5aa9ff", 3.8, [1.00000261, 0.01671123, -0.00001531, 100.46457166, 102.93768193, 0.0],
      [0.00000562, -0.00004392, -0.01294668, 35999.37244981, 0.32327364, 0.0]],
    ["mars", "Mars", "#ff7a52", 3.0, [1.52371034, 0.09339410, 1.84969142, -4.55343205, -23.94362959, 49.55953891],
      [0.00001847, 0.00007882, -0.00813131, 19140.30268499, 0.44441088, -0.29257343]],
    ["jupiter", "Jupiter", "#e8b98a", 7.0, [5.20288700, 0.04838624, 1.30439695, 34.39644051, 14.72847983, 100.47390909],
      [-0.00011607, -0.00013253, -0.00183714, 3034.74612775, 0.21252668, 0.20469106]],
    ["saturn", "Saturn", "#f3dfa2", 6.2, [9.53667594, 0.05386179, 2.48599187, 49.95424423, 92.59887831, 113.66242448],
      [-0.00125060, -0.00050991, 0.00193609, 1222.49362201, -0.41897216, -0.28867794]],
    ["uranus", "Uranus", "#9fe3e8", 5.0, [19.18916464, 0.04725744, 0.77263783, 313.23810451, 170.95427630, 74.01692503],
      [-0.00196176, -0.00004397, -0.00242939, 428.48202785, 0.40805281, 0.04240589]],
    ["neptune", "Neptune", "#6f8cff", 5.0, [30.06992276, 0.00859048, 1.77004347, -55.12002969, 44.96476227, 131.78422574],
      [0.00026291, 0.00005105, 0.00035372, 218.45945325, -0.32241464, -0.00508664]],
    ["pluto", "Pluto", "#d6c0a8", 2.4, [39.48211675, 0.24882730, 17.14001206, 238.92903833, 224.06891629, 110.30393684],
      [-0.00031596, 0.00005170, 0.00004818, 145.20780515, -0.04062942, -0.01183482]],
  ];

  // [id, name, kind, colour, a (AU), e, i, Omega, omega (deg), perihelion date (TT)]
  const SMALL = [
    ["ceres", "Ceres", "dwarf planet", "#c9c2b6", 2.7663, 0.0789, 10.587, 80.25, 73.29, "2022-12-07"],
    ["vesta", "Vesta", "asteroid", "#bfb3a3", 2.3617, 0.0887, 7.144, 103.71, 151.66, "2021-12-26"],
    ["halley", "1P/Halley", "comet", "#9fd8ff", 17.8341, 0.96714291, 162.26269, 58.42008, 111.33249, "1986-02-09T11:00"],
    ["encke", "2P/Encke", "comet", "#b8f0d0", 2.2150, 0.8483, 11.78, 334.57, 186.54, "2023-10-22"],
  ];

  const jdFromMs = (ms) => ms / DAY_MS + UNIX_JD;
  const msFromJd = (jd) => (jd - UNIX_JD) * DAY_MS;
  // "2026-10-26", "2026-10-26T18:00" or a bare Julian date; read as UTC (UTC vs TT is ~69 s, far below these elements' error).
  function parseDate(s) {
    if (s == null || s === "") return jdFromMs(Date.now());
    if (/^\d+(\.\d+)?$/.test(String(s)) && +s > 1e6) return +s;
    const t = Date.parse(/[zZ]|[+-]\d\d:?\d\d$/.test(s) ? s : (/T/.test(s) ? s + "Z" : s + "T00:00Z"));
    if (Number.isNaN(t)) throw new Error(`not a date: ${s}`);
    return jdFromMs(t);
  }
  const isoDate = (jd) => new Date(msFromJd(jd)).toISOString().slice(0, 16).replace("T", " ");
  const wrap180 = (d) => { d = ((d + 180) % 360 + 360) % 360 - 180; return d === -180 ? 180 : d; };

  // Kepler's equation M = E - e sin E, by Newton's method. Starting from E = pi for
  // e > 0.8 keeps it convergent near perihelion of a comet, where E ~ M fails.
  function kepler(M, e) {
    M = Math.atan2(Math.sin(M), Math.cos(M));          // to (-pi, pi]
    let E = e > 0.8 ? Math.PI * Math.sign(M || 1) : M + e * Math.sin(M);
    for (let i = 0; i < 50; i++) {
      const dE = (E - e * Math.sin(E) - M) / (1 - e * Math.cos(E));
      E -= dE;
      if (Math.abs(dE) < 1e-12) break;
    }
    return E;
  }

  // Elements of one body at Julian date jd, in radians: { a, e, i, node, peri, M }.
  function elementsAt(body, jd) {
    if (body.small) {
      const n = GAUSS_K / Math.pow(body.a, 1.5);       // mean motion, rad/day (massless body)
      return { a: body.a, e: body.e, i: body.i * DEG, node: body.node * DEG, peri: body.peri * DEG, M: n * (jd - body.tp) };
    }
    const T = (jd - J2000) / 36525, v = body.el.map((x, k) => x + body.rate[k] * T);
    const [a, e, I, L, varpi, node] = v;
    // Table 1 needs no extra terms: M = L - varpi, omega = varpi - Omega.
    return { a, e, i: I * DEG, node: node * DEG, peri: (varpi - node) * DEG, M: (L - varpi) * DEG };
  }

  // Heliocentric ecliptic J2000 position (AU) from elements: solve Kepler, place the
  // body in its orbital plane, rotate by omega, i, Omega (Standish section 8).
  function place(el) {
    const E = kepler(el.M, el.e);
    const xp = el.a * (Math.cos(E) - el.e), yp = el.a * Math.sqrt(1 - el.e * el.e) * Math.sin(E);
    const co = Math.cos(el.peri), so = Math.sin(el.peri), cO = Math.cos(el.node), sO = Math.sin(el.node);
    const ci = Math.cos(el.i), si = Math.sin(el.i);
    return {
      x: (co * cO - so * sO * ci) * xp + (-so * cO - co * sO * ci) * yp,
      y: (co * sO + so * cO * ci) * xp + (-so * sO + co * cO * ci) * yp,
      z: (so * si) * xp + (co * si) * yp,
      E,
    };
  }

  const position = (body, jd) => place(elementsAt(body, jd));

  // The whole orbit as a closed ring of points, sampled evenly in E so the
  // perihelion end of a comet's ellipse is as smooth as the far end.
  function orbitPath(body, jd, n) {
    const el = elementsAt(body, jd), pts = new Float64Array(n * 3);
    for (let k = 0; k < n; k++) {
      const E = (2 * Math.PI * k) / n, M = E - el.e * Math.sin(E);
      const p = place({ ...el, M });
      pts[3 * k] = p.x; pts[3 * k + 1] = p.y; pts[3 * k + 2] = p.z;
    }
    return pts;
  }

  const BODIES = [
    ...PLANETS.map(([id, name, color, size, el, rate]) => ({
      id, name, kind: id === "pluto" ? "dwarf planet" : "planet", color, size, el, rate, small: false,
      period: (360 * 36525) / rate[3],               // sidereal period in days, from the mean-longitude rate
    })),
    ...SMALL.map(([id, name, kind, color, a, e, i, node, peri, tp]) => ({
      id, name, kind, color, size: kind === "comet" ? 2.2 : 2.4, a, e, i, node, peri, tp: parseDate(tp), small: true,
      period: (2 * Math.PI) / (GAUSS_K / Math.pow(a, 1.5)),
    })),
  ];
  const byId = Object.fromEntries(BODIES.map((b) => [b.id, b]));

  // Everything the info card and the API report for one body at one date.
  function state(body, jd) {
    const el = elementsAt(body, jd), p = place(el);
    const r = Math.hypot(p.x, p.y, p.z);
    const nu = 2 * Math.atan2(Math.sqrt(1 + el.e) * Math.sin(p.E / 2), Math.sqrt(1 - el.e) * Math.cos(p.E / 2));
    return {
      id: body.id, x: p.x, y: p.y, z: p.z, r,
      lon: ((Math.atan2(p.y, p.x) / DEG) % 360 + 360) % 360,       // heliocentric ecliptic longitude
      lat: Math.asin(p.z / r) / DEG,
      a: el.a, e: el.e, i: el.i / DEG,
      M: wrap180(el.M / DEG), nu: nu / DEG,
    };
  }

  // Angle (deg) between two bodies as seen from a third (Earth for the sky, Sun for heliocentric).
  function separation(a, b, from) {
    const u = [a.x - from.x, a.y - from.y, a.z - from.z], v = [b.x - from.x, b.y - from.y, b.z - from.z];
    const dot = u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    const cx = u[1] * v[2] - u[2] * v[1], cy = u[2] * v[0] - u[0] * v[2], cz = u[0] * v[1] - u[1] * v[0];
    return Math.atan2(Math.hypot(cx, cy, cz), dot) / DEG;   // atan2 stays accurate near 0 and 180, acos does not
  }

  // Dates where the heliocentric longitudes of a and b coincide: for Earth and an
  // outer planet that is opposition, for Earth and an inner planet inferior
  // conjunction. Steps through time, then bisects each sign change to ~1 minute.
  function lineups(a, b, fromJd, count, horizonDays) {
    const f = (jd) => wrap180(state(b, jd).lon - state(a, jd).lon);
    const synodic = Math.abs(1 / (1 / a.period - 1 / b.period));
    const step = Math.max(0.5, Math.min(5, synodic / 200));
    const out = [];
    let t0 = fromJd, f0 = f(t0);
    for (let t1 = t0 + step; t1 <= fromJd + horizonDays && out.length < count; t1 += step) {
      const f1 = f(t1);
      // a real crossing passes through 0; a jump from +180 to -180 is the wrap, not a lineup
      if (f0 <= 0 && f1 > 0 || f0 >= 0 && f1 < 0) {
        if (Math.abs(f1 - f0) < 90) {
          let lo = t0, hi = t1, flo = f0;
          while (hi - lo > 1 / 1440) {
            const mid = (lo + hi) / 2, fm = f(mid);
            if ((fm <= 0) === (flo <= 0)) { lo = mid; flo = fm; } else hi = mid;
          }
          out.push((lo + hi) / 2);
        }
      }
      t0 = t1; f0 = f1;
    }
    return { events: out, synodic };
  }

  return {
    DEG, J2000, AU_KM, LIGHT_MIN_PER_AU, VALID, BODIES, byId,
    jdFromMs, msFromJd, parseDate, isoDate, wrap180,
    kepler, elementsAt, place, position, orbitPath, state, separation, lineups,
  };
});
