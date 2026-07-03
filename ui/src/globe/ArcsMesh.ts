// Instanced device→country arcs. ONE draw call for all arcs: a unit ribbon
// (PlaneGeometry, 64 segments) instanced N times; the vertex stage bends each
// instance along the great circle between its per-instance endpoints.
//
// Written in TSL (three shading language) with instanced buffer attributes
// ONLY — no storage buffers, no compute — so the identical node graph compiles
// to WGSL on WebGPU and GLSL on the WebGL2 fallback (M4 requirement).
//
// The altitude profile must stay in sync with math.arcAltitude (CPU picking).

import {
  AdditiveBlending,
  DoubleSide,
  InstancedBufferAttribute,
  InstancedBufferGeometry,
  Mesh,
  MeshBasicNodeMaterial,
  PlaneGeometry,
} from "three/webgpu";
import {
  Fn,
  acos,
  clamp,
  cos,
  cross,
  dot,
  exp,
  float,
  fract,
  instancedBufferAttribute,
  normalize,
  positionGeometry,
  select,
  sin,
  time,
  uv,
  vec2,
  vec3,
  vec4,
} from "three/tsl";
import type { Vec3 } from "./math";

export type ArcInstance = {
  start: Vec3; // unit vector (home)
  end: Vec3; // unit vector (country centroid)
  color: [number, number, number];
  /** 0..1 relative query volume — drives width + idle brightness. */
  weight: number;
  /** 0..1 deterministic per-arc phase/jitter seed. */
  seed: number;
};

const SEGMENTS = 64;
/** Arc altitude: base lift + extra per radian of angular span (see math.ts). */
export const ARC_H0 = 0.05;
export const ARC_H1 = 0.16;
const BASE_WIDTH = 0.008;

export class ArcsMesh {
  readonly mesh: Mesh;
  private geometry: InstancedBufferGeometry;
  private material: MeshBasicNodeMaterial;
  private capacity = 0;
  private count = 0;

  private aStart!: InstancedBufferAttribute;
  private aEnd!: InstancedBufferAttribute;
  private aColor!: InstancedBufferAttribute;
  private aAnim!: InstancedBufferAttribute; // (phase, speed, weight, width)
  private aState!: InstancedBufferAttribute; // (visible, pulse)

  constructor() {
    this.geometry = new InstancedBufferGeometry();
    const ribbon = new PlaneGeometry(1, 1, SEGMENTS, 1);
    this.geometry.index = ribbon.index;
    this.geometry.setAttribute("position", ribbon.getAttribute("position"));
    this.geometry.setAttribute("uv", ribbon.getAttribute("uv"));
    this.allocate(64);

    this.material = new MeshBasicNodeMaterial({
      transparent: true,
      depthWrite: false,
      side: DoubleSide,
      blending: AdditiveBlending,
    });
    this.buildNodes();

    this.mesh = new Mesh(this.geometry, this.material);
    // Vertex positions are shader-generated; the CPU-side bounds are wrong.
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 2;
  }

  private allocate(capacity: number) {
    this.capacity = capacity;
    const prev = {
      start: this.aStart?.array as Float32Array | undefined,
      end: this.aEnd?.array as Float32Array | undefined,
      color: this.aColor?.array as Float32Array | undefined,
      anim: this.aAnim?.array as Float32Array | undefined,
      state: this.aState?.array as Float32Array | undefined,
    };
    const grow = (old: Float32Array | undefined, size: number) => {
      const arr = new Float32Array(size);
      if (old) arr.set(old.subarray(0, Math.min(old.length, size)));
      return arr;
    };
    this.aStart = new InstancedBufferAttribute(grow(prev.start, capacity * 3), 3);
    this.aEnd = new InstancedBufferAttribute(grow(prev.end, capacity * 3), 3);
    this.aColor = new InstancedBufferAttribute(grow(prev.color, capacity * 3), 3);
    this.aAnim = new InstancedBufferAttribute(grow(prev.anim, capacity * 4), 4);
    this.aState = new InstancedBufferAttribute(grow(prev.state, capacity * 2), 2);
    this.geometry.setAttribute("iStart", this.aStart);
    this.geometry.setAttribute("iEnd", this.aEnd);
    this.geometry.setAttribute("iColor", this.aColor);
    this.geometry.setAttribute("iAnim", this.aAnim);
    this.geometry.setAttribute("iState", this.aState);
  }

