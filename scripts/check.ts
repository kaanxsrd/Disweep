// Everything a pull request has to pass, in the order that fails fastest.
//
//     bun run check

import { join } from "node:path";

const root = join(import.meta.dirname, "..");

const steps: [string, string[]][] = [
  ["Biome", ["bun", "run", "biome", "check", "."]],
  ["TypeScript", ["bun", "run", "tsc", "-b"]],
  ["Interface tests", ["bun", "run", "vitest", "run"]],
  ["Rust formatting", ["cargo", "fmt", "--all", "--check"]],
  ["Clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]],
  ["Rust tests", ["cargo", "test", "--workspace"]],
];

for (const [name, command] of steps) {
  console.log(`\n=== ${name}`);
  const result = Bun.spawnSync(command, { cwd: root, stdout: "inherit", stderr: "inherit" });
  if (result.exitCode !== 0) {
    console.error(`\n${name} failed.`);
    process.exit(result.exitCode ?? 1);
  }
}

console.log("\nAll checks passed.");
