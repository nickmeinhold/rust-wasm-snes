# Bench Harness

A determinism-first performance harness for the SNES emulator. It exists to
answer one question reliably: **did this change make the emulator faster
without changing what it computes?**

Every run emits a structured JSON record containing both timing distributions
and content fingerprints (FNV-1a 64-bit hashes over the framebuffer and over
the audio sample stream). The hashes are the safety net: if they change when
you didn't intend to change behavior, you have a regression — and the
comparator will scream about it.

There are two runners — a **native** one (Rust binary) and a **browser** one
(WASM under headless Chromium) — and a JSON diff tool that consumes either.

---

## Why both native and browser?

- **Native** isolates the emulator core. No JS/WASM boundary, no canvas, no
  Chromium scheduler. It's the cleanest signal for "did this Rust change
  affect emulator-internal cost?"
- **Browser** measures what users experience: WASM compilation time, the
  per-frame cost of crossing the JS/WASM boundary, copy vs zero-copy
  framebuffer access, audio drain overhead.

A perf change should usually move both — and the FB/audio hashes should
match between them and across runs. If they diverge, something is
non-deterministic (uninitialised memory, hash-map iteration order leaking
into output, time-dependent code paths).

---

## Quick start

### Native

```bash
# from the repo root
cargo run --release --bin bench -- rom/smw.smc --frames 600 --label baseline > bench/baseline-native.json
```

stdout is the JSON record. Diagnostics (loading, opcode histogram, progress)
go to stderr — **always redirect stdout, never `&>`**, or you'll corrupt the
JSON.

Flags:
- `[ROM_PATH]` — first positional argument. Defaults to `rom/zelda3.smc`.
- `--frames N` — frames to run. Default `600` (~10 seconds of gameplay at 60Hz).
- `--label NAME` — free-form string copied into the JSON. Use it to tag
  experiments (`baseline`, `after-simd`, `after-idle-loop`).

### Browser

```bash
cd bench
npm install        # one-time, pulls Playwright + Chromium
node bench-cli.js --rom ./rom/smw.smc --frames 600 --label baseline-browser --path copy > baseline-browser.json
```

The runner spins up a tiny static HTTP server on `127.0.0.1:8765`, serves
`web/bench.html`, and waits for the page to set `window.__benchResult`.

Flags:
- `--frames N` — frames to run. Default `600`.
- `--label NAME` — tag for the JSON record.
- `--rom URL` — path served by the local server (relative to `web/`).
- `--path copy|zero-copy` — how the page reads the framebuffer (see below).
- `--port N` — change the static server port. Default `8765`.
- `--headed` — show the Chromium window. Useful for debugging the page itself.

---

## The two browser paths

`web/bench.html` supports two framebuffer-access strategies, selected via
`?path=...` (or `--path` from the CLI):

- **`copy`** (default, legacy) — `emulator.run_frame()` returns a `Vec<u8>`,
  which wasm-bindgen copies into a fresh JS `Uint8Array` every frame. Simple,
  but allocates and copies ~256 KB per frame across the boundary.
- **`zero-copy`** — `emulator.run_frame_no_return()` leaves the framebuffer
  in WASM linear memory; JS constructs a `Uint8ClampedArray` view over it.
  No copy, no allocation. Audio uses the analogous pointer/length API
  (`audio_samples_ptr` + `clear_audio_samples`).

Both paths must produce identical FB and audio hashes. If they don't, the
zero-copy path has a bug (most likely: stale view across a WASM memory grow).

---

## Workflow: measure a change

```bash
# 1. Capture a baseline.
cargo run --release --bin bench -- rom/smw.smc --label baseline > bench/baseline-native.json

# 2. Make your change (optimization, refactor, whatever).

# 3. Re-run with the same ROM, frame count, and reset state.
cargo run --release --bin bench -- rom/smw.smc --label after > bench/after-native.json

# 4. Diff.
node bench/compare.js bench/baseline-native.json bench/after-native.json
```

`compare.js` prints a side-by-side table (mean / P50 / P95 / P99 / max,
emulated FPS, audio drain stats if present, total bytes crossing the
boundary) followed by the determinism check:

```
✓ framebuffer hash UNCHANGED:  54b3eed74f9f8432
✓ audio       hash UNCHANGED:  62300ecfc4da23e0

(emulation output is bit-identical on both pixels and samples —
 change is purely about speed/size)
```

If a hash changes:

```
✗ framebuffer hash CHANGED:
    baseline: 54b3eed74f9f8432
    after:    a1b2c3d4e5f60789

(emulation output is different — verify this was an intentional change…)
```

This is the most important line in the report. **Investigate before
celebrating any speedup.** SIMD reorderings, integer-overflow assumptions,
and floating-point rounding can all change semantics in ways that look like
free wins but actually mean the emulator now produces subtly wrong output.

You can also feed `compare.js` a native baseline and a browser after-run —
common metrics line up; browser-only fields (cold-load, ctor) are simply
omitted from the table.

---

## What the hashes mean

