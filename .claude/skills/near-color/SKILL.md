---
name: near-color
argument-hint: "[scope: 'rgb555', 'gamma', 'color-math', 'full']"
description: >
  Verify color handling against Near/byuu's color emulation article. Audits:
  RGB555 to RGB888 expansion (must use bit-repeat pattern r5<<3|r5>>2, not
  naive r5<<3), optional CRT gamma ramp (Overload's 32-entry table), and PPU
  color math correctness (saturating add/subtract, half-math, window masking).
  Single-pass.
---

# Near Color Compliance (`/near-color`)

Single-pass audit of color handling.

## Reference

From `video/color-emulation/README.md`:

Near on RGB555 expansion: "The solution to this is that the source bits should
repeat to fill in all of the target bits."

```
000 -> 000 000 00...
111 -> 111 111 11...
```

In code: `uint8 red = r << 5 | r << 2 | r >> 1` — bit-repeat, not zero-fill.

Near's CRT gamma ramp for SNES:
```cpp
static const uint8 gammaRamp[32] = {
    0x00, 0x01, 0x03, 0x06, 0x0a, 0x0f, 0x15, 0x1c,
    0x24, 0x2d, 0x37, 0x42, 0x4e, 0x5b, 0x69, 0x78,
    0x88, 0x90, 0x98, 0xa0, 0xa8, 0xb0, 0xb8, 0xc0,
    0xc8, 0xd0, 0xd8, 0xe0, 0xe8, 0xf0, 0xf8, 0xff,
};
```

"Darkens the lower half of the color palette, while leaving the upper half
alone."

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

Single-pass color audit. Report inline.

## Checks

### 1. RGB555 Expansion

Find where SNES RGB555 pixels are converted to RGB888 for display (likely in
the PPU or framebuffer output path).

SNES pixels are 15-bit: 5 bits red, 5 bits green, 5 bits blue (0bbbbbgggggrrrrr).

Correct expansion (bit-repeat):
  r8 = (r5 << 3) | (r5 >> 2)
  // r5=0b11111 -> 0b11111111 = 255 (correct)
  // r5=0b00000 -> 0b00000000 = 0 (correct)
  // r5=0b10000 -> 0b10000100 = 132 (correct)

Naive expansion (zero-fill):
  r8 = r5 << 3
  // r5=0b11111 -> 0b11111000 = 248 (wrong — white isn't white)

Verify the correct formula is used for all three channels.

### 2. Color Component Order

SNES RGB555 is stored as: bit 15 unused, bits 14-10 = blue, 9-5 = green, 4-0 = red.
(Little-endian: low byte = gggrrrrr, high byte = 0bbbbbgg)

Verify the extraction masks are correct:
  r = (color >>  0) & 0x1F
  g = (color >>  5) & 0x1F
  b = (color >> 10) & 0x1F

### 3. Gamma Ramp (Optional Enhancement)

Near's gamma ramp is an optional CRT approximation. Check:
- Is it implemented? (It's a nice-to-have, not a correctness issue)
- If implemented, does it match Near's 32-entry table?
- Is it toggleable? (Users may prefer raw colors)

### 4. PPU Color Math

The SNES PPU performs color math (addition/subtraction) between layers:
- Register $2130 (CGWSEL): color math enable, fixed color select
- Register $2131 (CGADSUB): add/subtract, half-math, per-layer enable
- Register $2132 (COLDATA): fixed color value

Verify:
- Addition clamps to 31 per channel (saturating, not wrapping)
- Subtraction clamps to 0 per channel
- Half-math divides the result by 2 (after add/subtract, before clamp? or after?)
  The SNES divides AFTER the operation: result = clamp((a ± b) / 2, 0, 31)
  Wait — actually it's: result = (a ± b) / 2, clamped. The division happens
  before the final clamp. Verify against hardware reference.
- Window masking interacts with color math (masked regions use the fixed color
  or transparency depending on CGWSEL settings)

### 5. Brightness

Register $2100 bits 0-3 control master brightness (0 = black, 15 = full).
The brightness multiplier is: output = pixel * brightness / 15.

Verify:
- Brightness 0 = force black (not just dark)
- Brightness 15 = no change
- Brightness is applied AFTER color math

## Report

For each check: PASS / FAIL / PARTIAL.
Color issues are subtle but visible — incorrect expansion makes the whole
palette slightly wrong.
```
