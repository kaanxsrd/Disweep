import { describe, expect, it } from "vitest";
import { ago, count, duration, interval, percent, tick } from "./format";

describe("count", () => {
  it("groups digits", () => {
    expect(count(1234567)).toBe("1,234,567");
  });
});

describe("tick", () => {
  it("stays full below ten thousand and compacts above", () => {
    expect(tick(9000)).toBe("9,000");
    expect(tick(20000)).toBe("20K");
    expect(tick(2500000)).toBe("2.5M");
    expect(tick(3000000)).toBe("3M");
  });
});

describe("duration", () => {
  it.each([
    [30_000, "under a minute"],
    [90_000, "2 min"],
    [3_600_000, "1 h"],
    [5_400_000, "1 h 30 min"],
    [86_400_000, "1 d"],
    [90_000_000, "1 d 1 h"],
  ])("formats %d", (input, expected) => {
    expect(duration(input)).toBe(expected);
  });
});

describe("interval", () => {
  it("uses the largest exact unit", () => {
    expect(interval(2_592_000)).toBe("1 month");
    expect(interval(86_400 * 3)).toBe("3 days");
    expect(interval(3600)).toBe("1 hour");
    expect(interval(90)).toBe("2 minutes");
  });
});

describe("ago", () => {
  const now = 1_700_000_000_000;

  it("describes recent and older moments", () => {
    expect(ago(now - 5_000, now)).toBe("just now");
    expect(ago(now - 180_000, now)).toBe("3 min ago");
    expect(ago(now - 7_200_000, now)).toBe("2 h ago");
    expect(ago(now - 86_400_000, now)).toBe("1 day ago");
  });
});

describe("percent", () => {
  it("does not show 100 until finished", () => {
    expect(percent(999, 1000)).toBe(99);
    expect(percent(1000, 1000)).toBe(100);
  });

  it("copes with an unknown total", () => {
    expect(percent(5, 0)).toBe(0);
  });
});
