// __HOST__: the page. React with htm, which reads like JSX and runs as written.
// Replace anything here; this is a starting point and nothing depends on it.
const { useEffect, useRef, useState } = React;
const html = htm.bind(React.createElement);

const HOST = '__HOST__';

// A hue of this site's own, from its name, so no two sites start alike.
const hueOf = (name) => [...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);

// The moving part: layers of dunes, each a sum of slow waves. Every frame is
// worked out from the time alone, so there is nothing to accumulate or leak.
// The pointer lifts the dunes near it, and a press sends a ripple through them.
function Dunes({ hue }) {
  const canvas = useRef(null);
  const hueNow = useRef(hue);
  hueNow.current = hue;

  useEffect(() => {
    const el = canvas.current;
    const ctx = el.getContext('2d');
    const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
    const dark = matchMedia('(prefers-color-scheme: dark)');
    const LAYERS = 12;
    const pointer = { x: -1e4, y: -1e4 };
    let ripples = [];
    let frame = 0;
    let w = 0;
    let h = 0;

    const fit = () => {
      const dpr = Math.min(devicePixelRatio || 1, 2);
      w = el.clientWidth;
      h = el.clientHeight;
      el.width = w * dpr;
      el.height = h * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };

    const draw = (ms) => {
      const t = ms / 1000;
      ctx.clearRect(0, 0, w, h);
      ripples = ripples.filter((r) => t - r.at < 3);
      for (let i = 0; i < LAYERS; i++) {
        const depth = i / (LAYERS - 1);
        const base = h * (0.46 + 0.5 * depth);
        const reach = h * (0.05 + 0.05 * (1 - depth));
        ctx.beginPath();
        ctx.moveTo(0, h);
        for (let x = 0; x <= w + 10; x += 10) {
          let y = base
            + reach * Math.sin(x * 0.0042 + t * (0.28 + depth * 0.2) + i * 0.9)
            + reach * 0.5 * Math.sin(x * 0.011 - t * 0.37 + i * 1.7)
            + reach * 0.2 * Math.sin(x * 0.027 + t * 0.61 + i * 2.3);
          const dx = x - pointer.x;
          y -= h * 0.09 * Math.exp(-(dx * dx) / 24000) * Math.exp(-(((pointer.y - base) / (h * 0.45)) ** 2));
          for (const r of ripples) {
            const age = t - r.at;
            const off = Math.hypot(x - r.x, (base - r.y) * 0.6) - age * 420;
            y += 26 * Math.exp(-(off * off) / 3000) * (1 - age / 3);
          }
          ctx.lineTo(x, y);
        }
        ctx.lineTo(w + 10, h);
        ctx.closePath();
        const light = dark.matches ? 0.26 + depth * 0.5 : 0.9 - depth * 0.6;
        ctx.fillStyle = `oklch(${light} ${0.05 + depth * 0.12} ${hueNow.current + depth * 55})`;
        ctx.fill();
      }
      if (!still) frame = requestAnimationFrame(draw);
    };

    const move = (e) => { pointer.x = e.clientX; pointer.y = e.clientY; };
    const leave = () => { pointer.x = -1e4; };
    const press = (e) => {
      ripples.push({ x: e.clientX, y: e.clientY, at: performance.now() / 1000 });
    };
    const size = new ResizeObserver(() => { fit(); if (still) draw(0); });
    size.observe(el);
    addEventListener('pointermove', move);
    addEventListener('pointerleave', leave);
    addEventListener('pointerdown', press);
    fit();
    frame = requestAnimationFrame(draw);
    return () => {
      cancelAnimationFrame(frame);
      size.disconnect();
      removeEventListener('pointermove', move);
      removeEventListener('pointerleave', leave);
      removeEventListener('pointerdown', press);
    };
  }, []);

  return html`<canvas ref=${canvas} className="dunes" aria-hidden="true"></canvas>`;
}

function App() {
  const [hue, setHue] = useState(hueOf(HOST));
  const [name, ...rest] = HOST.split('.');

  useEffect(() => {
    document.documentElement.style.setProperty('--hue', hue);
  }, [hue]);

  return html`
    <${Dunes} hue=${hue} />
    <main>
      <p className="live"><i></i> Live</p>
      <h1>${name}<span>.${rest.join('.')}</span></h1>
      <p className="lead">This site is yours and it is running. Say what it should be, and watch it change here.</p>
      <button onClick=${() => setHue((hue + 47) % 360)}>Try another colour</button>
    </main>
  `;
}

ReactDOM.createRoot(document.getElementById('root')).render(html`<${App} />`);
