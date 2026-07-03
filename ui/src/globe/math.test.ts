import { describe, expect, it } from "vitest";
import { arcAltitude, arcPolyline, dot, latLonToVec3, slerp } from "./math";

describe("latLonToVec3", () => {
  it("puts the north pole at +Y", () => {
    const [x, y, z] = latLonToVec3(90, 0);
    expect(x).toBeCloseTo(0, 6);
    expect(y).toBeCloseTo(1, 6);
    expect(z).toBeCloseTo(0, 6);
  });

  it("keeps the equator in the XZ plane and lon 90E on +X", () => {
    const [x, y, z] = latLonToVec3(0, 90);
    expect(x).toBeCloseTo(1, 6);
    expect(y).toBeCloseTo(0, 6);
    expect(z).toBeCloseTo(0, 6);
  });

  it("puts lon 0 on +Z (texture seam convention)", () => {
    const [x, y, z] = latLonToVec3(0, 0);
    expect(x).toBeCloseTo(0, 6);
    expect(y).toBeCloseTo(0, 6);
    expect(z).toBeCloseTo(1, 6);
  });

  it("produces unit vectors everywhere", () => {
    for (const [lat, lon] of [
      [12.97, 77.59],
      [-33.9, 151.2],
      [68.4, -133.5],
      [-90, 180],
    ]) {
      const v = latLonToVec3(lat, lon);
      expect(Math.hypot(...v)).toBeCloseTo(1, 6);
    }
  });

  it("scales by the radius argument", () => {
    const v = latLonToVec3(45, 45, 2.5);
    expect(Math.hypot(...v)).toBeCloseTo(2.5, 6);
  });
});

describe("slerp", () => {
  const a = latLonToVec3(0, 0);
  const b = latLonToVec3(0, 90);

  it("hits both endpoints exactly", () => {
    expect(slerp(a, b, 0)).toEqual(a);
    for (const [va, vb] of [
      [slerp(a, b, 1)[0], b[0]],
      [slerp(a, b, 1)[1], b[1]],
      [slerp(a, b, 1)[2], b[2]],
    ]) {
      expect(va).toBeCloseTo(vb, 6);
    }
  });

  it("stays on the unit sphere at the midpoint", () => {
    const mid = slerp(a, b, 0.5);
    expect(Math.hypot(...mid)).toBeCloseTo(1, 6);
    // Midpoint of a 90° arc is 45° from both ends.
    expect(dot(mid, a)).toBeCloseTo(Math.cos(Math.PI / 4), 6);
    expect(dot(mid, b)).toBeCloseTo(Math.cos(Math.PI / 4), 6);
  });

  it("handles near-parallel endpoints without NaN", () => {
    const c = latLonToVec3(10, 20);
    const d = latLonToVec3(10.000001, 20.000001);
    const mid = slerp(c, d, 0.5);
    expect(mid.every(Number.isFinite)).toBe(true);
    expect(Math.hypot(...mid)).toBeCloseTo(1, 5);
  });
});

describe("arcAltitude / arcPolyline", () => {
  it("is 1 (surface) at both endpoints and lifted at the middle", () => {
    expect(arcAltitude(0, 1, 0.05, 0.16)).toBeCloseTo(1, 9);
    expect(arcAltitude(1, 1, 0.05, 0.16)).toBeCloseTo(1, 9);
    expect(arcAltitude(0.5, 1, 0.05, 0.16)).toBeGreaterThan(1.1);
  });

  it("lifts longer arcs higher", () => {
    const short = arcAltitude(0.5, 0.2, 0.05, 0.16);
    const long = arcAltitude(0.5, 2.5, 0.05, 0.16);
    expect(long).toBeGreaterThan(short);
  });

  it("polyline endpoints sit on the sphere surface", () => {
    const line = arcPolyline(latLonToVec3(12.97, 77.59), latLonToVec3(38.8, -96.3), 9, 1, 0.05, 0.16);
    expect(line).toHaveLength(9);
    expect(Math.hypot(...line[0])).toBeCloseTo(1, 6);
    expect(Math.hypot(...line[8])).toBeCloseTo(1, 6);
    const midRadius = Math.hypot(...line[4]);
    expect(midRadius).toBeGreaterThan(1.05);
  });
});
