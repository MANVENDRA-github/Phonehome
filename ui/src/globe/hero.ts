// ?hero=1 choreography for the 10-second launch GIF (SPEC M4 proof asset):
// slow auto-rotation, camera pulled back so long-arc apexes stay in frame,
// and a device-emphasis cycle — the named device's arcs pulse at full
// brightness while the rest of the household stays dimly visible, so the GIF
// shows "labeled devices firing arcs" (RESEARCH §5) without losing the
// worldwide starburst. Runs until stopped; the Playwright harness records it.

import type { GlobeScene } from "./GlobeScene";

export type HeroHooks = {
  scene: GlobeScene;
  getDevices: () => { id: number; name: string }[];
  setCallout: (text: string | null) => void;
};

const ALL_HOUSEHOLD_MS = 2600;
const PER_DEVICE_MS = 1400;
const HERO_DISTANCE = 4.2; // long-arc apexes (~1.5R) fit at fov 40
const HERO_ROTATE = 0.05; // rad/s — ~29° across the 10 s GIF window

export function startHero(hooks: HeroHooks): () => void {
  hooks.scene.autoRotate = HERO_ROTATE;
  hooks.scene.setDistance(HERO_DISTANCE);
  let timer = 0;
  let stopped = false;

  const cycle = () => {
    if (stopped) return;
    const devices = hooks.getDevices();
    if (devices.length === 0) {
      timer = window.setTimeout(cycle, 500);
      return;
    }
    hooks.scene.setDeviceEmphasis(null);
    hooks.setCallout("your household");
    let i = 0;
    const step = () => {
      if (stopped) return;
      if (i < devices.length) {
        const d = devices[i++];
        hooks.scene.setDeviceEmphasis(new Set([d.id]));
        hooks.setCallout(d.name);
        timer = window.setTimeout(step, PER_DEVICE_MS);
      } else {
        timer = window.setTimeout(cycle, 200);
      }
    };
    timer = window.setTimeout(step, ALL_HOUSEHOLD_MS);
  };
  cycle();

  return () => {
    stopped = true;
    window.clearTimeout(timer);
    hooks.scene.autoRotate = 0.04;
    hooks.scene.setDistance(2.6);
    hooks.scene.setDeviceEmphasis(null);
    hooks.setCallout(null);
  };
}
