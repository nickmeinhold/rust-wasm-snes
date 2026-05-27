---
name: near-scheduler
argument-hint: "[scope: 'full', 'overflow', 'fractional', 'relative-vs-absolute']"
description: >
  Verify scheduler design against Near/byuu's schedulers article. Near defines
  relative schedulers (signed i64 per chip pair, good for SNES) and absolute
  schedulers (u64 timestamps, good for complex systems). This skill audits
  frequency accuracy (CPU 21.477 MHz, APU 24.607 MHz), overflow safety,
  fractional accumulator correctness, and consistency between master_clock
  (absolute) and last_apu_sync (delta tracking).
---

# Near Scheduler Compliance (`/near-scheduler`)

Single-pass audit of the scheduler against Near's schedulers article.

## Reference

From `design/schedulers/README.md`:

Near on relative schedulers: "fast, easy, and if done right, essentially perfect
at keeping track of time. Their biggest weakness is that they only work in a
1:1 relationship."

Near on SNES frequencies: CPU = 21,477,272 Hz, SMP = 24,576,000 Hz.
The relative scheduler subtracts `N * SMP_frequency` when CPU steps,
adds `N * CPU_frequency` when SMP steps.

Near on overflow: "2^63 / 24,576,000 tells us that the CPU can advance up to
375,299,968,947 clocks ahead of the SMP before underflow. That's 17,474
seconds."

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing the scheduler of an SNES emulator against Near's specification.
This is a single-pass audit — read, assess, report inline. No .tmp file needed.

## Checks

### 1. Frequency Accuracy

Read the master-to-SPC conversion in `Apu::catch_up()`. The divisor should be
derived from the CPU/APU frequency ratio:

- CPU: 315/88 × 6,000,000 = 21,477,272.727... Hz
- APU: documented as 24,576,000 Hz (spec) but measured at ~24,606,720 Hz
  (32040 × 768). See /near-oscillator for the distinction.
- Ratio: 21,477,272 / 1,024,000 ≈ 20.97, commonly rounded to 21.

Verify: does the code use 21? Is the rounding documented? What's the
cumulative error per frame? (At 262 × 1364 = 357,368 master cycles/frame,
dividing by 21 gives 17,017.5 SPC cycles. Real hardware: 357,368 / 20.97 =
17,038. Difference: ~20 SPC cycles/frame, ~0.12%.)

### 2. Overflow Safety

Verify:
- `master_clock: u64` — at 21.5M clocks/sec, u64 overflows in ~27 billion
  years. Safe.
- `cycle_target: u64` — at 1.024M SPC cycles/sec, u64 overflows in ~570
  billion years. Safe.
- `last_apu_sync: u64` — same as master_clock. Safe.
- `cycle_frac` — must be < 21 at all times. Verify modular arithmetic.

### 3. Fractional Accumulator

The `cycle_frac` in `Apu::catch_up()` accumulates the remainder when converting
master cycles to SPC cycles via integer division by 21.

Verify:
- Initialized to 0 on reset
- Serialized in save states
- `cycle_frac = (cycle_frac + master_cycles) % 21` — or equivalent
- Never exceeds 20

### 4. Relative vs Absolute Consistency

Our scheduler is a hybrid:
- `master_clock` is absolute (monotonically increasing)
- `last_apu_sync` tracks the last absolute sync point
- The delta `master_clock - last_apu_sync` is the relative offset

Verify:
- `master_clock >= last_apu_sync` always (no negative deltas)
- After sync_apu(), `last_apu_sync = master_clock`
- Idle-skip updates both correctly
- DMA cycles are credited to master_clock before sync

### 5. Multi-Chip Scaling

Near warns relative schedulers scale as O(n²). Currently we track:
- CPU ↔ APU (via master_clock / last_apu_sync)
- CPU ↔ PPU (implicit — same dot clock, scanline-driven)

If SA-1, SuperFX, or DSP-1 are added, additional scheduler pairs are needed.
Document this as a known limitation.

## Report

For each check, report: PASS / FAIL / PARTIAL with explanation.
Recommend fixes for any failures.
```
