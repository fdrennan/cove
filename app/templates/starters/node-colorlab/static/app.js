// Color Laboratory · __HOST__
// Every colour formula lives here and runs in the browser: the server only
// serves files and keeps saved palettes. The maths block has no DOM or React
// in it, so `node -e "require('./static/app.js')"` can test it (see selfTest).
'use strict';

// ===========================================================================
// Colour maths. Colours are [r, g, b] in 0..1, gamma-encoded sRGB, unless the
// name says linear, oklab ([L, a, b]) or oklch ([L, C, h in degrees]).
// ===========================================================================
const CM = (() => {
  const clamp01 = (x) => Math.min(1, Math.max(0, x));
  const lerp = (a, b, t) => a + (b - a) * t;

  // sRGB transfer function, IEC 61966-2-1.
  const toLinear = (c) => (c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
  const toGamma = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);

  function hexToRgb(hex) {
    const m = /^#?([0-9a-f]{6}|[0-9a-f]{3})$/i.exec(hex.trim());
    if (!m) return null;
    let h = m[1];
    if (h.length === 3) h = h.split('').map((c) => c + c).join('');
    return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
  }
  const rgbToHex = (rgb) => '#' + rgb.map((c) => Math.round(clamp01(c) * 255).toString(16).padStart(2, '0')).join('');

  // OKLab, Björn Ottosson (2020): linear sRGB -> LMS (M1), cube root, -> Lab (M2).
  function linearToOklab([r, g, b]) {
    const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
    const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
    const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    return [
      0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
      1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
      0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ];
  }
  function oklabToLinear([L, a, b]) {
    const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
    const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
    const s = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3;
    return [
      4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
      -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
      -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ];
  }
  const rgbToOklab = (rgb) => linearToOklab(rgb.map(toLinear));
  // Unclamped on purpose: values outside 0..1 are how we detect out-of-gamut.
  const oklabToRgb = (lab) => oklabToLinear(lab).map((c) => Math.sign(c) * toGamma(Math.abs(c)));
  function oklabToOklch([L, a, b]) {
    const C = Math.hypot(a, b);
    let h = (Math.atan2(b, a) * 180) / Math.PI;
    if (h < 0) h += 360;
    return [L, C, h];
  }
  const oklchToOklab = ([L, C, h]) => [L, C * Math.cos((h * Math.PI) / 180), C * Math.sin((h * Math.PI) / 180)];
  const rgbToOklch = (rgb) => oklabToOklch(rgbToOklab(rgb));

  const inGamut = (rgb, eps = 1e-4) => rgb.every((c) => c >= -eps && c <= 1 + eps);

  // Gamut mapping: keep lightness and hue, lower chroma until the colour fits
  // in sRGB (the CSS Color 4 idea, without its JND shortcut). Bisection, so
  // 24 steps reach far below one 8-bit step.
  function oklchToRgbMapped(lch) {
    const L = Math.min(1, Math.max(0, lch[0]));
    let rgb = oklabToRgb(oklchToOklab([L, lch[1], lch[2]]));
    if (inGamut(rgb)) return { rgb: rgb.map(clamp01), C: lch[1], mapped: false };
    let lo = 0, hi = lch[1];
    for (let i = 0; i < 24; i++) {
      const mid = (lo + hi) / 2;
      if (inGamut(oklabToRgb(oklchToOklab([L, mid, lch[2]])))) lo = mid; else hi = mid;
    }
    rgb = oklabToRgb(oklchToOklab([L, lo, lch[2]])).map(clamp01);
    return { rgb, C: lo, mapped: true };
  }
  const oklchToHex = (lch) => rgbToHex(oklchToRgbMapped(lch).rgb);

  function rgbToHsl([r, g, b]) {
    const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2, d = max - min;
    if (d === 0) return [0, 0, l];
    const s = d / (1 - Math.abs(2 * l - 1));
    let h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
    return [h, s, l];
  }
  function hslToRgb([h, s, l]) {
    const c = (1 - Math.abs(2 * l - 1)) * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = l - c / 2;
    const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
    return [r + m, g + m, b + m];
  }

  // WCAG 2.x relative luminance and contrast ratio (W3C, WCAG 2.1 §1.4.3).
  const luminance = (rgb) => {
    const [r, g, b] = rgb.map(toLinear);
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  function contrast(fg, bg) {
    const a = luminance(fg), b = luminance(bg);
    return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
  }

  // APCA 0.0.98G-4g (Myndex, Somers): Lc, signed. Positive = dark text on a
  // light background. Unlike the WCAG ratio it is not symmetric in fg/bg,
  // which is why it predicts dark-mode legibility better.
  function apca(txt, bg) {
    const Y = (rgb) => 0.2126729 * rgb[0] ** 2.4 + 0.7151522 * rgb[1] ** 2.4 + 0.0721750 * rgb[2] ** 2.4;
    const soft = (y) => (y > 0.022 ? y : y + (0.022 - y) ** 1.414);
    const yt = soft(Y(txt)), yb = soft(Y(bg));
    if (Math.abs(yb - yt) < 0.0005) return 0;
    if (yb > yt) {
      const s = (yb ** 0.56 - yt ** 0.57) * 1.14;
      return s < 0.1 ? 0 : (s - 0.027) * 100;
    }
    const s = (yb ** 0.65 - yt ** 0.62) * 1.14;
    return s > -0.1 ? 0 : (s + 0.027) * 100;
  }
  // APCA bronze-level guidance: the minimum |Lc| for each use.
  function apcaUse(lc) {
    const a = Math.abs(lc);
    return a >= 90 ? 'any text' : a >= 75 ? 'body text' : a >= 60 ? 'content text' : a >= 45 ? 'headlines' : a >= 30 ? 'non-text, UI' : 'fails';
  }

  // Colour-vision deficiency: Machado, Oliveira & Fernandes (2009), severity
  // 1.0 (dichromacy). The matrices act on linear RGB; each row sums to 1, so
  // white and greys are unchanged.
  const MACHADO = {
    protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
    deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.011820, 0.042940, 0.968881]],
    tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.303900]],
  };
  function simulate(rgb, kind) {
    const M = MACHADO[kind];
    if (!M) return rgb;
    const lin = rgb.map(toLinear);
    return M.map((row) => clamp01(toGamma(clamp01(row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2]))));
  }

  // Interpolation in three spaces. sRGB lerps the gamma-encoded numbers (what
  // CSS gradients did by default for 25 years); linear lerps light; OKLab
  // lerps perception, which keeps chroma through the middle.
  function mix(a, b, t, space) {
    if (space === 'srgb') return a.map((c, i) => lerp(c, b[i], t));
    if (space === 'linear') return a.map((c, i) => toGamma(lerp(toLinear(c), toLinear(b[i]), t)));
    const A = rgbToOklab(a), B = rgbToOklab(b);
    return oklabToRgb(A.map((c, i) => lerp(c, B[i], t))).map(clamp01);
  }
  function sampleGradient(stops, t, space) {
    if (stops.length === 1) return stops[0];
    const x = clamp01(t) * (stops.length - 1);
    const i = Math.min(stops.length - 2, Math.floor(x));
    return mix(stops[i], stops[i + 1], x - i, space);
  }

  const fmtOklch = ([L, C, h]) => `oklch(${(L * 100).toFixed(1)}% ${C.toFixed(3)} ${(C < 1e-4 ? 0 : h).toFixed(1)})`;

  // Known values, run in the page (shown in the Environment panel) and from node.
  function selfTest() {
    const out = [];
    const near = (a, b, tol) => Math.abs(a - b) <= tol;
    // Round trip over a 16^3 grid of sRGB colours, worst error in 0..1 units.
    let worst = 0;
    for (let r = 0; r < 16; r++) for (let g = 0; g < 16; g++) for (let b = 0; b < 16; b++) {
      const c = [r / 15, g / 15, b / 15];
      const back = oklabToRgb(rgbToOklab(c));
      for (let i = 0; i < 3; i++) worst = Math.max(worst, Math.abs(back[i] - c[i]));
    }
    // Ottosson publishes the matrices to 10 digits, so the inverse is exact to
    // ~1e-6: far below one 8-bit step (1/255 = 3.9e-3), so every hex returns.
    out.push({ name: 'sRGB -> OKLab -> sRGB, 4096 colours, max error', value: worst.toExponential(2), pass: worst < 1e-5 });
    let exact = 0;
    for (let i = 0; i < 4096; i++) {
      const hex = '#' + ((i * 4099) % 16777216).toString(16).padStart(6, '0');
      if (rgbToHex(oklabToRgb(rgbToOklab(hexToRgb(hex)))) === hex) exact++;
    }
    out.push({ name: '8-bit hex round trips exact', value: `${exact}/4096`, pass: exact === 4096 });
    const w = rgbToOklab([1, 1, 1]);
    out.push({ name: 'white in OKLab = (1, 0, 0)', value: w.map((x) => x.toFixed(6)).join(', '), pass: near(w[0], 1, 1e-4) && near(w[1], 0, 1e-4) && near(w[2], 0, 1e-4) });
    const red = rgbToOklch([1, 0, 0]);
    out.push({ name: '#ff0000 = oklch(62.80% 0.2577 29.23)', value: fmtOklch(red), pass: near(red[0], 0.62796, 2e-4) && near(red[1], 0.25768, 2e-4) && near(red[2], 29.23, 0.02) });
    const k = contrast([0, 0, 0], [1, 1, 1]);
    out.push({ name: 'WCAG #000 on #fff = 21', value: k.toFixed(4), pass: near(k, 21, 1e-9) });
    const g7 = contrast(hexToRgb('#777777'), [1, 1, 1]);
    out.push({ name: 'WCAG #777 on #fff = 4.48', value: g7.toFixed(4), pass: near(g7, 4.48, 0.005) });
    const lcBoW = apca([0, 0, 0], [1, 1, 1]), lcWoB = apca([1, 1, 1], [0, 0, 0]);
    out.push({ name: 'APCA #000 on #fff = Lc 106.04', value: lcBoW.toFixed(2), pass: near(lcBoW, 106.04, 0.01) });
    out.push({ name: 'APCA #fff on #000 = Lc -107.88', value: lcWoB.toFixed(2), pass: near(lcWoB, -107.88, 0.01) });
    const grey = hexToRgb('#808080');
    const cvdGrey = ['protan', 'deutan', 'tritan'].every((kd) => rgbToHex(simulate(grey, kd)) === '#808080');
    out.push({ name: 'Machado: greys unchanged', value: cvdGrey ? '#808080 -> #808080' : 'changed', pass: cvdGrey });
    return out;
  }

  return { clamp01, toLinear, toGamma, hexToRgb, rgbToHex, rgbToOklab, oklabToRgb, oklabToOklch, oklchToOklab, rgbToOklch,
    inGamut, oklchToRgbMapped, oklchToHex, rgbToHsl, hslToRgb, luminance, contrast, apca, apcaUse, simulate, mix,
    sampleGradient, fmtOklch, selfTest };
})();

