# CLAUDE.md — rust-wasm-snes session continuity

> **You are walking into mid-flight work.** This file orients you to where Nick
> and the previous Claude session left off, so you can pick up cleanly.
>
> Read this whole file once before doing anything substantive in this repo.
> Then **greet Nick and ask whether he wants to continue this work or start
> something new.** Don't auto-start — sessions begin from where Nick walks in,
> not from where the last one stopped.

---

## What this project is

A Rust SNES emulator targeting Zelda 3 (LTTP), compiled to WASM, runs in the
browser. See `README.md` for the project's framing — particularly the
audio-verified-against-reference and trace-oracle-debugging sections.

This file (`CLAUDE.md`) is purely for **session continuity**. It tracks what's
in flight, not what the project *is*.

---

## State of play (as of 2026-05-01)

### What's freshly built but uncommitted on `main`

A complete benchmarking + determinism harness, plus three shipped Phase A
optimizations. **All of this is currently uncommitted** in the working tree:

```
M  Cargo.toml          [[bin]] bench entry
M  src/cpu/mod.rs      opcode_counts: Box<[u64; 256]>; println→eprintln
M  src/lib.rs          zero-copy fb + audio APIs, audio_hash, run_frame_inner refactor
M  src/main.rs         (small)
M  src/rom.rs          (small)
M  web/index.html      production zero-copy fb + audio
M  web/serve.py        COOP/COEP headers (preparing for SAB)
?? .cargo/             config.toml — wasm32 target-feature=+simd128
?? PHASE_B_PLAN.md     integration spec for Worker + SAB + AudioWorklet
?? bench/              package.json, bench-cli.js, compare.js, baseline JSONs
?? src/bin/bench.rs    native bench: frame timing + opcode histogram + hashes
?? web/bench.html      browser bench page (copy + zero-copy paths)
```

**RECOMMENDED FIRST ACTION**: ask Nick if he wants to commit this Phase A work
to `main` before starting new work. Without committing, every git worktree
created from `main` will lack the bench harness — which is what blocked one of
the parallel agents in the last session (see Worktree State below).

A clean conventional-commit history would be roughly:

1. `feat(bench): add native + browser bench harness with determinism hashes`
2. `perf(emulator): zero-copy framebuffer via memory view`
3. `perf(build): enable WASM SIMD via target-feature`
4. `perf(emulator): zero-copy audio samples via memory view`
5. `chore(server): COOP/COEP headers for SharedArrayBuffer support`
6. `feat(cpu): per-opcode execution counter for hot-path profiling`
7. `docs: PHASE_B_PLAN integration spec`

### The determinism contract

The most important infrastructure piece. Both the native bench
(`cargo run --release --bin bench rom/smw.smc`) and the browser bench
(`cd bench && node bench-cli.js --frames 600 --label foo --path zero-copy`)
emit a JSON object containing:

- `final_fb_hash` — FNV-1a 64-bit hash of the framebuffer after frame 600
- `final_audio_hash` — FNV-1a 64-bit hash of all audio samples consumed

For SMW (`rom/smw.smc`) × 600 frames at default reset state:

| | |
|---|---|
| FB hash | `54b3eed74f9f8432` |
| Audio hash | `62300ecfc4da23e0` |

**Any code change that doesn't intentionally alter emulator semantics MUST
preserve both hashes.** The compare tool (`bench/compare.js`) prints a
clear ✓ / ✗ for each. Use this as a circuit breaker.

The hashes are bit-identical across native (x86_64) and browser (wasm32 in
Chromium). Cross-platform determinism is part of the contract.

### Phase A perf wins shipped (in main working tree, uncommitted)

Cumulative vs original baseline, browser bench × 600 frames × SMW:

| Metric | Original | After Phase A | Δ |
|---|---|---|---|
| Frame mean | 1864.83 µs | 1764.82 µs | -5.4% |
| Frame P50 | 2285 µs | 2140 µs | -6.3% |
| Frame max (tail) | **7255 µs** | **3680 µs** | **-49.3%** |
| Emulated FPS | 536.2 | 566.6 | +5.7% |
| WASM size | 109 KB | 125 KB | +14% (SIMD code) |
| Cold load | 14 ms | 22 ms | +56% (one-time cost) |

Tail latency is the headline. Mean improvement is real but modest; the felt
experience is dominated by the elimination of GC-pause stutters.

### Profiling finding (the actual punch line)

Running the native bench dumps an opcode histogram to stderr. For SMW:

```
CPU opcode histogram (top 5):
  rank  op   name      count        share   cumulative
     1  F0   BEQ        3,455,231   30.56%   30.56%   ← polling loop
     2  A5   LDA        3,454,342   30.55%   61.11%   ← polling loop
     3  D0   BNE          691,442    6.12%   67.23%
     4  CD   CMP          447,760    3.96%   71.19%
     5  CA   DEX          185,239    1.64%   72.83%
```

**Two opcodes account for 61% of all CPU dispatches in a tight LDA→BEQ
busy-wait loop.** This is a much bigger optimization opportunity than
anything Phase A touched. See task #10 below — idle-loop detection.

Real emulators (bsnes, snes9x, Mesen, ares) all do this. Plausible 10-100×
speedup on the polling fraction alone.

---

## Worktree state (parallel agent results from 2026-05-01)

