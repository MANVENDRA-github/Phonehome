// Vanilla three.js globe scene (no react-three-fiber): WebGPU renderer with
// automatic WebGL2 fallback, one instanced ArcsMesh, textured earth, country
// labels as a DOM overlay, and a ~60-line inertial orbit controller (a custom
// controller avoids three/examples addons, which import from the classic
// `three` build and would bundle the core twice next to `three/webgpu`).

import {
  AdditiveBlending,
  BackSide,
  Color,
  Group,
  Mesh,
  MeshBasicNodeMaterial,
  PerspectiveCamera,
  Scene,
  SphereGeometry,
  SRGBColorSpace,
  Texture,
  TextureLoader,
  Vector3,
  WebGPURenderer,
} from "three/webgpu";
import { Fn, abs, float, normalView, pow, vec3 } from "three/tsl";
import { ArcsMesh, ARC_H0, ARC_H1, type ArcInstance } from "./ArcsMesh";
import { COUNTRY_CENTROIDS } from "./countryCentroids";
import { FrameStats } from "./frameStats";
import { arcPolyline, latLonToVec3, type Vec3 as V3 } from "./math";
import earthTextureUrl from "../assets/earth_atmos_2048.jpg";

export type ArcDatum = {
  device_id: number;
  device_name: string;
  country: string;
  queries: number;
  tracker_queries: number;
};

export type Backend = "webgpu" | "webgl";

export type GlobeSceneOptions = {
  container: HTMLElement;
  forceWebGL: boolean;
  onArcClick: (index: number | null) => void;
  onReady: (backend: Backend) => void;
};

/** Default origin when no home is configured: mid-Atlantic, clearly nowhere. */
export const NEUTRAL_HOME = { lat: 25, lon: -40 };

const PICK_RADIUS_PX = 14;
const POLYLINE_SAMPLES = 9;

/** Tracker-share color ramp matching the app's accent language. */
function arcColor(trackerShare: number): [number, number, number] {
  const emerald = new Color("#34d399");
  const amber = new Color("#f59e0b");
  const rose = new Color("#f43f5e");
  const c =
    trackerShare <= 0.5
      ? emerald.lerp(amber, trackerShare * 2)
      : amber.lerp(rose, (trackerShare - 0.5) * 2);
  return [c.r, c.g, c.b];
}

export class GlobeScene {
  readonly frameStats = new FrameStats();
  backend: Backend = "webgl";
  autoRotate = 0.04; // rad/s; hero mode raises it

  private opts: GlobeSceneOptions;
  private renderer!: WebGPURenderer;
  private camera!: PerspectiveCamera;
  private scene = new Scene();
  private world = new Group(); // rotates; camera stays put
  private arcsMesh = new ArcsMesh();
  private labelLayer!: HTMLDivElement;
  private labels = new Map<string, HTMLDivElement>();
  private disposed = false;

  private arcData: ArcDatum[] = [];
  private polylines: V3[][] = [];
  private home = NEUTRAL_HOME;

  // Orbit state.
  private yaw = 0.8;
  private pitch = 0.35;
  private yawVel = 0;
  private pitchVel = 0;
  private distance = 2.6;
  private dragging = false;
  private lastPointer = { x: 0, y: 0 };
  private downPointer = { x: 0, y: 0 };
  private lastInteraction = 0;
  private lastFrame = 0;

  constructor(opts: GlobeSceneOptions) {
    this.opts = opts;
  }

