---
name: near-hierarchy
argument-hint: "[scope: 'rom-detect', 'controllers', 'peripherals', 'full']"
description: >
  Verify emulator hierarchy design against Near/byuu's hierarchy article. Near
  describes hard-coded, list-based, and tree-based peripheral models. For a
  single-system SNES emulator, hard-coded is acceptable, but the controller
  port model should support multiple input devices, and ROM detection must
  handle edge cases (Sufami Turbo game code collision with Sailor Moon).
  Single-pass.
---

# Near Hierarchy Compliance (`/near-hierarchy`)

Single-pass audit of peripheral and ROM detection design.

## Reference

From `design/hierarchy/README.md`:

Near on hard-coding: "there is no doubt however that if you're absolutely
certain you only ever want to write a single emulator, this can produce an
extremely friendly user interface without a lot of work."

Near on Sufami Turbo detection: "You might be tempted to use the four-letter
game code in the extended header to identify the cartridge, but beware: this
has the game code set to A9PJ, which is shared with the game 'Bishoujo Senshi
Sailor Moon SuperS.'"

Near on Micro Machines 2: the game cartridge itself has extra controller ports.
The hierarchy must be dynamic to support this.

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

Single-pass hierarchy audit. Report inline.

## Checks

### 1. ROM Header Parsing

Find the ROM loading code. Verify:
- LoROM/HiROM detection works correctly
- Checksum validation exists
- Internal header name is extracted
- Region/country code is used for NTSC vs PAL

### 2. ROM Type Detection Edge Cases

Near warns about Sufami Turbo's game code collision:
- Sufami Turbo: game code A9PJ
- Sailor Moon SuperS: game code A9PJ

If the emulator ever needs to detect Sufami Turbo, it must use ROM size +
hash or the BANDAI SFC-ADX header string, not the game code alone.

Check: does our ROM detection use game codes? If so, is the collision handled?
(Probably not relevant yet if we only support standard cartridges.)

### 3. Controller Port Model

Verify the controller input abstraction:
- Is the gamepad hard-coded, or is there an abstraction for different
  input devices?
- Could a Mouse, Super Scope, or Justifier be added without restructuring?
- Is the Super Multitap representable (one port → four sub-ports)?

For a single-system emulator targeting browser play, hard-coded gamepad
is acceptable. But document the limitation.

### 4. Memory Map Flexibility

LoROM and HiROM have different bank/address mappings:
- LoROM: ROM at banks $00-$7D, upper half ($8000-$FFFF)
- HiROM: ROM at banks $40-$7D, full 64KB per bank
- ExHiROM: extended addressing for ROMs > 4MB

Verify:
- Both LoROM and HiROM are supported
- The correct mapping is selected based on the ROM header
- Mirrors are correct (banks $80-$FF mirror $00-$7F with FastROM speed)

### 5. Special Chip Detection

Some SNES cartridges contain coprocessors:
- DSP-1/2/3/4: math coprocessor
- SA-1: secondary 65816 CPU
- SuperFX: RISC GPU
- SDD-1: graphics decompression
- Cx4: Capcom's wire-frame chip

Verify:
- Is there detection for special chips? (ROM header byte $D6)
- If a special chip ROM is loaded, what happens? (Graceful error vs crash)
- Document which chips are supported and which are not

## Report

For each check: PASS / FAIL / PARTIAL / NOT_APPLICABLE.
This is the lowest-priority Near compliance skill — most checks are about
future extensibility rather than current correctness.
```
