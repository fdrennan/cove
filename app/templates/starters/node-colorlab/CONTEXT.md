# Color Laboratory · __HOST__
A Node app with a React page, started from Cove's template: colour science for
themes and gradients. Pick a base colour in OKLCH, OKLab, sRGB or HSL; the page
builds a palette from it (analogous, complementary, split, triadic, tints and
shades) in OKLCH, so every role keeps the lightness you meant. It scores every
text/background pair with the WCAG 2.x contrast ratio and APCA, simulates
protanopia, deuteranopia and tritanopia on a live mock UI, compares multi-stop
gradients interpolated in sRGB, linear light and OKLab (blue to yellow goes
grey through the middle in sRGB, #808080 at C 0.000; OKLab keeps C 0.077), and
exports CSS custom properties with OKLCH values. Palettes save by name.
Change first: `buildTheme` in static/app.js - the roles and the lightness each
one gets in dark and light themes.

Why React: the page is one state (base colour, harmony, theme mode,
overrides, vision, gradient stops) and every panel is a component derived from
it with `useMemo`; change the base and the palette, contrast table, mock UI,
gradients and CSS recompute together. `useCanvas` is a custom hook (canvas +
ResizeObserver at device pixel ratio). No compile step: htm gives JSX-like
templates that run as written.

## Files
- static/app.js - all colour maths (`CM`: sRGB transfer, OKLab/OKLCH, gamut
  mapping, HSL, WCAG, APCA, Machado CVD, interpolation, `selfTest`) and the
  React components (Picker, Palette, MockUI, Contrast, Gradients, Export,
  Saved, Environment). The maths block has no DOM: `node -e
  "require('./static/app.js').CM.selfTest()"` runs the known-value checks
- static/index.html, static/style.css - the page and its glass panels; on a
  narrow screen the controls fold behind one button
- server.js - Node standard library only: serves static/ (and static/vendor/),
  GET /health, GET /api/env (runtime, stack modules, package versions and
  hashes, start time, CPUs), GET /api/palettes, POST /api/palettes
  (`{"name","colors":{role:"#rrggbb"},"base","harmony","mode"}` -> 201)
- data/palettes.json - saved palettes, created on the first save, written
  whole then renamed into place
- run.sh - loads Node from the ndexr stack and runs server.js
- app.env - language (node), kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - `node --check` every script and verify
  the vendored libraries' sha256; start in the background and wait for
  /health (pid and log in .run/); stop; load nodejs from the ndexr stack
- static/vendor/ - React 18.3.1, ReactDOM 18.3.1, htm 3.1.1, laid down by the
  Node lane with their checksums (static/vendor/README.md)

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
Node comes from the stack (`ml nodejs/22.22.2-GCCcore-13.3.0`); there is no
npm, no package.json and no network fetch.

## Built on
- Ottosson, B. (2020), "A perceptual color space for image processing"
  (OKLab), bottosson.github.io/posts/oklab; CSS Color Module Level 4 (OKLCH,
  gamut mapping by chroma reduction).
- IEC 61966-2-1:1999, the sRGB transfer function.
- W3C, WCAG 2.1 §1.4.3 and §1.4.6: relative luminance and contrast ratio
  (AA 4.5:1, large text 3:1, AAA 7:1).
- Somers, A. (Myndex), APCA 0.0.98G-4g, the Accessible Perceptual Contrast
  Algorithm proposed for WCAG 3.
- Machado, G. M., Oliveira, M. M. & Fernandes, L. A. F. (2009), "A
  Physiologically-based Model for Simulation of Color Vision Deficiency",
  IEEE TVCG 15(6), severity-1.0 matrices on linear RGB.

Checked by `selfTest` (shown in the page's Environment panel): #000 on #fff =
21:1, #777 on #fff = 4.48:1, APCA black on white Lc 106.04 and white on black
Lc -107.88, #ff0000 = oklch(62.8% 0.258 29.2), all 4096 sampled hex colours
round-trip through OKLab exactly.

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- CVD simulation is full dichromacy only; Machado's partial-severity tables
  (0.1 to 0.9) are not included.
