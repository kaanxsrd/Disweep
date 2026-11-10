import { describe, expect, it } from "vitest";
import { homebrewCask, parseChecksums, scoopManifest } from "./package-managers";

const sums = parseChecksums(
  [
    "aaa  disweep_1.2.0_darwin_arm64.zip",
    "bbb *disweep_1.2.0_darwin_amd64.zip",
    "ccc  disweep_1.2.0_windows_amd64.zip",
    "ddd  disweep_1.2.0_windows_arm64.zip",
  ].join("\n"),
);

describe("parseChecksums", () => {
  it("reads both sha256sum formats", () => {
    expect(sums.get("disweep_1.2.0_darwin_arm64.zip")).toBe("aaa");
    expect(sums.get("disweep_1.2.0_darwin_amd64.zip")).toBe("bbb");
  });
});

describe("the Homebrew cask", () => {
  const cask = homebrewCask("1.2.0", sums, "owner/Disweep");

  it("pins each architecture to its hash", () => {
    expect(cask).toContain('sha256 "aaa"');
    expect(cask).toContain('sha256 "bbb"');
    expect(cask).toContain("disweep_#{version}_darwin_arm64.zip");
  });

  it("installs the app", () => {
    expect(cask).toContain('app "Disweep.app"');
  });
});

describe("the Scoop manifest", () => {
  const manifest = JSON.parse(scoopManifest("1.2.0", sums, "owner/Disweep"));

  it("covers both Windows architectures", () => {
    expect(manifest.architecture["64bit"].hash).toBe("ccc");
    expect(manifest.architecture.arm64.hash).toBe("ddd");
    expect(manifest.version).toBe("1.2.0");
  });
});

describe("missing artifacts", () => {
  it("are reported instead of publishing a broken manifest", () => {
    expect(() => homebrewCask("9.9.9", sums, "owner/Disweep")).toThrow(/no entry/);
  });
});
