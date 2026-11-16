// Video transcoding into several resolutions, timed: the JavaScript (Node)
// twin of transcode.mim, step for step.
//
//   1. ffmpeg alone: one ffmpeg run per resolution, then one run for all.
//   2. Our own scaler: ffmpeg decodes once into raw YUV 4:2:0 frames, this
//      program scales every plane of every frame (the same bilinear, 8-bit
//      fixed-point arithmetic as transcode.mim, so the bytes are the same),
//      and ffmpeg encodes each result.
//
// The frames are shared out over worker threads, one per core (as Mimic
// uses every core by itself); --threads 1 keeps it on one.
//
//   node transcode.mjs                       # the clip transcode.mim made
//   node transcode.mjs video.mp4 3           # your video, 3 seconds
//   node transcode.mjs --threads 1
//
// Results go to out/results-js.json; compare.mjs compares them with Mimic's.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism } from "node:os";
import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";

const SIZES = [
  { w: 1280, h: 720 },
  { w: 854, h: 480 },
  { w: 640, h: 360 },
  { w: 426, h: 240 },
];
const label = (s) => `${s.h}p`;
const ENCODE = ["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-an"];

// ---- the scaler (also run by the workers) --------------------------------

// Output bytes [lo, hi) of every frame, as transcode.mim's scale_all: a frame
// is its Y plane (w×h) then U and V (w/2 × h/2 each). Integer arithmetic
// only; the floored division of Mimic equals JS's truncation here (every
// operand is non-negative).
function scaleRange(src, dst, lo, hi, w, h, w2, h2) {
  const cw = w >> 1, ch = h >> 1, cw2 = w2 >> 1, ch2 = h2 >> 1;
  const inFrame = w * h + 2 * cw * ch;
  const outFrame = w2 * h2 + 2 * cw2 * ch2;
  const div = (a, b) => Math.floor(a / b);
  for (let o = lo; o < hi; o++) {
    const f = div(o, outFrame);
    const r = o % outFrame;
    const luma = r < w2 * h2;
    const k = luma ? r : (r - w2 * h2) % (cw2 * ch2);
    const plane = luma ? 0 : 1 + div(r - w2 * h2, cw2 * ch2);
    const pw = luma ? w : cw, ph = luma ? h : ch, qw = luma ? w2 : cw2, qh = luma ? h2 : ch2;
    const base = f * inFrame + (plane === 0 ? 0 : w * h + (plane - 1) * cw * ch);
    const sx = div((k % qw) * (pw - 1) * 256, qw - 1);
    const sy = div(div(k, qw) * (ph - 1) * 256, qh - 1);
    const x0 = sx >> 8, y0 = sy >> 8, fx = sx & 255, fy = sy & 255;
    const x1 = Math.min(x0 + 1, pw - 1), y1 = Math.min(y0 + 1, ph - 1);
    const top = src[base + y0 * pw + x0] * (256 - fx) + src[base + y0 * pw + x1] * fx;
    const bot = src[base + y1 * pw + x0] * (256 - fx) + src[base + y1 * pw + x1] * fx;
    // (top * (256 - fy) + bot * fy) < 2^32: plain numbers, exact
    dst[o] = Math.floor((top * (256 - fy) + bot * fy + 32768) / 65536);
  }
}

// ---- the program -----------------------------------------------------------

function must(program, args) {
  try {
    return execFileSync(program, args, { encoding: "utf8", maxBuffer: 1 << 26 });
  } catch (e) {
    throw new Error(`${program} failed: ${e.stderr || e.message} (is it installed and on the PATH?)`);
  }
}

const now = () => process.hrtime.bigint();
const secondsSince = (t0) => Number(now() - t0) / 1e9;

async function scaleAll(src, frames, w, h, w2, h2, threads) {
  const outFrame = w2 * h2 + 2 * (w2 >> 1) * (h2 >> 1);
  const n = frames * outFrame;
  const dst = new Uint8Array(new SharedArrayBuffer(n));
  if (threads <= 1) {
    scaleRange(src, dst, 0, n, w, h, w2, h2);
    return dst;
  }
  const step = Math.ceil(n / threads);
  const jobs = [];
  for (let lo = 0; lo < n; lo += step) {
    const hi = Math.min(n, lo + step);
    jobs.push(
      new Promise((ok, fail) => {
        const wk = new Worker(new URL(import.meta.url), { workerData: { src: src.buffer, dst: dst.buffer, lo, hi, w, h, w2, h2 } });
        wk.once("message", ok);
        wk.once("error", fail);
      }),
    );
  }
  await Promise.all(jobs);
  return dst;
}