// ===========================================================================
// Palette from one base colour. Built in OKLCH, so "same lightness, new hue"
// means the same perceived lightness, which HSL cannot promise.
// ===========================================================================
const HARMONIES = {
  analogous: { label: 'Analogous', offsets: [-30, 30] },
  complementary: { label: 'Complementary', offsets: [180, 180] },
  split: { label: 'Split', offsets: [150, 210] },
  triadic: { label: 'Triadic', offsets: [120, 240] },
  mono: { label: 'Tints & shades', offsets: [0, 0] },
};
const ROLES = ['background', 'surface', 'text', 'muted', 'primary', 'on-primary', 'accent', 'accent-2'];

function buildTheme(base, harmony, mode) {
  const [L, C, h] = base;
  const hue = (o) => (h + o + 360) % 360;
  const [o1, o2] = HARMONIES[harmony].offsets;
  const shiftL = (d) => Math.min(0.95, Math.max(0.25, L + d));
  // Complementary and mono have one hue to spare: vary lightness instead.
  const accent = harmony === 'mono' ? [shiftL(0.18), C * 0.8, h] : [L, C, hue(o1)];
  const accent2 = harmony === 'mono' ? [shiftL(-0.18), C * 0.8, h]
    : harmony === 'complementary' ? [shiftL(L > 0.6 ? -0.2 : 0.2), C, hue(o2)] : [L, C, hue(o2)];
  const tint = Math.min(0.03, C * 0.2);
  const dark = mode === 'dark';
  const t = {
    background: CM.oklchToHex(dark ? [0.17, tint * 0.6, h] : [0.985, tint * 0.25, h]),
    surface: CM.oklchToHex(dark ? [0.23, tint * 0.8, h] : [0.94, tint * 0.5, h]),
    text: CM.oklchToHex(dark ? [0.95, 0.01, h] : [0.2, 0.02, h]),
    muted: CM.oklchToHex(dark ? [0.74, 0.025, h] : [0.47, 0.03, h]),
    primary: CM.oklchToHex(base),
    accent: CM.oklchToHex(accent),
    'accent-2': CM.oklchToHex(accent2),
  };
  // Text on the primary: whichever of near-white and near-black APCA rates higher.
  const p = CM.hexToRgb(t.primary);
  const light = CM.oklchToHex([0.99, 0.01, h]), darkInk = CM.oklchToHex([0.16, 0.02, h]);
  t['on-primary'] = Math.abs(CM.apca(CM.hexToRgb(light), p)) >= Math.abs(CM.apca(CM.hexToRgb(darkInk), p)) ? light : darkInk;
  return t;
}

