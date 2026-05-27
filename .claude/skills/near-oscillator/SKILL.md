---
name: near-oscillator
argument-hint: "[scope: 'apu-freq', 'video-rate', 'interlace', 'determinism', 'full']"
description: >
  Verify oscillator frequency accuracy against Near/byuu's articles. Near
  emphasizes that the SNES APU ceramic resonator runs at ~32040 Hz (not 32000),
  the NTSC refresh rate is ~60.0985 Hz (not 60), and interlace adds a half-line
  changing the rate to ~59.984 Hz. This skill audits frequency constants,
  determinism implications, and the interaction with DRC.
---

# Near Oscillator Compliance (`/near-oscillator`)

Single-pass audit of oscillator frequency handling.

## Reference

From `audio/dynamic-rate-control/README.md`:

Near on frequencies:
- CPU: "315 / 88 * 6000000hz, or approximately ~21.477MHz"
- APU: "32000 * 768hz, or approximately ~24.576MHz" (spec)
- But: "most observations place SNES APU oscillators to be closer to
  32040 * 768, or ~24.607MHz, in practice"
- Video: "315 / 88 * 6000000 / 1364 / 262 = 60.098477561hz"
- Interlace: "315 / 88 * 6000000 / 1364 / 525 * 2 = 59.9840042665hz"

Near on oscillator inaccuracy: "The age of the oscillator, its manufacturing
run, and even the current temperature can slightly affect the oscillator rate."

From `game-bugs/snes/README.md`:

Near on Super Bonk: "happens due to the natural variance in the SNES CPU and
APU oscillators."

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

Single-pass oscillator audit. Report inline.

## Checks

### 1. APU Sample Rate

The emulator generates DSP samples at a rate determined by:
  master_cycles_per_frame / 21 / 32

With master_cycles_per_frame = 262 × 1364 = 357,368:
  357,368 / 21 / 32 = 531.8 samples/frame
  531.8 × 60.0985 fps = 31,961 Hz

But real hardware generates at ~32,040 Hz (measured). The difference (~0.25%)
means our emulator produces slightly fewer samples per second than real
hardware.

Verify:
- What sample rate does the code assume? (32000? 32040? Derived from cycles?)
- Is the rate documented?
- Does DRC compensate for the shortfall?

### 2. CPU Frequency

SNES CPU: 315/88 × 6,000,000 = 21,477,272.727... Hz

The emulator likely uses integer cycle counts per scanline:
  1364 master cycles per scanline (non-interlace)
  262 scanlines per frame (NTSC)

Verify these constants in the frame loop.

### 3. Video Refresh Rate

NTSC: 21,477,272 / 1364 / 262 = 60.0985 Hz (not exactly 60)
PAL: 21,477,272 / 1364 / 312 = 50.0070 Hz (not exactly 50)

In the browser, requestAnimationFrame typically fires at 60Hz (display refresh).
The emulator runs at ~60.0985 Hz emulated time but is paced at ~60 Hz real time.

Verify:
- The frame loop produces frames at the SNES rate, not the host rate
- Any drift between SNES rate and host rate is absorbed by DRC (audio) and
  rAF timing (video)

### 4. Interlace Timing

With interlace enabled, odd frames have 263 scanlines (extra half-line).
This changes the effective refresh rate to ~59.984 Hz.

Verify:
- Interlace support exists (or is documented as not implemented)
- If implemented, the extra scanline is present on odd frames only
- The frame timing adjusts accordingly

### 5. Master-to-SPC Divisor

The conversion from master cycles to SPC cycles uses integer division by 21:
  21,477,272 / 1,024,000 = 20.97 ≈ 21

This 0.14% rounding error means the APU runs slightly slower than real hardware.
Over 600 frames (10 seconds), this is:
  600 × 262 × 1364 / 21 = 10,210,514 SPC cycles (emulated)
  600 × 262 × 1364 / 20.97 = 10,225,123 SPC cycles (real)
  Difference: 14,609 SPC cycles ≈ 14ms of audio

Verify:
- The divisor of 21 is documented
- The cumulative error is understood
- DRC absorbs this error (it does — this is exactly what DRC is for)

### 6. Determinism vs Realism

Our emulator is fully deterministic (fixed oscillator ratio). Real SNES
hardware has drift and variance between units.

Document:
- Which games are affected by oscillator variance (Super Bonk)
- Whether our fixed ratio produces "always works" or "always fails" for each
- Whether configurable jitter is desirable (probably not — determinism is
  more valuable for TAS and debugging)

## Report

For each check: PASS / FAIL / PARTIAL.
Note that most of these are documentation issues, not bugs — the emulator's
frequencies are close enough for correct emulation, but DRC must absorb the
remainder.
```
