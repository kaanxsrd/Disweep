// Regenerates the TypeScript types in src/lib/bindings from the Rust ones.
//
//     bun scripts/bindings.ts

import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const result = Bun.spawnSync(["cargo", "test", "--workspace", "--lib", "export_bindings"], {
  cwd: root,
  stdout: "inherit",
  stderr: "inherit",
});
process.exit(result.exitCode);
