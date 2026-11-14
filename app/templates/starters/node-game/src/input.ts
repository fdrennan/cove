// Input, read by the simulation once per step - never inside event handlers,
// so a step sees one consistent state however many events arrived since the
// last. Keys by their physical position (KeyW), so other layouts work too.
export interface Input {
  x: number; // -1 left .. 1 right
  y: number; // -1 up .. 1 down
  // A held pointer or touch steers toward a point in world coordinates.
  pointing: boolean;
  tx: number;
  ty: number;
}

export interface Controls {
  read(): Input;
  dispose(): void;
}

export function controls(target: HTMLElement, toWorld: (cx: number, cy: number) => { x: number; y: number }): Controls {
  const down = new Set<string>();
  const now: Input = { x: 0, y: 0, pointing: false, tx: 0, ty: 0 };
  const key = (e: KeyboardEvent, on: boolean) => {
    if (on) down.add(e.code);
    else down.delete(e.code);
  };
  const kd = (e: KeyboardEvent) => key(e, true);
  const ku = (e: KeyboardEvent) => key(e, false);
  const point = (e: PointerEvent) => {
    const w = toWorld(e.clientX, e.clientY);
    now.tx = w.x;
    now.ty = w.y;
  };
  const pd = (e: PointerEvent) => {
    now.pointing = true;
    point(e);
    target.setPointerCapture(e.pointerId);
  };
  const pm = (e: PointerEvent) => {
    if (now.pointing) point(e);
  };
  const pu = () => {
    now.pointing = false;
  };
  window.addEventListener('keydown', kd);
  window.addEventListener('keyup', ku);
  window.addEventListener('blur', () => down.clear());
  target.addEventListener('pointerdown', pd);
  target.addEventListener('pointermove', pm);
  target.addEventListener('pointerup', pu);
  target.addEventListener('pointercancel', pu);
  const any = (...codes: string[]) => codes.some((c) => down.has(c));
  return {
    read() {
      now.x = (any('KeyD', 'ArrowRight') ? 1 : 0) - (any('KeyA', 'ArrowLeft') ? 1 : 0);
      now.y = (any('KeyS', 'ArrowDown') ? 1 : 0) - (any('KeyW', 'ArrowUp') ? 1 : 0);
      return now;
    },
    dispose() {
      window.removeEventListener('keydown', kd);
      window.removeEventListener('keyup', ku);
    },
  };
}
