---
name: near-hw-variance
argument-hint: "[scope: 'wram', 'dsp-init', 'oscillator', 'full']"
description: >
  Verify hardware variance modeling against Near/byuu's game bugs article. Near
  documents SNES games that break due to non-deterministic hardware: WRAM
  initialization (Dirt Racer, Hurricanes), DSP register initialization (Magical
  Drop), and oscillator variance (Super Bonk). An accurate emulator must support
  configurable initialization (zeroed, randomized, patterned) and document its
  determinism contract. Single-pass audit.
---

# Near Hardware Variance Compliance (`/near-hw-variance`)

Single-pass audit of hardware variance modeling.

## Reference

From `game-bugs/snes/README.md`:

- **Dirt Racer**: "will freeze on the startup splash screen sometimes when SNES
  WRAM contains zero bits in certain locations. SNES WRAM is non-deterministic
  at startup."

- **Hurricanes**: "caused by the game not initializing WRAM before transferring
  data into VRAM for BG2. If an emulator randomizes RAM at startup as real
  hardware would, this bug will be seen."

- **Magical Drop**: "caused by the game not initializing the SNES DSP registers
  at startup. The register values are non-deterministic at SNES startup."

- **Super Bonk**: "happens due to the natural variance in the SNES CPU and APU
  oscillators."

Near on determinism: "emulators are deterministic in nature (for the sake of
tool-assisted speedruns and bug reproducibility), this bug is likely to either
always occur or never occur under emulation."

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

Single-pass audit. Report inline, no .tmp file.

## Checks

### 1. WRAM Initialization

Find where WRAM (128KB, banks $7E-$7F) is initialized. Verify:
- Default: zeroed (most compatible, hides bugs)
- Option: randomized (hardware-accurate, exposes Dirt Racer / Hurricanes bugs)
- Option: patterned (e.g., $55/$AA, useful for debugging uninitialized reads)
- Is the initialization mode configurable?

### 2. DSP Register Initialization

Find where the S-DSP registers are initialized (likely in `src/spc700/dsp.rs`).
Verify:
- All 128 registers have defined initial values
- The initial values match hardware power-on state (mostly $00, but some
  registers like ENDX ($7C) may have non-zero defaults)
- Magical Drop's bug: PITCH and ENVX at wrong values cause soft-lock.
  Is there a way to reproduce this?

### 3. APU RAM Initialization

The SPC700 has 64KB of RAM, separate from WRAM. Verify:
- Initialized to known state (IPL ROM is loaded at $FFC0-$FFFF)
- Remaining RAM initialization matches hardware (typically $00 on power-on,
  but some SNES models differ)

### 4. Oscillator Variance

Near notes CPU and APU oscillators drift. Our emulator uses a fixed ratio
(master cycles / 21 for SPC conversion). Verify:
- The ratio is documented
- The determinism implication is documented (Super Bonk attract: always works
  or always fails, never intermittent)
- If oscillator jitter is ever desired (e.g., for TAS comparison), the
  architecture supports it

### 5. Determinism Contract

Verify the emulator documents:
- Same ROM + same inputs + same initial state = same output (always)
- What "initial state" means (WRAM, DSP regs, APU RAM, oscillator ratio)
- The sacred hashes are part of this contract

## Report

For each check: PASS / FAIL / PARTIAL.
List games that would be affected by each finding.
```
