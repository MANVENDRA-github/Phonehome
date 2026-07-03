import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type ArcsResponse, type Config, type Device, type Health } from "./api";
import { Devices } from "./components/Devices";
import { DrillPanel, type DrillSelection } from "./components/DrillPanel";
import { FilterRail } from "./components/FilterRail";
import { FixtureBadge } from "./components/FixtureBadge";
import { Hud } from "./components/Hud";
import { startHero } from "./globe/hero";
import type { GlobeScene } from "./globe/GlobeScene";
import { useSSE } from "./useSSE";

// The globe (three.js) loads as its own chunk so the device list paints
// instantly and non-3D pages never pay for it.
const GlobeCanvas = lazy(() => import("./globe/GlobeCanvas"));

const HOME_STORAGE_KEY = "phonehome.home";

type WindowChoice = { label: string; hours?: number };
const WINDOWS: WindowChoice[] = [
  { label: "24 h", hours: 24 },
  { label: "7 d", hours: 168 },
  { label: "all", hours: undefined },
];

function urlParams() {
  const p = new URLSearchParams(window.location.search);
  return {
    hud: p.get("hud") === "1",
    hero: p.get("hero") === "1",
    stress: Math.max(0, Number(p.get("stress") ?? 0) || 0),
  };
}

/** Home origin: localStorage override > /api/config (env) > null (hint). */
function storedHome(): { lat: number; lon: number } | null {
  try {
    const raw = localStorage.getItem(HOME_STORAGE_KEY);
    if (!raw) return null;
    const v = JSON.parse(raw) as { lat: number; lon: number };
    if (Math.abs(v.lat) <= 90 && Math.abs(v.lon) <= 180) return v;
  } catch {
    // fall through
  }
  return null;
}

