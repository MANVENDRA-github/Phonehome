// Pure form logic for the first-run setup wizard (M5). Kept out of the React
// component so it is unit-testable under vitest (src/**/*.test.ts), matching the
// repo's "test the pure logic" convention.

export type SourceKind = "pihole" | "adguard";

export type SourceForm = {
  kind: SourceKind;
  baseUrl: string;
  username: string;
  secret: string;
};

export type Validation = { ok: boolean; error?: string };

/** Client-side gate before the "Test connection" button is enabled. The daemon
 * re-validates for real (probe) — this only catches obvious mistakes early. */
export function validateSource(f: SourceForm): Validation {
  const url = f.baseUrl.trim();
  if (!url) return { ok: false, error: "enter your Pi-hole / AdGuard address" };
  if (!/^https?:\/\//i.test(url)) {
    return { ok: false, error: "address must start with http:// or https://" };
  }
  if (!f.secret.trim()) {
    return { ok: false, error: "enter the app password / API token" };
  }
  if (f.kind === "adguard" && !f.username.trim()) {
    return { ok: false, error: "AdGuard also needs a username" };
  }
  return { ok: true };
}

export type HomeInput = { lat: string; lon: string };

/** Builds the JSON body for POST /api/sources[/test]. Only includes `username`
 * for AdGuard, and only includes home coords when both parse to an in-range
 * number (an empty or half-filled home is silently omitted, never an error). */
export function buildSourceBody(
  f: SourceForm,
  home?: HomeInput,
): Record<string, unknown> {
  const body: Record<string, unknown> = {
    kind: f.kind,
    base_url: f.baseUrl.trim(),
    secret: f.secret,
  };
  if (f.kind === "adguard") body.username = f.username.trim();
  if (home && home.lat.trim() && home.lon.trim()) {
    const lat = Number(home.lat);
    const lon = Number(home.lon);
    if (
      Number.isFinite(lat) &&
      Number.isFinite(lon) &&
      Math.abs(lat) <= 90 &&
      Math.abs(lon) <= 180
    ) {
      body.home_lat = lat;
      body.home_lon = lon;
    }
  }
  return body;
}
