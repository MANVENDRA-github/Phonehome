import { useRef, useState } from "react";

export function NameEditor(props: {
  initial: string;
  onCommit: (name: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(props.initial);
  // Enter/Escape resolve the editor, which unmounts this input; a blur fired
  // during that unmount must not commit again (Escape would otherwise rename).
  const resolved = useRef(false);
  const commit = () => {
    if (resolved.current) return;
    resolved.current = true;
    props.onCommit(value);
  };
  const cancel = () => {
    if (resolved.current) return;
    resolved.current = true;
    props.onCancel();
  };
  return (
    <input
      autoFocus
      className="rounded border border-emerald-500/50 bg-slate-950 px-2 py-1 text-slate-100 outline-none"
      value={value}
      placeholder="device name (blank to reset)"
      onChange={(e) => setValue(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") cancel();
      }}
      onBlur={commit}
    />
  );
}