async function main() {
  const argv = process.argv.slice(2);
  let threads = availableParallelism();
  const ti = argv.indexOf("--threads");
  if (ti >= 0) {
    threads = Number(argv[ti + 1]);
    argv.splice(ti, 2);
  }
  const seconds = argv[1] || "2";
  mkdirSync("out", { recursive: true });
  let src = argv[0];
  if (!src) {
    src = "out/source-1080p.mp4";
    if (!existsSync(src)) {
      console.log(`making a ${seconds} s 1920×1080 test clip: ${src}`);
      must("ffmpeg", ["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30", "-t", seconds, "-pix_fmt", "yuv420p", "-c:v", "libx264", "-preset", "veryfast", src]);
    }
  }
  const [w, h] = must("ffprobe", ["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0", src]).trim().split(",").map(Number);
  console.log(`source: ${src}, ${w}×${h}, ${threads} thread(s)`);

  // 1. ffmpeg alone
  const oneByOne = SIZES.map((s) => {
    const t0 = now();
    must("ffmpeg", ["-y", "-v", "error", "-i", src, "-t", seconds, "-vf", `scale=${s.w}:${s.h}`, ...ENCODE, `out/ffmpeg-js-${label(s)}.mp4`]);
    const t = { name: label(s), seconds: secondsSince(t0) };
    console.log(`ffmpeg ${t.name.padStart(5)}: ${t.seconds.toFixed(2)} s`);
    return t;
  });
  const n = SIZES.length;
  const splits = SIZES.map((_, i) => `[v${i}]`).join("");
  const scales = SIZES.map((s, i) => `[v${i}]scale=${s.w}:${s.h}[o${i}]`).join(";");
  const outs = SIZES.flatMap((s, i) => ["-map", `[o${i}]`, ...ENCODE, `out/ffmpeg-js-all-${label(s)}.mp4`]);
  const tAll = now();
  must("ffmpeg", ["-y", "-v", "error", "-i", src, "-t", seconds, "-filter_complex", `[0:v]split=${n}${splits};${scales}`, ...outs]);
  const allSeconds = secondsSince(tAll);
  console.log(`ffmpeg all at once: ${allSeconds.toFixed(2)} s`);

  // 2. decode once, scale here, encode each
  const tDec = now();
  must("ffmpeg", ["-y", "-v", "error", "-i", src, "-t", seconds, "-f", "rawvideo", "-pix_fmt", "yuv420p", "out/frames-js.yuv"]);
  const file = readFileSync("out/frames-js.yuv");
  // shared with the workers
  const raw = new Uint8Array(new SharedArrayBuffer(file.length));
  raw.set(file);
  const frameBytes = w * h + 2 * (w >> 1) * (h >> 1);
  const frames = Math.floor(raw.length / frameBytes);
  const decodeSeconds = secondsSince(tDec);
  console.log(`decoded ${frames} frames (${Math.floor(raw.length / 1e6)} MB) in ${decodeSeconds.toFixed(2)} s`);
  const own = [];
  for (const s of SIZES) {
    const t0 = now();
    const out = await scaleAll(raw, frames, w, h, s.w, s.h, threads);
    let checksum = 0;
    for (let i = 0; i < out.length; i++) checksum += out[i];
    const scaleSeconds = secondsSince(t0);
    const rawPath = `out/js-${label(s)}.yuv`;
    writeFileSync(rawPath, out);
    const t1 = now();
    must("ffmpeg", ["-y", "-v", "error", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", `${s.w}x${s.h}`, "-r", "30", "-i", rawPath, ...ENCODE, `out/js-${label(s)}.mp4`]);
    const encodeSeconds = secondsSince(t1);
    rmSync(rawPath);
    console.log(`js     ${label(s).padStart(5)}: scale ${scaleSeconds.toFixed(3)} s, encode ${encodeSeconds.toFixed(2)} s, checksum ${checksum}`);
    own.push({ name: label(s), scale_seconds: scaleSeconds, encode_seconds: encodeSeconds, checksum, bytes: out.length });
  }
  rmSync("out/frames-js.yuv");
  const results = { language: "js", threads, source: src, width: w, height: h, frames, ffmpeg_each: oneByOne, ffmpeg_all_seconds: allSeconds, decode_seconds: decodeSeconds, own };
  writeFileSync(`out/results-js${threads === 1 ? "-1thread" : ""}.json`, JSON.stringify(results, null, 2));
  const total = own.reduce((a, x) => a + x.scale_seconds, 0);
  console.log(`scaling all ${n} resolutions: ${total.toFixed(3)} s; results in out/results-js${threads === 1 ? "-1thread" : ""}.json`);
}

// a worker scales its share; the main thread runs the program
if (!isMainThread) {
  const { src, dst, lo, hi, w, h, w2, h2 } = workerData;
  scaleRange(new Uint8Array(src), new Uint8Array(dst), lo, hi, w, h, w2, h2);
  parentPort.postMessage("done");
} else {
  await main();
}
