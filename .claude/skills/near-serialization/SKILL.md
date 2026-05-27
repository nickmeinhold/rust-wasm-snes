---
name: near-serialization
argument-hint: "[scope: 'round-trip', 'format', 'determinism', 'full']"
description: >
  Read-only audit of save state serialization against Near/byuu's cooperative
  serialization article. Writes findings to serialization.tmp. Intended to be
  run as part of /near-sweep1 (parallel with near-scheduler, near-oscillator,
  near-jit-sync), whose fix agent reads all .tmp files.
---

# Near Serialization Compliance (`/near-serialization`)

Read-only audit. Writes `serialization.tmp`. Does NOT modify code.

## Reference

From `design/cooperative-serialization/README.md`:

Near's Method 1 (fast sync): "The above code breaks determinism by allowing the
CPU to call bus.read(PC) even though it might be ahead of the APU."

Near on determinism: "Imagine you have a pre-recorded sequence of controller
inputs to play back... That small action may cause a very slight
desynchronization... in a domino-like effect."

Near's experience: "I have never had a single report from a user where a manual
save state (using methods 1 and 2) have resulted in failure."

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing save state serialization for an SNES emulator. READ-ONLY.
Write exactly one file: `serialization.tmp`.

Scope: $ARGUMENTS (default: full)

## Steps

### 1. Catalog Serialized State

Read the snapshot/save state code (likely `src/snapshot.rs` or similar).
Build a table of every field that is serialized:

| Component | Field | Type | Serialized? | Notes |
|-----------|-------|------|-------------|-------|

Pay special attention to:
- `Apu.cycle_target: u64` — absolute, monotonic. Must be preserved exactly.
- `Apu.cycle_frac` — fractional accumulator. Must be preserved.
- `Bus.master_clock: u64` — must be preserved for JIT sync.
- `Bus.last_apu_sync: u64` — must be preserved to prevent double-crediting.
- `Cpu.cycles` — current CPU cycle count.
- DSP state (echo buffer position, envelope state, BRR decode position).
- Timer counters and dividers.

### 2. Check for Missing State

Compare serialized fields against all mutable state in each component.
Any mutable field NOT serialized is a potential desynchronization source.

Focus on:
- SPC700 timer dividers and counters
- DSP voice state (envelope phase, BRR block position, pitch counter)
- DMA channel state (mid-transfer position, HDMA line counter)
- IRQ/NMI pending flags
- Joypad auto-read state

### 3. Round-Trip Determinism

Assess whether serialize→unserialize→run(N) would produce identical output to
run(N) without the save/load cycle.

Check:
- Does unserialize recreate ALL state, or does it rely on implicit state
  (e.g., DSP filter history initialized to zero)?
- Are there any one-time initialization paths that won't re-run after
  unserialize?
- Is the audio output filter state serialized? (It has history buffers.)

### 4. Format Migration

Check for snapshot VERSION handling. The HANDOVER mentions VERSION 2 for the
cycle_target format change (previously cycle_debt). Verify:
- VERSION 2 correctly reads cycle_target
- VERSION 1 states are either rejected or migrated
- The version number is checked before deserialization

### 5. Determinism Under Rapid Save/Load

For run-ahead, the system must serialize/unserialize 60 times per second.

Assess:
- Performance: how large is the serialized state?
- Allocation: does serialize allocate? Per-frame allocation = GC pressure.
- Side effects: does serialize or unserialize modify any state?

### 6. Write serialization.tmp

Include:
- Complete field inventory (serialized vs missing)
- Round-trip determinism assessment
- Format migration correctness
- Run-ahead readiness assessment
- Near's method classification (which method does our approach correspond to?)
- Recommended fixes (numbered, with effort S/M/L)
```
