import { useEffect, useState } from "react";

type Health = { status: string; version: string };

export default function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    fetch("/api/health")
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`${r.status}`))))
      .then((h: Health) => setHealth(h))
      .catch(() => setError(true));
  }, []);

  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-6 px-6">
      <h1 className="text-5xl font-bold tracking-tight">
        phonehome
        <span className="ml-2 inline-block h-3 w-3 animate-pulse rounded-full bg-emerald-400 align-middle" />
      </h1>
      <p className="max-w-md text-center text-slate-400">
        Meet everything your house talks to.
      </p>
      <div className="rounded-full border border-slate-700 bg-slate-900/60 px-4 py-1.5 font-mono text-sm">
        {health ? (
          <span className="text-emerald-400">
            daemon: {health.status} · v{health.version}
          </span>
        ) : error ? (
          <span className="text-rose-400">daemon: unreachable</span>
        ) : (
          <span className="text-slate-500">daemon: connecting…</span>
        )}
      </div>
      <p className="text-xs text-slate-600">
        M0 scaffold — the globe arrives at M4. Everything stays local.
      </p>
    </main>
  );
}
