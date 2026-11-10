import type { Filters } from "./bindings/Filters";
import type { HasKind } from "./bindings/HasKind";

export type TimeUnit = "hours" | "days" | "weeks" | "months" | "years";
export type WhenMode = "any" | "older" | "newer" | "between";

export interface FilterDraft {
  when: WhenMode;
  amount: number;
  unit: TimeUnit;
  from: string;
  to: string;
  contains: string;
  keep: string;
  pattern: string;
  has: HasKind[];
  includePinned: boolean;
  includeNsfw: boolean;
  afterMessage: string;
  beforeMessage: string;
}

export const emptyDraft: FilterDraft = {
  when: "any",
  amount: 30,
  unit: "days",
  from: "",
  to: "",
  contains: "",
  keep: "",
  pattern: "",
  has: [],
  includePinned: false,
  includeNsfw: false,
  afterMessage: "",
  beforeMessage: "",
};

const UNIT_MS: Record<TimeUnit, number> = {
  hours: 3_600_000,
  days: 86_400_000,
  weeks: 604_800_000,
  months: 2_592_000_000,
  years: 31_536_000_000,
};

export function unitMilliseconds(unit: TimeUnit): number {
  return UNIT_MS[unit];
}

const localDay = (text: string) => new Date(`${text}T00:00:00`).getTime();
const DAY_MS = 86_400_000;

function clean(text: string): string | null {
  const trimmed = text.trim();
  return trimmed === "" ? null : trimmed;
}

export function toFilters(draft: FilterDraft, now: number): Filters {
  let after: number | null = null;
  let before: number | null = null;
  const span = draft.amount * UNIT_MS[draft.unit];

  if (draft.when === "older") before = now - span;
  if (draft.when === "newer") after = now - span;
  if (draft.when === "between") {
    if (draft.from) after = localDay(draft.from);

    if (draft.to) before = localDay(draft.to) + DAY_MS;
  }

  return {
    after,
    before,
    afterMessage: clean(draft.afterMessage),
    beforeMessage: clean(draft.beforeMessage),
    contains: clean(draft.contains),
    keepIfContains: draft.keep
      .split(",")
      .map((word) => word.trim())
      .filter((word) => word !== ""),
    pattern: clean(draft.pattern),
    has: draft.has,
    includePinned: draft.includePinned,
    includeNsfw: draft.includeNsfw,
  };
}

export function draftProblem(draft: FilterDraft): string | null {
  if (draft.when === "older" || draft.when === "newer") {
    if (!Number.isFinite(draft.amount) || draft.amount <= 0) {
      return "Enter how long ago, as a number above zero.";
    }
  }
  if (draft.when === "between") {
    if (!draft.from && !draft.to) return "Choose at least one date.";
    if (draft.from && draft.to && draft.from > draft.to) {
      return "The start date is after the end date.";
    }
  }
  for (const id of [draft.afterMessage, draft.beforeMessage]) {
    if (id.trim() !== "" && !/^\d+$/.test(id.trim())) return "Message IDs are numbers only.";
  }
  return null;
}
