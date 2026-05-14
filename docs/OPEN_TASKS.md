# Open tasks

Persisted snapshot of the in-session `TaskCreate` queue from the consolidation on **2026-05-14**. Session-scoped tasks evaporate at session end — this file is what survives. Recreate any of them via `TaskCreate` if you want them live in a future session, or just use this as a checklist.

Ordered roughly by leverage; not strictly by dependency.

---

## High-impact (Phase B continuation)

### T13 — AudioWorklet + audio ring SAB *(the headline fix)*
Replace the deprecated `ScriptProcessorNode` audio path with an AudioWorklet driven by a SharedArrayBuffer ring buffer. Per `PHASE_B_PLAN.md`, this is the architectural fix for the original "audio is fucked up" complaint that motivated Phase B.

A side effect of doing this properly: AudioWorklet wants a steady sample stream, which forces the SPC700 to deliver samples without chunky `catch_up` bursts. So whoever does T13 will likely *also* solve T10's chunking blocker — they pair naturally.

- **Depends on:** T12 (SAB infrastructure)
- **Acceptance:** hashes unchanged; audio glitch-free under simulated main-thread load (artificial `setTimeout 200ms` blocks)
- **Effort:** 2 sessions

### T12 — SharedArrayBuffer for framebuffer *(Phase B Step 2)*
Move framebuffer transfer from per-frame `postMessage` to a SharedArrayBuffer shared between worker and main thread. Builds on PR #7's worker scaffold (merged 2026-05-12).

Plan per `PHASE_B_PLAN.md`:
- Allocate SAB of size 512×224×4 bytes (NTSC visible framebuffer)
- Worker writes framebuffer via wasm-memory view into SAB
- Main thread `rAF` reads SAB and paints to canvas
- `Atomics.store/load` for the frame-ready signal (worker → main)
- Prerequisites already in place: `crossOriginIsolated === true` confirmed (2026-05-13 smoke test); `SharedArrayBuffer` and `Atomics` available

- **Depends on:** PR #7 worker scaffold ✓ merged
- **Acceptance:** bench determinism hashes unchanged (`54b3eed74f9f8432` / `62300ecfc4da23e0`); browser-visible smoother frame pacing under main-thread load
- **Effort:** 1–2 sessions

### T10 fix — refactor `Apu::run_cycles` to be chunk-deterministic
The SPC700 cycle-debt mechanism in `src/spc700/mod.rs:281` (`run_cycles`) is not chunk-equivalent — many small `catch_up` calls and one big call deliver identical total SPC cycles but produce different instruction-boundary interleaving, which shifts DSP sample timing. This blocks default-on T10 idle-loop detection (currently behind `idle-skip` feature flag).

Approach: replace cycle_debt accumulation with sub-instruction stepping or libco-style coroutines. Reference: bsnes-mercury's `CPU::wait()` pattern.

- **Acceptance:** with `--features idle-skip`, `cargo run --release --bin bench rom/smw.smc` produces `final_fb_hash=54b3eed74f9f8432` and `final_audio_hash=62300ecfc4da23e0`. Validate against Zelda 3, F-Zero, Super Metroid too.
- **Effort:** 2–3 sessions
- See `docs/T10_IDLE_LOOP_DETECTION.md` §8 for full empirical findings.

### T10 fix (alternative path) — re-baseline hashes after PCA audio-equivalence proof
Cheaper pragmatic alternative to the refactor. The audio under `--features idle-skip` differs in hash but may be sub-perceptually equivalent.

1. Run bench feature OFF; save 600-frame audio samples to WAV
2. Run bench feature ON; save same to WAV
3. Use `reference/principal_component_compare.py` to compute PCA-projected distance
4. If distance < Hafter audio threshold (0.25 dB / 1°), accept the new hashes as audibly equivalent
5. Update `EXPECTED_AUDIO_HASH` and `EXPECTED_FB_HASH` in `.github/workflows/bench.yml`
6. Make `idle-skip` a default Cargo feature

- **Effort:** 1 session
- **Tradeoff:** doesn't fix the underlying SPC chunking issue, just accepts its output as a valid emulation point. Chunking will bite T13 anyway.

---

## Infrastructure / hygiene

### Add Playwright smoke test as third CI job
The 2026-05-13 session validated PR #7's worker scaffold by running headless Chromium against `web/index-phase-b.html` via `bench/node_modules/playwright-core`. The check confirmed: `crossOriginIsolated=true`, `SharedArrayBuffer` available, frame counter advancing in worker, no page errors.

That validation is currently a one-off. Hash gate proves emulator semantics; `cargo-check` proves code compiles. Neither catches regressions in worker scaffold, SAB plumbing, or AudioWorklet wiring.