  async init() {
    const { container, forceWebGL } = this.opts;

    let renderer: WebGPURenderer | null = null;
    if (!forceWebGL) {
      try {
        renderer = new WebGPURenderer({ antialias: true });
        await renderer.init();
      } catch {
        renderer?.dispose();
        renderer = null;
      }
    }
    if (!renderer) {
      renderer = new WebGPURenderer({ antialias: true, forceWebGL: true });
      await renderer.init();
    }
    if (this.disposed) {
      renderer.dispose();
      return;
    }
    this.renderer = renderer;
    const backend = renderer.backend as unknown as { isWebGPUBackend?: boolean };
    this.backend = backend.isWebGPUBackend === true ? "webgpu" : "webgl";
    this.frameStats.backend = this.backend;

    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.domElement.style.display = "block";
    this.renderer.domElement.style.touchAction = "none";
    container.appendChild(this.renderer.domElement);

    this.labelLayer = document.createElement("div");
    this.labelLayer.style.cssText =
      "position:absolute;inset:0;overflow:hidden;pointer-events:none;font:11px ui-monospace,monospace";
    container.appendChild(this.labelLayer);

    this.camera = new PerspectiveCamera(40, 1, 0.1, 100);
    this.scene.add(this.world);
    this.buildGlobe();
    this.world.add(this.arcsMesh.mesh);
    this.bindInput();
    this.resize();

    this.renderer.setAnimationLoop((t) => this.frame(t));
    this.opts.onReady(this.backend);
  }

  private buildGlobe() {
    const texture = new TextureLoader().load(earthTextureUrl, (t: Texture) => {
      t.colorSpace = SRGBColorSpace;
    });
    const earthMat = new MeshBasicNodeMaterial({ map: texture });
    // Darken toward the app's slate palette so the red arcs carry the scene.
    earthMat.color = new Color("#8193a8");
    const earth = new Mesh(new SphereGeometry(1, 64, 48), earthMat);
    earth.renderOrder = 0;
    this.world.add(earth);

    // Rim-glow atmosphere: back-side shell, brightness peaking at the silhouette.
    const atmosphereMat = new MeshBasicNodeMaterial({
      transparent: true,
      side: BackSide,
      depthWrite: false,
      blending: AdditiveBlending,
    });
    atmosphereMat.colorNode = vec3(0.23, 0.51, 0.96);
    atmosphereMat.opacityNode = Fn(() =>
      pow(float(1).sub(abs(normalView.z)), 3).mul(0.7),
    )();
    const atmosphere = new Mesh(new SphereGeometry(1.045, 64, 48), atmosphereMat);
    atmosphere.renderOrder = 1;
    this.world.add(atmosphere);
  }

  setHome(home: { lat: number; lon: number } | null) {
    this.home = home ?? NEUTRAL_HOME;
    // Until the user grabs the globe, keep home facing the camera — arcs
    // radiate from there, so that's where the story is.
    if (this.lastInteraction === 0) {
      const v = latLonToVec3(this.home.lat, this.home.lon);
      this.yaw = Math.atan2(-v[0], v[2]);
      this.pitch = Math.max(-1.2, Math.min(1.2, Math.atan2(v[1], Math.hypot(v[0], v[2]))));
    }
    this.rebuildArcs();
  }

  setArcs(arcs: ArcDatum[]) {
    this.arcData = arcs;
    this.rebuildArcs();
  }

  private rebuildArcs() {
    const start = latLonToVec3(this.home.lat, this.home.lon);
    const maxQueries = Math.max(1, ...this.arcData.map((a) => a.queries));
    const instances: ArcInstance[] = [];
    this.polylines = [];
    for (const [i, a] of this.arcData.entries()) {
      const centroid = COUNTRY_CENTROIDS[a.country];
      if (!centroid) continue;
      const end = latLonToVec3(centroid.lat, centroid.lon);
      const weight = Math.sqrt(a.queries / maxQueries);
      const seed = ((i * 2654435761) % 1000) / 1000; // deterministic per index
      instances.push({
        start,
        end,
        color: arcColor(a.queries ? a.tracker_queries / a.queries : 0),
        weight,
        seed,
      });
      this.polylines.push(arcPolyline(start, end, POLYLINE_SAMPLES, 1, ARC_H0, ARC_H1));
    }
    this.arcsMesh.setArcs(instances);
    this.syncLabels(start);
  }

  /** Benchmark mode: replace real arcs with synthetic ones (non-pickable). */
  setStress(instances: ArcInstance[]) {
    this.arcData = [];
    this.polylines = [];
    this.arcsMesh.setArcs(instances);
    this.labelLayer?.replaceChildren();
    this.labels.clear();
  }

