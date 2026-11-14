// __HOST__: the game, put together. The simulation (sim.ts) moves in fixed
// steps; the renderer (render.ts, PixiJS) draws between them; the loop
// (loop.ts) runs both; React (hud.ts) shows menus and numbers, and is told
// about the game a few times a second, never per frame.
import { controls } from './input.js';
import { startLoop } from './loop.js';
import { createRenderer } from './render.js';
import { createSim } from './sim.js';
import type { Sim } from './sim.js';
import { mountHud } from './hud.js';
import type { Mode } from './hud.js';

const BEST = '__HOST__/best';

async function main(): Promise<void> {
  const stage = document.getElementById('stage') as HTMLElement;
  const renderer = await createRenderer(stage);
  const input = controls(renderer.canvas, renderer.toWorld);

  let mode: Mode = 'title';
  let seed = 1;
  let sim: Sim = createSim(3000, seed);
  renderer.use(sim);

  const hud = mountHud(
    document.getElementById('hud') as HTMLElement,
    {
      mode,
      score: 0,
      best: Number(localStorage.getItem(BEST) ?? 0),
      time: 0,
      count: sim.count,
      backend: renderer.backend,
      timing: { fps: 0, simMs: 0, renderMs: 0, steps: 0 },
    },
    {
      play(count: number) {
        // A new seed each game; the same seed would replay the same swarm.
        seed = (seed * 1103515245 + 12345) >>> 0;
        sim = createSim(count, seed);
        renderer.use(sim);
        setMode('playing');
      },
      pause: () => setMode('paused'),
      resume: () => setMode('playing'),
    },
  );

  function setMode(m: Mode): void {
    mode = m;
    hud.set({ mode, score: sim.score, time: sim.time, count: sim.count });
  }

  window.addEventListener('keydown', (e) => {
    if (e.code === 'KeyP' || e.code === 'Escape') {
      if (mode === 'playing') setMode('paused');
      else if (mode === 'paused') setMode('playing');
    }
  });
  // A hidden tab pauses: the loop stops being called, and coming back should
  // not find the ship already caught.
  document.addEventListener('visibilitychange', () => {
    if (document.hidden && mode === 'playing') setMode('paused');
  });

  startLoop({
    step() {
      if (mode !== 'playing') return;
      sim.step(input.read());
      if (!sim.ship.alive) {
        const best = Math.max(hud.get().best, sim.score);
        localStorage.setItem(BEST, String(best));
        hud.set({ best });
        setMode('over');
      }
    },
    render: (alpha) => renderer.draw(mode === 'playing' ? alpha : 1),
    timing(t) {
      hud.set({ timing: t, score: sim.score, time: sim.time });
    },
  });
}

main().catch((e) => {
  document.body.insertAdjacentHTML('beforeend', `<pre class="fail">${String(e && e.stack ? e.stack : e)}</pre>`);
});
