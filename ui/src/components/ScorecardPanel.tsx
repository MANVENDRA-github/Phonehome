import { useEffect, useState } from "react";
import { api, type Scorecard } from "../api";
import { Meter } from "./Meter";

export function ScorecardPanel({ id }: { id: number }) {
  const [card, setCard] = useState<Scorecard | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    api
      .scorecard(id)
      .then(setCard)
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
