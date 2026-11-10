// Regenerates every app icon from packaging/disweep.svg.
//
//     bun scripts/icons.ts

import { mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import sharp from "sharp";

const root = join(import.meta.dirname, "..");
const workspace = join(tmpdir(), `disweep-icons-${process.pid}`);
mkdirSync(workspace, { recursive: true });

try {
  const master = join(workspace, "icon.png");
  await sharp(join(root, "packaging/disweep.svg"), { density: 300 })
    .resize(1024, 1024)
    .png()
    .toFile(master);

  const tauri = Bun.spawnSync(["bun", "tauri", "icon", master, "-o", "src-tauri/icons"], {
    cwd: root,
    stdout: "inherit",
    stderr: "inherit",
  });
  if (tauri.exitCode !== 0) process.exit(tauri.exitCode);

  // Only the desktop sizes are shipped.
  for (const folder of ["android", "ios"]) {
    rmSync(join(root, "src-tauri/icons", folder), { recursive: true, force: true });
  }
} finally {
  rmSync(workspace, { recursive: true, force: true });
}
