// Compare the transcoding runs of transcode.mim and transcode.mjs: reads
// out/results-*.json and prints, per resolution, the time each language
// took to scale every frame, how many times faster Mimic was, and whether
// the scaled frames are the same bytes (their checksums). Then the whole
// pipelines: ffmpeg alone, and decode + own scaling + encode.
//
//   mimic run --release transcode.mim
//   MIMIC_THREADS=1 mimic run --release transcode.mim -- out/source-1080p.mp4
//   node transcode.mjs && node transcode.mjs --threads 1
//   node compare.mjs

import { existsSync, readFileSync } from "node:fs";

const load = (name) => (existsSync(`out/${name}.json`) ? JSON.parse(readFileSync(`out/${name}.json`, "utf8")) : undefined);
const runs = [
  ["Mimic, all cores", load("results-mimic")],
  ["JS, worker threads", load("results-js")],
  ["Mimic, 1 thread", load("results-mimic-1thread")],
  ["JS, 1 thread", load("results-js-1thread")],
].filter(([, r]) => r);
if (!runs.length) {
  console.error("no results in out/: run transcode.mim and transcode.mjs first");
  process.exit(1);
}

const fmt = (s) => `${s.toFixed(3)} s`;
const pad = (s, n) => String(s).padEnd(n);
const lpad = (s, n) => String(s).padStart(n);
const first = runs[0][1];
console.log(`${first.source}: ${first.width}×${first.height}, ${first.frames} frames\n`);

// 1. scaling, per resolution
const names = first.own.map((x) => x.name);
console.log("Scaling every frame (the languages' own code)");
console.log(pad("", 22) + names.map((n) => lpad(n, 11)).join("") + lpad("total", 11));
for (const [who, r] of runs) {
  const t = r.own.map((x) => x.scale_seconds);
  console.log(pad(who, 22) + t.map((x) => lpad(fmt(x), 11)).join("") + lpad(fmt(t.reduce((a, b) => a + b, 0)), 11));
}
const total = (r) => r.own.reduce((a, x) => a + x.scale_seconds, 0);
const pairs = [
  ["results-mimic", "results-js", "all cores"],
  ["results-mimic-1thread", "results-js-1thread", "1 thread"],
  ["results-mimic-1thread", "results-mimic", "Mimic: all cores against 1 thread"],
];
console.log();
for (const [a, b, what] of pairs) {
  const ra = load(a), rb = load(b);
  if (!ra || !rb) continue;
  if (a.endsWith("1thread") && b === "results-mimic") {
    console.log(`${what}: ${(total(ra) / total(rb)).toFixed(1)}× faster on every core`);
  } else {
    console.log(`Mimic is ${(total(rb) / total(ra)).toFixed(1)}× faster than JS (${what})`);
  }
}

// 2. the same frames?
console.log();
const sums = runs.map(([who, r]) => [who, r.own.map((x) => x.checksum).join(",")]);
const same = sums.every(([, s]) => s === sums[0][1]);
console.log(same ? "The scaled frames are the same bytes in every run (checksums match)." : "CHECKSUMS DIFFER:\n" + sums.map(([w, s]) => `  ${w}: ${s}`).join("\n"));

// 3. whole pipelines
console.log("\nWhole transcodes (ffmpeg's time included)");
for (const [who, r] of runs) {
  const each = r.ffmpeg_each.reduce((a, x) => a + x.seconds, 0);
  const own = r.decode_seconds + r.own.reduce((a, x) => a + x.scale_seconds + x.encode_seconds, 0);
  console.log(`${pad(who, 22)}ffmpeg one by one ${fmt(each)}, ffmpeg all at once ${fmt(r.ffmpeg_all_seconds)}, decode + own scaling + encode ${fmt(own)}`);
}
