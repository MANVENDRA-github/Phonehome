import { useCallback, useEffect, useState } from "react";

type Health = { status: string; version: string };

type Device = {
  id: number;
  display_name: string;
  identity_key: string;
  is_mac: boolean;
  mac: string | null;
  ip_hint: string | null;
  vendor: string | null;
  name_user: string | null;
  queries: number;
  blocked: number;
  distinct_domains: number;
};

export default function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [healthError, setHealthError] = useState(false);

  useEffect(() => {
    fetch("/api/health")
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`${r.status}`))))
      .then((h: Health) => setHealth(h))
      .catch(() => setHealthError(true));
  }, []);

  return (
    <main className="mx-auto flex min-h-screen max-w-5xl flex-col gap-8 px-6 py-10">
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

      <Devices />

      <p className="text-center text-xs text-slate-600">
        M2 device identity — the globe arrives at M4. Everything stays local.
      </p>
    </main>
  );
}

function Devices() {
  const [devices, setDevices] = useState<Device[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [editing, setEditing] = useState<number | null>(null);

  const refresh = useCallback(() => {
    fetch("/api/devices")
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`${r.status}`))))
      .then((d: Device[]) => {
        setDevices(d);
        setLoaded(true);
      })
      .catch(() => setLoaded(true));
  }, []);

  // Poll while data is still arriving (fixture replay / live ingestion).
  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  }, [refresh]);

  const rename = async (id: number, name: string) => {
    setEditing(null);
    await fetch("/api/devices/rename", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id, name }),
    });
    refresh();
  };

  const merge = async (source: number, into: number) => {
    await fetch("/api/devices/merge", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ source, into }),
    });
    refresh();
  };

  if (loaded && devices.length === 0) {
    return (
      <section className="rounded-xl border border-slate-800 bg-slate-900/40 p-8 text-center text-slate-400">
        No devices yet. Point Phonehome at a Pi-hole/AdGuard source, or replay the
        dev fixture, and your household appears here.
      </section>
    );
  }

  return (
    <section className="overflow-hidden rounded-xl border border-slate-800 bg-slate-900/40">
      <div className="flex items-center justify-between border-b border-slate-800 px-5 py-3">
        <h2 className="text-sm font-semibold text-slate-300">
          Devices <span className="text-slate-500">({devices.length})</span>
        </h2>
        <span className="text-xs text-slate-600">click a name to rename</span>
      </div>
      <table className="w-full text-left text-sm">
        <thead className="text-xs uppercase tracking-wide text-slate-500">
          <tr>
            <th className="px-5 py-2 font-medium">Device</th>
            <th className="px-3 py-2 font-medium">Vendor</th>
            <th className="px-3 py-2 font-medium">MAC / IP</th>
            <th className="px-3 py-2 text-right font-medium">Queries</th>
            <th className="px-3 py-2 text-right font-medium">Blocked</th>
            <th className="px-3 py-2 text-right font-medium">Domains</th>
            <th className="px-3 py-2 font-medium">Merge into</th>
          </tr>
        </thead>
        <tbody>
          {devices.map((d) => (
            <tr key={d.id} className="border-t border-slate-800/70 hover:bg-slate-900/60">
              <td className="px-5 py-2.5">
                {editing === d.id ? (
                  <NameEditor
                    initial={d.name_user ?? ""}
                    onCommit={(name) => rename(d.id, name)}
                    onCancel={() => setEditing(null)}
                  />
                ) : (
                  <button
                    className="font-medium text-slate-100 hover:text-emerald-400"
                    onClick={() => setEditing(d.id)}
                    title="Rename"
                  >
                    {d.display_name}
                  </button>
                )}
              </td>
              <td className="px-3 py-2.5 text-slate-400">{d.vendor ?? "—"}</td>
              <td className="px-3 py-2.5 font-mono text-xs text-slate-500">
                {d.mac ?? d.ip_hint ?? d.identity_key}
              </td>
              <td className="px-3 py-2.5 text-right tabular-nums text-slate-300">
                {d.queries.toLocaleString()}
              </td>
              <td className="px-3 py-2.5 text-right tabular-nums text-rose-400">
                {d.blocked.toLocaleString()}
              </td>
              <td className="px-3 py-2.5 text-right tabular-nums text-slate-400">
                {d.distinct_domains}
              </td>
              <td className="px-3 py-2.5">
                <select
                  className="rounded border border-slate-700 bg-slate-900 px-2 py-1 text-xs text-slate-300"
                  value=""
                  onChange={(e) => {
                    const into = Number(e.target.value);
                    if (into) merge(d.id, into);
                  }}
                >
                  <option value="">merge into…</option>
                  {devices
                    .filter((o) => o.id !== d.id)
                    .map((o) => (
                      <option key={o.id} value={o.id}>
                        {o.display_name}
                      </option>
                    ))}
                </select>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function NameEditor(props: {
  initial: string;
  onCommit: (name: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(props.initial);
  return (
    <input
      autoFocus
      className="rounded border border-emerald-500/50 bg-slate-950 px-2 py-1 text-slate-100 outline-none"
      value={value}
      placeholder="device name (blank to reset)"
      onChange={(e) => setValue(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") props.onCommit(value);
        if (e.key === "Escape") props.onCancel();
      }}
      onBlur={() => props.onCommit(value)}
    />
  );
}
