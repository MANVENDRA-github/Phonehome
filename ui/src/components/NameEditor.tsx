import { useState } from "react";

export function NameEditor(props: {
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
