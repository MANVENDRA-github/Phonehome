export function Meter({ label, value }: { label: string; value: number }) {
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
