import { Fragment, useState } from "react";
import { api, type Device } from "../api";
import { NameEditor } from "./NameEditor";
import { ScorecardPanel } from "./ScorecardPanel";

export function Devices({
  devices,
  loaded,
  onChanged,
}: {
  devices: Device[];
  loaded: boolean;
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState<number | null>(null);
  const [expanded, setExpanded] = useState<number | null>(null);

  const rename = async (id: number, name: string) => {
    setEditing(null);
    await api.rename(id, name);
    onChanged();
  };

  const merge = async (source: number, into: number) => {
    await api.merge(source, into);
    onChanged();
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