  /** Device filter: hide arcs whose device isn't in `ids` (null = show all). */
  setVisibleDevices(ids: Set<number> | null) {
    this.arcsMesh.setVisible((i) => ids === null || ids.has(this.arcData[i]?.device_id));
  }

  /** SSE pulse routing; false when the (device, country) pair has no arc yet. */
  pulseByKey(deviceId: number, country: string | null): boolean {
    if (!country) return true; // unmapped destinations have no arc to light up
    const index = this.arcData.findIndex(
      (a) => a.device_id === deviceId && a.country === country,
    );
    if (index === -1) return false;
    this.arcsMesh.pulse(index);
    return true;
  }

  get arcCount() {
    return this.arcsMesh.arcCount;
  }

  /** Screen-space anchor of one arc (Playwright click target). Scans the
   * polyline outward from the midpoint for a camera-facing, in-canvas sample
   * so the returned point is actually clickable. */
  arcScreenPoint(index: number): { x: number; y: number } | null {
    const line = this.polylines[index];
    if (!line || !this.renderer) return null;
    const el = this.renderer.domElement;
    const mid = Math.floor(line.length / 2);
    const order = line
      .map((_, i) => i)
      .sort((a, b) => Math.abs(a - mid) - Math.abs(b - mid));
    for (const i of order) {
      if (!this.facesCamera(line[i])) continue;
      const s = this.project(line[i]);
      if (!s) continue;
      if (s.x >= 4 && s.y >= 4 && s.x <= el.clientWidth - 4 && s.y <= el.clientHeight - 4) {
        return s;
      }
    }
    return null;
  }

  private project(p: V3): { x: number; y: number } | null {
    const v = new Vector3(p[0], p[1], p[2]).applyMatrix4(this.world.matrixWorld);
    const behind = v.clone().sub(this.camera.position).dot(new Vector3(0, 0, -1).applyQuaternion(this.camera.quaternion)) <= 0;
    v.project(this.camera);
    if (behind || v.z > 1) return null;
    const el = this.renderer.domElement;
    return {
      x: ((v.x + 1) / 2) * el.clientWidth,
      y: ((1 - v.y) / 2) * el.clientHeight,
    };
  }

  /** True when a world-space point faces the camera (not behind the globe). */
  private facesCamera(p: V3): boolean {
    const world = new Vector3(p[0], p[1], p[2]).applyMatrix4(this.world.matrixWorld);
    const normal = world.clone().normalize();
    const toCamera = this.camera.position.clone().sub(world).normalize();
    return normal.dot(toCamera) > 0.05;
  }

  private syncLabels(homeVec: V3) {
    if (!this.labelLayer) return;
    this.labelLayer.replaceChildren();
    this.labels.clear();
    const seen = new Set<string>();
    for (const a of this.arcData) {
      if (seen.has(a.country) || !COUNTRY_CENTROIDS[a.country]) continue;
      seen.add(a.country);
      const el = document.createElement("div");
      el.textContent = COUNTRY_CENTROIDS[a.country].name;
      el.style.cssText =
        "position:absolute;transform:translate(-50%,-140%);color:#cbd5e1;text-shadow:0 1px 3px #000;white-space:nowrap";
      this.labelLayer.appendChild(el);
      this.labels.set(a.country, el);
    }
    const homeEl = document.createElement("div");
    homeEl.textContent = "⌂ home";
    homeEl.style.cssText =
      "position:absolute;transform:translate(-50%,-140%);color:#34d399;font-weight:600;text-shadow:0 1px 3px #000;white-space:nowrap";
    this.labelLayer.appendChild(homeEl);
    this.labels.set("__home__", homeEl);
    void homeVec;
  }

  private updateLabels() {
    if (this.labels.size === 0) return;
    const place = (el: HTMLDivElement, p: V3) => {
      const screen = this.facesCamera(p) ? this.project(p) : null;
      if (!screen) {
        el.style.visibility = "hidden";
        return;
      }
      el.style.visibility = "visible";
      el.style.left = `${screen.x}px`;
      el.style.top = `${screen.y}px`;
    };
    for (const [country, el] of this.labels) {
      if (country === "__home__") {
        place(el, latLonToVec3(this.home.lat, this.home.lon, 1.01));
      } else {
        const c = COUNTRY_CENTROIDS[country];
        place(el, latLonToVec3(c.lat, c.lon, 1.01));
      }
    }
  }

