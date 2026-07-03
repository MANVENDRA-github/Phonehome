/** Always-visible provenance label whenever a fixture source is configured
 * (D-009): any screen recording or GIF made of the app inherently carries it —
 * the honesty rule is enforced structurally, not by remembering to edit media. */
export function FixtureBadge({ show }: { show: boolean }) {
  if (!show) return null;
  return (
    <div
      className="pointer-events-none fixed bottom-4 left-4 z-50 rounded-md border border-amber-500/40 bg-amber-950/80 px-2.5 py-1 font-mono text-xs text-amber-300 shadow-lg"
      data-testid="fixture-badge"
    >
      ● replayed fixture — synthetic data
    </div>
  );
}
