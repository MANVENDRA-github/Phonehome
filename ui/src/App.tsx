import { Fragment, useCallback, useEffect, useState } from "react";

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
  tracker_queries: number;
  distinct_domains: number;
};

type Scorecard = {
  score: number;
  components: {
    tracker_share: number;
    entity_spread: number;
    country_spread: number;
    chattiness: number;
  };
  inputs: {
    total_queries: number;
    blocked_queries: number;
    tracker_queries: number;
    distinct_tracker_entities: number;
    distinct_countries: number;
  };
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
        M3 enrichment + scorecard — the globe arrives at M4. Everything stays local.
      </p>
    </main>
  );
}

function Devices() {
  const [devices, setDevices] = useState<Device[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [editing, setEditing] = useState<number | null>(null);
  const [expanded, setExpanded] = useState<number | null>(null);

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
        <span className="text-xs text-slate-600">
          click a row for its privacy scorecard · click a name to rename
        </span>
      </div>
      <table className="w-full text-left text-sm">
        <thead className="text-xs uppercase tracking-wide text-slate-500">
          <tr>
            <th className="px-5 py-2 font-medium">Device</th>
            <th className="px-3 py-2 font-medium">Vendor</th>
            <th className="px-3 py-2 text-right font-medium">Queries</th>
            <th className="px-3 py-2 text-right font-medium">Trackers</th>
            <th className="px-3 py-2 text-right font-medium">Blocked</th>
            <th className="px-3 py-2 font-medium">Merge into</th>
          </tr>
        </thead>
        <tbody>
          {devices.map((d) => {
            const trackerPct = d.queries ? Math.round((d.tracker_queries / d.queries) * 100) : 0;
            return (
              <Fragment key={d.id}>
                <tr
                  className="cursor-pointer border-t border-slate-800/70 hover:bg-slate-900/60"
                  onClick={() => setExpanded(expanded === d.id ? null : d.id)}
                >
                  <td className="px-5 py-2.5" onClick={(e) => e.stopPropagation()}>
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
                  <td className="px-3 py-2.5 text-right tabular-nums text-slate-300">
                    {d.queries.toLocaleString()}
                  </td>
                  <td className="px-3 py-2.5 text-right tabular-nums">
                    <span className={trackerPct >= 40 ? "text-amber-400" : "text-slate-400"}>
                      {d.tracker_queries.toLocaleString()}{" "}
                      <span className="text-xs text-slate-600">({trackerPct}%)</span>
                    </span>
                  </td>
                  <td className="px-3 py-2.5 text-right tabular-nums text-rose-400">
                    {d.blocked.toLocaleString()}
                  </td>
                  <td className="px-3 py-2.5" onClick={(e) => e.stopPropagation()}>
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
                {expanded === d.id && (
                  <tr className="border-t border-slate-800/70 bg-slate-950/60">
                    <td colSpan={6} className="px-5 py-4">
                      <ScorecardPanel id={d.id} />
                    </td>
                  </tr>
                )}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}

function ScorecardPanel({ id }: { id: number }) {
  const [card, setCard] = useState<Scorecard | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    fetch(`/api/devices/${id}/scorecard`)
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`${r.status}`))))
      .then((c: Scorecard) => setCard(c))
      .catch(() => setFailed(true));
  }, [id]);

  if (failed) return <span className="text-xs text-rose-400">scorecard unavailable</span>;
  if (!card) return <span className="text-xs text-slate-500">computing scorecard…</span>;

  const band =
    card.score >= 66 ? "text-rose-400" : card.score >= 33 ? "text-amber-400" : "text-emerald-400";

  return (
    <div className="flex flex-col gap-4 sm:flex-row sm:items-center">
      <div className="flex flex-col items-center justify-center rounded-lg border border-slate-800 bg-slate-900/60 px-6 py-3">
        <span className={`text-4xl font-bold tabular-nums ${band}`}>{card.score}</span>
        <span className="text-[10px] uppercase tracking-wide text-slate-500">privacy risk</span>
      </div>
      <div className="flex-1">
        <div className="grid gap-2">
          <Meter label="Tracker share" value={card.components.tracker_share} />
          <Meter label="Tracker companies" value={card.components.entity_spread} />
          <Meter label="Country spread" value={card.components.country_spread} />
          <Meter label="Chattiness" value={card.components.chattiness} />
        </div>
        <p className="mt-3 text-xs text-slate-500">
          {card.inputs.tracker_queries.toLocaleString()} of{" "}
          {card.inputs.total_queries.toLocaleString()} queries to trackers ·{" "}
          {card.inputs.distinct_tracker_entities} tracker{" "}
          {card.inputs.distinct_tracker_entities === 1 ? "company" : "companies"} ·{" "}
          {card.inputs.distinct_countries}{" "}
          {card.inputs.distinct_countries === 1 ? "country" : "countries"} ·{" "}
          {card.inputs.blocked_queries.toLocaleString()} blocked
        </p>
      </div>
    </div>
  );
}

function Meter({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex items-center gap-3 text-xs">
      <span className="w-36 shrink-0 text-slate-400">{label}</span>
      <div className="h-2 flex-1 overflow-hidden rounded-full bg-slate-800">
        <div
          className="h-full rounded-full bg-gradient-to-r from-emerald-500 via-amber-500 to-rose-500"
          style={{ width: `${value}%` }}
        />
      </div>
      <span className="w-9 shrink-0 text-right tabular-nums text-slate-400">{value}</span>
    </div>
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
