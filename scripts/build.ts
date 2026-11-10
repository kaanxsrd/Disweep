// Builds the app and proves the result starts before anyone downloads it.
//
//     bun scripts/build.ts
//
// The results land in release/, named disweep_<version>_<os>_<arch>.<ext>.

import { chmodSync, cpSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { appendFile } from "node:fs/promises";
import { join } from "node:path";
import { readVersion } from "./release";

const root = join(import.meta.dirname, "..");
const output = join(root, "release");
const bundles = join(root, "target/release/bundle");
const isWindows = process.platform === "win32";
const isMac = process.platform === "darwin";

function run(command: string[], cwd = root) {
  const result = Bun.spawnSync(command, { cwd, stdout: "inherit", stderr: "inherit" });
  if (result.exitCode !== 0) {
    throw new Error(`${command.join(" ")} exited with ${result.exitCode}`);
  }
}

/** A window app can't be asked politely, so these flags are the way to see whether it works. */
function smokeTest(executable: string, version: string) {
  const checks: [string, string][] = [
    ["--version", `Disweep ${version}`],
    ["--check", "Installation OK"],
  ];
  for (const [flag, expected] of checks) {
    console.log(`smoke test: ${flag}`);
    const result = Bun.spawnSync([executable, flag], { stdout: "pipe", stderr: "pipe" });
    const text = result.stdout.toString();
    if (result.exitCode !== 0 || !text.includes(expected)) {
      throw new Error(
        `Expected ${expected}, got exit ${result.exitCode}:\n${text}\n${result.stderr}`,
      );
    }
  }
}

function findOne(folder: string, suffix: string): string {
  const match = readdirSync(folder).find((name) => name.endsWith(suffix));
  if (!match) throw new Error(`No ${suffix} found in ${folder}`);
  return join(folder, match);
}

function archForName(): string {
  return process.arch === "x64" ? "amd64" : process.arch;
}

function packageWindows(stem: string, version: string): string[] {
  const executable = join(root, "target/release/disweep.exe");
  smokeTest(executable, version);

  const staging = join(output, "staging");
  mkdirSync(staging, { recursive: true });
  for (const file of [executable, join(root, "LICENSE"), join(root, "NOTICE.md")]) {
    cpSync(file, join(staging, file.split(/[\\/]/).pop() ?? ""));
  }
  const zip = join(output, `${stem}.zip`);
  run([
    "powershell",
    "-NoProfile",
    "-Command",
    `Compress-Archive -Path '${staging}\\*' -DestinationPath '${zip}' -Force`,
  ]);
  rmSync(staging, { recursive: true });

  const setup = join(output, `${stem}_setup.exe`);
  cpSync(findOne(join(bundles, "nsis"), "-setup.exe"), setup);
  return [zip, setup];
}

function packageMac(stem: string, version: string): string[] {
  const app = join(bundles, "macos/Disweep.app");
  const executable = join(app, "Contents/MacOS/disweep");
  chmodSync(executable, 0o755);
  smokeTest(executable, version);

  // ditto keeps the app bundle's signature and permissions, which zip does not.
  const zip = join(output, `${stem}.zip`);
  run(["ditto", "-c", "-k", "--keepParent", app, zip]);

  const dmg = join(output, `${stem}.dmg`);
  cpSync(findOne(join(bundles, "dmg"), ".dmg"), dmg);
  return [zip, dmg];
}

async function main() {
  const version = readVersion();
  const system = isWindows ? "windows" : isMac ? "darwin" : process.platform;
  const stem = `disweep_${version}_${system}_${archForName()}`;

  if (existsSync(output)) rmSync(output, { recursive: true });
  mkdirSync(output, { recursive: true });

  run(["bun", "run", "tauri", "build"]);
  const artifacts = isWindows
    ? packageWindows(stem, version)
    : isMac
      ? packageMac(stem, version)
      : [];
  if (artifacts.length === 0) throw new Error("Disweep is built for Windows and macOS only.");

  for (const artifact of artifacts) {
    const megabytes = (Bun.file(artifact).size / 2 ** 20).toFixed(1);
    console.log(`built ${artifact} (${megabytes} MiB)`);
  }

  const githubOutput = process.env.GITHUB_OUTPUT;
  if (githubOutput) await appendFile(githubOutput, `folder=${output}\n`);
}

await main();
