import { useEffect, useState } from "react";
import type { FrameSummary } from "../globe/frameStats";

/** ?hud=1 overlay: live frame stats from the render loop. The Playwright perf
 * harness reads the same numbers via window.__phonehome.frameStats(). */
export function Hud() {
  const [stats, setStats] = useState<FrameSummary | undefined>();
  const [arcs, setArcs] = useState(0);

  useEffect(() => {
    const t = setInterval(() => {
      setStats(window.__phonehome?.frameStats());
      setArcs(window.__phonehome?.arcCount ?? 0);
    }, 500);
    return () => clearInterval(t);
  }, []);

  if (!stats || stats.frames === 0) return null;
  return (
    <div className="pointer-events-none fixed right-4 top-4 z-50 rounded-md border border-slate-700 bg-slate-950/85 px-3 py-2 font-mono text-xs text-slate-300 shadow-lg">
      <div>
        {stats.fps.toFixed(0)} fps · {stats.backend} · {arcs.toLocaleString()} arcs
      </div>
      <div className="text-slate-500">
        avg {stats.avg_ms.toFixed(1)}ms · p95 {stats.p95_ms.toFixed(1)}ms · p99{" "}
        {stats.p99_ms.toFixed(1)}ms · dpr {stats.dpr}
      </div>
    </div>
  );
}
