// Pure helpers for the weekly-diff view (M6). Kept out of the component so they
// are vitest-testable (src/**/*.test.ts).

export type Direction = "up" | "down" | "flat";

export type Delta = {
  value: number;
  direction: Direction;
  /** No previous week to compare against (first week for this device). */
  isNew: boolean;
};

export function delta(cur: number, prev: number | null): Delta {
  if (prev === null) return { value: cur, direction: "flat", isNew: true };
  const value = cur - prev;
  const direction = value > 0 ? "up" : value < 0 ? "down" : "flat";
  return { value, direction, isNew: false };
}

/** The score is a privacy RISK, so a rising score is BAD (rose) and a falling
 * score is GOOD (emerald) — the inverse of a "growth is good" metric. */
export function riskDeltaClass(scoreDelta: number): string {
  if (scoreDelta > 0) return "text-rose-400";
  if (scoreDelta < 0) return "text-emerald-400";
  return "text-slate-500";
}

/** "+3" / "−2" / "0", for a signed count delta. */
export function signed(value: number): string {
  if (value > 0) return `+${value}`;
  if (value < 0) return `−${Math.abs(value)}`;
  return "0";
}
