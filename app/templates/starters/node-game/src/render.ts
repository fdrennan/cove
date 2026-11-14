// Drawing, with PixiJS 8: it reads the simulation and never changes it. Each
// frame places everything between its last two steps (alpha from the loop),
// so motion is smooth at any display rate. The drones are one
// ParticleContainer - thousands of sprites in a handful of draw calls, with
// only position and rotation sent to the GPU each frame - and nothing is
// allocated per frame. Textures are drawn at start, so there are no assets.
import { H, W } from './sim.js';
import type { Sim } from './sim.js';

const COLORS = [0x5ec8f2, 0x8f7cf6, 0x52d6a4, 0xf28fb3];

export interface Renderer {
  /** "webgl" or "webgpu": what PixiJS chose on this device. */
  readonly backend: string;
  readonly canvas: HTMLCanvasElement;
  /** A new simulation (another drone count, a new game). */
  use(sim: Sim): void;
  draw(alpha: number): void;
  /** A point on the page as world coordinates (for pointer steering). */
  toWorld(cx: number, cy: number): { x: number; y: number };
}

export async function createRenderer(host: HTMLElement): Promise<Renderer> {
  const app = new PIXI.Application();
  await app.init({
    resizeTo: host,
    background: 0x0b0e14,
    antialias: false,
    autoDensity: true,
    resolution: Math.min(window.devicePixelRatio || 1, 2),
    preference: 'webgl',
  });
  host.appendChild(app.canvas);
  // The loop draws (app.render), not Pixi's own ticker.
  app.ticker.stop();

  const world = new PIXI.Container();
  app.stage.addChild(world);
  world.addChild(new PIXI.Graphics().rect(0, 0, W, H).fill(0x0f131b).stroke({ width: 2, color: 0x263042 }));

  const dart = app.renderer.generateTexture(new PIXI.Graphics().poly([7, 0, -5, -4, -3, 0, -5, 4]).fill(0xffffff));
  const drones = new PIXI.ParticleContainer({ dynamicProperties: { position: true, rotation: true } });
  world.addChild(drones);

  const orb = new PIXI.Graphics().circle(0, 0, 9).fill(0xf2b84b).circle(0, 0, 15).stroke({ width: 2, color: 0xf2b84b, alpha: 0.5 });
  const ring = new PIXI.Graphics().circle(0, 0, 100).stroke({ width: 3, color: 0xf2b84b });
  const ship = new PIXI.Graphics().poly([16, 0, -10, -10, -5, 0, -10, 10]).fill(0xf4f6fb);
  world.addChild(orb, ring, ship);

  let sim: Sim | null = null;
  let particles: any[] = [];
  let seenW = 0;
  let seenH = 0;

  // The world, scaled to fit the screen and centred.
  const fit = () => {
    const s = Math.min(app.screen.width / W, app.screen.height / H);
    world.scale.set(s);
    world.position.set((app.screen.width - W * s) / 2, (app.screen.height - H * s) / 2);
    seenW = app.screen.width;
    seenH = app.screen.height;
  };
  fit();

  return {
    backend: String(app.renderer.name ?? 'webgl'),
    canvas: app.canvas,
    use(next: Sim) {
      sim = next;
      drones.removeParticles();
      particles = new Array(next.count);
      for (let i = 0; i < next.count; i++) {
        const p = new PIXI.Particle({ texture: dart, anchorX: 0.5, anchorY: 0.5, tint: COLORS[i % COLORS.length] });
        particles[i] = p;
        drones.addParticle(p);
      }
    },
    draw(alpha: number) {
      if (!sim) return;
      if (app.screen.width !== seenW || app.screen.height !== seenH) fit();
      const { x, y, px, py, heading } = sim;
      for (let i = 0; i < particles.length; i++) {
        const p = particles[i];
        p.x = px[i] + (x[i] - px[i]) * alpha;
        p.y = py[i] + (y[i] - py[i]) * alpha;
        p.rotation = heading[i];
      }
      const s = sim.ship;
      ship.position.set(s.px + (s.x - s.px) * alpha, s.py + (s.y - s.py) * alpha);
      ship.rotation = s.angle;
      ship.alpha = s.alive ? 1 : 0.35;
      orb.position.set(sim.orb.x, sim.orb.y);
      ring.visible = sim.pulse > 0;
      ring.position.copyFrom(ship.position);
      ring.scale.set(2.6 * (1 - sim.pulse) + 0.2);
      ring.alpha = sim.pulse;
      app.render();
    },
    toWorld(cx: number, cy: number) {
      const r = app.canvas.getBoundingClientRect();
      return { x: (cx - r.left - world.x) / world.scale.x, y: (cy - r.top - world.y) / world.scale.y };
    },
  };
}
