// Dolphin Jump, the page: React 18 components written with htm, no compile step.
//
// The game state lives in a ref, not in React state: it changes sixty times a
// second and only the canvas needs every frame. React renders the frame
// around the canvas (score, combo, hearts, the start and game-over cards) from
// a snapshot taken ten times a second. The score board is the server's
// (GET/POST /api/scores), shared by everyone who plays this site.
"use strict";

const { useState, useEffect, useRef, useCallback } = React;
const html = htm.bind(React.createElement);

// ---- the game --------------------------------------------------------------
const W = 960, H = 560, SEA = 352, TAU = Math.PI * 2;
const GRAVITY = 830, JUMP = -460, JUMP_COOLDOWN = 0.18;
const clamp = (n, a, b) => Math.max(a, Math.min(b, n));
const rnd = (a, b) => a + Math.random() * (b - a);
const storedBest = () => Number(localStorage.getItem("dolphin-best") || 0);

function initial(mode = "ready") {
  return {
    mode, time: 0, played: 0, score: 0, combo: 0, lives: 3, stars: 0, best: storedBest(),
    x: 195, y: SEA - 30, vy: 0, angle: 0, cooldown: 0,
    objects: [], particles: [], spawn: 0, speed: 230, distance: 0,
  };
}

function burst(g, x, y, n, color, spread) {
  for (let i = 0; i < n; i++) {
    g.particles.push({ x, y, vx: rnd(-spread, spread), vy: rnd(-spread, spread * 0.8), life: rnd(0.25, 0.85), r: rnd(2, 5), color });
  }
}

// One step of dt seconds. The dolphin falls under gravity, bounces off the
// sea floor's ceiling of 108 px below the waves, and the world scrolls left
// faster the further it has swum.
function step(g, dt) {
  g.time += dt;
  if (g.mode === "playing") {
    g.played += dt;
    g.distance += g.speed * dt;
    g.speed = Math.min(410, 230 + g.distance / 75);
    g.cooldown = Math.max(0, g.cooldown - dt);
    g.vy += GRAVITY * dt;
    g.y += g.vy * dt;
    const wave = SEA + Math.sin(g.time * 1.8 + g.x * 0.019) * 5;
    if (g.y > wave + 108) { g.y = wave + 108; g.vy = -240; }
    if (g.y < 58) { g.y = 58; g.vy = 65; }
    g.angle = clamp(g.vy / 760, -0.63, 0.66);

    g.spawn -= dt;
    if (g.spawn <= 0) {
      const kind = Math.random() < 0.72 ? "star" : "jelly";
      g.objects.push({ kind, x: W + 50, y: kind === "star" ? rnd(130, SEA + 58) : rnd(SEA + 36, SEA + 125), hit: false });
      g.spawn = rnd(0.65, 1.2);
    }
    for (const o of g.objects) {
      o.x -= g.speed * dt;
      if (o.hit || Math.hypot(g.x - o.x, (g.y - o.y) * 1.1) >= (o.kind === "star" ? 43 : 46)) continue;
      o.hit = true;
      if (o.kind === "star") {
        g.combo++;
        g.stars++;
        g.score += 10 + Math.min(50, (g.combo - 1) * 5);
        burst(g, o.x, o.y, 12, "#ffe07d", 95);
      } else {
        g.lives--;
        g.combo = 0;
        burst(g, o.x, o.y, 16, "#ff8ca3", 110);
        if (g.lives <= 0) {
          g.mode = "over";
          g.best = Math.max(g.score, g.best);
          localStorage.setItem("dolphin-best", String(g.best));
        }
      }
    }
    g.objects = g.objects.filter((o) => o.x > -60 && !o.hit);
  }
  for (const p of g.particles) { p.x += p.vx * dt; p.y += p.vy * dt; p.vy += 60 * dt; p.life -= dt; }
  g.particles = g.particles.filter((p) => p.life > 0);
}

