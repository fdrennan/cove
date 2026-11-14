// Menus and the heads-up display, in React - and only those: React never runs
// in the frame loop. The game writes a few numbers into a small store about
// four times a second (and when the mode changes); React redraws only then.
// htm gives JSX-like templates (html`<${Hud} />`) with no compile step.
import type { Timing } from './loop.js';

const html = htm.bind(React.createElement);

export type Mode = 'title' | 'playing' | 'paused' | 'over';

export interface HudState {
  mode: Mode;
  score: number;
  best: number;
  time: number;
  count: number;
  backend: string;
  timing: Timing;
}

export interface HudActions {
  play(count: number): void;
  pause(): void;
  resume(): void;
}

export interface Hud {
  get(): HudState;
  set(patch: Partial<HudState>): void;
}

const COUNTS = [1000, 3000, 10000, 30000];

export function mountHud(el: HTMLElement, first: HudState, actions: HudActions): Hud {
  let state = first;
  const listeners = new Set<() => void>();
  const store: Hud = {
    get: () => state,
    set(patch: Partial<HudState>) {
      state = { ...state, ...patch };
      listeners.forEach((l) => l());
    },
  };
  const subscribe = (l: () => void) => {
    listeners.add(l);
    return () => listeners.delete(l);
  };

  function Stats({ s }: { s: HudState }) {
    const t = s.timing;
    return html`<div class="hud perf">
      <b>${t.fps.toFixed(0)}</b> fps
      <span>sim ${t.simMs.toFixed(2)} ms</span>
      <span>draw ${t.renderMs.toFixed(2)} ms</span>
      <span>${t.steps.toFixed(2)} steps/frame</span>
      <span>${s.count.toLocaleString()} drones · ${s.backend}</span>
    </div>`;
  }

  function Menu({ s }: { s: HudState }) {
    const [count, setCount] = React.useState(s.count);
    if (s.mode === 'playing') return null;
    const title = s.mode === 'paused' ? 'Paused' : s.mode === 'over' ? 'Caught' : 'Swarm';
    return html`<div class="menu">
      <h1>${title}</h1>
      ${s.mode === 'over' && html`<p class="big">${s.score} orbs · ${s.time.toFixed(1)} s</p>`}
      ${s.mode === 'title' && html`<p>Take the orbs; each one pushes the swarm back. Arrow keys or WASD, or hold a finger or the mouse where you want to go. P or Esc pauses.</p>`}
      ${s.mode !== 'paused' &&
      html`<div class="counts">
        ${COUNTS.map((n) => html`<button key=${n} class=${n === count ? 'on' : ''} onClick=${() => setCount(n)}>${n.toLocaleString()}</button>`)}
      </div>`}
      ${s.mode === 'paused'
        ? html`<button class="go" onClick=${actions.resume}>Resume</button>`
        : html`<button class="go" onClick=${() => actions.play(count)}>${s.mode === 'over' ? 'Again' : 'Play'}</button>`}
      <p class="best">best ${s.best} orbs</p>
    </div>`;
  }

  function Root() {
    const s = React.useSyncExternalStore(subscribe, store.get);
    return html`<${React.Fragment}>
      <div class="hud score"><b>${s.score}</b> orbs <span>${s.time.toFixed(1)} s</span></div>
      <${Stats} s=${s} />
      ${s.mode === 'playing' && html`<button class="hud pause" onClick=${actions.pause} aria-label="pause">❚❚</button>`}
      <${Menu} s=${s} />
    </${React.Fragment}>`;
  }

  ReactDOM.createRoot(el).render(html`<${Root} />`);
  return store;
}
