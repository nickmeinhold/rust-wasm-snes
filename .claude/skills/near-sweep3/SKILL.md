---
name: near-sweep3
description: >
  Near compliance sweep 3: CPU, Video & Compatibility. Runs 4 read-only audit
  agents in parallel (near-alu, near-color, near-hw-variance, near-hierarchy),
  each writing a .tmp file. Then a single fix agent reads all 4 .tmp files and
  applies fixes. Covers: ALU flag computation (SBC carry sense), RGB555
  expansion, WRAM/DSP initialization, and ROM/peripheral detection.
---

# Near Compliance Sweep 3: CPU, Video & Compatibility

Orchestrator. You launch 4 read-only agents in parallel, wait for all to
complete, then launch 1 fix agent.

## Phase 1: Parallel Read-Only Audits

Launch ALL FOUR agents simultaneously. Each writes one .tmp file.

### Agent 1: near-alu → `alu.tmp`

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing ALU operations for an SNES emulator against Near/byuu's
specification. READ-ONLY. Write exactly one file: `alu.tmp`.

## Checks

### 1. Find the ALU
Search for ADC, SBC, CMP in `src/cpu/`. The 65816 has 8-bit (M=1) and
16-bit (M=0) modes.

### 2. SBC Carry Sense (THE #1 ALU BUG)
The 65816's SBC uses carry (not borrow):
  SBC(A, operand) = ADC(A, ~operand, C)
NOT: ADC(A, ~operand, !C)  ← WRONG, that's borrow-based (Z80/x86)
Verify the carry is passed directly, not inverted.

### 3. Overflow Flag
ADC: V = (target ^ result) & (source ^ result)
SBC via ADC: automatic from ~source.
Verify both are correct.

### 4. Carry Flag
Near's formula: C = (carries ^ overflow) & sign.
For 8-bit: sign = 0x80. For 16-bit: sign = 0x8000.
Verify sign mask changes with M flag.

### 5. Decimal Mode
BCD for ADC/SBC when D flag set. Verify nibble adjustment.
V flag is undefined on 65816 in decimal mode.

### 6. 16-bit Arithmetic
When M=0: carry/overflow from bit 15, N from bit 15, Z from full 16 bits.

### 7. CMP/CPX/CPY
Subtraction with forced C=1. V flag NOT affected by CMP on 65816.

### 8. No Half-Carry
65816 has no half-carry flag. Verify none exists.

Write all findings to `alu.tmp` with PASS/FAIL per check.
Flag the SBC carry sense prominently.
```

### Agent 2: near-color → `color.tmp`

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing color handling for an SNES emulator. READ-ONLY.
Write exactly one file: `color.tmp`.

## Checks

### 1. RGB555 Expansion
Find where RGB555 → RGB888 conversion happens. Correct:
  r8 = (r5 << 3) | (r5 >> 2)  // bit-repeat
Wrong:
  r8 = r5 << 3  // white = 248, not 255

### 2. Color Component Order
SNES RGB555: bits 14-10=blue, 9-5=green, 4-0=red.
Verify extraction masks are correct.

### 3. Gamma Ramp
Is Near's CRT gamma ramp implemented? (Optional enhancement, not correctness.)

### 4. PPU Color Math
Registers $2130-$2132. Verify:
- Saturating add (clamp to 31 per channel)
- Saturating subtract (clamp to 0)
- Half-math divides AFTER operation
- Window masking interaction

### 5. Brightness
$2100 bits 0-3. Brightness 0 = force black. Applied AFTER color math.

Write all findings to `color.tmp` with PASS/FAIL/PARTIAL per check.
```

### Agent 3: near-hw-variance → `hw-variance.tmp`

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing hardware variance modeling for an SNES emulator. READ-ONLY.
Write exactly one file: `hw-variance.tmp`.

## Checks

### 1. WRAM Initialization
Find where WRAM (128KB) is initialized. Is it: zeroed (default), randomized
(hardware-accurate), or configurable? Near's Dirt Racer and Hurricanes bugs
depend on WRAM contents at startup.

### 2. DSP Register Initialization
Find where S-DSP registers are initialized. All 128 registers should have
defined values. Magical Drop soft-locks on wrong PITCH/ENVX values.

### 3. APU RAM Initialization
SPC700 has 64KB RAM. IPL ROM at $FFC0-$FFFF. Verify remaining RAM is
initialized appropriately.

### 4. Oscillator Variance
Our emulator uses fixed ratios (deterministic). Document which games are
affected (Super Bonk) and our behavior.

### 5. Determinism Contract
Document: same ROM + same inputs + same initial state = same output.

Write all findings to `hw-variance.tmp` with PASS/FAIL/PARTIAL per check.
```

### Agent 4: near-hierarchy → `hierarchy.tmp`

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing ROM detection and peripheral design for an SNES emulator.
READ-ONLY. Write exactly one file: `hierarchy.tmp`.

## Checks

### 1. ROM Header Parsing
Verify LoROM/HiROM detection, checksum validation, region/country code.

### 2. ROM Detection Edge Cases
Near warns: Sufami Turbo game code A9PJ collides with Sailor Moon SuperS.
Check if ROM detection uses game codes (risky) or size+hash (safe).

### 3. Controller Port Model
Is the gamepad hard-coded or abstracted? Could Mouse/Super Scope be added?

### 4. Memory Map Flexibility
Verify LoROM and HiROM both work. Check mirrors ($80-$FF → $00-$7F).

### 5. Special Chip Detection
What happens when a SA-1/SuperFX/DSP-1 ROM is loaded? Graceful error?

Write all findings to `hierarchy.tmp` with PASS/FAIL/NOT_APPLICABLE per check.
```

## Between Phases

After all 4 agents return:
1. Confirm all .tmp files exist: `alu.tmp`, `color.tmp`, `hw-variance.tmp`,
   `hierarchy.tmp`
2. Briefly summarize findings to the user
3. Proceed to Phase 2

## Phase 2: Fix Agent

Launch ONE agent that reads all 4 .tmp files.

```
First, read ~/.claude/AGENT.md for instructions.

You are the fix agent for Near compliance sweep 3 (CPU, Video & Compatibility).
Read these 4 audit files:
- alu.tmp
- color.tmp
- hw-variance.tmp
- hierarchy.tmp

Apply fixes in priority order.

CRITICAL: After ALL changes, verify determinism:
  cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
Expected: fb=54b3eed74f9f8432, audio=62300ecfc4da23e0

## Fix Priority

1. **SBC carry sense** — if inverted, this is a critical CPU bug affecting
   every game that uses subtraction. Fix immediately.

2. **RGB555 expansion** — if using naive r5<<3 instead of bit-repeat, all
   colors are slightly wrong. Fix immediately. This WILL change the fb hash.

3. **Color math** — saturating add/subtract, half-math order. These affect
   specific visual effects (Mode 7 fog, transparency).

4. **WRAM initialization** — add configurable init mode if not present.

5. **ROM detection** — ensure no game code collision issues.

## Tests to Write

Place in `tests/near_sweep3.rs`:

- `sbc_carry_not_inverted`: SBC uses carry directly, not !carry
- `adc_overflow_correct`: V = (target ^ result) & (source ^ result)
- `rgb555_white_is_255`: r5=31 expands to r8=255, not 248
- `rgb555_black_is_0`: r5=0 expands to r8=0
- `brightness_zero_is_black`: $2100=0 forces all-black output
- `color_math_saturates`: adding 31+31 clamps to 31, not wraps to 30

## Cleanup

Delete all 4 .tmp files. Report:
- Fixes applied
- Hash changes
- Tests written
- Remaining compatibility gaps
```
