import { describe, expect, it } from "vitest";
import { draftProblem, emptyDraft, toFilters } from "./filters";

const now = 1_700_000_000_000;
const DAY = 86_400_000;

describe("toFilters", () => {
  it("leaves everything open by default", () => {
    const filters = toFilters(emptyDraft, now);
    expect(filters.after).toBeNull();
    expect(filters.before).toBeNull();
    expect(filters.contains).toBeNull();
    expect(filters.includePinned).toBe(false);
  });

  it("older than sets only an upper bound", () => {
    const filters = toFilters({ ...emptyDraft, when: "older", amount: 30, unit: "days" }, now);
    expect(filters.before).toBe(now - 30 * DAY);
    expect(filters.after).toBeNull();
  });

  it("newer than sets only a lower bound", () => {
    const filters = toFilters({ ...emptyDraft, when: "newer", amount: 2, unit: "weeks" }, now);
    expect(filters.after).toBe(now - 14 * DAY);
    expect(filters.before).toBeNull();
  });

  it("includes the whole of the end date in a range", () => {
    const filters = toFilters(
      { ...emptyDraft, when: "between", from: "2024-03-01", to: "2024-03-02" },
      now,
    );
    expect(filters.after).toBe(new Date("2024-03-01T00:00:00").getTime());
    expect(filters.before).toBe(new Date("2024-03-03T00:00:00").getTime());
  });

  it("ignores amounts when the mode doesn't use them", () => {
    const filters = toFilters({ ...emptyDraft, when: "any", amount: 5 }, now);
    expect(filters.before).toBeNull();
  });

  it("trims text and splits the keep list", () => {
    const filters = toFilters(
      { ...emptyDraft, contains: "  secret ", keep: " invoice, , receipt " },
      now,
    );
    expect(filters.contains).toBe("secret");
    expect(filters.keepIfContains).toEqual(["invoice", "receipt"]);
  });

  it("treats blank text as no filter", () => {
    const filters = toFilters({ ...emptyDraft, contains: "   ", pattern: " " }, now);
    expect(filters.contains).toBeNull();
    expect(filters.pattern).toBeNull();
  });
});

describe("draftProblem", () => {
  it("accepts the default form", () => {
    expect(draftProblem(emptyDraft)).toBeNull();
  });

  it("needs a positive amount", () => {
    expect(draftProblem({ ...emptyDraft, when: "older", amount: 0 })).toMatch(/above zero/);
    expect(draftProblem({ ...emptyDraft, when: "newer", amount: Number.NaN })).toMatch(
      /above zero/,
    );
  });

  it("needs sensible dates", () => {
    expect(draftProblem({ ...emptyDraft, when: "between" })).toMatch(/at least one/);
    expect(
      draftProblem({ ...emptyDraft, when: "between", from: "2024-05-01", to: "2024-04-01" }),
    ).toMatch(/after the end/);
  });

  it("needs numeric message IDs", () => {
    expect(draftProblem({ ...emptyDraft, afterMessage: "abc" })).toMatch(/numbers only/);
    expect(draftProblem({ ...emptyDraft, afterMessage: " 123 " })).toBeNull();
  });
});
