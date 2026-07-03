import { describe, expect, it } from "vitest";
import { COUNTRY_CENTROIDS } from "./countryCentroids";

// Every country the entity map can emit must have a centroid, or its arcs
// silently vanish from the globe. Keep in sync with core/data/entities.toml.
const ENTITY_MAP_COUNTRIES = [
  "US", "KR", "JP", "CN", "SE", // pre-M4
  "FR", "SG", "RU", "IN", "DE", "NL", "TW", "AU", "CH", "GB", // M4 widening
];

describe("COUNTRY_CENTROIDS", () => {
  it("covers every entities.toml country", () => {
    for (const iso of ENTITY_MAP_COUNTRIES) {
      expect(COUNTRY_CENTROIDS[iso], `missing centroid for ${iso}`).toBeDefined();
    }
  });

  it("has valid coordinate ranges and names everywhere", () => {
    const entries = Object.entries(COUNTRY_CENTROIDS);
    expect(entries.length).toBeGreaterThan(200);
    for (const [iso, c] of entries) {
      expect(iso).toMatch(/^[A-Z]{2}$/);
      expect(Math.abs(c.lat), `${iso} lat`).toBeLessThanOrEqual(90);
      expect(Math.abs(c.lon), `${iso} lon`).toBeLessThanOrEqual(180);
      expect(c.name.length, `${iso} name`).toBeGreaterThan(0);
    }
  });

  it("sanity: a few well-known centroids land in the right hemisphere", () => {
    expect(COUNTRY_CENTROIDS.US.lon).toBeLessThan(0);
    expect(COUNTRY_CENTROIDS.US.lat).toBeGreaterThan(0);
    expect(COUNTRY_CENTROIDS.AU.lat).toBeLessThan(0);
    expect(COUNTRY_CENTROIDS.IN.lon).toBeGreaterThan(60);
    expect(COUNTRY_CENTROIDS.ES.name).toBe("Spain");
  });
});