  // --- input: drag-rotate with inertia, wheel zoom, click-to-pick ---

  private bindInput() {
    const el = this.renderer.domElement;
    el.addEventListener("pointerdown", (e) => {
      this.dragging = true;
      this.lastPointer = { x: e.clientX, y: e.clientY };
      this.downPointer = { x: e.clientX, y: e.clientY };
      el.setPointerCapture(e.pointerId);
    });
    el.addEventListener("pointermove", (e) => {
      if (!this.dragging) return;
      const dx = e.clientX - this.lastPointer.x;
      const dy = e.clientY - this.lastPointer.y;
      this.lastPointer = { x: e.clientX, y: e.clientY };
      this.yawVel = dx * 0.005;
      this.pitchVel = dy * 0.005;
      this.yaw += this.yawVel;
      this.pitch = Math.max(-1.2, Math.min(1.2, this.pitch + this.pitchVel));
      this.lastInteraction = performance.now();
    });
    el.addEventListener("pointerup", (e) => {
      this.dragging = false;
      const moved = Math.hypot(e.clientX - this.downPointer.x, e.clientY - this.downPointer.y);
      if (moved < 5) this.pick(e);
    });
    el.addEventListener("wheel", (e) => {
      e.preventDefault();
      this.distance = Math.max(1.6, Math.min(5, this.distance + e.deltaY * 0.002));
      this.lastInteraction = performance.now();
    });
  }

  private pick(e: PointerEvent) {
    const rect = this.renderer.domElement.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    let best = -1;
    let bestDist = PICK_RADIUS_PX;
    // Only real arcs have polylines; stress-mode arcs are never pickable.
    for (let i = 0; i < this.polylines.length; i++) {
      for (const p of this.polylines[i]) {
        if (!this.facesCamera(p)) continue;
        const s = this.project(p);
        if (!s) continue;
        const d = Math.hypot(s.x - px, s.y - py);
        if (d < bestDist) {
          bestDist = d;
          best = i;
        }
      }
    }
    this.opts.onArcClick(best === -1 ? null : best);
  }

  arcAt(index: number): ArcDatum | undefined {
    return this.arcData[index];
  }

  resize() {
    if (!this.renderer) return;
    const { container } = this.opts;
    const w = container.clientWidth;
    const h = container.clientHeight;
    if (w === 0 || h === 0) return;
    this.renderer.setSize(w, h, false);
    this.renderer.domElement.style.width = "100%";
    this.renderer.domElement.style.height = "100%";
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
  }

  private frame(timeMs: number) {
    const dt = this.lastFrame ? (timeMs - this.lastFrame) / 1000 : 0.016;
    this.lastFrame = timeMs;
    this.frameStats.tick(timeMs);

    // Inertia + auto-rotate (resumes a moment after the user lets go).
    if (!this.dragging) {
      this.yaw += this.yawVel;
      this.pitch = Math.max(-1.2, Math.min(1.2, this.pitch + this.pitchVel));
      this.yawVel *= 0.94;
      this.pitchVel *= 0.94;
      if (timeMs - this.lastInteraction > 2500) this.yaw += this.autoRotate * dt;
    }
    this.world.rotation.set(this.pitch, this.yaw, 0);
    this.world.updateMatrixWorld();
    this.camera.position.set(0, 0, this.distance);
    this.camera.lookAt(0, 0, 0);

    this.arcsMesh.update(dt);
    this.updateLabels();
    this.renderer.render(this.scene, this.camera);
  }

  dispose() {
    this.disposed = true;
    if (this.renderer) {
      this.renderer.setAnimationLoop(null);
      this.renderer.domElement.remove();
      this.renderer.dispose();
    }
    this.labelLayer?.remove();
    this.arcsMesh.dispose();
  }
}
