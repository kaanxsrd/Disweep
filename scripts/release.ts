import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const VERSION_FILE = join(root, "Cargo.toml");
const CHANGELOG = join(root, "CHANGELOG.md");
const VERSION_LINE = /^version = "(\d+\.\d+\.\d+)"$/m;
const VERSION = /^\d+\.\d+\.\d+$/;
const UNRELEASED = "## [Unreleased]";

export class ReleaseError extends Error {}

export function parseVersion(source: string): string {
  const match = VERSION_LINE.exec(source);
  if (!match?.[1]) throw new ReleaseError("No version line found in Cargo.toml.");
  return match[1];
}

export function readVersion(): string {
  return parseVersion(readFileSync(VERSION_FILE, "utf8"));
}

export function replaceVersion(source: string, version: string): string {
  return source.replace(VERSION_LINE, `version = "${version}"`);
}

const parts = (version: string) => version.split(".").map(Number) as [number, number, number];

function isNewer(candidate: string, current: string): boolean {
  const [a, b] = [parts(candidate), parts(current)];
  for (const index of [0, 1, 2] as const) {
    if (a[index] !== b[index]) return a[index] > b[index];
  }
  return false;
}

export function nextVersion(current: string, request: string): string {
  const [major, minor, patch] = parts(current);
  if (request === "major") return `${major + 1}.0.0`;
  if (request === "minor") return `${major}.${minor + 1}.0`;
  if (request === "patch") return `${major}.${minor}.${patch + 1}`;
  const candidate = request.replace(/^v/, "");
  if (!VERSION.test(candidate)) {
    throw new ReleaseError(`Use major, minor, patch, or X.Y.Z; got ${request}.`);
  }
  if (!isNewer(candidate, current)) {
    throw new ReleaseError(`${candidate} is not newer than the current version ${current}.`);
  }
  return candidate;
}

function sectionSpan(changelog: string, headingStart: number): [number, number] {
  const bodyStart = changelog.indexOf("\n", headingStart) + 1;
  const following = changelog.slice(bodyStart).search(/^## \[/m);
  return [bodyStart, following < 0 ? changelog.length : bodyStart + following];
}

export function rollChangelog(changelog: string, version: string, day: string): string {
  const start = changelog.indexOf(UNRELEASED);
  if (start < 0) throw new ReleaseError(`CHANGELOG.md has no ${UNRELEASED} section.`);
  const [bodyStart, bodyEnd] = sectionSpan(changelog, start);
  if (changelog.slice(bodyStart, bodyEnd).trim() === "") {
    throw new ReleaseError(`Add the changes to ${UNRELEASED} in CHANGELOG.md first.`);
  }
  const heading = `## [${version}] - ${day}\n`;
  return `${changelog.slice(0, bodyStart)}\n${heading}${changelog.slice(bodyStart)}`;
}

export function releaseNotes(changelog: string, version: string): string {
  const escaped = version.replaceAll(".", "\\.");
  const heading = new RegExp(`^## \\[${escaped}\\]`, "m").exec(changelog);
  if (!heading) throw new ReleaseError(`CHANGELOG.md has no section for ${version}.`);
  const [bodyStart, bodyEnd] = sectionSpan(changelog, heading.index);
  const notes = changelog.slice(bodyStart, bodyEnd).trim();
  if (notes === "") throw new ReleaseError(`The ${version} section of CHANGELOG.md is empty.`);
  return notes;
}

export function tagVersion(tag: string): string {
  const version = tag.startsWith("v") ? tag.slice(1) : "";
  if (!VERSION.test(version)) throw new ReleaseError(`Release tags look like v1.2.3; got ${tag}.`);
  return version;
}

export function check(tag: string, versionSource: string, changelog: string) {
  const version = tagVersion(tag);
  const declared = parseVersion(versionSource);
  if (declared !== version) {
    throw new ReleaseError(`Tag ${tag} does not match Cargo.toml version ${declared}.`);
  }
  releaseNotes(changelog, version);
}

function git(...args: string[]): string {
  const result = Bun.spawnSync(["git", ...args], { cwd: root });
  if (result.exitCode !== 0) {
    throw new ReleaseError(`git ${args.join(" ")} failed:\n${result.stderr.toString().trim()}`);
  }
  return result.stdout.toString().trim();
}

function cut(request: string) {
  const source = readFileSync(VERSION_FILE, "utf8");
  const changelog = readFileSync(CHANGELOG, "utf8");
  const version = nextVersion(parseVersion(source), request);
  if (git("status", "--porcelain")) {
    throw new ReleaseError("Commit or stash your changes before cutting a release.");
  }
  if (git("tag", "--list", `v${version}`))
    throw new ReleaseError(`Tag v${version} already exists.`);

  const today = new Date().toISOString().slice(0, 10);
  const rolled = rollChangelog(changelog, version, today);
  writeFileSync(VERSION_FILE, replaceVersion(source, version));
  writeFileSync(CHANGELOG, rolled);

  const lock = Bun.spawnSync(["cargo", "update", "--workspace"], { cwd: root });
  if (lock.exitCode !== 0) throw new ReleaseError("cargo couldn't update Cargo.lock.");

  git("add", VERSION_FILE, CHANGELOG, join(root, "Cargo.lock"));
  git("commit", "-m", `chore: release ${version}`);
  git("tag", "-a", `v${version}`, "-m", `Disweep ${version}`);
  console.log(`Created commit and tag v${version}. Publish with:\n\n    git push --follow-tags\n`);
}

function main(args: string[]): number {
  try {
    const [first, second] = args;
    if (args.length === 1 && first && first !== "check" && first !== "notes") {
      cut(first);
    } else if (first === "check" && second) {
      check(second, readFileSync(VERSION_FILE, "utf8"), readFileSync(CHANGELOG, "utf8"));
      console.log(`${second} is consistent with the code and the changelog.`);
    } else if (first === "notes" && second) {
      console.log(releaseNotes(readFileSync(CHANGELOG, "utf8"), tagVersion(second)));
    } else {
      console.error("Usage: bun scripts/release.ts patch|minor|major|X.Y.Z|check TAG|notes TAG");
      return 2;
    }
  } catch (error) {
    if (error instanceof ReleaseError) {
      console.error(`error: ${error.message}`);
      return 1;
    }
    throw error;
  }
  return 0;
}

if (import.meta.main) process.exit(main(process.argv.slice(2)));
