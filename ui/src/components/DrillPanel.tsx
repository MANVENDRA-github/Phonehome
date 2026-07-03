import { useEffect, useState } from "react";
import { api, type ArcDomainRow, type RollupRow } from "../api";
import { COUNTRY_CENTROIDS } from "../globe/countryCentroids";

export type DrillSelection = {
  device_id: number;
  device_name: string;
  country: string;
};

const CATEGORY_COLORS: Record<string, string> = {
  advertising: "text-rose-400",
  analytics: "text-rose-300",
  telemetry: "text-amber-400",
  cdn: "text-slate-400",
  functional: "text-slate-300",
  first_party: "text-emerald-400",
  unknown: "text-slate-500",
};

/** Arc click-through: level 1 = domains behind the arc, level 2 = the raw
 * hourly rollup buckets for one domain (2 clicks to raw data, SPEC M4). */
export function DrillPanel({
  selection,
  windowHours,
  onClose,
}: {
  selection: DrillSelection | null;
  windowHours?: number;
  onClose: () => void;
}) {
  const [domains, setDomains] = useState<ArcDomainRow[] | null>(null);
  const [domain, setDomain] = useState<string | null>(null);
  const [rollups, setRollups] = useState<RollupRow[] | null>(null);

  useEffect(() => {
    setDomains(null);
    setDomain(null);
    setRollups(null);
    if (!selection) return;
    api
      .arcDomains(selection.device_id, selection.country, windowHours)
      .then(setDomains)
      .catch(() => setDomains([]));
  }, [selection, windowHours]);

  useEffect(() => {
    setRollups(null);
    if (!selection || !domain) return;
    api
      .rollups(selection.device_id, domain, windowHours)
      .then(setRollups)
      .catch(() => setRollups([]));
  }, [selection, domain, windowHours]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (domain) setDomain(null);
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [domain, onClose]);

  if (!selection) {
    return (
      <aside className="flex w-72 shrink-0 items-center justify-center rounded-xl border border-slate-800 bg-slate-900/40 p-4 text-center text-xs text-slate-600">
        click an arc to see the domains behind it
      </aside>
    );
  }

  const countryName = COUNTRY_CENTROIDS[selection.country]?.name ?? selection.country;

  return (
    <aside
      className="flex max-h-full w-72 shrink-0 flex-col overflow-hidden rounded-xl border border-slate-800 bg-slate-900/40"
      data-testid="drill-panel"
    >
      <div className="border-b border-slate-800 px-3 py-2">
        <div className="flex items-center justify-between">
          <nav className="min-w-0 text-xs text-slate-400">
            <span className="text-slate-200">{selection.device_name}</span>
            <span className="mx-1 text-slate-600">→</span>
            <button
              className={domain ? "text-emerald-400 hover:text-emerald-300" : "text-slate-200"}
              onClick={() => setDomain(null)}
            >
              {countryName}
            </button>
            {domain && (
              <>
                <span className="mx-1 text-slate-600">→</span>
                <span className="break-all text-slate-200">{domain}</span>
              </>
            )}
          </nav>
          <button
            className="ml-2 shrink-0 text-slate-500 hover:text-slate-300"
            onClick={onClose}
            title="Close (Esc)"
          >
            ✕
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {!domain ? (
          domains === null ? (
            <p className="p-3 text-xs text-slate-500">loading domains…</p>
          ) : domains.length === 0 ? (
            <p className="p-3 text-xs text-slate-500">no domains in this window</p>
          ) : (
            <ul className="divide-y divide-slate-800/60 text-xs" data-testid="drill-domains">
              {domains.map((d) => (
                <li key={d.domain}>
                  <button
                    className="w-full px-3 py-2 text-left hover:bg-slate-800/60"
                    onClick={() => setDomain(d.domain)}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="break-all font-medium text-slate-200">{d.domain}</span>
                      <span className="shrink-0 tabular-nums text-slate-400">
                        {d.queries.toLocaleString()}
                      </span>
                    </div>
                    <div className="mt-0.5 flex items-center gap-2 text-[11px]">
                      <span className={CATEGORY_COLORS[d.category] ?? "text-slate-500"}>
                        {d.category}
                      </span>
                      {d.is_tracker && (
                        <span className="rounded bg-rose-500/15 px-1 text-rose-400">tracker</span>
                      )}
                      <span className="text-slate-500">{d.entity ?? "unknown entity"}</span>
                      {d.blocked > 0 && (
                        <span className="ml-auto text-rose-400">{d.blocked} blocked</span>
                      )}
                    </div>
                  </button>
                </li>
              ))}
            </ul>
          )
        ) : rollups === null ? (
          <p className="p-3 text-xs text-slate-500">loading rollups…</p>
        ) : (
          <table className="w-full text-left text-xs tabular-nums" data-testid="drill-rollups">
            <thead className="text-[10px] uppercase tracking-wide text-slate-500">
              <tr>
                <th className="px-3 py-1.5 font-medium">Hour (UTC)</th>
                <th className="px-2 py-1.5 text-right font-medium">Queries</th>
                <th className="px-3 py-1.5 text-right font-medium">Blocked</th>
              </tr>
            </thead>
            <tbody>
              {rollups.map((r) => (
                <tr key={r.bucket_hour} className="border-t border-slate-800/60">
                  <td className="px-3 py-1 text-slate-300">
                    {new Date(r.bucket_hour * 3_600_000).toISOString().slice(0, 13)}h
                  </td>
                  <td className="px-2 py-1 text-right text-slate-300">{r.count}</td>
                  <td className="px-3 py-1 text-right text-rose-400">{r.blocked_count}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      <p className="border-t border-slate-800 px-3 py-1.5 text-[10px] text-slate-600">
        {domain
          ? "raw hourly rollups — the rawest data phonehome keeps (D-005)"
          : "click a domain for its raw hourly rollups"}
      </p>
    </aside>
  );
}
