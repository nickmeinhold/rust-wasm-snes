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

A Rust SNES emulator targeting Zelda 3 (LTTP), SMW, and a growing list of
other titles. Compiled to WASM, runs in the browser. See `README.md` for the
project framing — particularly the audio-verified-against-reference and
trace-oracle-debugging sections.

This file (`CLAUDE.md`) is purely for **session continuity**. It tracks what's
in flight, not what the project *is*.

---

## State of play (as of 2026-05-14)

### What's shipped on `main`

Phase A foundation is done. Phase B Step 1 is done. Most of the infrastructure
the previous prior-session session listed as "uncommitted" is now committed and
hash-validated.

Recent main log:

```
61d7f10  feat(cpu): T10 idle-loop fast path behind `idle-skip` feature (#16)
c64abab  docs(T10): idle-loop detection design + corrected framing (#15)
2a26710  ci: add cargo-check job that runs without ROM secret (#14)
c1f2381  fix: remove duplicate [[bin]] bench from Cargo.toml + unused helpers (#13)
f427744  feat(web): Phase B Step 1 — Web Worker emulator scaffold (#7)
cf9a4e3  ci(bench): determinism hash gate + opcode counters (#9)
73276d0  docs(bench): README for the bench harness (#8)
a0af77c  feat(snapshot): save-state support (#10)
8ddd1b3  Merge Phase A foundation (#11)
```

The Phase A foundation merge (`#11`) carries five story-driven commits:
gitignore noise, bench harness + hash contract, zero-copy fb/audio + SIMD +
opcode counters, web bench/compare pages + COOP/COEP, docs.

### The determinism contract (sacred)

For SMW × 600 frames at default reset state:

| | |
|---|---|
| `final_fb_hash` | `54b3eed74f9f8432` |
| `final_audio_hash` | `62300ecfc4da23e0` |

Bit-identical across native (x86_64) and browser (wasm32 in Chromium).
**Any code change that doesn't intentionally alter emulator semantics MUST
preserve both hashes.** `bench/compare.js` prints clear ✓/✗ for each.

Validate locally:
```bash
cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
```

### CI status (be aware: gate is partially skipped)

`.github/workflows/bench.yml` runs two jobs:

1. **`cargo-check`** (added in #14) — runs unconditionally on every push/PR.
   `cargo check --all-targets --locked`. Catches manifest errors, type errors,
   lockfile drift. This job is actively enforcing.

2. **`bench-hash-gate`** — runs the native bench against SMW and asserts the
   contract hashes. **Currently always skips** because the `SMW_ROM_B64` secret
   isn't set. (Tried setting it — 422 "Value is too large", GH Actions secrets
   max at 48KB; SMW base64 is ~683KB.) So the contract is enforced locally
   only. See `docs/OPEN_TASKS.md` for candidate fixes.

### Browser Phase B prerequisite stack (verified 2026-05-13)

`web/serve.py` on port 8090 with COOP/COEP headers gives:
- `crossOriginIsolated === true` ✓
- `SharedArrayBuffer` available ✓
- `Atomics` available ✓

