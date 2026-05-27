---
name: near-run-ahead
argument-hint: "[scope: 'feasibility', 'serialization', 'performance', 'full']"
description: >
  Assess run-ahead feasibility against Near/byuu's run-ahead article. Run-ahead
  emulates N+1 frames but displays only the last, reducing perceived input
  latency by N×16ms. Requires deterministic serialize/unserialize 60 times/sec,
  frame-skip optimization (skip video on intermediate frames), and sufficient
  performance headroom. This skill assesses prerequisites, not implementation.
  Single-pass.
---

# Near Run-Ahead Compliance (`/near-run-ahead`)

Single-pass feasibility assessment for run-ahead.

## Reference

From `input/run-ahead/README.md`:

Near's run-ahead loop:
```cpp
void Emulator::runFrameAhead(unsigned int runAhead) {
    input.poll();
    emulator.run();  // frame 1 (discarded)
    auto saveState = emulator.serialize();
    while(runAhead > 1) {
        emulator.run();  // intermediate frames (discarded)
        runAhead--;
    }
    auto [videoFrame, audioFrames] = emulator.run();  // final frame (displayed)
    video.output(videoFrame);
    audio.output(audioFrames);
    emulator.unserialize(saveState);  // restore to frame 1 state
}
```

Near on overhead: "each frame of run-ahead only adds about 40% of additional
overhead" (when skipping video generation on intermediate frames).

Near on compatibility: "virtually every single Super Nintendo game has at least
one frame of internal processing delays, and so a setting of 1 works for all
but maybe 0.1% of the library."

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

Single-pass feasibility assessment. Report inline.

## Prerequisites to Check

### 1. Serialization Speed

Run-ahead requires serialize + unserialize every frame (60Hz).
Budget: ~16ms total per frame, minus emulation time.

Assess:
- How large is the serialized state? (CPU + Bus + PPU + APU + WRAM + SRAM)
- Is serialization allocation-free? (Per-frame allocation = GC pressure in WASM)
- Estimated serialize + unserialize time?
- Does our snapshot code support this throughput?

### 2. Round-Trip Determinism

serialize→unserialize→run(1 frame) must be bit-identical to run(1 frame).
Cross-reference with /near-serialization findings.

If round-trip determinism is broken, run-ahead produces visual artifacts
(the save/load cycle introduces drift that accumulates at 60Hz).

### 3. Frame-Skip Capability

Near says intermediate frames should skip video generation for performance.
Assess:
- Can the PPU be disabled for one frame? (Don't render scanlines)
- Can audio be skipped? (Don't push to sample buffer)
- Is there a `skip_video: bool` parameter on the frame loop?

Without frame-skip, each run-ahead frame costs 100% overhead.
With frame-skip (PPU skipped), each costs ~40% (Near's measurement).

### 4. Performance Headroom

From the bench results:
- Current frame time: ~1.7ms (WASM), ~1.0ms (native)
- Budget per frame: ~16.6ms (60Hz)
- Headroom: ~15ms (WASM), ~15.6ms (native)

This suggests we could handle run-ahead = 8+ in theory. But:
- Serialization overhead is additive
- GC pauses in WASM eat into headroom
- Audio processing must still happen for the displayed frame

Estimate maximum practical run-ahead level.

### 5. Audio Handling

Only the LAST frame's audio should be output. Intermediate frames must
produce audio (to advance DSP state correctly) but discard it.

Assess:
- Can sample_buffer be temporarily suppressed?
- Does the audio hash depend on intermediate-frame samples?
- Would discarding intermediate audio affect DRC buffer levels?

### 6. Turbo Mode Interaction

Near says run-ahead should be disabled during turbo/fast-forward.
Assess whether the architecture supports toggling run-ahead per-frame.

## Report

| Prerequisite | Status | Blocker? | Effort to Fix |
|---|---|---|---|
| Serialization speed | ? | ? | ? |
| Round-trip determinism | ? | ? | ? |
| Frame-skip capability | ? | ? | ? |
| Performance headroom | ? | ? | ? |
| Audio handling | ? | ? | ? |
| Turbo interaction | ? | ? | ? |

Verdict: READY / NEEDS_WORK / BLOCKED with explanation.
```