function jump(g) {
  if (g.cooldown > 0) return;
  g.vy = JUMP;
  g.cooldown = JUMP_COOLDOWN;
  for (let i = 0; i < 11; i++) {
    g.particles.push({ x: g.x - 20, y: g.y + 14, vx: rnd(-90, 50), vy: rnd(-100, 40), life: rnd(0.3, 0.8), r: rnd(2, 5), color: "#b5fff5" });
  }
}

// ---- drawing ---------------------------------------------------------------
function dolphin(ctx, x, y, angle) {
  ctx.save(); ctx.translate(x, y); ctx.rotate(angle);
  ctx.fillStyle = "#0a7397"; ctx.beginPath(); ctx.moveTo(-34, 0); ctx.quadraticCurveTo(-59, -24, -67, -34); ctx.quadraticCurveTo(-58, 11, -29, 16); ctx.fill();
  ctx.fillStyle = "#43d6e7"; ctx.beginPath(); ctx.moveTo(-38, -8); ctx.quadraticCurveTo(-5, -39, 31, -11); ctx.quadraticCurveTo(45, -9, 58, -5); ctx.quadraticCurveTo(45, 3, 32, 6); ctx.quadraticCurveTo(8, 25, -31, 10); ctx.closePath(); ctx.fill();
  ctx.fillStyle = "#d6fff7"; ctx.beginPath(); ctx.moveTo(-27, 10); ctx.quadraticCurveTo(10, 22, 39, 2); ctx.quadraticCurveTo(6, 28, -27, 10); ctx.fill();
  ctx.fillStyle = "#28b4cd"; ctx.beginPath(); ctx.moveTo(-8, -17); ctx.lineTo(-20, -42); ctx.quadraticCurveTo(3, -34, 10, -20); ctx.fill();
  ctx.beginPath(); ctx.moveTo(-5, 15); ctx.lineTo(4, 37); ctx.quadraticCurveTo(15, 24, 17, 12); ctx.fill();
  ctx.fillStyle = "#0b2540"; ctx.beginPath(); ctx.arc(31, -10, 3.4, 0, TAU); ctx.fill();
  ctx.fillStyle = "white"; ctx.beginPath(); ctx.arc(32, -11, 1, 0, TAU); ctx.fill();
  ctx.strokeStyle = "#17688b"; ctx.lineWidth = 1.5; ctx.beginPath(); ctx.arc(39, -2, 6, 0.1, 1.2); ctx.stroke();
  ctx.restore();
}

function star(ctx, x, y, size, rot) {
  ctx.save(); ctx.translate(x, y); ctx.rotate(rot); ctx.beginPath();
  for (let i = 0; i < 10; i++) {
    const a = -Math.PI / 2 + i * Math.PI / 5, r = i % 2 ? size * 0.45 : size;
    ctx.lineTo(Math.cos(a) * r, Math.sin(a) * r);
  }
  ctx.closePath(); ctx.fillStyle = "#ffd95c"; ctx.shadowColor = "#ffce3a"; ctx.shadowBlur = 17; ctx.fill();
  ctx.restore();
}

function jelly(ctx, x, y, t) {
  ctx.save(); ctx.translate(x, y);
  ctx.fillStyle = "#fa7997"; ctx.beginPath(); ctx.ellipse(0, 0, 25, 18, 0, 0, TAU); ctx.fill();
  ctx.fillStyle = "#fff0aa"; ctx.beginPath(); ctx.arc(-4, -4, 4, 0, TAU); ctx.fill(); ctx.beginPath(); ctx.arc(13, -3, 4, 0, TAU); ctx.fill();
  ctx.fillStyle = "#4b1946"; ctx.beginPath(); ctx.arc(-3, -4, 2, 0, TAU); ctx.arc(13, -3, 2, 0, TAU); ctx.fill();
  ctx.strokeStyle = "#ffabc2"; ctx.lineWidth = 4;
  for (let k = 0; k < 4; k++) { ctx.beginPath(); ctx.moveTo(-17 + k * 10, 13); ctx.lineTo(-22 + k * 12, 29 + Math.sin(t * 6 + k) * 4); ctx.stroke(); }
  ctx.restore();
}