```
git worktree list:
  rust-wasm-snes/                       main                       ← uncommitted Phase A work
  rust-wasm-snes-task11/                task11-worker-scaffold     ← Phase B Step 1: Web Worker scaffold
  rust-wasm-snes-task16/                task16-bench-readme        ← bench/README.md (270 lines)
  rust-wasm-snes-task17/                task17-ci-hash-gate        ← .github/workflows/bench.yml
  rust-wasm-snes-task18/                task18-save-states         ← src/snapshot.rs + bin/snapshot_test.rs
```

All four worktree branches are at commit `aef377b` (the same commit as `main`)
because nothing has been committed yet. The agents did their work but couldn't
commit cleanly without the Phase A files being part of the base commit.

**Suggested merge order** once Phase A is committed to main:
1. `task18-save-states` (most validated; round-trip preserves FB hash)
2. `task16-bench-readme` (docs only, low risk)
3. `task17-ci-hash-gate` (CI workflow; enforces determinism contract on every push)
4. `task11-worker-scaffold` (Phase B foundation; should be tested manually first)

---

## Pending tasks

These were tracked in the previous session's task list. They're listed here
in approximate priority order so you can suggest them to Nick.

### High-impact, well-bounded

**T10 — Idle-loop detection in 65816** (design phase complete 2026-05-13;
implementation pending. See `docs/T10_IDLE_LOOP_DETECTION.md`.)
- Detect tight LDA→BEQ polling loops; skip CPU cycles forward to next event
- **Framing correction from prior plan**: no current SNES emulator (bsnes,
  snes9x, Mesen2, ares) does this. snes9x had it pre-1.50 but removed it due
  to SA-1/DSP-1 bugs. Native C++ at 3GHz doesn't need it; WASM at our pace
  does. Useful prior art is mGBA, not SNES.
- Tier 1 design: detect `A5 xx F0 FD` pattern only (~80 LOC), pure-memory
  address gate via `bus.is_pure_memory()`, skip to end-of-scanline minus
  safety margin, explicit APU `catch_up` for skipped cycles (most likely
  failure mode if missed)
- Behind a Cargo feature flag for the first PR
- Validate via determinism hashes on SMW + Zelda 3 + F-Zero + Super Metroid;
  must remain `54b3eed74f9f8432` / `62300ecfc4da23e0` for SMW
- Expected 10-30% wall-clock win on browser bench (not 10-100× — that was
  speculative)
- Sibling repo `/Users/nick/git/experiments/alexar-the-kidd/alex-kidd-hack`
  has a related task (T14) that closes a 14-year-old jsSMS audio loop

**T14 — Apply ring-buffer + AudioWorklet fix to alex-kidd-hack** (in progress
on branch `alex-kidd-audio-fix` in alexar-the-kidd repo)
- Agent rewrote `writeAudio()` in `alex-kidd-hack/src/ui.js` (~95 lines)
- Server running on http://localhost:8088 last we checked; needs subjective
  audio verification by Nick (open `/alexar.html`, Start, Enter, listen)
- Honors jsSMS Issue #1 from 2012

### Phase B — architectural step-change (sequential)

**T11 → T12 → T13 → T15** — see `PHASE_B_PLAN.md` for the full integration spec
- T11: Worker scaffold (DONE in worktree task11-worker-scaffold; visual test pending)
- T12: SharedArrayBuffer for framebuffer (depends on T11)
- T13: AudioWorklet + audio ring SAB (depends on T12) — **the fix that
  addresses the original "audio is fucked up" question**
- T15 (optional): OffscreenCanvas — paint in Worker thread

### Infrastructure

**T16 — Document the bench harness** (DONE in worktree task16-bench-readme,
270-line `bench/README.md`)

**T17 — CI hash gate** (DONE in worktree task17-ci-hash-gate, blocked on
Phase A being committed; agent flagged this explicitly)

### Future enhancement

**T18 — Save states via WASM linear memory snapshot** (DONE in worktree
task18-save-states; ~484KB snapshot format, snapshot+restore validated
to preserve FB hash on both SMW and Zelda 3)

---

## Quick reference: how to validate any change

```bash
# Native (fast, no browser)
cargo build --release --bin bench
./target/release/bench --frames 600 rom/smw.smc 2>&1 | tail -5

# Browser (slower; needs WASM rebuild)
wasm-pack build --target web
cd bench && node bench-cli.js --frames 600 --label foo --path zero-copy > foo.json
node compare.js baseline-with-audio-browser.json foo.json
```

If hash check shows `✓ framebuffer hash UNCHANGED` AND `✓ audio hash UNCHANGED`,
your change is semantics-preserving. If either shows `✗`, either it's an
intentional behavioral fix (rare) or a regression (much more common — investigate).

---

## Notes on prior session character

- Nick was excited and energetic in the prior session — direct ("let's
  goooooo"), comfortable pushing back on framing, valued rigor + measurement
- Profile mismatch caught a real config bug: `web/rom/zelda3.smc` is a
  symlink to `../../rom/smw.smc`, so the project name doesn't match what the
  browser actually loads. Bench uses SMW for both native and browser to keep
  things apples-to-apples.
- The session pattern that worked well: pushback on flawed framing, scope
  honestly, build measurement infrastructure first, then iterate optimizations
  with the hash as a circuit breaker.

---

## When in doubt

- Read `PHASE_B_PLAN.md` for the architectural target
- Read `bench/README.md` (after Phase A is committed) for the harness
- Run the bench (`cargo run --release --bin bench rom/smw.smc`) — it tells you
  what state the emulator is in via the histogram + hashes
- The reference values for SMW × 600 frames are sacred: `54b3eed74f9f8432`
  (FB), `62300ecfc4da23e0` (audio). If they change without intent, stop.
