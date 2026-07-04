import { useEffect, useState } from "react";
import { api, type DiffsResponse, type WeekDiff } from "../api";
import { delta, riskDeltaClass, signed } from "../diff";

/** Weekly-diff view (M6): per-device week-over-week change — the "your doorbell
 * added 6 tracker domains this week" retention hook. Fetches /api/diffs; shows
 * only devices whose score moved or that picked up new domains, worst first. */
export function WeeklyDiffPanel({ refreshKey }: { refreshKey: number }) {
  const [data, setData] = useState<DiffsResponse | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    api
      .diffs()
      .then((d) => {
        setData(d);
        setFailed(false);
      })
      .catch(() => setFailed(true));
  }, [refreshKey]);

  if (failed || !data) return null;
  // No comparison yet (only one week of data) — nothing worth showing.
  if (data.previous_week_start === null) return null;

  const changed = data.devices.filter(
    (d) => d.previous && (d.current.score !== d.previous.score || d.new_domains.length > 0),
  );
  if (changed.length === 0) return null;

  return (
    <section className="flex flex-col gap-3" data-testid="weekly-diff">
      <h2 className="text-sm font-semibold text-slate-300">
        This week vs last{" "}
        <span className="text-xs font-normal text-slate-500">
          week-over-week change · rising risk is worse
        </span>
      </h2>
      <div className="grid gap-3">
        {changed.map((d) => (
          <DiffCard key={d.device_id} diff={d} />
        ))}
      </div>
    </section>
  );
}

function DiffCard({ diff }: { diff: WeekDiff }) {
  const prev = diff.previous!;
  const scoreDelta = delta(diff.current.score, prev.score);
  const band =
    diff.current.score >= 66
      ? "text-rose-400"
      : diff.current.score >= 33
        ? "text-amber-400"
        : "text-emerald-400";

  const counts: { label: string; d: number }[] = [
    { label: "domains", d: diff.current.distinct_domains - prev.distinct_domains },
    { label: "trackers", d: diff.current.tracker_domains - prev.tracker_domains },
    { label: "companies", d: diff.current.distinct_entities - prev.distinct_entities },
    { label: "countries", d: diff.current.distinct_countries - prev.distinct_countries },
  ];

  return (
    <div
      className="rounded-xl border border-slate-800 bg-slate-900/40 p-4"
      data-testid={`diff-device-${diff.device_id}`}
    >
      <div className="flex items-center justify-between gap-3">
        <span className="truncate text-sm font-medium text-slate-200">{diff.device_name}</span>
        <div className="flex items-center gap-2 tabular-nums">
          <span className={`text-2xl font-bold ${band}`}>{diff.current.score}</span>
          <span className={`text-xs font-semibold ${riskDeltaClass(scoreDelta.value)}`}>
            {scoreDelta.value === 0 ? "±0" : `${scoreDelta.value > 0 ? "▲" : "▼"} ${signed(scoreDelta.value)}`}
          </span>
        </div>
      </div>

      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-slate-500">
        {counts.map((c) => (
          <span key={c.label}>
            {c.label}{" "}
            <span
              className={
                c.d > 0 ? "text-rose-400" : c.d < 0 ? "text-emerald-400" : "text-slate-500"
              }
            >
              {signed(c.d)}
            </span>
          </span>
        ))}
      </div>

      {diff.new_domains.length > 0 && (
        <div className="mt-3" data-testid="diff-new-domains">
          <p className="mb-1 text-xs font-medium text-slate-400">
            new this week ({diff.new_domains.length})
          </p>
          <ul className="flex flex-col gap-0.5 text-xs">
            {diff.new_domains.map((n) => (
              <li key={n.domain} className="flex items-center gap-2">
                <span className={n.is_tracker ? "text-rose-400" : "text-slate-300"}>
                  {n.is_tracker ? "●" : "○"}
                </span>
                <span className="font-mono text-slate-300">{n.domain}</span>
                {n.country && <span className="text-slate-600">{n.country}</span>}
                <span className="ml-auto tabular-nums text-slate-600">
                  {n.queries.toLocaleString()}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