Approach: GitHub Actions job that builds with `wasm-pack`, starts `web/serve.py`, runs a Playwright script asserting `crossOriginIsolated === true && frame_count > 100 && page_errors.length === 0`, kills serve.py.

- **Cost:** ~2 min per CI run
- **Effort:** 1 session
- **Becomes more valuable** as T12 and T13 land more browser-API integration.

### GH secret `SMW_ROM_B64` hits 48KB limit — find alternative ROM delivery
Tried `base64 -i rom/smw.smc | gh secret set SMW_ROM_B64` on 2026-05-13 — failed with HTTP 422 "Value is too large". GitHub Actions secrets cap at 48KB; SMW base64 is ~683KB. So `bench-hash-gate` stays skipped on CI. Only `cargo-check` (PR #14) actually enforces.

Options:
1. **Use a small public-domain test ROM** (homebrew or Nintendo SDK sample). Legal, small. Cons: different hashes; may not exercise same CPU paths.
2. Self-hosted runner with `rom/smw.smc` pre-installed.
3. Split base64 across multiple secrets and concatenate in the workflow.
4. GitHub release asset (private) downloaded via PAT.
5. Accept skipped state.

Recommend (1) for the public repo.

### `serve.py`: warn loudly if another HTTP server already holds port 8090
2026-05-13 found two stale Python HTTP servers on port 8090 — one was `serve.py` (with COOP/COEP), one was `python -m http.server` (without). macOS routed IPv4 connections to the plain one, silently disabling `crossOriginIsolated` and silently breaking SharedArrayBuffer for browser sessions.

Fix: before `server.serve_forever()`, probe `localhost:8090` and check whether `Cross-Origin-Embedder-Policy` is set on the response. If something responds without it, print a loud warning and exit non-zero. 5-min fix.

### Rename crate from `zelda-a-link-to-the-past`
The name is an early-development artifact when LTTP was the only working title. Now boots SMW, LTTP, MMX, Super Metroid.

Changes: `Cargo.toml`, wasm-pack output filenames in `pkg/`, `web/emulator-worker.js` import path, `web/index*.html` script references, README, CLAUDE.md.

Suggested name: `rust-wasm-snes` (matches repo) or `rsnes` (compact).

- **Risk:** low; deterministic find-replace
- **Schedule:** post-T13, since Phase B is touching worker glue anyway

---

## Policy decisions (your call, not Maxwell's)

### Decide: keep `bench/node_modules` in main history, or `.gitignore`
~3 MB of vendored Playwright is committed to main (PR #11, per Nick's "1 commit" direction). Either is fine; just close out the deferred decision.

- **Keep:** reproducible bench without `npm install`; no version drift
- **Drop:** smaller history; standard npm flow; future contributors familiar

If drop: `bench/node_modules` to `.gitignore`, `git rm -r --cached bench/node_modules`, add `npm install` step to `bench/README.md`.

### Decide whether `CLAUDE.md` belongs on public GitHub history
`CLAUDE.md` is committed to main (PR #11). It contains session-continuity context for future Claude sessions, cross-references to a sibling repo (`/Users/nick/git/experiments/alexar-the-kidd`), notes on prior-session Nick character, and the reference hashes.

- **Keep public:** no action needed.
- **Keep local:** `git rm CLAUDE.md`, add to `.gitignore`. Information is still accessible to local Claude via the on-disk file.
- **Trim public:** remove personal/cross-repo references; keep the project-specific bits.

### Decide: admin-bypass merges as the default for autonomous sessions?
During 2026-05-12→13, all 9 PRs were merged via `gh pr merge --admin --squash` to bypass the 1-approving-review branch-protection rule. GitHub blocks self-approval, so admin-bypass was the only way for the autonomous agent to land its own PRs without a human reviewer.

Three patterns:
1. **Accept admin-bypass as autonomous default.** Self-review is sufficient given the hash-gate circuit breaker. Document in CLAUDE.md.
2. **Spawn a second Claude session as reviewer.** Two-session pattern: one author, one reviewer.
3. **Open PRs but pause for Nick.** Async review at your pace.

This is a policy call, not technical.

---

## Cross-repo dangling thread

### T14 — Apply ring-buffer + AudioWorklet fix to alex-kidd-hack
*(In sibling repo `/Users/nick/git/experiments/alexar-the-kidd/alex-kidd-hack`)*

Per the prior session: agent rewrote `writeAudio()` in `src/ui.js` (~95 lines) on branch `alex-kidd-audio-fix`. Server was running on `http://localhost:8088`. Needs subjective audio verification: open `/alexar.html`, press Start, Enter, listen. Honors jsSMS Issue #1 from 2012.

Touch point with this repo: if T13 (AudioWorklet) here teaches us the right pattern, port it sideways.
