import { describe, expect, it } from "vitest";
import {
  check,
  nextVersion,
  parseVersion,
  ReleaseError,
  releaseNotes,
  replaceVersion,
  rollChangelog,
  tagVersion,
} from "./release";

const CARGO = '[workspace.package]\n# note\nversion = "1.2.3"\nedition = "2024"\n';
const CHANGELOG =
  "# Changelog\n\n## [Unreleased]\n\n- New thing.\n\n## [1.2.3] - 2026-01-01\n\n- Old thing.\n";

describe("versions", () => {
  it("reads and replaces the workspace version", () => {
    expect(parseVersion(CARGO)).toBe("1.2.3");
    expect(parseVersion(replaceVersion(CARGO, "2.0.0"))).toBe("2.0.0");
  });

  it.each([
    ["major", "2.0.0"],
    ["minor", "1.3.0"],
    ["patch", "1.2.4"],
    ["v1.5.0", "1.5.0"],
  ])("resolves %s", (request, expected) => {
    expect(nextVersion("1.2.3", request)).toBe(expected);
  });

  it("refuses a version that doesn't move forward", () => {
    expect(() => nextVersion("1.2.3", "1.2.3")).toThrow(ReleaseError);
    expect(() => nextVersion("1.2.3", "1.1.9")).toThrow(ReleaseError);
    expect(() => nextVersion("1.2.3", "banana")).toThrow(ReleaseError);
  });

  it("compares numerically rather than as text", () => {
    expect(nextVersion("1.9.0", "1.10.0")).toBe("1.10.0");
  });
});

describe("the changelog", () => {
  it("dates the Unreleased notes and leaves a fresh section", () => {
    const rolled = rollChangelog(CHANGELOG, "1.3.0", "2026-11-10");
    expect(rolled).toContain("## [Unreleased]\n\n## [1.3.0] - 2026-11-10\n");
    expect(releaseNotes(rolled, "1.3.0")).toBe("- New thing.");
  });

  it("refuses to release nothing", () => {
    const empty = "## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- x\n";
    expect(() => rollChangelog(empty, "1.1.0", "2026-11-10")).toThrow(/first/);
  });

  it("reads one section and no further", () => {
    expect(releaseNotes(CHANGELOG, "1.2.3")).toBe("- Old thing.");
    expect(() => releaseNotes(CHANGELOG, "9.9.9")).toThrow(ReleaseError);
  });
});

describe("tags", () => {
  it("must look like a version", () => {
    expect(tagVersion("v1.2.3")).toBe("1.2.3");
    expect(() => tagVersion("1.2.3")).toThrow(ReleaseError);
    expect(() => tagVersion("vbanana")).toThrow(ReleaseError);
  });

  it("must agree with the code and have notes", () => {
    expect(() => check("v1.2.3", CARGO, CHANGELOG)).not.toThrow();
    expect(() => check("v1.2.4", CARGO, CHANGELOG)).toThrow(/does not match/);
    expect(() => check("v1.2.3", CARGO, "# Changelog\n")).toThrow(/no section/);
  });
});
