import type { Device } from "../api";

/** Tracker-share chip color matching the globe's arc ramp. */
function chipColor(device: Device): string {
  const share = device.queries ? device.tracker_queries / device.queries : 0;
  if (share >= 0.5) return "bg-rose-400";
  if (share >= 0.2) return "bg-amber-400";
  return "bg-emerald-400";
}

export function FilterRail({
  devices,
  filter,
  onChange,
}: {
  devices: Device[];
  filter: Set<number> | null;
  onChange: (filter: Set<number> | null) => void;
}) {
  const toggle = (id: number) => {
    if (filter === null) {
      onChange(new Set([id]));
    } else {
      const next = new Set(filter);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      onChange(next.size === 0 || next.size === devices.length ? null : next);
    }
  };

  return (
    <aside
      className="flex max-h-full w-52 shrink-0 flex-col overflow-hidden rounded-xl border border-slate-800 bg-slate-900/40"
      data-testid="filter-rail"
    >
      <div className="flex items-center justify-between border-b border-slate-800 px-3 py-2">
        <span className="text-xs font-semibold uppercase tracking-wide text-slate-400">
          Devices
        </span>
        {filter !== null && (
          <button
            className="text-xs text-emerald-400 hover:text-emerald-300"
            onClick={() => onChange(null)}
          >
            show all
          </button>
        )}
      </div>
      <ul className="flex-1 overflow-y-auto py-1 text-sm">
        {devices.map((d) => {
          const active = filter === null || filter.has(d.id);
          return (
            <li key={d.id}>
              <button
                className={`flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-slate-800/60 ${
                  active ? "text-slate-200" : "text-slate-600"
                }`}
                onClick={() => toggle(d.id)}
                title={`${d.tracker_queries.toLocaleString()} tracker queries`}
              >
                <span
                  className={`h-2 w-2 shrink-0 rounded-full ${chipColor(d)} ${
                    active ? "" : "opacity-30"
                  }`}
                />
                <span className="truncate">{d.display_name}</span>
              </button>
            </li>
          );
        })}
        {devices.length === 0 && (
          <li className="px-3 py-2 text-xs text-slate-600">waiting for devices…</li>
        )}
      </ul>
    </aside>
  );
}
