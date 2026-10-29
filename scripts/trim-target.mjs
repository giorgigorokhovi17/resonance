// Keeps src-tauri/target from growing without end: Cargo never deletes old build output, and it
// reached over 100 GB. Runs before `tauri dev` (see the "dev" script in package.json). At most
// once a day it adds up the folder's size; past LIMIT_GB it runs `cargo clean`, so the next build
// starts from scratch (a few minutes). Never fails the dev start.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { readdir, stat } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const LIMIT_GB = 40;
const DAY_MS = 24 * 60 * 60 * 1000;

const crate = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri");
const target = join(crate, "target");
const stamp = join(target, ".size-checked");

async function size(dir) {
  let total = 0;
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return 0;
  }
  await Promise.all(
    entries.map(async (e) => {
      const p = join(dir, e.name);
      if (e.isDirectory()) total += await size(p);
      else if (e.isFile()) total += await stat(p).then((s) => s.size, () => 0);
    }),
  );
  return total;
}

try {
  const last = existsSync(stamp) ? Number(readFileSync(stamp, "utf8")) || 0 : 0;
  if (existsSync(target) && Date.now() - last > DAY_MS) {
    const gb = (await size(target)) / 1024 ** 3;
    if (gb > LIMIT_GB) {
      console.log(`src-tauri/target is ${gb.toFixed(0)} GB, over ${LIMIT_GB} GB: running cargo clean. The next build takes a few minutes.`);
      execFileSync("cargo", ["clean"], { cwd: crate, stdio: "inherit" });
    }
    if (existsSync(target)) writeFileSync(stamp, String(Date.now()));
  }
} catch (e) {
  console.warn(`Skipped the build folder size check: ${e?.message ?? e}`);
}