function paint(ctx, g) {
  const t = g.time;
  const sky = ctx.createLinearGradient(0, 0, 0, SEA);
  sky.addColorStop(0, "#062b5d"); sky.addColorStop(0.65, "#217bb7"); sky.addColorStop(1, "#9cead9");
  ctx.fillStyle = sky; ctx.fillRect(0, 0, W, SEA);

  const sun = ctx.createRadialGradient(770, 104, 10, 770, 104, 150);
  sun.addColorStop(0, "#ffe4a9aa"); sun.addColorStop(1, "#ffe4a900");
  ctx.fillStyle = sun; ctx.fillRect(610, 0, 340, 280);
  ctx.fillStyle = "#ffe7a9"; ctx.beginPath(); ctx.arc(770, 104, 44, 0, TAU); ctx.fill();
  for (let i = 0; i < 5; i++) {
    const x = ((i * 265 - t * 10) % 1240 + 1240) % 1240 - 120, y = 80 + (i % 3) * 53;
    ctx.fillStyle = "#e0ffff45"; ctx.beginPath(); ctx.ellipse(x, y, 64, 11, 0, 0, TAU); ctx.ellipse(x + 25, y - 9, 38, 15, 0, 0, TAU); ctx.fill();
  }
  ctx.fillStyle = "#155f83"; ctx.beginPath(); ctx.moveTo(0, SEA);
  for (let x = 0; x <= W; x += 14) ctx.lineTo(x, SEA - 14 - 12 * Math.sin(x * 0.009 + t * 0.1) - 10 * Math.sin(x * 0.024));
  ctx.lineTo(W, SEA); ctx.fill();

  const sea = ctx.createLinearGradient(0, SEA, 0, H);
  sea.addColorStop(0, "#18a7c9"); sea.addColorStop(0.35, "#08769f"); sea.addColorStop(1, "#04345c");
  ctx.fillStyle = sea; ctx.fillRect(0, SEA, W, H - SEA);
  const swell = ["#c0fff8aa", "#bbffeb39", "#72dfe82e", "#87ecf122", "#8debf619"];
  for (let k = 0; k < 5; k++) {
    ctx.beginPath();
    for (let x = 0; x <= W + 8; x += 8) {
      const y = SEA + Math.sin(x * 0.019 + t * 1.8 + k) * 5 + Math.sin(x * 0.044 - t * 0.7) * 3 + k * 14;
      if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = swell[k]; ctx.lineWidth = k ? 2 : 4; ctx.stroke();
  }
  for (let i = 0; i < 18; i++) {
    const x = ((i * 137 - t * 30) % 1050 + 1050) % 1050, y = SEA + 40 + (i * 47) % 155;
    ctx.strokeStyle = "#77e6e033"; ctx.beginPath(); ctx.arc(x, y, 2 + i % 3, 0, TAU); ctx.stroke();
  }
  ctx.fillStyle = "#032b50"; ctx.beginPath(); ctx.moveTo(0, H);
  for (let x = 0; x <= W; x += 20) ctx.lineTo(x, H - 17 + 7 * Math.sin(x * 0.02));
  ctx.lineTo(W, H); ctx.fill();
  for (let i = 0; i < 12; i++) {
    const x = i * 91 + 15, y = H - 19;
    ctx.strokeStyle = "#0c7482"; ctx.lineWidth = 3; ctx.beginPath();
    ctx.moveTo(x, y); ctx.quadraticCurveTo(x - 18, y - 25, x - 8, y - 38);
    ctx.moveTo(x, y); ctx.quadraticCurveTo(x + 17, y - 25, x + 12, y - 51); ctx.stroke();
  }

  for (const o of g.objects) {
    if (o.kind === "star") star(ctx, o.x, o.y, 17, Math.sin(t * 2 + o.x) * 0.25);
    else jelly(ctx, o.x, o.y, t);
  }
  for (const p of g.particles) {
    ctx.globalAlpha = clamp(p.life / 0.8, 0, 1); ctx.fillStyle = p.color;
    ctx.beginPath(); ctx.arc(p.x, p.y, p.r, 0, TAU); ctx.fill();
  }
  ctx.globalAlpha = 1;
  dolphin(ctx, g.x, g.y, g.angle);
  ctx.font = "600 13px system-ui"; ctx.fillStyle = "#e3ffffa8"; ctx.fillText("A LITTLE OCEAN ADVENTURE", 24, 32);
}

// ---- the page --------------------------------------------------------------
const snapshot = (g) => ({ mode: g.mode, score: g.score, lives: g.lives, best: g.best, combo: g.combo, stars: g.stars, played: g.played });

function Board({ rows, mine }) {
  if (!rows.length) return null;
  return html`<div class="board">
    <h3>Best runs on this site</h3>
    <ol>${rows.slice(0, 5).map((r, i) => html`<li key=${i} class=${i + 1 === mine ? "mine" : ""}>
      <span>${i + 1}. ${r.name}</span><b>${r.score.toLocaleString()}</b></li>`)}</ol>
  </div>`;
}

// Saving a finished run: a name (remembered for next time) and one POST.
function SaveRun({ hud, onSaved }) {
  const [name, setName] = useState(localStorage.getItem("dolphin-name") || "");
  const [state, setState] = useState("idle");
  const save = (e) => {
    e.preventDefault();
    localStorage.setItem("dolphin-name", name);
    setState("saving");
    fetch("/api/scores", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, score: hud.score, stars: hud.stars, seconds: hud.played }),
    }).then((r) => r.json()).then((r) => { setState(r.kept ? `saved: #${r.rank}` : "not in the top 20"); onSaved(r); })
      .catch(() => setState("the server did not answer"));
  };
  if (!hud.score) return null;
  if (state !== "idle" && state !== "saving") return html`<p class="saved">${state}</p>`;
  return html`<form class="save" onSubmit=${save} onPointerDown=${(e) => e.stopPropagation()}>
    <input value=${name} maxLength="16" placeholder="Your name" onInput=${(e) => setName(e.target.value)} />
    <button type="submit" disabled=${state === "saving"}>Save score</button>
  </form>`;
}

