---
name: near-sweep2
description: >
  Near compliance sweep 2: Audio & Input. Runs 4 read-only audit agents in
  parallel (near-drc, near-auto-joypad, near-input-latency, near-run-ahead),
  each writing a .tmp file. Then a single fix agent reads all 4 .tmp files and
  applies fixes. Covers: dynamic rate control, auto-joypad timing (12 games
  broken), input latency, and run-ahead feasibility.
---

# Near Compliance Sweep 2: Audio & Input

Orchestrator. You launch 4 read-only agents in parallel, wait for all to
complete, then launch 1 fix agent.

## Phase 1: Parallel Read-Only Audits

Launch ALL FOUR agents simultaneously. Each writes one .tmp file.

### Agent 1: near-drc → `drc.tmp`

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing Dynamic Rate Control for an SNES emulator's AudioWorklet.
READ-ONLY. Write exactly one file: `drc.tmp`.

## Checks

### 1. Formula Correctness
Find the DRC implementation (likely `web/audio-worklet-processor.js`). Verify
it matches Near's formula:
  dynamicFrequency = ((1.0 - maxDelta) + 2.0 * fillLevel * maxDelta) * baseFreq
Where maxDelta = 0.005, fillLevel = 0.0-1.0, targeting 0.5.
Verify the direction: empty buffer → speed up production, full → slow down.

### 2. Base Frequency
Near says real SNES APU runs at ~32040 Hz, not 32000 Hz. Verify which value
the emulator uses and whether DRC compensates for the ~0.25% shortfall from
the master_cycles/21/32 derivation (~31,961 Hz).

### 3. Ring Buffer
Read the SharedArrayBuffer ring buffer. Verify: size, Atomics usage for
read/write pointers, wrap-around handling, fill level calculation.

### 4. Video-Sync Priority
Near says DRC assumes video sync is primary. Verify the browser implementation
uses requestAnimationFrame as the timing source.

### 5. Pitch Distortion Bound
With maxDelta=0.005, max shift is ±0.5%. Verify no code path allows larger.

Write all findings to `drc.tmp` with PASS/FAIL/NOT_IMPLEMENTED per check.
```

### Agent 2: near-auto-joypad → `auto-joypad.tmp`

Use the audit agent prompt from `.claude/skills/near-auto-joypad/SKILL.md`.

### Agent 3: near-input-latency → `input-latency.tmp`

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing input latency for an SNES emulator. READ-ONLY.
Write exactly one file: `input-latency.tmp`.

## Checks

### 1. Polling Location
Trace from the frame loop through joypad handling to the WASM/JS boundary.
Is input polled at frame start (bad — adds 16ms) or on-demand when the game
reads controller registers (good — Near's JIT polling)?

### 2. Browser Input Model
Map how keyboard/gamepad events reach the Rust emulator. Document any
frame-of-latency between JS read and Rust use.

### 3. Overscan Interaction
Does the frame loop handle both NTSC (V=225) and PAL (V=240) correctly?

### 4. Auto-Joypad vs Manual Poll
SNES reads input two ways: auto-joypad ($4218-$421F at VBlank) and manual
($4016/$4017 bit-serial). Verify both paths exist.

### 5. Latency Budget
Document total input-to-display chain: browser event → JS → WASM → emulated
read → frame render → canvas display. Estimate total ms.

Write all findings to `input-latency.tmp` with PASS/FAIL/PARTIAL per check.
```

### Agent 4: near-run-ahead → `run-ahead.tmp`

```
First, read ~/.claude/AGENT.md for instructions.

You are assessing run-ahead feasibility for an SNES emulator. READ-ONLY.
Write exactly one file: `run-ahead.tmp`.

## Checks

### 1. Serialization Speed
Run-ahead requires serialize+unserialize every frame (60Hz). Assess:
state size, allocation-free?, estimated time.

### 2. Round-Trip Determinism
serialize→unserialize→run(1 frame) must be bit-identical. Cross-reference
with serialization audit findings if available.

### 3. Frame-Skip Capability
Can the PPU be disabled for one frame (skip video generation)? Can audio
be suppressed for intermediate frames? Near says frame-skip reduces overhead
from 100% to ~40% per extra frame.

### 4. Performance Headroom
Current: ~1.7ms/frame (WASM). Budget: 16.6ms. Headroom: ~15ms.
Estimate maximum practical run-ahead level.

### 5. Audio Handling
Only the last frame's audio should be output. Can sample_buffer be
temporarily suppressed?

Write all findings to `run-ahead.tmp` with READY/NEEDS_WORK/BLOCKED per check.
```

## Between Phases

After all 4 agents return:
1. Confirm all .tmp files exist: `drc.tmp`, `auto-joypad.tmp`,
   `input-latency.tmp`, `run-ahead.tmp`
2. Briefly summarize findings to the user
3. Proceed to Phase 2

## Phase 2: Fix Agent

Launch ONE agent that reads all 4 .tmp files.

```
First, read ~/.claude/AGENT.md for instructions.

You are the fix agent for Near compliance sweep 2 (Audio & Input).
Read these 4 audit files:
- drc.tmp
- auto-joypad.tmp
- input-latency.tmp
- run-ahead.tmp

Apply fixes in priority order.

CRITICAL: After ALL changes, verify determinism:
  cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
Expected: fb=54b3eed74f9f8432, audio=62300ecfc4da23e0

NOTE: Auto-joypad fixes WILL change hashes (the current implementation is
known to be wrong). Document the new hashes.

## Fix Priority

1. **Auto-joypad timing** — the #1 bug. 12 games broken. Implement:
   - `auto_joypad_counter: u32` on Bus, set to 4224 at VBlank start
   - Decrement per master cycle, $4212 bit 0 = 1 while counter > 0
   - $4218-$421F latched only when counter reaches 0
   This WILL change hashes. The new hashes are more accurate.

2. **DRC formula** — verify or fix the AudioWorklet implementation.
   If the base frequency is wrong (32000 vs 32040), fix it.

3. **Input latency** — if input is polled at frame start, move to on-demand.

4. **Run-ahead** — document feasibility, don't implement yet.

## Tests to Write

Place in `tests/near_sweep2.rs`:

- `hvbjoy_busy_during_polling`: $4212 bit 0 = 1 for 4224 cycles after VBlank
- `hvbjoy_clear_after_polling`: $4212 bit 0 = 0 after 4224 cycles
- `joypad_regs_valid_after_polling`: $4218-$421F correct after busy clears
- `polling_starts_at_vblank`: Timing aligned to scanline 225

## Cleanup

Delete all 4 .tmp files. Report:
- Fixes applied
- Hash changes (expected for auto-joypad)
- Tests written
- Run-ahead readiness verdict
```