  private buildNodes() {
    // Interior graph nodes are deliberately untyped: three 0.185's TSL type
    // surface cannot follow node types through select()/mix() chains (unions
    // explode). The node compiler type-checks the graph when it builds the
    // WGSL/GLSL anyway, and the ?gl=1 A/B run exercises both outputs.
    /* eslint-disable @typescript-eslint/no-explicit-any */
    type N = any;
    const iStart: N = vec3(instancedBufferAttribute(this.aStart, "vec3") as N);
    const iEnd: N = vec3(instancedBufferAttribute(this.aEnd, "vec3") as N);
    const iColor: N = vec3(instancedBufferAttribute(this.aColor, "vec3") as N);
    const iAnim: N = vec4(instancedBufferAttribute(this.aAnim, "vec4") as N);
    const iState: N = vec2(instancedBufferAttribute(this.aState, "vec2") as N);

    this.material.positionNode = Fn(() => {
      const t: N = uv().x.toVar();
      const phase: N = iAnim.x;
      const s: N = normalize(iStart).toVar();
      const e: N = normalize(iEnd).toVar();

      const cosOmega: N = clamp(dot(s, e), -1, 1).toVar();
      const omega: N = acos(cosOmega).toVar();
      const sinOmega: N = sin(omega).toVar();
      const degenerate: N = omega.lessThan(1e-3);

      // Great-circle slerp with a normalized-lerp fallback for tiny spans.
      const wa: N = sin(omega.mul(float(1).sub(t))).div(sinOmega);
      const wb: N = sin(omega.mul(t)).div(sinOmega);
      const nlerp: N = normalize(s.mul(float(1).sub(t)).add(e.mul(t)));
      const p: N = select(degenerate, nlerp, s.mul(wa).add(e.mul(wb))).toVar();

      // Analytic tangent of the slerp (for the ribbon's sideways direction).
      const dwa: N = cos(omega.mul(float(1).sub(t))).mul(omega).div(sinOmega).negate();
      const dwb: N = cos(omega.mul(t)).mul(omega).div(sinOmega);
      const tangent: N = select(
        degenerate,
        normalize(e.sub(s)),
        normalize(s.mul(dwa).add(e.mul(dwb))),
      ).toVar();
      const binormal: N = normalize(cross(p, tangent)).toVar();

      // Altitude: sin(π·t) lift, higher for longer arcs, de-stacked by a
      // per-instance jitter so the many arcs sharing home don't overlap.
      const jitter: N = phase.sub(0.5).mul(0.03);
      const lift: N = float(ARC_H0).add(omega.mul(ARC_H1)).add(jitter);
      const radius: N = float(1).add(lift.mul(sin(t.mul(Math.PI))));

      // Lateral de-stack (also phase-derived, vanishes at the endpoints).
      const lateral: N = binormal.mul(phase.sub(0.5).mul(0.05).mul(sin(t.mul(Math.PI))));

      const width: N = float(BASE_WIDTH).mul(iAnim.w);
      return p
        .mul(radius)
        .add(binormal.mul(positionGeometry.y.mul(width)))
        .add(lateral);
    })();

    // Faint persistent trail + a traveling comet head; SSE pulses boost it.
    const brightness: N = Fn(() => {
      const t: N = uv().x;
      const phase: N = iAnim.x;
      const speed: N = iAnim.y;
      const weight: N = iAnim.z;
      const pulse: N = iState.y;

      const head: N = fract(time.mul(speed).add(phase));
      const behind: N = fract(head.sub(t));
      const comet: N = exp(behind.mul(-10));

      const idle: N = float(0.22).add(weight.mul(0.18));
      return idle.add(comet.mul(float(0.8).add(pulse.mul(1.6))));
    })();

    this.material.colorNode = iColor.mul(brightness);
    this.material.opacityNode = clamp(brightness, 0, 1).mul(iState.x);
    /* eslint-enable @typescript-eslint/no-explicit-any */
  }

  get arcCount() {
    return this.count;
  }

  setArcs(arcs: ArcInstance[]) {
    if (arcs.length > this.capacity) {
      let cap = this.capacity;
      while (cap < arcs.length) cap *= 2;
      this.allocate(cap);
      this.buildNodes(); // attributes were re-created; rebind the node graph
      this.material.needsUpdate = true;
    }
    const start = this.aStart.array as Float32Array;
    const end = this.aEnd.array as Float32Array;
    const color = this.aColor.array as Float32Array;
    const anim = this.aAnim.array as Float32Array;
    const state = this.aState.array as Float32Array;
    arcs.forEach((a, i) => {
      start.set(a.start, i * 3);
      end.set(a.end, i * 3);
      color.set(a.color, i * 3);
      anim[i * 4] = a.seed;
      anim[i * 4 + 1] = 0.08 + a.seed * 0.1 + a.weight * 0.12; // comet speed
      anim[i * 4 + 2] = a.weight;
      anim[i * 4 + 3] = 0.6 + a.weight * 1.4; // width scale
      state[i * 2] = 1; // visible
      state[i * 2 + 1] = 0; // pulse
    });
    this.count = arcs.length;
    this.geometry.instanceCount = arcs.length;
    for (const attr of [this.aStart, this.aEnd, this.aColor, this.aAnim, this.aState]) {
      attr.needsUpdate = true;
    }
  }

  /** Show/hide per instance without rebuilding geometry (device filter). */
  setVisible(visible: (index: number) => boolean) {
    const state = this.aState.array as Float32Array;
    for (let i = 0; i < this.count; i++) {
      state[i * 2] = visible(i) ? 1 : 0;
    }
    this.aState.needsUpdate = true;
  }

  /** SSE pulse: kick one arc's brightness; decays in update(). */
  pulse(index: number) {
    if (index < 0 || index >= this.count) return;
    const state = this.aState.array as Float32Array;
    state[index * 2 + 1] = 1;
    this.aState.needsUpdate = true;
  }

  /** Per-frame pulse decay. */
  update(dtSeconds: number) {
    const state = this.aState.array as Float32Array;
    let dirty = false;
    const decay = Math.exp(-dtSeconds * 1.8);
    for (let i = 0; i < this.count; i++) {
      const v = state[i * 2 + 1];
      if (v > 0.003) {
        state[i * 2 + 1] = v * decay;
        dirty = true;
      } else if (v !== 0) {
        state[i * 2 + 1] = 0;
        dirty = true;
      }
    }
    if (dirty) this.aState.needsUpdate = true;
  }

  dispose() {
    this.geometry.dispose();
    this.material.dispose();
  }
}
