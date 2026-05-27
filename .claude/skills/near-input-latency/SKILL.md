---
name: near-input-latency
argument-hint: "[scope: 'polling', 'jit', 'browser', 'full']"
description: >
  Verify input latency handling against Near/byuu's input latency article. Near
  proposes JIT input polling: poll hardware on-demand when the emulated game
  reads controller registers, with a 5ms timeout. Audits: polling location
  (must be at game read time, not frame start), no pre-frame polling pattern,
  browser-specific input model mapping, and overscan interaction. Single-pass.
---

# Near Input Latency Compliance (`/near-input-latency`)

Single-pass audit of input latency against Near's article.

## Reference

From `input/latency/README.md`:

Near's problematic pattern:
```cpp
void Program::run() {
    while(stopped() == false) {
        hardware.pollInputs();  // <-- inputs polled HERE
        emulator.runFrame();    // <-- but used ~16ms later at V=225
        video.drawFrame();
    }
}
```

Near's JIT polling solution:
```cpp
bool Program::readInput(uint inputID) {
    hardware.pollInputs();  // <-- poll at the moment the game reads
    // 5ms timeout prevents excessive polling
    ...
}
```

Near: "Whenever the emulated system tries to poll the inputs, we poll the host
machine inputs at that time."

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

Single-pass audit of input latency. Report inline.

## Checks

### 1. Polling Location

Find where controller input is read in the emulation loop. Trace from:
- The frame loop in `src/lib.rs`
- Through the joypad/controller handling
- To the WASM/JS boundary where browser input arrives

Is input polled at frame start (Near's bad pattern) or on-demand when the
game reads $4016/$4017 or $4218-$421F (Near's JIT pattern)?

### 2. Browser Input Model

In the browser, input doesn't come from DirectInput — it comes from:
- `keydown`/`keyup` events (asynchronous, buffered by browser)
- `gamepadAPI` polling (synchronous, but only in rAF callback)
- `requestAnimationFrame` callback timing

Map how our web frontend delivers input to the WASM emulator:
- When does the JS side read keyboard/gamepad state?
- How does that state reach the Rust emulator?
- Is there a frame of latency between the JS read and the Rust use?

### 3. Overscan Interaction

Near warns: exiting runFrame() at scanline 225 is wrong for PAL overscan
(V=240) or games that toggle overscan mid-frame.

Check: does our frame loop handle both NTSC (V=225) and PAL (V=240) correctly?
Does it handle mid-frame overscan toggles?

### 4. Auto-Joypad vs Manual Poll

SNES games read input two ways:
1. Auto-joypad: hardware reads controller at VBlank, latches into $4218-$421F
2. Manual: game reads $4016/$4017 directly (bit-serial protocol)

For auto-joypad, the input must be latched at VBlank start.
For manual polling, the input should reflect the latest state at read time.

Verify both paths exist and have correct timing.

### 5. Latency Budget

Document the total input-to-display latency chain:
- Browser event → JS state update: ~0-8ms (event loop tick)
- JS state → WASM emulator: ~0ms (shared memory) or ~0-16ms (next rAF)
- Emulator input read → frame rendered: ~0-16ms (depends on V-position)
- Frame rendered → canvas displayed: ~0-16ms (rAF + compositor)
- Total: ~0-58ms typical

Near's JIT polling removes one full frame (~16ms) from this chain.

## Report

For each check: PASS / FAIL / PARTIAL / NOT_APPLICABLE.
Note that browser constraints differ from Near's native context — document
what translates and what doesn't.
```
