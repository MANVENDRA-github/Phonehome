// Pure globe math — no three.js imports so it stays trivially unit-testable.
//
// Coordinate convention (matches an equirectangular texture on a default
// three.js SphereGeometry): +Y = north pole, lon 0 on the +Z…+X side:
//   phi   = (90 - lat)°   (polar angle from +Y)
//   theta = (90 - lon)°   (azimuth)
//   x = sin(phi)·cos(theta) · r
//   y = cos(phi)           · r
//   z = sin(phi)·sin(theta) · r

export type Vec3 = [number, number, number];

const DEG2RAD = Math.PI / 180;

export function latLonToVec3(lat: number, lon: number, r = 1): Vec3 {
  const phi = (90 - lat) * DEG2RAD;
  const theta = (90 - lon) * DEG2RAD;
  const sinPhi = Math.sin(phi);
  return [r * sinPhi * Math.cos(theta), r * Math.cos(phi), r * sinPhi * Math.sin(theta)];
}

export function dot(a: Vec3, b: Vec3): number {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

export function normalize(v: Vec3): Vec3 {
  const len = Math.hypot(v[0], v[1], v[2]) || 1;
  return [v[0] / len, v[1] / len, v[2] / len];
}

/// Spherical interpolation between two unit vectors (the great-circle path
/// arcs follow). Falls back to normalized lerp for near-parallel endpoints.
export function slerp(a: Vec3, b: Vec3, t: number): Vec3 {
  const cosOmega = Math.min(1, Math.max(-1, dot(a, b)));
  const omega = Math.acos(cosOmega);
  if (omega < 1e-4) {
    return normalize([
      a[0] + (b[0] - a[0]) * t,
      a[1] + (b[1] - a[1]) * t,
      a[2] + (b[2] - a[2]) * t,
    ]);
  }
  const sinOmega = Math.sin(omega);
  const wa = Math.sin((1 - t) * omega) / sinOmega;
  const wb = Math.sin(t * omega) / sinOmega;
  return [a[0] * wa + b[0] * wb, a[1] * wa + b[1] * wb, a[2] * wa + b[2] * wb];
}

/// The altitude profile the arc shader uses: lifts the great-circle point by
/// sin(π·t), scaled by base height plus a share of the arc's angular span so
/// long arcs fly higher. Mirrored in ArcsMesh's positionNode — keep in sync.
export function arcAltitude(t: number, omega: number, h0: number, h1: number): number {
  return 1 + (h0 + h1 * omega) * Math.sin(Math.PI * t);
}

/// CPU polyline along one arc (for click picking / tests): `samples` points
/// from start to end, on a sphere of radius r, following the shader's
/// altitude profile.
export function arcPolyline(
  start: Vec3,
  end: Vec3,
  samples: number,
  r: number,
  h0: number,
  h1: number,
): Vec3[] {
  const cosOmega = Math.min(1, Math.max(-1, dot(start, end)));
  const omega = Math.acos(cosOmega);
  const points: Vec3[] = [];
  for (let i = 0; i < samples; i++) {
    const t = i / (samples - 1);
    const p = slerp(start, end, t);
    const radius = r * arcAltitude(t, omega, h0, h1);
    points.push([p[0] * radius, p[1] * radius, p[2] * radius]);
  }
  return points;
}