Smoke-tested via headless Chromium against `web/index-phase-b.html`:
emulator runs in the worker, frame counter advances, ROM loaded ("SUPER
MARIOWORLD"), zero page errors. PR #7's worker scaffold genuinely works —
not just hash-equivalent.

**Watch out**: if `lsof -i :8090` shows a non-`serve.py` Python process, kill
it. A `python -m http.server` (without COOP/COEP) silently disables SAB for
browser sessions. Task in `docs/OPEN_TASKS.md` to make `serve.py` warn about
this.

### T10 idle-loop detection — landed behind a feature flag

`docs/T10_IDLE_LOOP_DETECTION.md` has the full design + implementation
findings. Status:

- **Tier 1 implementation in tree behind `idle-skip` Cargo feature, default OFF.**
- **CPU semantics correct.** With feature on + capped to one skip, fb_hash is
  bit-identical to reference. Verified empirically.
- **Audio diverges.** Even with chunk-simulated catch_up, audio hash drifts.
  Root cause: `Apu::run_cycles` cycle-debt mechanism (`src/spc700/mod.rs:281`)
  is not chunk-equivalent — different chunk sequences delivering identical
  total cycles produce different SPC instruction-boundary timing.
- **Framing correction from the 2026-05-01 plan:** no SNES emulator currently
  does idle-loop detection (verified by direct grep across bsnes, snes9x,
  Mesen2, ares). snes9x had it pre-1.50 but tore it out due to SA-1/DSP-1 bugs.
  The useful prior art is mGBA, not SNES.
- **Perf when on:** native bench 575 → 626 emulated_fps (+8.9%), 88K hits
  per 600-frame run, 52% of master cycles fast-forwarded.
- **Bonus correction to the design doc:** Tier 1 pattern is `A5 xx F0 FC`
  (offset −4), not `F0 FD` (−3) as originally written. The emulator's
  `relative8` in `addressing.rs:184` uses PC-after-fetch as the branch base.

Four candidate fix paths in `docs/T10_IDLE_LOOP_DETECTION.md` §8 (and
`docs/OPEN_TASKS.md`). T13 (AudioWorklet) likely fixes the chunking blocker
as a side effect.

### Phase A perf wins (still valid)

Cumulative vs original baseline, browser bench × 600 frames × SMW:

| Metric | Original | After Phase A | Δ |
|---|---|---|---|
| Frame mean | 1864.83 µs | 1764.82 µs | -5.4% |
| Frame P50 | 2285 µs | 2140 µs | -6.3% |
| Frame max (tail) | **7255 µs** | **3680 µs** | **-49.3%** |
| Emulated FPS | 536.2 | 566.6 | +5.7% |
| WASM size | 109 KB | 125 KB | +14% (SIMD) |

Tail latency is the headline — the felt experience is dominated by the
elimination of GC-pause stutters.

### Profiling finding (still the punch line)

Native bench dumps a CPU opcode histogram for SMW:

```
rank  op   name      count        share   cumulative
   1  F0   BEQ        3,455,231   30.56%   30.56%
   2  A5   LDA        3,454,342   30.55%   61.11%
```

Two opcodes = 61% of dispatches in a tight LDA→BEQ polling loop. T10 attacks
this; see status above.

---

## Repo state

- Single worktree at `rust-wasm-snes/` on `main`. No `task*` worktrees.
- Local branches: just `main` (and `fix/irq-hblank-gaussian` pre-existing).
- Remote branches: same.
- `bench/node_modules/` is vendored in main history (~3 MB Playwright). See
  `docs/OPEN_TASKS.md` for the keep-vs-gitignore decision.

---

## Pending tasks

**Full list lives in `docs/OPEN_TASKS.md`** — the in-session `TaskCreate` queue
is session-scoped and won't survive into the next session. The markdown file
is what survives.

Highlights for prioritisation:

- **T13 — AudioWorklet** is the headline fix per the original project
  framing ("audio is fucked up"). It depends on T12 and also likely fixes
  the SPC chunking blocker that gates default-on T10. Natural next target.
- **T12 — SharedArrayBuffer for framebuffer** — Phase B Step 2. Unblocked
  by the verified browser prerequisite stack.
- **T10 fix (refactor `run_cycles`)** — principled fix to the chunking issue;
  multi-session. T10 fix alt (PCA re-baseline) is the pragmatic shortcut.
- **Playwright smoke test as a CI job** — would catch worker/SAB regressions
  the hash gate can't see.

Three policy decisions are also parked in `OPEN_TASKS.md`: bench/node_modules,
CLAUDE.md public/local, admin-bypass-as-default. Nick's calls; not technical.

---

## Quick reference: how to validate any change

```bash
# Native (fast, no browser)
cargo run --release --bin bench rom/smw.smc 2>&1 | tail -5

# Browser (slower; needs WASM rebuild + serve.py running)
wasm-pack build --target web
python3 web/serve.py &        # NOT `python -m http.server` — must be serve.py
cd bench && node bench-cli.js --frames 600 --label foo --path zero-copy > foo.json
node compare.js baseline-with-audio-browser.json foo.json
```

If hash check shows `✓ framebuffer hash UNCHANGED` AND `✓ audio hash UNCHANGED`,
your change is semantics-preserving. If either shows `✗`, either it's an
intentional behavioral fix (rare) or a regression (much more common).

If you're touching Phase B browser code: confirm `crossOriginIsolated === true`
in DevTools before you blame the worker code.

---

## How sessions tend to run on this project

- Nick has been comfortable running long autonomous sessions (e.g. 2026-05-12→13,
  9 PRs merged via admin-bypass). Whether that's the default going forward is a
  decision parked in `docs/OPEN_TASKS.md`.
- Bench hashes as a circuit breaker is the load-bearing safety net. Trust it
  more than you trust the optimization you just wrote.
- The hardest bugs in this codebase historically aren't found by reading code
  — they're found by diffing execution traces against Mesen2. `cargo build
  --features trace` plus `reference/diff_trace.py` is the workflow.

---

## When in doubt

- Read `PHASE_B_PLAN.md` for the architectural target
- Read `bench/README.md` for the harness
- Read `docs/T10_IDLE_LOOP_DETECTION.md` for the idle-loop work
- Read `docs/OPEN_TASKS.md` for the prioritised queue
- Run the bench (`cargo run --release --bin bench rom/smw.smc`) — it tells you
  what state the emulator is in via the histogram + hashes
- The reference values for SMW × 600 frames are sacred: `54b3eed74f9f8432`
  (FB), `62300ecfc4da23e0` (audio). If they change without intent, stop.