// Ten steps of even perceived lightness at the base hue, the 50..900 scale.
const SCALE_NAMES = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900];
const buildScale = (base) => SCALE_NAMES.map((n, i) => ({ name: n, hex: CM.oklchToHex([0.97 - i * (0.75 / 9), base[1], base[2]]) }));

const VISIONS = { normal: 'Normal', protan: 'Protanopia', deutan: 'Deuteranopia', tritan: 'Tritanopia' };
const simHex = (hex, kind) => (kind === 'normal' ? hex : CM.rgbToHex(CM.simulate(CM.hexToRgb(hex), kind)));

function exportCss(theme, scale) {
  const line = (name, hex) => `  --${name}: ${hex};`;
  const lch = (name, hex) => `    --${name}: ${CM.fmtOklch(CM.rgbToOklch(CM.hexToRgb(hex)))};`;
  const all = [...ROLES.map((r) => [r, theme[r]]), ...scale.map((s) => [`base-${s.name}`, s.hex])];
  return [
    ':root {', ...all.map(([n, h]) => line(n, h)), '}',
    '',
    '/* Same colours in OKLCH for browsers that read it (all evergreen ones since 2023). */',
    '@supports (color: oklch(0% 0 0)) {', '  :root {', ...all.map(([n, h]) => lch(n, h)), '  }', '}',
  ].join('\n');
}

// Node loads this file for the maths tests; the browser goes on to React.
if (typeof module !== 'undefined' && module.exports) {
  module.exports = { CM, buildTheme, buildScale, exportCss, HARMONIES };
} else {
  mount();
}

