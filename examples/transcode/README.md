# Transcoding a video into several resolutions

A 1080p video transcoded to 720p, 480p, 360p and 240p, timed, in Mimic
(`transcode.mim`) and in JavaScript (`transcode.mjs`), then compared
(`compare.mjs`). Both run the same two pipelines:

1. **ffmpeg alone**: one ffmpeg run per resolution (decode, scale, encode),
   then one ffmpeg run that writes every resolution at once.
2. **The language's own scaler**: ffmpeg decodes the video once into raw
   YUV 4:2:0 frames; the program scales every plane of every frame to each
   resolution itself (bilinear, 8-bit fixed point); ffmpeg encodes each
   result. In Mimic the scaler is one map over all the output pixels, which
   the compiler fuses into one loop and runs on every core. In JavaScript
   the frames are shared out over worker threads (`--threads 1`: one).

The scaling is integer arithmetic, so the frames must come out as the same
bytes in every language: `compare.mjs` checks their checksums.

Needs ffmpeg and ffprobe on the PATH, Node 18 or newer for the JavaScript
side, and Mimic 0.3.16 or newer.

```sh
mimic run --release transcode.mim                    # makes a 2 s 1080p test clip in out/
MIMIC_THREADS=1 mimic run --release transcode.mim -- out/source-1080p.mp4
node transcode.mjs
node transcode.mjs --threads 1
node compare.mjs
```

Your own video: `mimic run --release transcode.mim -- video.mp4 3` (the
first 3 seconds; the same arguments for `node transcode.mjs`).

## Results

On an M3 Pro (12 cores), 60 frames of 1920×1080, the time to scale every
frame to all four resolutions:

| | all cores | one thread |
|---|---|---|
| Mimic | 1.23 s | 6.43 s |
| JavaScript (Node 22) | 4.06 s | 12.09 s |
| C (`clang -O3`, one thread, same arithmetic) | | 5.00 s |

Mimic is 3.3× faster than JavaScript on all cores and 1.9× faster on one
thread; on all cores it is 4× faster than the single-threaded C. On one
thread, C is ahead by what Mimic checks and C does not: every `src[i]` is
bounds-checked (memory safety), and `div` and `%` round towards minus
infinity (exact when the operands are not negative, which the compiler
uses: a plain unsigned divide then). The frames are the same bytes in all of
them.

The whole transcodes are dominated by ffmpeg's encoder (x264): with Mimic
scaling, decode + scale + encode of all four takes about 3.5 s; ffmpeg alone
does it in about 1.2 s when it writes all four outputs from one decode, and
about 2.5 s one resolution at a time.
