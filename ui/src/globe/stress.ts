// ?stress=N benchmark mode (SPEC M4 perf gate, target >=10k visible arcs):
// N deterministic synthetic arcs from home to seeded random endpoints. These
// replace the real arcs for the measurement run and are never pickable.

import type { ArcInstance } from "./ArcsMesh";
import { latLonToVec3 } from "./math";

/** Same LCG family as the fixture generator — deterministic across runs. */
class Lcg {
  constructor(private state: bigint) {}
  next(): number {
    this.state = (this.state * 6364136223846793005n + 1442695040888963407n) & 0xffffffffffffffffn;
    return Number(this.state >> 32n) / 0x1_0000_0000;
  }
}

export function stressArcs(n: number, home: { lat: number; lon: number }): ArcInstance[] {
  const rng = new Lcg(0x5eed_2026_0703n);
  const start = latLonToVec3(home.lat, home.lon);
  const arcs: ArcInstance[] = [];
  for (let i = 0; i < n; i++) {
    const lat = rng.next() * 150 - 75;
    const lon = rng.next() * 360 - 180;
    const trackerShare = rng.next();
    arcs.push({
      start,
      end: latLonToVec3(lat, lon),
      color:
        trackerShare > 0.5
          ? [0.957, 0.247, 0.369] // rose
          : [0.204, 0.827, 0.6], // emerald
      weight: 0.2 + rng.next() * 0.8,
      seed: rng.next(),
    });
  }
  return arcs;
}
