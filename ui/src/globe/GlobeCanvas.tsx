// The only React <-> three.js boundary: owns the container element, constructs
// and disposes GlobeScene, and forwards props as imperative calls. Also
// installs the window.__phonehome hook — the contract the Playwright smoke and
// perf harness drive (PR-3).

import { useEffect, useRef, useState } from "react";
import type { ArcRow, Pulse } from "../api";
import type { FrameSummary } from "./frameStats";
import { GlobeScene, NEUTRAL_HOME, type ArcDatum, type Backend } from "./GlobeScene";
import { stressArcs } from "./stress";

declare global {
  interface Window {
    __phonehome?: {
      ready: boolean;
      arcCount: number;
      backend: string;
      frameStats: () => FrameSummary | undefined;
      resetStats: () => void;
      arcScreenPoint: (i: number) => { x: number; y: number } | null;
      /** Test hook: freeze (0) or speed up rotation so click targets hold still. */
      setAutoRotate: (radPerSec: number) => void;
    };
  }
}

type Props = {
  arcs: ArcRow[];
  home: { lat: number; lon: number } | null;
  filter: Set<number> | null;
  /** >0 replaces real arcs with N synthetic ones (perf benchmark mode). */
  stress: number;
  subscribePulse: (cb: (p: Pulse) => void) => () => void;
  onArcClick: (arc: ArcDatum | null) => void;
  /** A pulse arrived for a (device, country) with no arc yet — refetch. */
  onUnknownPulse: () => void;
  onBackend?: (backend: Backend) => void;
  /** Fires with the live scene once ready (null on unmount) — hero mode uses it. */
  onScene?: (scene: GlobeScene | null) => void;
};

export default function GlobeCanvas(props: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const sceneRef = useRef<GlobeScene | null>(null);
  const propsRef = useRef(props);
  propsRef.current = props;
  const [ready, setReady] = useState(false);
  const readyRef = useRef(false);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const params = new URLSearchParams(window.location.search);
    const scene = new GlobeScene({
      container,
      forceWebGL: params.get("gl") === "1",
      onArcClick: (index) =>
        propsRef.current.onArcClick(index === null ? null : (scene.arcAt(index) ?? null)),
      onReady: (backend) => {
        readyRef.current = true;
        setReady(true);
        propsRef.current.onBackend?.(backend);
        propsRef.current.onScene?.(scene);
      },
    });
    sceneRef.current = scene;
    void scene.init();
    const observer = new ResizeObserver(() => scene.resize());
    observer.observe(container);

    window.__phonehome = {
      get ready() {
        return readyRef.current;
      },
      get arcCount() {
        return sceneRef.current?.arcCount ?? 0;
      },
      get backend() {
        return sceneRef.current?.backend ?? "none";
      },
      frameStats: () => sceneRef.current?.frameStats.summary(),
      resetStats: () => sceneRef.current?.frameStats.reset(),
      arcScreenPoint: (i: number) => sceneRef.current?.arcScreenPoint(i) ?? null,
      setAutoRotate: (radPerSec: number) => {
        if (sceneRef.current) sceneRef.current.autoRotate = radPerSec;
      },
    };

    return () => {
      observer.disconnect();
      propsRef.current.onScene?.(null);
      scene.dispose();
      sceneRef.current = null;
      readyRef.current = false;
      delete window.__phonehome;
    };
  }, []);

  const { arcs, home, stress, filter, subscribePulse, onUnknownPulse } = props;

  useEffect(() => {
    const scene = sceneRef.current;
    if (!ready || !scene) return;
    if (stress > 0) {
      scene.setStress(stressArcs(stress, home ?? NEUTRAL_HOME));
    } else {
      scene.setHome(home);
      scene.setArcs(arcs);
      scene.setVisibleDevices(propsRef.current.filter);
    }
  }, [ready, arcs, home, stress]);

  useEffect(() => {
    const scene = sceneRef.current;
    if (!ready || !scene || stress > 0) return;
    scene.setVisibleDevices(filter);
  }, [ready, filter, stress]);

  useEffect(() => {
    const scene = sceneRef.current;
    if (!ready || !scene) return;
    return subscribePulse((pulse) => {
      if (!scene.pulseByKey(pulse.device_id, pulse.country)) onUnknownPulse();
    });
  }, [ready, subscribePulse, onUnknownPulse]);

  return <div ref={containerRef} className="relative h-full w-full" data-testid="globe" />;
}
