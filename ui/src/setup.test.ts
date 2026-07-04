import { describe, expect, it } from "vitest";
import { buildSourceBody, validateSource, type SourceForm } from "./setup";

const base: SourceForm = {
  kind: "pihole",
  baseUrl: "http://pi.hole",
  username: "",
  secret: "token",
};

describe("validateSource", () => {
  it("accepts a complete pihole form", () => {
    expect(validateSource(base)).toEqual({ ok: true });
  });

  it("requires a non-empty address", () => {
    expect(validateSource({ ...base, baseUrl: "  " }).ok).toBe(false);
  });

  it("requires an http(s) scheme", () => {
    const v = validateSource({ ...base, baseUrl: "pi.hole" });
    expect(v.ok).toBe(false);
    expect(v.error).toMatch(/http/);
  });

  it("requires a secret", () => {
    expect(validateSource({ ...base, secret: "" }).ok).toBe(false);
  });

  it("requires a username for adguard only", () => {
    expect(validateSource({ ...base, kind: "adguard" }).ok).toBe(false);
    expect(
      validateSource({ ...base, kind: "adguard", username: "admin" }).ok,
    ).toBe(true);
  });
});

describe("buildSourceBody", () => {
  it("omits username for pihole", () => {
    const body = buildSourceBody(base);
    expect(body).toEqual({
      kind: "pihole",
      base_url: "http://pi.hole",
      secret: "token",
    });
  });

  it("includes trimmed username for adguard", () => {
    const body = buildSourceBody({
      ...base,
      kind: "adguard",
      username: " admin ",
    });
    expect(body.username).toBe("admin");
  });

  it("attaches home when both coords are valid numbers", () => {
    const body = buildSourceBody(base, { lat: "12.97", lon: "77.59" });
    expect(body.home_lat).toBe(12.97);
    expect(body.home_lon).toBe(77.59);
  });

  it("omits home when a coord is blank or out of range", () => {
    expect(buildSourceBody(base, { lat: "12.97", lon: "" }).home_lat).toBeUndefined();
    expect(buildSourceBody(base, { lat: "999", lon: "10" }).home_lat).toBeUndefined();
  });
});
