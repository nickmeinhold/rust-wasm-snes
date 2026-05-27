---
name: near-auto-joypad
argument-hint: "[scope: 'timing', 'hvbjoy', 'registers', 'games', 'full']"
description: >
  Read-only audit of auto-joypad polling against Near/byuu's emulation bugs
  article. This is the #1 source of game breakage — 12 games broken by
  incorrect timing. Writes findings to auto-joypad.tmp. Intended to be run as
  part of /near-sweep2 (parallel with near-drc, near-input-latency,
  near-run-ahead), whose fix agent reads all .tmp files.
---

# Near Auto-Joypad Compliance (`/near-auto-joypad`)

Read-only audit. Writes `auto-joypad.tmp`. Does NOT modify code.

## Reference

From `emulation-bugs/snes/README.md`, EVERY game listed breaks due to
incorrect auto-joypad polling:

- **Nuke**: "Inputs do not work unless auto joypad polling is correct."
- **Wolverine (USA)**: Hangs after logos.
- **Taikyoku Igo**: Skips intro, can't start game.
- **Super Conflict**: Random phantom inputs.
- **Williams Arcade**: Autonomous firing.
- **World Masters Golf**: D-pad moves continuously instead of once.
- **Zenkoku Koukou Soccer 2**: Pause unreliable (reads mid-poll data).

## The Specification

Auto-joypad polling on real SNES hardware:
1. Starts at the beginning of VBlank (scanline 225 NTSC / 240 PAL)
2. Takes 4224 master cycles (~3.1 scanlines) to complete
3. During polling, $4212 bit 0 (HVBJOY auto-read busy) = 1
4. $4218-$421F contain partially-complete data during polling
5. After polling completes, $4212 bit 0 = 0, $4218-$421F contain final data

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing auto-joypad polling for an SNES emulator. READ-ONLY.
Write exactly one file: `auto-joypad.tmp`.

## Steps

### 1. Find the Implementation

Search for auto-joypad, joypad, $4212, $4218, HVBJOY in the codebase.
The implementation may be in `src/bus.rs`, `src/joypad.rs`, or `src/lib.rs`.

### 2. Check HVBJOY ($4212)

Read the handler for register $4212. Verify:
- Bit 0 (auto-read busy) is set during auto-joypad polling
- Bit 0 is cleared when polling completes
- Bits 6-7 (H/V blank flags) are independent of auto-read

The HANDOVER says auto-joypad "completes instantaneously." If bit 0 is never
set to 1 (or is set and immediately cleared), that's the bug.

### 3. Check Timing

Verify:
- Polling starts at the correct scanline (225 for NTSC, 240 for PAL overscan)
- Polling duration is 4224 master cycles
- The busy flag is visible for the full duration
- Games that poll $4212 in a tight loop will see bit 0 = 1 for ~3 scanlines

If the polling is instantaneous, document the delta: 0 cycles (current) vs
4224 cycles (correct). This is a 3-scanline timing error.

### 4. Check $4218-$421F

Verify:
- These registers contain the FINAL button state after polling completes
- During polling, they contain partially-complete data (shifting in bit by bit)
- Reading mid-poll returns whatever bits have been shifted in so far

### 5. Assess Impact

For each of Near's 12 affected games, assess whether our implementation would
reproduce the bug or hide it:

| Game | Bug | Requires | Our Behavior | Match? |
|------|-----|----------|-------------|--------|
| Nuke | No inputs | Correct HVBJOY timing | ? | ? |
| Wolverine | Hangs | Correct HVBJOY timing | ? | ? |
| ... | ... | ... | ... | ... |

### 6. Write auto-joypad.tmp

Include:
- Current implementation location and behavior
- HVBJOY timing assessment
- $4218-$421F correctness
- Per-game impact matrix
- Recommended fix with cycle-level specification
- Effort estimate: S/M/L
```