function mount() {
  const { useState, useMemo, useEffect, useRef, useCallback } = React;
  const html = htm.bind(React.createElement);

  // A canvas that redraws when its inputs or its size change, at device pixel ratio.
  function useCanvas(draw, deps) {
    const ref = useRef(null);
    useEffect(() => {
      const cv = ref.current;
      if (!cv) return undefined;
      const paint = () => {
        const dpr = window.devicePixelRatio || 1;
        const w = Math.max(1, Math.round(cv.clientWidth * dpr)), h = Math.max(1, Math.round(cv.clientHeight * dpr));
        if (cv.width !== w || cv.height !== h) { cv.width = w; cv.height = h; }
        draw(cv.getContext('2d'), w, h, dpr);
      };
      paint();
      const ro = new ResizeObserver(paint);
      ro.observe(cv);
      return () => ro.disconnect();
    }, deps);
    return ref;
  }

  function Slider({ label, value, min, max, step, fmt, onChange, track }) {
    return html`<label className="row">${label} <output>${fmt(value)}</output>
      <input type="range" min=${min} max=${max} step=${step} value=${value}
        style=${track ? { background: track } : null}
        onChange=${(e) => onChange(Number(e.target.value))} /></label>`;
  }

  // ---- The picker: one OKLCH value, edited through any of four spaces ----
  function Picker({ lch, setLch }) {
    const [space, setSpace] = useState('oklch');
    const [hexText, setHexText] = useState('');
    const mapped = useMemo(() => CM.oklchToRgbMapped(lch), [lch]);
    const hex = CM.rgbToHex(mapped.rgb);
    useEffect(() => setHexText(hex), [hex]);
    const lab = CM.oklchToOklab(lch);
    const rgb255 = mapped.rgb.map((c) => Math.round(c * 255));
    const hsl = CM.rgbToHsl(mapped.rgb);
    // Converting from sRGB/HSL loses hue at zero chroma: keep the old hue then.
    const fromRgb = (rgb) => {
      const n = CM.rgbToOklch(rgb);
      setLch([n[0], n[1], n[1] < 1e-4 ? lch[2] : n[2]]);
    };
    const fromLab = (l) => {
      const n = CM.oklabToOklch(l);
      setLch([n[0], n[1], n[1] < 1e-4 ? lch[2] : n[2]]);
    };
    // Slider tracks drawn from the maths: what each position would look like.
    const track = (f) => `linear-gradient(90deg, ${Array.from({ length: 13 }, (_, i) => f(i / 12)).join(', ')})`;
    const sliders = {
      oklch: [
        html`<${Slider} key="L" label="Lightness L" value=${lch[0]} min=${0} max=${1} step=${0.001} fmt=${(v) => (v * 100).toFixed(1) + '%'}
          track=${track((t) => CM.oklchToHex([t, lch[1], lch[2]]))} onChange=${(v) => setLch([v, lch[1], lch[2]])} />`,
        html`<${Slider} key="C" label="Chroma C" value=${lch[1]} min=${0} max=${0.37} step=${0.001} fmt=${(v) => v.toFixed(3)}
          track=${track((t) => CM.oklchToHex([lch[0], t * 0.37, lch[2]]))} onChange=${(v) => setLch([lch[0], v, lch[2]])} />`,
        html`<${Slider} key="H" label="Hue h" value=${lch[2]} min=${0} max=${360} step=${0.5} fmt=${(v) => v.toFixed(1) + '°'}
          track=${track((t) => CM.oklchToHex([lch[0], lch[1], t * 360]))} onChange=${(v) => setLch([lch[0], lch[1], v])} />`,
      ],
      oklab: [
        html`<${Slider} key="L" label="L" value=${lab[0]} min=${0} max=${1} step=${0.001} fmt=${(v) => v.toFixed(3)} onChange=${(v) => fromLab([v, lab[1], lab[2]])} />`,
        html`<${Slider} key="a" label="a (green - red)" value=${lab[1]} min=${-0.4} max=${0.4} step=${0.001} fmt=${(v) => v.toFixed(3)}
          track=${track((t) => CM.rgbToHex(CM.oklabToRgb([lab[0], -0.4 + 0.8 * t, lab[2]]).map(CM.clamp01)))} onChange=${(v) => fromLab([lab[0], v, lab[2]])} />`,
        html`<${Slider} key="b" label="b (blue - yellow)" value=${lab[2]} min=${-0.4} max=${0.4} step=${0.001} fmt=${(v) => v.toFixed(3)}
          track=${track((t) => CM.rgbToHex(CM.oklabToRgb([lab[0], lab[1], -0.4 + 0.8 * t]).map(CM.clamp01)))} onChange=${(v) => fromLab([lab[0], lab[1], v])} />`,
      ],
      srgb: ['R', 'G', 'B'].map((n, i) => html`<${Slider} key=${n} label=${n} value=${rgb255[i]} min=${0} max=${255} step=${1} fmt=${(v) => v}
          track=${track((t) => CM.rgbToHex(mapped.rgb.map((c, j) => (j === i ? t : c))))}
          onChange=${(v) => fromRgb(rgb255.map((c, j) => (j === i ? v : c) / 255))} />`),
      hsl: [
        html`<${Slider} key="h" label="Hue" value=${Math.round(hsl[0])} min=${0} max=${360} step=${1} fmt=${(v) => v + '°'}
          track=${track((t) => CM.rgbToHex(CM.hslToRgb([t * 359.9, hsl[1], hsl[2]])))} onChange=${(v) => fromRgb(CM.hslToRgb([v % 360, hsl[1], hsl[2]]))} />`,
        html`<${Slider} key="s" label="Saturation" value=${Math.round(hsl[1] * 100)} min=${0} max=${100} step=${1} fmt=${(v) => v + '%'}
          onChange=${(v) => fromRgb(CM.hslToRgb([hsl[0], v / 100, hsl[2]]))} />`,
        html`<${Slider} key="l" label="Lightness" value=${Math.round(hsl[2] * 100)} min=${0} max=${100} step=${1} fmt=${(v) => v + '%'}
          onChange=${(v) => fromRgb(CM.hslToRgb([hsl[0], hsl[1], v / 100]))} />`,
      ],
    };
    return html`<div className="controls" style=${{ gap: '12px' }}>
      <div className="preview">
        <div className="chip" style=${{ background: hex }}>
          <input type="color" value=${hex} aria-label="system colour picker" onChange=${(e) => fromRgb(CM.hexToRgb(e.target.value))} />
        </div>
        <div className="meta">
          <input type="text" value=${hexText} aria-label="hex" spellCheck=${false}
            onChange=${(e) => { setHexText(e.target.value); const c = CM.hexToRgb(e.target.value); if (c) fromRgb(c); }} />
          <span>${CM.fmtOklch([lch[0], mapped.C, lch[2]])}</span>
          <span className=${'gamut ' + (mapped.mapped ? 'out' : 'in')}>
            ${mapped.mapped ? `outside sRGB: chroma ${lch[1].toFixed(3)} -> ${mapped.C.toFixed(3)}` : 'inside sRGB gamut'}</span>
        </div>
      </div>
      <div className="seg">${[['oklch', 'OKLCH'], ['oklab', 'OKLab'], ['srgb', 'sRGB'], ['hsl', 'HSL']].map(([k, n]) =>
        html`<button key=${k} type="button" className=${space === k ? 'on' : ''} onClick=${() => setSpace(k)}>${n}</button>`)}</div>
      ${sliders[space]}
    </div>`;
  }

  function Seg({ options, value, onChange }) {
    return html`<div className="seg">${Object.entries(options).map(([k, v]) =>
      html`<button key=${k} type="button" className=${value === k ? 'on' : ''} onClick=${() => onChange(k)}>${typeof v === 'string' ? v : v.label}</button>`)}</div>`;
  }

  // ---- Palette: roles (click to override), the 50..900 scale, CVD strips ----
  function Palette({ theme, overrides, setOverride, scale }) {
    return html`<section className="glass card wide">
      <h2>Palette <small>click a swatch to override it; OKLCH under each</small></h2>
      <div className="roles">${ROLES.map((r) => {
        const lch = CM.rgbToOklch(CM.hexToRgb(theme[r]));
        return html`<div className="role" key=${r}>
          <div className="sw" style=${{ background: theme[r] }}>
            ${overrides[r] ? html`<i>set</i>` : null}
            <input type="color" value=${theme[r]} aria-label=${r} onChange=${(e) => setOverride(r, e.target.value)} />
          </div>
          <div className="lbl"><b>${r}</b><span>${theme[r]}</span><br /><span>L ${(lch[0] * 100).toFixed(0)} C ${lch[1].toFixed(2)}</span></div>
        </div>`;
      })}</div>
      <div className="scale">${scale.map((s, i) => html`<div key=${s.name} style=${{ background: s.hex, color: i < 5 ? '#0b1020' : '#f4f6fb' }}>${s.name}</div>`)}</div>
      <div className="strips">${Object.entries(VISIONS).map(([k, n]) => html`<div className="strip" key=${k}>
        <span>${n}</span>
        <div className="cells">${ROLES.map((r) => html`<div key=${r} title=${r} style=${{ background: simHex(theme[r], k) }}></div>`)}</div>
      </div>`)}</div>
    </section>`;
  }

  // ---- Gradients: the same stops interpolated three ways ----
  const PRESETS = {
    'blue-yellow': ['#0000ff', '#ffff00'],
    'red-cyan': ['#ff0000', '#00ffff'],
    'green-magenta': ['#00c853', '#e040fb'],
    'black-white': ['#000000', '#ffffff'],
  };
  const SPACES = [['srgb', 'sRGB (gamma)', '#ef9a5c'], ['linear', 'linear light', '#b49cf0'], ['oklab', 'OKLab', '#4cc3df']];

  function GradientRow({ stops, space, label }) {
    const rgbs = useMemo(() => stops.map(CM.hexToRgb), [stops]);
    const ref = useCanvas((ctx, w, h) => {
      const img = ctx.createImageData(w, 1);
      for (let x = 0; x < w; x++) {
        const c = CM.sampleGradient(rgbs, x / (w - 1 || 1), space);
        img.data.set([c[0] * 255, c[1] * 255, c[2] * 255, 255], x * 4);
      }
      for (let y = 0; y < h; y++) ctx.putImageData(img, 0, y);
    }, [rgbs, space]);
    return html`<div className="grad-row"><span className="k">${label}</span><canvas ref=${ref}></canvas></div>`;
  }

  function Gradients({ stops, setStops, theme }) {
    const rgbs = useMemo(() => stops.map(CM.hexToRgb), [stops]);
    // Chroma and lightness along each path, sampled once and shared by chart and readout.
    const curves = useMemo(() => SPACES.map(([sp]) => Array.from({ length: 121 }, (_, i) => CM.rgbToOklch(CM.sampleGradient(rgbs, i / 120, sp)))), [rgbs]);
    const chart = useCanvas((ctx, w, h, dpr) => {
      ctx.clearRect(0, 0, w, h);
      const pad = 6 * dpr, maxC = Math.max(0.05, ...curves.flat().map((p) => p[1])) * 1.1;
      ctx.strokeStyle = 'rgba(255,255,255,.08)';
      ctx.lineWidth = dpr;
      for (let i = 1; i < 4; i++) { const y = (h * i) / 4; ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke(); }
      curves.forEach((pts, k) => {
        const col = SPACES[k][2];
        for (const [idx, scale, dash] of [[1, maxC, []], [0, 1, [5 * dpr, 4 * dpr]]]) {
          ctx.beginPath();
          ctx.setLineDash(dash);
          ctx.strokeStyle = col;
          ctx.lineWidth = 2 * dpr;
          pts.forEach((p, i) => {
            const x = (i / 120) * w, y = h - pad - (p[idx] / scale) * (h - 2 * pad);
            i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
          });
          ctx.stroke();
        }
      });
      ctx.setLineDash([]);
    }, [curves]);
    const setStop = (i, v) => setStops(stops.map((s, j) => (j === i ? v : s)));
    return html`<section className="glass card wide">
      <h2>Gradients <small>same stops, three interpolation spaces</small></h2>
      <div className="stops">
        ${stops.map((s, i) => html`<div className="stop" key=${i} style=${{ background: s }}>
          <input type="color" value=${s} aria-label=${'stop ' + (i + 1)} onChange=${(e) => setStop(i, e.target.value)} />
          ${stops.length > 2 ? html`<button type="button" aria-label="remove stop" onClick=${() => setStops(stops.filter((_, j) => j !== i))}>x</button>` : null}
        </div>`)}
        ${stops.length < 6 ? html`<button className="btn small" type="button" onClick=${() => setStops([...stops, stops[stops.length - 1]])}>+ stop</button>` : null}
        <select style=${{ width: 'auto' }} value="" aria-label="preset" onChange=${(e) => {
          const v = e.target.value;
          if (v === 'palette') setStops([theme.primary, theme.accent, theme['accent-2']]);
          else if (PRESETS[v]) setStops(PRESETS[v]);
        }}>
          <option value="">Presets…</option>
          <option value="palette">From palette</option>
          ${Object.keys(PRESETS).map((k) => html`<option key=${k} value=${k}>${k.replace('-', ' → ')}</option>`)}
        </select>
      </div>
      ${SPACES.map(([sp, label]) => html`<${GradientRow} key=${sp} stops=${stops} space=${sp} label=${label} />`)}
      <canvas className="chart" ref=${chart} aria-label="chroma and lightness along each gradient"></canvas>
      <div className="legend">${SPACES.map(([sp, n, col]) => html`<span key=${sp}><i style=${{ background: col }}></i>${n} chroma</span>`)}
        <span><i className="dash" style=${{ borderColor: '#9aa6c2' }}></i>lightness (dashed)</span></div>
      <div className="mids">${SPACES.map(([sp, n], k) => {
        const mid = CM.sampleGradient(rgbs, 0.5, sp), m = CM.rgbToOklch(mid);
        const dip = Math.min(...curves[k].map((p) => p[1]));
        return html`<div className="mid" key=${sp}><div style=${{ background: CM.rgbToHex(mid) }}></div>
          <div><b>${n}</b> middle ${CM.rgbToHex(mid)}<br />L ${(m[0] * 100).toFixed(0)}%, C ${m[1].toFixed(3)}, lowest C ${dip.toFixed(3)}</div></div>`;
      })}</div>
    </section>`;
  }

  // ---- Mock UI themed by the palette, seen through the chosen vision ----
  function MockUI({ theme, vision, setVision }) {
    const vars = {};
    for (const r of ROLES) vars['--m-' + r] = simHex(theme[r], vision);
    const bars = [0.55, 0.8, 0.4, 0.95, 0.65, 0.3, 0.75];
    const barCol = ['--m-primary', '--m-accent', '--m-accent-2'];
    return html`<section className="glass card">
      <h2>Live UI <small>the palette at work</small></h2>
      <${Seg} options=${VISIONS} value=${vision} onChange=${setVision} />
      <div className="mock" style=${{ ...vars, marginTop: '12px' }}>
        <nav><b>◆ Orbit</b><span>Overview</span><span>Reports</span><span>Team</span></nav>
        <div className="body">
          <div className="mcard">
            <span className="badge">new</span>
            <h3>Quarterly signal</h3>
            <p>Seven weeks of readings, coloured by series.</p>
            <svg viewBox="0 0 140 60" preserveAspectRatio="none">${bars.map((b, i) =>
              html`<rect key=${i} x=${i * 20 + 2} y=${60 - b * 58} width="14" height=${b * 58} rx="2" style=${{ fill: `var(${barCol[i % 3]})` }} />`)}</svg>
          </div>
          <div className="mcard">
            <h3>Invite a colleague</h3>
            <div className="minput">name@example.org</div>
            <div className="buttons"><button className="mbtn p" type="button">Send invite</button><button className="mbtn o" type="button">Preview</button></div>
            <div className="alert">Invites expire after 7 days.</div>
          </div>
        </div>
      </div>
    </section>`;
  }

  // ---- WCAG 2.x and APCA for every text/background pair ----
  function Contrast({ theme }) {
    const pairs = [];
    for (const bg of ['background', 'surface']) for (const fg of ['text', 'muted', 'primary', 'accent', 'accent-2']) pairs.push([fg, bg]);
    pairs.push(['on-primary', 'primary']);
    // WCAG 2.x levels for normal text: AAA 7:1, AA 4.5:1; 3:1 passes for large text only.
    const level = (k) => (k >= 7 ? 'AAA' : k >= 4.5 ? 'AA' : k >= 3 ? 'AA large' : 'fail');
    return html`<section className="glass card wide">
      <h2>Contrast <small>WCAG 2.x ratio and APCA Lc</small></h2>
      <div className="tbl-wrap"><table>
        <thead><tr><th>text / bg</th><th>sample</th><th>WCAG ratio</th><th>level</th><th>APCA Lc</th><th>APCA use</th></tr></thead>
        <tbody>${pairs.map(([fg, bg]) => {
          const f = CM.hexToRgb(theme[fg]), b = CM.hexToRgb(theme[bg]);
          const k = CM.contrast(f, b), lc = CM.apca(f, b), use = CM.apcaUse(lc);
          return html`<tr key=${fg + bg}>
            <td>${fg} / ${bg}</td>
            <td className="sample" style=${{ color: theme[fg], background: theme[bg] }}>Aa</td>
            <td>${k.toFixed(2)}:1</td><td><span className=${k >= 4.5 ? 'pass' : 'fail'}>${level(k)}</span></td>
            <td>${lc.toFixed(1)}</td><td><span className=${use === 'fails' ? 'fail' : 'pass'}>${use}</span></td>
          </tr>`;
        })}</tbody>
      </table></div>
    </section>`;
  }

  function Export({ css }) {
    const [msg, setMsg] = useState('');
    const copy = async () => {
      try { await navigator.clipboard.writeText(css); setMsg('copied'); } catch { setMsg('select the text and copy: the clipboard needs https'); }
      setTimeout(() => setMsg(''), 2500);
    };
    return html`<section className="glass card wide">
      <h2>CSS custom properties <small>${msg || html`<button className="btn small" type="button" onClick=${copy}>Copy</button>`}</small></h2>
      <pre className="css">${css}</pre>
    </section>`;
  }

  // ---- Saved palettes: GET/POST /api/palettes -> data/palettes.json ----
  function Saved({ current, onLoad }) {
    const [list, setList] = useState([]);
    const [name, setName] = useState('');
    const [msg, setMsg] = useState('');
    const refresh = useCallback(() => fetch('/api/palettes').then((r) => r.json()).then(setList).catch((e) => setMsg(String(e))), []);
    useEffect(() => { refresh(); }, [refresh]);
    const save = async () => {
      const r = await fetch('/api/palettes', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ ...current, name }) });
      const j = await r.json();
      setMsg(r.ok ? `saved "${j.name}"` : j.error);
      if (r.ok) refresh();
    };
    return html`<div className="controls" style=${{ gap: '8px' }}>
      <span className="k">Saved palettes</span>
      <div className="buttons" style=${{ flexWrap: 'nowrap' }}>
        <input type="text" placeholder="name" value=${name} maxLength=${60} onChange=${(e) => setName(e.target.value)} />
        <button className="btn primary" type="button" disabled=${!name.trim()} onClick=${save}>Save</button>
      </div>
      <div className="msg">${msg}</div>
      <div className="saved">${list.map((p) => html`<button key=${p.name} type="button" onClick=${() => onLoad(p)}>
        <span>${p.name}</span><span className="dots">${Object.values(p.colors).slice(0, 8).map((c, i) => html`<i key=${i} style=${{ background: c }}></i>`)}</span>
      </button>`)}</div>
    </div>`;
  }

  // ---- Environment: what this server runs on, read live from /api/env ----
  function Environment() {
    const [env, setEnv] = useState(null);
    const [err, setErr] = useState('');
    const tests = useMemo(() => CM.selfTest(), []);
    const onToggle = (e) => {
      if (e.currentTarget.open && !env) fetch('/api/env').then((r) => r.json()).then(setEnv).catch((x) => setErr(String(x)));
    };
    const passed = tests.filter((t) => t.pass).length;
    return html`<details className="glass env" onToggle=${onToggle}>
      <summary>Environment</summary>
      ${err ? html`<p className="msg">${err}</p>` : null}
      ${env ? html`<dl>
        <dt>runtime</dt><dd>${env.runtime.name} ${env.runtime.version} (V8 ${env.runtime.v8})</dd>
        <dt>stack</dt><dd>${env.stack_modules.length ? env.stack_modules.join(', ') : 'no module loaded'} · ${env.loaded_module_count} modules</dd>
        <dt>binary</dt><dd>${env.exec_path}</dd>
        <dt>started</dt><dd>${new Date(env.started).toLocaleString()}</dd>
        <dt>host</dt><dd>${env.cpus} CPUs · ${env.platform}</dd>
      </dl>
      <table><thead><tr><th>package</th><th>where</th><th>version</th><th>sha256</th></tr></thead>
        <tbody>${env.packages.map((p) => html`<tr key=${p.name}><td>${p.name}</td><td>${p.where}</td><td>${p.version || '?'}</td><td>${p.sha256 || ''}</td></tr>`)}</tbody></table>`
      : err ? null : html`<p className="msg">reading /api/env…</p>`}
      <table><thead><tr><th colSpan="2">colour maths, ${passed}/${tests.length} checks pass</th></tr></thead>
        <tbody>${tests.map((t) => html`<tr key=${t.name}><td>${t.name}</td><td><span className=${t.pass ? 'pass' : 'fail'}>${t.value}</span></td></tr>`)}</tbody></table>
    </details>`;
  }

  function App() {
    const [base, setBase] = useState([0.62, 0.16, 255]);
    const [harmony, setHarmony] = useState('triadic');
    const [mode, setMode] = useState('dark');
    const [overrides, setOverrides] = useState({});
    const [vision, setVision] = useState('normal');
    const [stops, setStops] = useState(PRESETS['blue-yellow']);
    const [folded, setFolded] = useState(true);

    const theme = useMemo(() => ({ ...buildTheme(base, harmony, mode), ...overrides }), [base, harmony, mode, overrides]);
    const scale = useMemo(() => buildScale(base), [base]);
    const css = useMemo(() => exportCss(theme, scale), [theme, scale]);
    const setOverride = (role, hex) => setOverrides((o) => ({ ...o, [role]: hex }));

    // The page's own glow follows the palette.
    useEffect(() => {
      const s = document.documentElement.style;
      s.setProperty('--glow-1', theme.primary);
      s.setProperty('--glow-2', theme.accent);
      s.setProperty('--glow-3', theme['accent-2']);
    }, [theme]);

    const load = (p) => {
      if (p.base) {
        const l = CM.rgbToOklch(CM.hexToRgb(p.base));
        setBase(l);
      }
      if (p.harmony && HARMONIES[p.harmony]) setHarmony(p.harmony);
      if (p.mode) setMode(p.mode);
      setOverrides(p.colors);
    };

    return html`<div className="stage">
      <div className="glow"><span></span><span></span><span></span></div>
      <aside className=${'glass panel' + (folded ? ' folded' : '')}>
        <header>
          <div><h1>Color Laboratory</h1><p>OKLCH palettes, contrast and vision.</p></div>
          <button className="btn small fold-btn" type="button" onClick=${() => setFolded(!folded)}>${folded ? 'Controls' : 'Hide'}</button>
        </header>
        <div className="controls">
          <${Picker} lch=${base} setLch=${(l) => { setBase(l); }} />
          <span className="k">Harmony</span>
          <${Seg} options=${HARMONIES} value=${harmony} onChange=${setHarmony} />
          <${Seg} options=${{ dark: 'Dark theme', light: 'Light theme' }} value=${mode} onChange=${setMode} />
          ${Object.keys(overrides).length ? html`<button className="btn small" type="button" onClick=${() => setOverrides({})}>
            Clear ${Object.keys(overrides).length} override${Object.keys(overrides).length > 1 ? 's' : ''}</button>` : null}
          <${Saved} current=${{ colors: theme, base: CM.oklchToHex(base), harmony, mode }} onLoad=${load} />
          <${Environment} />
        </div>
      </aside>
      <main className="work">
        <${Palette} theme=${theme} overrides=${overrides} setOverride=${setOverride} scale=${scale} />
        <${MockUI} theme=${theme} vision=${vision} setVision=${setVision} />
        <${Contrast} theme=${theme} />
        <${Gradients} stops=${stops} setStops=${setStops} theme=${theme} />
        <${Export} css=${css} />
      </main>
    </div>`;
  }

  ReactDOM.createRoot(document.getElementById('root')).render(html`<${App} />`);
}
