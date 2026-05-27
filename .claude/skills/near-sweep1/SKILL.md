---
name: near-sweep1
description: >
  Near compliance sweep 1: Threading & Timing. Runs 4 read-only audit agents
  in parallel (near-scheduler, near-oscillator, near-jit-sync, near-serialization),
  each writing a .tmp file. Then a single fix agent reads all 4 .tmp files and
  applies fixes. Covers: scheduler design, oscillator frequencies, JIT sync
  compliance, and save state correctness.
---

# Near Compliance Sweep 1: Threading & Timing

Orchestrator. You launch 4 read-only agents in parallel, wait for all to
complete, then launch 1 fix agent that reads all findings.

## Phase 1: Parallel Read-Only Audits

Launch ALL FOUR agents simultaneously using the Agent tool. Each writes one
.tmp file. All are read-only.

### Agent 1: near-scheduler → `scheduler.tmp`

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing the scheduler of an SNES emulator against Near/byuu's
specification. READ-ONLY. Write exactly one file: `scheduler.tmp`.

## Checks

### 1. Frequency Accuracy
Read the master-to-SPC conversion in `Apu::catch_up()`. The divisor should be
~21 (from CPU 21,477,272 Hz / APU 1,024,000 Hz ≈ 20.97). Verify the divisor
and document the rounding error.

### 2. Overflow Safety
Verify `master_clock: u64`, `cycle_target: u64`, `last_apu_sync: u64` cannot
overflow in any realistic scenario.

### 3. Fractional Accumulator
Verify `cycle_frac` in `Apu::catch_up()`:
- Initialized to 0 on reset
- Serialized in save states
- Never exceeds 20 (must be < 21)

### 4. Relative vs Absolute Consistency
Our scheduler is a hybrid: `master_clock` (absolute), `last_apu_sync` (last
sync point), delta = difference. Verify:
- `master_clock >= last_apu_sync` always
- After sync_apu(), `last_apu_sync = master_clock`
- Idle-skip updates both correctly
- DMA cycles credited to master_clock before sync

### 5. Multi-Chip Scaling
Document that relative schedulers scale as O(n²). If SA-1/SuperFX are added,
the model needs revision.

Write all findings to `scheduler.tmp` with PASS/FAIL/PARTIAL per check and
recommended fixes with S/M/L effort.
```

### Agent 2: near-oscillator → `oscillator.tmp`

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing oscillator frequency accuracy for an SNES emulator. READ-ONLY.
Write exactly one file: `oscillator.tmp`.

## Checks

### 1. APU Sample Rate
The emulator generates DSP samples at master_cycles/21/32. With 357,368
master cycles/frame ÷ 21 ÷ 32 = 531.8 samples/frame × 60.098 fps = ~31,961 Hz.
Real hardware: ~32,040 Hz. Document the shortfall and whether DRC compensates.

### 2. CPU Frequency Constants
Verify: 1364 master cycles/scanline, 262 scanlines/frame (NTSC).

### 3. Video Refresh Rate
NTSC: 21,477,272 / 1364 / 262 = 60.0985 Hz (not 60). Document whether the
frame loop assumes exactly 60 Hz.

### 4. Interlace Timing
Odd frames should have 263 scanlines. Verify support or document as unimplemented.

### 5. Master-to-SPC Divisor
Document the ÷21 rounding and its cumulative error (~0.12%/frame, ~14ms/10sec).

### 6. Determinism vs Realism
Document that we use fixed ratios (no jitter). Note Super Bonk attract sequence
behavior under our deterministic model.

Write all findings to `oscillator.tmp` with PASS/FAIL/PARTIAL per check.
```

### Agent 3: near-jit-sync → `jit-sync.tmp`

Use the audit agent prompt from `.claude/skills/near-jit-sync/SKILL.md`.

### Agent 4: near-serialization → `serialization.tmp`

Use the audit agent prompt from `.claude/skills/near-serialization/SKILL.md`.

## Between Phases

After all 4 agents return:
1. Confirm all .tmp files exist: `scheduler.tmp`, `oscillator.tmp`,
   `jit-sync.tmp`, `serialization.tmp`
2. Briefly summarize the findings to the user (5-10 lines)
3. Proceed to Phase 2

## Phase 2: Fix Agent

Launch ONE agent that reads all 4 .tmp files and applies fixes.

```
First, read ~/.claude/AGENT.md for instructions.

You are the fix agent for Near compliance sweep 1 (Threading & Timing).
Read these 4 audit files:
- scheduler.tmp
- oscillator.tmp
- jit-sync.tmp
- serialization.tmp

Apply fixes in priority order. For each fix, assess whether it changes the
determinism hashes.

CRITICAL: After ALL changes, verify determinism:
  cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
Expected: fb=54b3eed74f9f8432, audio=62300ecfc4da23e0

If hashes change, assess whether the new output is MORE accurate before
reverting. Document any hash changes.

## Fix Priority

1. **JIT sync gaps** — missing sync_apu() on APU port access, or spurious
   sync on pure memory. These are correctness fixes.
2. **Serialization gaps** — missing fields in save state. These break
   round-trip determinism.
3. **Scheduler issues** — fractional accumulator bugs, overflow risks.
4. **Oscillator documentation** — frequency constants, rounding errors.

## Tests to Write

Place in `tests/near_sweep1.rs`:

- `apu_port_read_triggers_sync`: Reading $2140-$2143 calls sync_apu()
- `pure_memory_read_no_sync`: ROM/WRAM reads do NOT call sync_apu()
- `scanline_flush_unconditional`: End-of-scanline sync always runs
- `cycle_frac_bounded`: cycle_frac < 21 after any catch_up call
- `master_clock_monotonic`: master_clock never decreases
- `round_trip_determinism`: serialize→unserialize→run == straight run
  (if ROM available)

## Cleanup

Delete all 4 .tmp files. Report:
- Fixes applied (with file:line)
- Tests written
- Hash impact
- Remaining issues requiring Level 2 cooperative threading
```

## After Phase 2

Report to the user:
- Per-skill compliance verdicts
- Fixes applied
- Hash impact
- What remains
