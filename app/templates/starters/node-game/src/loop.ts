// The frame loop: the simulation in fixed steps, drawing at whatever rate the
// display runs (60, 120, 144 Hz), and the two never mixed. Each frame adds
// the time that passed to an accumulator and takes whole STEPs from it; what
// is left over (alpha, 0..1) tells the renderer how far between the last two
// steps to draw. A slow frame runs at most MAX_STEPS steps and drops the
// rest, so one hitch cannot turn into a spiral of ever-longer frames.
import { STEP } from './sim.js';

const MAX_STEPS = 5;
const MAX_FRAME = 0.25;

export interface Timing {
  fps: number;
  /** Milliseconds per frame spent stepping, and drawing (averages). */
  simMs: number;
  renderMs: number;
  /** Steps per frame on average: 1 at 60 Hz, 0.5 at 120 Hz. */
  steps: number;
}

export interface LoopHooks {
  step(): void;
  render(alpha: number): void;
  /** About four times a second: for the HUD, never per frame. */
  timing(t: Timing): void;
}

export function startLoop(hooks: LoopHooks): () => void {
  let last = performance.now();
  let acc = 0;
  let running = true;
  let frames = 0;
  let steps = 0;
  let simT = 0;
  let renderT = 0;
  let since = last;

  const frame = (now: number) => {
    if (!running) return;
    acc += Math.min((now - last) / 1000, MAX_FRAME);
    last = now;
    const t0 = performance.now();
    let n = 0;
    while (acc >= STEP && n < MAX_STEPS) {
      hooks.step();
      acc -= STEP;
      n++;
    }
    if (n === MAX_STEPS) acc = 0;
    const t1 = performance.now();
    hooks.render(acc / STEP);
    const t2 = performance.now();
    frames++;
    steps += n;
    simT += t1 - t0;
    renderT += t2 - t1;
    if (now - since >= 250) {
      hooks.timing({ fps: (frames * 1000) / (now - since), simMs: simT / frames, renderMs: renderT / frames, steps: steps / frames });
      frames = steps = simT = renderT = 0;
      since = now;
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
  return () => {
    running = false;
  };
}