// Read from GET /api/env when opened: what runs, from which build.
function Environment() {
  const [open, setOpen] = useState(false);
  const [env, setEnv] = useState(null);
  useEffect(() => { if (open) fetch("/api/env").then((r) => r.json()).then(setEnv).catch(() => setEnv({ error: true })); }, [open]);
  const row = (k, v) => html`<div class="env-row" key=${k}><span class="k">${k}</span><span class="v">${v}</span></div>`;
  return html`<details class="env" onToggle=${(e) => setOpen(e.target.open)}>
    <summary>Environment</summary>
    ${env && env.error && row("server", "did not answer")}
    ${env && !env.error && [
      row("runtime", `${env.runtime.name} ${env.runtime.version} · ${env.runtime.platform}`),
      row("module", env.stack.runtime.join(", ") || "not from the stack"),
      row("server", env.server.map((s) => s.name).join(", ")),
      ...env.browser.map((b) => row(b.name, html`${b.version} · ${b.licence} · <span class=${b.verified ? "ok" : "bad"}>${b.verified ? "sha256 verified" : "sha256 differs"}</span>`)),
      row("board", `${env.board.runs} runs in ${env.board.file}`),
      row("process", `pid ${env.process.pid} · up ${env.process.uptime_s} s · ${env.host.cpus} CPUs`),
    ]}
  </details>`;
}