Both hashes are **FNV-1a 64-bit** (the same constants as in
[fowler-noll-vo](http://www.isthe.com/chongo/tech/comp/fnv/)). The native
binary and the browser page implement the algorithm identically — the
browser uses `BigInt` to dodge JS's lack of `u64`. Cheap, dependency-free,
and good enough for "are these byte streams identical?"

- **`final_fb_hash`** — FNV-1a over the RGBA bytes of the framebuffer at the
  end of the last frame. A single pixel difference flips it.
- **`final_audio_hash`** — FNV-1a maintained _inside_ the emulator, updated
  as audio samples are drained each frame. The native bench drains via
  `get_audio_samples()` every frame; the browser does the same. If you skip
  draining, the hash stays at its initial state and tells you nothing —
  this is intentional, mirroring how real consumers behave.

Equality across two runs ⇒ semantic-preserving change (almost certainly safe).
Inequality ⇒ behaviour changed; flag and investigate.

---

## Reference values

For **SMW (`rom/smw.smc`) × 600 frames from default reset state**:

| Metric            | Value                |
|-------------------|----------------------|
| `final_fb_hash`   | `54b3eed74f9f8432`   |
| `final_audio_hash`| `62300ecfc4da23e0`   |
| Frame mean (native)  | ~1640 µs           |
| Frame mean (browser) | ~1765 µs           |

The hashes are deterministic across hardware and across native/browser
runners. The frame times are **not** — they depend on the host CPU,
thermal state, and (in the browser) Chromium version. Treat the µs numbers
as a per-machine baseline; treat the hashes as universal contracts.

If your hashes don't match those values, you're either on a different ROM,
running a different number of frames, or the emulator core has drifted from
the version that produced this README. Don't treat the inequality as broken
until you've ruled out those three.

---

## JSON shape

Native (`src/bin/bench.rs`):

```json
{
  "label": "baseline",
  "rom": "rom/smw.smc",
  "rom_size_bytes": 524288,
  "frames": 600,
  "init_time_us": 1234,
  "frame_time_us": { "min": ..., "mean": ..., "p50": ..., "p95": ..., "p99": ..., "max": ... },
  "emulated_fps": 609.8,
  "run_total_us": 984000,
  "total_fb_bytes_returned": 157286400,
  "final_fb_hash": "54b3eed74f9f8432",
  "total_audio_samples": 19200,
  "final_audio_hash": "62300ecfc4da23e0"
}
```

The native binary additionally prints a CPU opcode histogram to **stderr**
(top 15 by dispatch count, with cumulative share). Use it to answer "is the
hot path concentrated enough to be worth optimising a single opcode, or is
dispatch-overhead itself the cost?"

Browser (`web/bench.html`) adds:

- `wasm_init_ms`, `rom_fetch_ms`, `ctor_ms`, `cold_load_ms` — staged
  startup costs.
- `audio_drain_us` — distribution stats over the per-frame cost of pulling
  audio samples back across the boundary.
- `path` — `"copy"` or `"zero-copy"` (echo of the URL param).
- `user_agent`, `hardware_concurrency` — for cross-machine sanity checks.

---

## Pitfalls

- **Port 8765 in use.** The browser runner picks `127.0.0.1:8765` by
  default. If something else (a stale Playwright run, another dev server)
  is already listening, the runner fails with `EADDRINUSE`. Either
  `lsof -i :8765` and kill the offender, or pass `--port 8766`.
- **Don't merge stderr into stdout.** The native binary writes JSON to
  stdout and diagnostics to stderr deliberately, so you can do
  `bench > result.json` and still see progress. Using `&>` or `2>&1 >`
  will inject the opcode histogram into your JSON file and make
  `compare.js` choke.
- **Reset state matters.** The hashes are only stable for the same ROM,
  frame count, and starting state. If you've added an option that injects
  inputs, randomises reset state, or changes init order, expect new
  reference hashes — and update them in this README.
- **Browser runs are noisier than native.** Chromium scheduling, GC, and
  background tabs all add jitter. For perf claims, prefer native; use the
  browser bench to verify the boundary cost specifically.
- **Audio hash is zero if you don't drain.** If you add a code path that
  calls `run_frame()` without `get_audio_samples()` / `clear_audio_samples()`,
  the audio hash stops being a meaningful probe.

---

## Files

- `../src/bin/bench.rs` — native runner.
- `../web/bench.html` — browser bench page (loads the WASM build, runs N
  frames, populates `window.__benchResult`).
- `bench-cli.js` — Playwright driver that hosts `web/`, opens
  `bench.html`, and forwards the JSON to stdout.
- `compare.js` — diff tool for two bench JSON files.
- `baseline-*.json`, `after-*.json` — captured runs from previous experiments.
  Keep these as historical reference points; don't overwrite without thought.

## Extending

- **A new metric**: add a field to the native or browser bench JSON, then
  add a `row(...)` line in `compare.js`. The comparator already tolerates
  missing fields, so old baselines remain readable.
- **A new ROM**: just pass it as the positional / `--rom` argument and
  capture a fresh reference hash. Different ROMs will have different
  reference values — that's expected.
- **CI gating**: see task #17. The intended shape is "run native bench on
  every commit, fail the build if either hash changes without an explicit
  acknowledgement."

See `PHASE_B_PLAN.md` for the next round of work this harness is being
used to validate (Web Worker, SharedArrayBuffer, AudioWorklet,
OffscreenCanvas).
