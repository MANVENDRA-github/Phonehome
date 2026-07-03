// Regenerates src/globe/countryCentroids.ts from the world-countries-centroids
// dataset (gavinr/world-countries-centroids, MIT, Esri-derived):
//
//   curl -sL https://raw.githubusercontent.com/gavinr/world-countries-centroids/master/dist/countries.csv -o countries.csv
//   node scripts/gen-centroids.mjs countries.csv src/globe/countryCentroids.ts
//
// Columns: longitude,latitude,COUNTRY,ISO,COUNTRYAFF,AFF_ISO
import { readFileSync, writeFileSync } from "node:fs";

// The dataset lists some ISO codes once per territory; pick the canonical row.
const DUPE_PREFERENCE = {
  BQ: "Bonaire",
  ES: "Spain",
  TF: "French Southern Territories",
};

// ISO codes the dataset omits entirely; centroids added by hand.
const SUPPLEMENTS = [
  { iso: "TW", name: "Taiwan", lat: 23.75, lon: 120.95 },
];

function parseCsvLine(line) {
  const fields = [];
  let cur = "";
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (quoted) {
      if (ch === '"' && line[i + 1] === '"') {
        cur += '"';
        i++;
      } else if (ch === '"') {
        quoted = false;
      } else {
        cur += ch;
      }
    } else if (ch === '"') {
      quoted = true;
    } else if (ch === ",") {
      fields.push(cur);
      cur = "";
    } else {
      cur += ch;
    }
  }
  fields.push(cur);
  return fields;
}

const [, , csvPath, outPath] = process.argv;
const lines = readFileSync(csvPath, "utf8").trim().split(/\r?\n/).slice(1);

const byIso = new Map();
for (const line of lines) {
  const [lon, lat, country, iso, countryAff] = parseCsvLine(line);
  if (!/^[A-Z]{2}$/.test(iso)) continue;
  const row = { iso, name: country, lat: Number(lat), lon: Number(lon) };
  const existing = byIso.get(iso);
  if (!existing) {
    byIso.set(iso, row);
  } else {
    const preferred = DUPE_PREFERENCE[iso] ?? (country === countryAff ? country : null);
    if (preferred === country) byIso.set(iso, row);
  }
}

for (const s of SUPPLEMENTS) {
  if (!byIso.has(s.iso)) byIso.set(s.iso, s);
}

const rows = [...byIso.values()].sort((a, b) => a.iso.localeCompare(b.iso));
const body = rows
  .map(
    (r) =>
      `  ${r.iso}: { lat: ${r.lat.toFixed(4)}, lon: ${r.lon.toFixed(4)}, name: ${JSON.stringify(r.name)} },`,
  )
  .join("\n");

const out = `// Country centroids (ISO-3166 alpha-2 -> lat/lon/name) for globe arc endpoints.
// GENERATED - do not hand-edit. Source: gavinr/world-countries-centroids (MIT),
// https://github.com/gavinr/world-countries-centroids (Esri-derived centroids).
// Regenerate: see ui/scripts/gen-centroids.mjs

export type Centroid = { lat: number; lon: number; name: string };

export const COUNTRY_CENTROIDS: Record<string, Centroid> = {
${body}
};
`;
writeFileSync(outPath, out);
console.error(`wrote ${rows.length} centroids -> ${outPath}`);
