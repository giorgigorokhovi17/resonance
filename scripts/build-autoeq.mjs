// Builds public/autoeq.json: every AutoEQ (github.com/jaakkopasanen/AutoEq, MIT) parametric EQ
// profile, so the headphone EQ finder works offline. Re-run to refresh: `node scripts/build-autoeq.mjs`
//
// Entry format (compact): [name, source, rig, kind, preamp, [type, fc, gain, q, type, fc, ...]]
//   kind: "over-ear" | "in-ear" | "earbud"; type: "PK" | "LSC" | "HSC" | "LP" | "HP"
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const RAW = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/";
const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "autoeq.json");
const CONCURRENCY = 24;

async function get(url, tries = 4) {
  for (let i = 1; ; i++) {
    try {
      const r = await fetch(url);
      if (r.status === 404) return null;
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      return await r.text();
    } catch (e) {
      if (i >= tries) throw new Error(`${url}: ${e.message}`);
      await new Promise((res) => setTimeout(res, 500 * i * i));
    }
  }
}

const TYPES = { PK: "PK", PEQ: "PK", LSC: "LSC", LS: "LSC", HSC: "HSC", HS: "HSC", LP: "LP", LPQ: "LP", HP: "HP", HPQ: "HP" };

function parse(text) {
  let preamp = 0;
  const filters = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    const pre = /^preamp:\s*(-?[\d.]+)/i.exec(line);
    if (pre) {
      preamp = +pre[1];
      continue;
    }
    const m = /^filter\s*\d*:\s*on\s+(\w+)\s+fc\s+([\d.]+)\s*hz(?:\s+gain\s+(-?[\d.]+)\s*db)?(?:\s+q\s+([\d.]+))?/i.exec(line);
    if (!m || !TYPES[m[1].toUpperCase()]) continue;
    filters.push(TYPES[m[1].toUpperCase()], +m[2], +(m[3] ?? 0), +(m[4] ?? 0.707));
  }
  return filters.length ? { preamp, filters } : null;
}

function kindOf(folder) {
  const f = folder.toLowerCase();
  if (f.includes("in-ear")) return "in-ear";
  if (f.includes("earbud")) return "earbud";
  return "over-ear";
}

const index = await get(RAW + "INDEX.md");
const items = [];
for (const line of index.split(/\r?\n/)) {
  const m = /^- \[(.+)\]\(\.\/(.+)\) by (.+?)(?: on (.+))?$/.exec(line.trim());
  if (!m) continue;
  const [, name, link, source, rig] = m;
  const parts = link.split("/");
  if (parts.length < 3) continue;
  const url = RAW + link + "/" + parts[parts.length - 1] + "%20ParametricEQ.txt";
  items.push({ name, source, rig: rig ?? "", kind: kindOf(decodeURIComponent(parts[1])), url });
}
console.log(`${items.length} profiles in index`);

const entries = [];
let done = 0;
let missing = 0;
let next = 0;
async function worker() {
  while (next < items.length) {
    const it = items[next++];
    const text = await get(it.url);
    const eq = text && parse(text);
    if (eq) entries.push([it.name, it.source, it.rig, it.kind, eq.preamp, eq.filters]);
    else missing++;
    if (++done % 500 === 0) console.log(`${done}/${items.length}`);
  }
}
await Promise.all(Array.from({ length: CONCURRENCY }, worker));
entries.sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));

await mkdir(dirname(OUT), { recursive: true });
await writeFile(OUT, JSON.stringify({ generated: new Date().toISOString().slice(0, 10), entries }));
console.log(`wrote ${entries.length} profiles (${missing} without a ParametricEQ) to ${OUT}`);
