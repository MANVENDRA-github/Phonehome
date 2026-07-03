// ?hero=1 choreography for the 10-second launch GIF (SPEC M4 proof asset):
// slow auto-rotation while the device filter cycles through the household, one
// named device at a time, so the GIF shows "labeled devices firing arcs"
// (RESEARCH §5). Runs until stopped; the Playwright harness records ~12 s.

import type { GlobeScene } from "./GlobeScene";

export type HeroHooks = {
  scene: GlobeScene;
  getDevices: () => { id: number; name: string }[];
  setFilter: (ids: Set<number> | null) => void;
  setCallout: (text: string | null) => void;
};

const ALL_HOUSEHOLD_MS = 2600;
const PER_DEVICE_MS = 1400;

export function startHero(hooks: HeroHooks): () => void {
  hooks.scene.autoRotate = 0.12;
  let timer = 0;
  let stopped = false;

  const cycle = () => {
    if (stopped) return;
    const devices = hooks.getDevices();
    if (devices.length === 0) {
      timer = window.setTimeout(cycle, 500);
      return;
    }
    hooks.setFilter(null);
    hooks.setCallout("your household");
    let i = 0;
    const step = () => {
      if (stopped) return;
      if (i < devices.length) {
        const d = devices[i++];
        hooks.setFilter(new Set([d.id]));
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
    hooks.setFilter(null);
    hooks.setCallout(null);
  };
}
