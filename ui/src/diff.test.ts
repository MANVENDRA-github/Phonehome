import { describe, expect, it } from "vitest";
import { delta, riskDeltaClass, signed } from "./diff";

describe("delta", () => {
  it("computes a rising delta", () => {
    expect(delta(10, 6)).toEqual({ value: 4, direction: "up", isNew: false });
  });
  it("computes a falling delta", () => {
    expect(delta(3, 8)).toEqual({ value: -5, direction: "down", isNew: false });
  });
  it("flags flat", () => {
    expect(delta(5, 5).direction).toBe("flat");
  });
  it("marks a device with no previous week as new", () => {
    expect(delta(7, null)).toEqual({ value: 7, direction: "flat", isNew: true });
  });
});

describe("riskDeltaClass", () => {
  it("rising risk is rose (worse)", () => {
    expect(riskDeltaClass(5)).toContain("rose");
  });
  it("falling risk is emerald (better)", () => {
    expect(riskDeltaClass(-5)).toContain("emerald");
  });
  it("no change is slate", () => {
    expect(riskDeltaClass(0)).toContain("slate");
  });
});

describe("signed", () => {
  it("formats positive, negative, zero", () => {
    expect(signed(3)).toBe("+3");
    expect(signed(-2)).toBe("−2");
    expect(signed(0)).toBe("0");
  });
});
