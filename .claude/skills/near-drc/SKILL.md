---
name: near-drc
argument-hint: "[scope: 'formula', 'buffer', 'frequency', 'worklet', 'full']"
description: >
  Verify Dynamic Rate Control implementation against Near/byuu's DRC article.
  Near's formula: dynamicFrequency = ((1.0 - maxDelta) + 2.0 * fillLevel *
  maxDelta) * outputFrequency. Audits: formula correctness (maxDelta=0.005),
  base frequency (32040 Hz not 32000), ring buffer sizing, fill-level query
  correctness, video-sync priority, pitch distortion bound, and the ~0.4%
  frequency shortfall from master_cycles/21/32.
---

# Near DRC Compliance (`/near-drc`)

Audit of Dynamic Rate Control against Near's article.

## Reference

From `audio/dynamic-rate-control/README.md`:

Near's DRC formula:
```cpp
auto maxDelta = 0.005;
double fillLevel = instance->level();
double dynamicFrequency = ((1.0 - maxDelta) + 2.0 * fillLevel * maxDelta)
                          * instance->frequency;
```

Near on the APU oscillator: "most observations place SNES APU oscillators to
be closer to 32040 * 768, or ~24.607MHz, in practice."

Near on synchronization: "In this mode, we will synchronize only to the video.
The goal with dynamic rate control is to keep the audio buffer approximately
half-full at all times."

Near on pitch: "By adjusting the audio resampling ratio, we do actually alter
the pitch. And so it's very important that we never adjust the pitch *too* much
in any one step."

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing the DRC implementation for an SNES emulator's AudioWorklet.
This is a single-pass audit — report inline.

## Checks

### 1. Formula Correctness

Find the DRC implementation (likely in `web/audio-worklet-processor.js` or
similar). Verify the formula matches Near's exactly:

  dynamicFrequency = ((1.0 - maxDelta) + 2.0 * fillLevel * maxDelta) * baseFreq

Where:
- maxDelta = 0.005 (0.5% max pitch distortion)
- fillLevel = ring buffer fill level, 0.0 (empty) to 1.0 (full)
- baseFreq = output frequency (browser's AudioContext.sampleRate, typically 48000)

When fillLevel = 0.5 (target): dynamicFrequency = baseFreq (no adjustment).
When fillLevel = 0.0 (empty): dynamicFrequency = (1.0 - 0.005) * baseFreq = 0.995 * baseFreq (speed up production).
When fillLevel = 1.0 (full): dynamicFrequency = (1.0 + 0.005) * baseFreq = 1.005 * baseFreq (slow down production).

Wait — re-read Near's formula. The adjustment is on the INPUT frequency, not
output. When the buffer is empty, we want MORE samples, so we DECREASE the
input frequency (making the ratio input/output smaller, producing more output
samples per input sample). When full, INCREASE input frequency.

Verify the direction is correct in the implementation.

### 2. Base Frequency

Near says the real SNES APU runs at ~32040 Hz, not 32000 Hz. The emulator's
master_cycles / 21 / 32 gives ~31,909 Hz (documented in ASYNC_MODEL.md).

Verify:
- The AudioWorklet uses 32040 as the SNES-side sample rate (or whatever the
  emulator actually produces — measure it)
- The resampling converts from the SNES rate to the browser's rate (48000)
- DRC adjusts the SNES-side rate, not the browser-side rate

### 3. Ring Buffer

Read the SharedArrayBuffer ring buffer implementation. Verify:
- Size is appropriate (the current 16384 i16 samples = ~256ms at 32kHz stereo)
- Read/write pointers use Atomics for cross-thread safety
- Wrap-around is handled correctly (modular arithmetic on buffer size)
- Fill level = (writePtr - readPtr) % size / size
- Fill level handles the case where writePtr < readPtr (wrap-around)

### 4. Buffer Fill Level Query

Near shows platform-specific implementations (OSS SNDCTL_DSP_GETOSPACE, waveOut
block counting). In our AudioWorklet context:

- The worklet (consumer) knows how many samples it has consumed
- The main thread (producer) knows how many samples it has written
- Fill level is derived from the difference

Verify the fill level is computed on the correct side (wherever the DRC
adjustment happens) and that it's fresh (not stale by multiple frames).

### 5. Video-Sync Priority

Near says DRC assumes video sync is primary. In the browser:
- requestAnimationFrame is the video sync (typically 60Hz)
- AudioWorklet runs on its own real-time thread (128 samples at 48kHz = 2.67ms)
- The emulator should run one frame per rAF, producing ~534 audio samples
  (32040/60 = 534 stereo pairs)

Verify the emulation loop is driven by rAF, not by audio demand.

### 6. Pitch Distortion Bound

With maxDelta = 0.005, the maximum frequency shift is ±0.5%. At 440 Hz (A4),
that's ±2.2 Hz — below the just-noticeable difference for most listeners.

Verify no code path allows a larger shift (e.g., a fallback that doubles the
rate when the buffer is critically low).

### 7. Frequency Derivation

Document the actual sample generation rate:
- Master cycles per frame: 262 × 1364 = 357,368
- SPC cycles per frame: 357,368 / 21 = 17,017.5
- DSP samples per frame: 17,017.5 / 32 = 531.8
- At 60.098 fps: 531.8 × 60.098 = 31,961 Hz

This is ~0.25% below 32040 Hz. DRC must compensate for this shortfall or the
buffer will slowly drain. Verify DRC handles this correctly.

## Report

For each check: PASS / FAIL / NOT_IMPLEMENTED with explanation.
If the AudioWorklet hasn't been tested in a browser yet, note what can be
verified from code alone vs what needs runtime testing.
```
