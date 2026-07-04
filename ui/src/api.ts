// All daemon API types + fetch helpers. Relative /api URLs: the daemon serves
// the SPA in production; vite proxies /api to :8480 in dev (vite.config.ts).

export type Health = { status: string; version: string };

export type Device = {
  id: number;
  display_name: string;
  identity_key: string;
  is_mac: boolean;
  mac: string | null;
  ip_hint: string | null;
  vendor: string | null;
  name_user: string | null;
  queries: number;
  blocked: number;
  tracker_queries: number;
  distinct_domains: number;
};

export type Scorecard = {
  score: number;
  components: {
    tracker_share: number;
    entity_spread: number;
    country_spread: number;
    chattiness: number;
  };
  inputs: {
    total_queries: number;
    blocked_queries: number;
    tracker_queries: number;
    distinct_tracker_entities: number;
    distinct_countries: number;
  };
};

export type ArcRow = {
  device_id: number;
  device_name: string;
  country: string;
  queries: number;
  blocked: number;
  tracker_queries: number;
  domains: number;
};

export type ArcsResponse = {
  arcs: ArcRow[];
  unmapped_queries: number;
};

export type ArcDomainRow = {
  domain: string;
  entity: string | null;
  category: string;
  is_tracker: boolean;
  queries: number;
  blocked: number;
  last_bucket_hour: number;
};

export type RollupRow = {
  bucket_hour: number;
  count: number;
  blocked_count: number;
};

export type Config = {
  home: { lat: number; lon: number } | null;
  version: string;
  // True on a fresh install with no source configured — show the setup wizard (M5).
  needs_setup: boolean;
};

// A configured source as returned by GET /api/sources — never carries the
// secret (D-014).
export type SourceSummary = {
  id: string;
  kind: string;
  base_url: string;
  username: string | null;
  interval_s: number;
  enabled: boolean;
};

export type Pulse = {
  device_id: number;
  device_name: string;
  domain: string;
  country: string | null;
  is_tracker: boolean;
  count: number;
};

export type SourceState = {
  id: string;
  kind: string;
  cursor: string | null;
  last_ok_at: number | null;
};

export type Stats = {
  total_queries: number;
  total_blocked: number;
  distinct_domains: number;
  distinct_clients: number;
  distinct_devices: number;
  rollup_rows: number;
  sources: SourceState[];
};

async function getJson<T>(url: string): Promise<T> {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return r.json() as Promise<T>;
}

export const api = {
  health: () => getJson<Health>("/api/health"),
  config: () => getJson<Config>("/api/config"),
  stats: () => getJson<Stats>("/api/stats"),
  devices: () => getJson<Device[]>("/api/devices"),
  scorecard: (id: number) => getJson<Scorecard>(`/api/devices/${id}/scorecard`),
  arcs: (windowHours?: number) =>
    getJson<ArcsResponse>(windowHours ? `/api/arcs?window=${windowHours}` : "/api/arcs"),
  arcDomains: (device: number, country: string, windowHours?: number) =>
    getJson<ArcDomainRow[]>(
      `/api/arcs/domains?device=${device}&country=${encodeURIComponent(country)}` +
        (windowHours ? `&window=${windowHours}` : ""),
    ),
  rollups: (device: number, domain: string, windowHours?: number) =>
    getJson<RollupRow[]>(
      `/api/rollups?device=${device}&domain=${encodeURIComponent(domain)}` +
        (windowHours ? `&window=${windowHours}` : ""),
    ),
  rename: (id: number, name: string) =>
    fetch("/api/devices/rename", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id, name }),
    }),
  merge: (source: number, into: number) =>
    fetch("/api/devices/merge", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ source, into }),
    }),
  sources: () => getJson<SourceSummary[]>("/api/sources"),
  // Setup wizard (M5): both return the raw Response so the caller can read the
  // status and the {ok|error} body. `body` is built by src/setup.ts.
  testSource: (body: Record<string, unknown>) =>
    fetch("/api/sources/test", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }),
  saveSource: (body: Record<string, unknown>) =>
    fetch("/api/sources", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }),
};