export default function App() {
  const params = useMemo(urlParams, []);
  const [health, setHealth] = useState<Health | null>(null);
  const [healthError, setHealthError] = useState(false);
  const [config, setConfig] = useState<Config | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [devicesLoaded, setDevicesLoaded] = useState(false);
  const [arcs, setArcs] = useState<ArcsResponse | null>(null);
  const [windowHours, setWindowHours] = useState<number | undefined>(undefined);
  const [filter, setFilter] = useState<Set<number> | null>(null);
  const [drill, setDrill] = useState<DrillSelection | null>(null);
  const [hasFixtureSource, setHasFixtureSource] = useState(false);
  const [heroCallout, setHeroCallout] = useState<string | null>(null);
  const [homeOverride] = useState(storedHome);
  const sse = useSSE();

  const home = homeOverride ?? config?.home ?? null;

  useEffect(() => {
    api
      .health()
      .then(setHealth)
      .catch(() => setHealthError(true));
    api.config().then(setConfig).catch(() => setConfig(null));
  }, []);

  const refreshDevices = useCallback(() => {
    api
      .devices()
      .then((d) => {
        setDevices(d);
        setDevicesLoaded(true);
      })
      .catch(() => setDevicesLoaded(true));
    api
      .stats()
      .then((s) => setHasFixtureSource(s.sources.some((src) => src.kind === "fixture")))
      .catch(() => {});
  }, []);

  const refreshArcs = useCallback(() => {
    api
      .arcs(windowHours)
      .then(setArcs)
      .catch(() => {});
  }, [windowHours]);

  // Initial load + slow polling fallback. While the SSE stream is open the
  // debounced pulse-driven refresh below carries the updates instead.
  useEffect(() => {
    refreshDevices();
    refreshArcs();
    const interval = sse.status === "open" ? 15000 : 3000;
    const t = setInterval(() => {
      refreshDevices();
      refreshArcs();
    }, interval);
    return () => clearInterval(t);
  }, [refreshDevices, refreshArcs, sse.status]);

  // SSE pulses: debounce a devices+arcs refresh so bursts coalesce.
  const refreshTimer = useRef(0);
  const scheduleRefresh = useCallback(() => {
    if (refreshTimer.current) return;
    refreshTimer.current = window.setTimeout(() => {
      refreshTimer.current = 0;
      refreshDevices();
      refreshArcs();
    }, 2000);
  }, [refreshDevices, refreshArcs]);
  useEffect(() => sse.subscribe(scheduleRefresh), [sse.subscribe, scheduleRefresh]);
  useEffect(() => () => window.clearTimeout(refreshTimer.current), []);

  // Hero choreography (?hero=1): needs the live scene + device names.
  const devicesRef = useRef(devices);
  devicesRef.current = devices;
  const [scene, setScene] = useState<GlobeScene | null>(null);
  useEffect(() => {
    if (!params.hero || !scene) return;
    return startHero({
      scene,
      getDevices: () =>
        devicesRef.current.map((d) => ({ id: d.id, name: d.display_name })),
      setFilter,
      setCallout: setHeroCallout,
    });
  }, [params.hero, scene]);

  const arcRows = arcs?.arcs ?? [];

  return (
    <main className="mx-auto flex min-h-screen max-w-6xl flex-col gap-8 px-6 py-10">
      <header className="flex flex-col items-center gap-3 text-center">
        <h1 className="text-4xl font-bold tracking-tight">
          phonehome
          <span className="ml-2 inline-block h-3 w-3 animate-pulse rounded-full bg-emerald-400 align-middle" />
        </h1>
        <p className="text-slate-400">Meet everything your house talks to.</p>
        <div className="rounded-full border border-slate-700 bg-slate-900/60 px-4 py-1.5 font-mono text-xs">
          {health ? (
            <span className="text-emerald-400">
              daemon: {health.status} · v{health.version}
            </span>
          ) : healthError ? (
            <span className="text-rose-400">daemon: unreachable</span>
          ) : (
            <span className="text-slate-500">daemon: connecting…</span>
          )}
        </div>
      </header>

      <section className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold text-slate-300">
            The globe{" "}
            <span className="text-xs font-normal text-slate-500">
              device → destination-country arcs · red = trackers
            </span>
          </h2>
          <div className="flex items-center gap-1 text-xs">
            {WINDOWS.map((w) => (
              <button
                key={w.label}
                className={`rounded px-2 py-0.5 ${
                  windowHours === w.hours
                    ? "bg-slate-700 text-slate-100"
                    : "text-slate-500 hover:text-slate-300"
                }`}
                onClick={() => setWindowHours(w.hours)}
              >
                {w.label}
              </button>
            ))}
          </div>
        </div>

        <div className="flex h-[540px] gap-4">
          <FilterRail devices={devices} filter={filter} onChange={setFilter} />
          <div className="relative flex-1 overflow-hidden rounded-xl border border-slate-800 bg-slate-950">
            <Suspense
              fallback={
                <div className="flex h-full items-center justify-center text-xs text-slate-500">
                  loading globe…
                </div>
              }
            >
              <GlobeCanvas
                arcs={arcRows}
                home={home}
                filter={filter}
                stress={params.stress}
                subscribePulse={sse.subscribe}
                onArcClick={(arc) =>
                  setDrill(
                    arc
                      ? {
                          device_id: arc.device_id,
                          device_name: arc.device_name,
                          country: arc.country,
                        }
                      : null,
                  )
                }
                onUnknownPulse={scheduleRefresh}
                onScene={setScene}
              />
            </Suspense>
            {heroCallout && (
              <div className="pointer-events-none absolute inset-x-0 bottom-6 flex justify-center">
                <span className="rounded-full border border-slate-700 bg-slate-950/85 px-4 py-1.5 text-sm font-semibold text-slate-100 shadow-lg">
                  {heroCallout}
                </span>
              </div>
            )}
            {home === null && params.stress === 0 && (
              <div className="pointer-events-none absolute inset-x-0 top-3 flex justify-center">
                <span className="rounded-full border border-amber-500/40 bg-amber-950/70 px-3 py-1 text-xs text-amber-300">
                  set PHONEHOME_HOME_LAT / _LON to place your home — arcs use a neutral
                  mid-Atlantic origin until then
                </span>
              </div>
            )}
          </div>
          <DrillPanel selection={drill} windowHours={windowHours} onClose={() => setDrill(null)} />
        </div>

        {arcs !== null && arcs.unmapped_queries > 0 && (
          <p className="text-right text-xs text-slate-600" data-testid="unmapped-note">
            {arcs.unmapped_queries.toLocaleString()} queries went to destinations with no mapped
            country — counted, not drawn.
          </p>
        )}
      </section>

      <Devices devices={devices} loaded={devicesLoaded} onChanged={refreshDevices} />

      <p className="text-center text-xs text-slate-600">
        M4: the globe. Everything stays local.
      </p>

      <FixtureBadge show={hasFixtureSource} />
      {params.hud && <Hud />}
    </main>
  );
}
