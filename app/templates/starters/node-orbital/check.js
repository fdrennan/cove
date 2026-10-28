// The known values orbits.js must reproduce, run as `node check.js` (exit 1 on a miss).
// Each tolerance is what Standish's Table 1 promises for 1800-2050, not tighter.
"use strict";
const O = require("./static/orbits.js");

const results = [];
const check = (name, got, want, tol, unit) => {
  const ok = Math.abs(got - want) <= tol;
  results.push(ok);
  console.log(`${ok ? "ok  " : "FAIL"} ${name}: ${got.toFixed(6)} ${unit} (want ${want} ± ${tol})`);
};

// Earth (the Earth-Moon barycentre) at J2000.0: JPL Horizons gives x -0.1771, y 0.9672,
// z ~0 AU; r is near perihelion (early January), 0.9833 AU.
const e0 = O.state(O.byId.earth, O.J2000);
check("Earth x at J2000", e0.x, -0.1771, 0.0005, "AU");
check("Earth y at J2000", e0.y, 0.9672, 0.0005, "AU");
check("Earth r at J2000", e0.r, 0.9833, 0.0002, "AU");

// Earth stays between perihelion and aphelion all year: 1 AU within e = 0.0167.
let lo = Infinity, hi = -Infinity;
for (let d = 0; d < 366; d++) { const r = O.state(O.byId.earth, O.J2000 + d).r; lo = Math.min(lo, r); hi = Math.max(hi, r); }
check("Earth perihelion 2000", lo, 0.9833, 0.0003, "AU");
check("Earth aphelion 2000", hi, 1.0167, 0.0003, "AU");

// Mars: mean synodic period 779.94 d; oppositions 2003-08-28 and 2027-02-19 (published dates).
const L = O.lineups(O.byId.earth, O.byId.mars, O.parseDate("2003-01-01"), 12, 9000);
check("Mars synodic period", L.synodic, 779.94, 0.1, "d");
check("Mars mean gap 2003-2027", (L.events[11] - L.events[0]) / 11, 779.94, 2, "d");
check("Mars opposition 2003", L.events[0], O.parseDate("2003-08-28T12:00"), 1, "JD");
check("Mars opposition 2027", L.events[11], O.parseDate("2027-02-19T12:00"), 1, "JD");

// Kepler's equation closes on a comet orbit: M = E - e sin E to 1e-12.
const E = O.kepler(0.01, 0.967);
check("Kepler residual, e = 0.967", E - 0.967 * Math.sin(E) - 0.01, 0, 1e-12, "rad");

const bad = results.filter((x) => !x).length;
console.log(bad ? `${bad} of ${results.length} failed` : `all ${results.length} checks pass`);
process.exit(bad ? 1 : 0);