function App() {
  const canvas = useRef(null);
  const game = useRef(initial());
  const [hud, setHud] = useState(snapshot(game.current));
  const [board, setBoard] = useState([]);
  const [mine, setMine] = useState(null);

  useEffect(() => { fetch("/api/scores").then((r) => r.json()).then(setBoard).catch(() => {}); }, []);

  const act = useCallback(() => {
    const g = game.current;
    if (g.mode === "playing") return jump(g);
    game.current = g.mode === "over" ? initial("playing") : Object.assign(g, { mode: "playing" });
    setMine(null);
    setHud(snapshot(game.current));
  }, []);

  useEffect(() => {
    const key = (e) => {
      if (e.target.tagName === "INPUT") return;
      if (["Space", "ArrowUp", "KeyW"].includes(e.code)) { e.preventDefault(); if (!e.repeat) act(); }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [act]);

  useEffect(() => {
    const ctx = canvas.current.getContext("2d");
    let raf, last = 0, since = 0;
    const frame = (ms) => {
      const dt = Math.min((ms - last) / 1000 || 0.016, 0.035);
      last = ms;
      const g = game.current;
      step(g, dt);
      paint(ctx, g);
      since += dt;
      if (since > 0.09) { since = 0; setHud(snapshot(g)); }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, []);

  const over = hud.mode === "over";
  const saved = (r) => { setBoard(r.board); setMine(r.rank); };
  return html`<main class="shell">
    <header>
      <div class="brand"><span class="logo">◈</span><span>COVE <small>PLAYGROUND</small></span></div>
      <div class="language">⚛ REACT STARTER</div>
    </header>
    <section class="intro">
      <div class="eyebrow">YOUR FIRST OCEAN ADVENTURE</div>
      <h1>Dolphin <em>Jump</em> 🐬</h1>
      <p>Leap through the waves, chase the stars, and make a splash.</p>
    </section>
    <section class="game">
      <div class="stats">
        <div><small>SCORE</small><strong>${hud.score.toLocaleString()}</strong></div>
        <div><small>COMBO</small><strong>${hud.combo > 1 ? `×${hud.combo}` : "—"}</strong></div>
        <div><small>BEST</small><strong>${hud.best.toLocaleString()}</strong></div>
        <div><small>HEARTS</small><strong class="hearts">${"♥".repeat(Math.max(0, hud.lives))}${"♡".repeat(3 - Math.max(0, hud.lives))}</strong></div>
      </div>
      <div class="canvasWrap" onPointerDown=${(e) => { if (e.target.tagName === "CANVAS") { e.preventDefault(); act(); } }}>
        <canvas ref=${canvas} width=${W} height=${H}></canvas>
        ${hud.mode !== "playing" && html`<div class="overlay"><div class="overlayCard">
          <div class="emoji">${over ? "🌊" : "🐬"}</div>
          <h2>${over ? "Ocean adventure over!" : "Ready to make waves?"}</h2>
          <p>${over ? `You scored ${hud.score} points with ${hud.stars} stars. Dive in again!` : "Jump for golden stars. Dodge the pink jellyfish."}</p>
          ${over && html`<${SaveRun} key=${hud.score + ":" + hud.played} hud=${hud} onSaved=${saved} />`}
          <button onClick=${act}>${over ? "Play again ↻" : "Start swimming →"}</button>
          <${Board} rows=${board} mine=${mine} />
        </div></div>`}
      </div>
      <footer>
        <span><kbd>SPACE</kbd> or <kbd>↑</kbd> to jump</span>
        <span>📱 Tap anywhere to jump</span>
        <span>⭐ Collect stars · 🪼 Avoid jellyfish</span>
      </footer>
    </section>
    <${Environment} />
    <div class="bottom">BUILT FOR CURIOUS MINDS <span>•</span> COVE DEFAULT PROJECT</div>
  </main>`;
}

ReactDOM.createRoot(document.getElementById("root")).render(html`<${App} />`);
