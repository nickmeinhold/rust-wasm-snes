# NEAR_COMPLIANCE.md — Audit Skills Derived from Near's Emulation Articles

> Near (byuu) authored [emulation-articles](https://github.com/higan-emu/emulation-articles)
> covering the hard-won design principles behind bsnes/higan. This file
> extracts **concrete compliance checks** from those articles and frames them
> as audit skills that can verify this emulator against the reference standard.
>
> These skills complement the seven in `WHAT_WE_NEED.md`. Where those skills
> audit *what we've built*, these skills audit *how faithfully we follow
> Near's design wisdom*.

---

## Article Index

| # | Article | Path | Core Principle |
|---|---------|------|----------------|
| 1 | Cooperative Threading | `design/cooperative-threading` | JIT sync, state machine vs coroutine tradeoffs |
| 2 | Cooperative Serialization | `design/cooperative-serialization` | Save state correctness under cooperative threading |
| 3 | Schedulers | `design/schedulers` | Relative vs absolute time tracking |
| 4 | Dynamic Rate Control | `audio/dynamic-rate-control` | Audio buffer management and resampling |
| 5 | Emulation Bugs (SNES) | `emulation-bugs/snes` | Auto-joypad polling failures |
| 6 | Game Bugs (SNES) | `game-bugs/snes` | Hardware variance that emulators must reproduce |
| 7 | Input Latency | `input/latency` | JIT input polling |
| 8 | Run-Ahead | `input/run-ahead` | Time-shifting latency reduction |
| 9 | Color Emulation | `video/color-emulation` | CRT gamma ramp, RGB555 expansion |
| 10 | ALU | `cpu/alu` | Branchless flag computation for ADC/SBC |
| 11 | Emulator Hierarchy | `design/hierarchy` | System tree design for peripherals |

---

## Skills to Build

### 1. `/near-jit-sync-compliance`

**Source**: Cooperative Threading, article #1

Near's JIT sync insight: "the CPU will keep on running until it tries to read
from a shared memory region with the APU. Only then will it catch up the APU
to the CPU before performing the read."

**Compliance checks**:

- [ ] **Sync-on-access**: Every CPU read/write to $2140-$2143 must trigger
  `sync_apu()` *before* the port value is read or written. Verify no code path
  reaches `cpu_read`/`cpu_write` without a preceding sync.

- [ ] **No sync on pure memory**: ROM reads, WRAM reads, SRAM reads must NOT
  trigger APU sync. Verify `sync_apu()` is never called for addresses that
  `is_pure_memory()` returns true for. Unnecessary syncs are a performance
  regression that Near explicitly warns against.

- [ ] **Deadlock prevention**: Near notes that when the CPU runs far ahead
  (JIT sync), "you *must* eventually force-switch to the APU to prevent the
  APU from deadlocking if the chips never communicate." Verify there is a
  maximum stale window (end-of-scanline flush) and that no game scenario
  can cause the APU to fall behind by more than one scanline.

- [ ] **Stale-state window analysis**: Under JIT sync, non-port APU activity
  (timers, DSP samples) may be stale until the scanline flush. Verify that
  no game relies on mid-scanline timer or DSP reads outside the port range.

- [ ] **Context switch count**: Near claims bsnes "only has to synchronize
  the CPU and APU a few thousand times a second, instead of millions."
  Instrument sync_apu() call count per frame for SMW. Expect ~2-5 per
  scanline × 262 scanlines = ~500-1300 per frame, NOT the ~15,000+ that
  per-instruction sync would produce.

---

### 2. `/near-serialization-compliance`

**Source**: Cooperative Serialization, article #2

Near identifies three serialization methods for cooperative threading, each
with different determinism guarantees. Our emulator uses a state-machine
approach (not coroutines), so the concerns differ — but the *contract* is the
same.

**Compliance checks**:

- [ ] **Round-trip determinism**: `serialize() → unserialize() → run(N)` must
  produce bit-identical output to running `N` frames without the save/load.
  Test: run 300 frames, serialize, unserialize, run 300 more frames. Compare
  hashes to running 600 frames straight. Near says this is *essential* for
  rewind and run-ahead.

- [ ] **Desynchronization on save**: Near's Method 1 (fast sync) allows one
  instruction of desynchronization. Verify our save/load does NOT advance
  time. The snapshot should capture the exact cycle position of both CPU
  and APU.

- [ ] **cycle_target preservation**: The `cycle_target: u64` in `Apu` is
  absolute and monotonic. Verify it is serialized/unserialized correctly.
  If the old code serialized `cycle_debt`, verify the format migration
  (snapshot VERSION 2) handles the conversion.

- [ ] **No stack-embedded state**: Since we use state machines (not
  coroutines), verify ALL emulation state lives in serializable structs.
  Grep for any state stored in function-local variables across yield points
  — there should be none, since we don't yield.

- [ ] **Determinism under rewind**: If run-ahead or rewind is ever
  implemented, the serialize/unserialize cycle will happen 60 times/sec.
  Near warns that even tiny desynchronizations "start to add up." Verify
  the save state is complete enough for frame-level round-trips.

---

### 3. `/near-scheduler-compliance`

**Source**: Schedulers, article #3

Near describes relative schedulers (signed 64-bit, 1:1 relationships) for
simple systems like the SNES, and absolute schedulers (unsigned 64-bit
timestamps, N:N) for complex ones. Our emulator uses a hybrid: `master_clock`
is absolute, but the APU relationship is tracked as a delta.

**Compliance checks**:

- [ ] **Frequency accuracy**: SNES CPU frequency is `315/88 × 6,000,000 Hz`
  (≈21.477 MHz). APU frequency is `32040 × 768 Hz` (≈24,606,720 Hz). Note:
  Near uses 32000×768 in the article but acknowledges real hardware measures
  closer to 32040. Verify the master-to-SPC conversion ratio (÷21) is
  correct: 21,477,272 / 1,024,000 ≈ 20.97, rounded to 21.

- [ ] **Overflow safety**: Near says with 63-bit precision, the CPU can run
  375 billion clocks ahead before overflow (17,474 seconds). Verify
  `master_clock: u64` cannot overflow in any realistic scenario. At
  ~21.5M clocks/sec, u64 overflow takes ~27 billion years.

- [ ] **Fractional accumulator correctness**: The `cycle_frac` in
  `Apu::catch_up()` accumulates the remainder of `master_cycles / 21`.
  Verify it is initialized to 0 on reset and serialized correctly. Verify
  `cycle_frac` never exceeds 20 (must be `< 21`).

- [ ] **Relative-to-absolute consistency**: `last_apu_sync` tracks the
  absolute master cycle of the last APU sync. Verify that
  `master_clock - last_apu_sync` never goes negative (would indicate
  double-crediting). Verify idle-skip correctly updates `last_apu_sync`
  to prevent the frame loop from re-crediting the skipped cycles.

- [ ] **Multi-chip scaling**: Near warns that relative schedulers scale
  poorly when adding more chips. If DSP-1, SA-1, or SuperFX are ever
  added, the scheduler model will need to be revisited. Document this
  as a known limitation.

---

### 4. `/near-drc-compliance`

**Source**: Dynamic Rate Control, article #4

Near's DRC formula:
```
dynamicFrequency = ((1.0 - maxDelta) + 2.0 * fillLevel * maxDelta) * outputFrequency
```

**Compliance checks**:

- [ ] **Formula correctness**: Verify the AudioWorklet DRC implementation
  matches Near's formula exactly. `maxDelta` should be 0.005 (0.5% max
  pitch distortion). `fillLevel` should range from 0.0 (empty) to 1.0
  (full), targeting 0.5 (half-full).

- [ ] **Base frequency**: Near explicitly says the SNES APU oscillator is
  closer to 32040 Hz than 32000 Hz. Verify the emulator uses 32040 as
  the base sample rate, not 32000. The difference (~0.125%) is enough to
  cause buffer underruns without DRC.

- [ ] **Ring buffer sizing**: Near suggests a 20ms default buffer. At
  32040 Hz stereo, that's ~1282 samples. Our AudioWorklet uses 16384 i16
  samples (~256ms). Verify this is intentional — larger buffers add
  latency but are more robust to jitter. Document the tradeoff.

- [ ] **Buffer fill level query**: Near shows platform-specific APIs
  (OSS `SNDCTL_DSP_GETOSPACE`, waveOut callback counting) for querying
  buffer fill. In the AudioWorklet context, the fill level comes from
  the SharedArrayBuffer ring buffer read/write pointers. Verify the
  fill level calculation handles wrap-around correctly.

- [ ] **Video-sync priority**: Near says DRC assumes video sync is primary
  ("we will synchronize only to the video"). Verify the browser
  implementation uses `requestAnimationFrame` as the primary timing
  source, with DRC adjusting audio to match, not the other way around.

- [ ] **Pitch distortion bound**: With `maxDelta=0.005`, the resampling
  ratio can shift by at most ±0.5%. Verify no code path allows a larger
  shift. Larger shifts produce audible pitch artifacts.

- [ ] **Frequency derivation**: Near derives the SNES video refresh rate as
  `315/88 × 6,000,000 / 1364 / 262 ≈ 60.0985 Hz`. One audio sample every
  32 SPC cycles at 1.024 MHz gives 32,000 Hz. But master_cycles / 21 / 32
  gives ~31,909 Hz (per ASYNC_MODEL.md). Document whether DRC compensates
  for this ~0.4% shortfall.

---

### 5. `/near-auto-joypad-compliance`

**Source**: Emulation Bugs (SNES), article #5

This is the single largest source of game breakage in Near's bug list. **Every
single game** in the emulation-bugs article breaks due to incorrect auto-joypad
polling. The games are: Nuke, Secret of Mana, SpellCraft, Sufami Turbo,
Super Conflict, Super Star Wars, Taikyoku Igo, Tatakae Genshijin 2,
Williams Arcade, Wolverine, World Masters Golf, Zenkoku Koukou Soccer 2.

**Compliance checks**:

- [ ] **HVBJOY busy flag ($4212 bit 0)**: Auto-joypad polling takes 4224
  master cycles (~3.1 scanlines) starting at the beginning of VBlank.
  During this period, bit 0 of $4212 must read as 1. Games that poll
  $4212 before reading $4218-$421F rely on this. Verify the flag is set
  and cleared at the correct cycle.

- [ ] **$4218-$421F latched values**: Auto-joypad reads the full 16-bit
  button state into $4218/$4219 (pad 1) and $421A/$421B (pad 2). These
  must NOT be updated until auto-polling completes. Reading mid-poll
  returns *partially-complete* data (Zenkoku Koukou Soccer 2 bug).

- [ ] **Polling timing**: Auto-joypad runs at VBlank start (scanline 225
  for NTSC, 240 for PAL overscan). Verify it starts at the correct
  scanline, not at an arbitrary point in VBlank.

- [ ] **Duration**: The 4224 master-cycle duration is critical. Too short
  and the busy flag clears early (games read stale data). Too long and
  games that busy-wait on $4212 are delayed. Verify the duration is
  exactly 4224 master cycles, not approximated.

- [ ] **Instantaneous completion bug**: The HANDOVER notes "Currently it
  completes instantaneously." This is the exact bug Near documents. Games
  that don't check $4212 get correct data from our emulator but would get
  stale data on real hardware. Verify status and create a fix plan.

- [ ] **Game-specific regression tests**: For each game in Near's list,
  define a pass condition. Even without ROMs, document what behavior each
  game depends on so future changes can be checked.

---

### 6. `/near-hardware-variance-compliance`

**Source**: Game Bugs (SNES), article #6

Near documents bugs in SNES games that stem from non-deterministic hardware
behavior. An accurate emulator must be able to *reproduce* these bugs under
the right conditions.

**Compliance checks**:

- [ ] **WRAM initialization**: Dirt Racer and Hurricanes break when WRAM
  contains specific values at startup. Verify the emulator supports
  configurable WRAM init: zeroed (default), randomized (hardware-accurate),
  or patterned (for debugging). Near implies randomized is most accurate.

- [ ] **DSP register initialization**: Magical Drop soft-locks when DSP
  PITCH and ENVX registers have wrong values at startup. Verify DSP
  registers are initialized to their hardware-default values, or support
  randomization.

- [ ] **Oscillator variance**: Super Bonk's attract sequence desyncs due
  to "natural variance in the SNES CPU and APU oscillators." Our emulator
  is deterministic (fixed ratio), which means this bug either always or
  never occurs. Document which behavior we produce and whether it matches
  a typical SNES console.

- [ ] **WRAM-to-VRAM transfer hazard**: Hurricanes transfers uninitialized
  WRAM to VRAM. Verify DMA from WRAM to VRAM uses the actual WRAM
  contents (including uninitialized regions), not zeroed memory.

- [ ] **Determinism documentation**: Near notes "emulators are deterministic
  in nature (for the sake of tool-assisted speedruns and bug
  reproducibility)." Document our determinism contract explicitly:
  same ROM + same inputs + same initial state = same output.

---

### 7. `/near-input-latency-compliance`

**Source**: Input Latency, article #7

Near proposes JIT input polling: instead of polling hardware once per frame
at the start of the run loop, poll it on-demand when the emulated system
reads the controller registers, with a 5ms timeout to prevent excessive
polling.

**Compliance checks**:

- [ ] **Polling location**: Verify controller input is read at the point
  the emulated game polls it (typically VBlank), not at the start of the
  frame loop. The difference is ~16ms of latency.

- [ ] **No pre-frame polling**: Near says the standard pattern of
  `pollInputs(); runFrame(); drawFrame()` adds one frame of latency
  because inputs are polled before the frame starts but read near the end.
  Verify our WASM/browser integration does NOT poll inputs at frame start.

- [ ] **JIT polling timeout**: If implemented, verify there's a minimum
  interval between hardware polls (Near suggests 5ms). This prevents
  pathological games that poll every scanline from causing 15,000+
  DirectInput calls per second.

- [ ] **Overscan interaction**: Near warns that exiting `runFrame()` at
  scanline 225 is wrong for games that use PAL overscan (V=240) or that
  toggle overscan mid-frame. Verify the frame exit point handles both
  modes correctly.

- [ ] **Browser-specific**: In the browser, input comes from
  `requestAnimationFrame` callbacks or event listeners, not from direct
  hardware polling. Document how our web frontend maps to Near's JIT
  polling model.

---

### 8. `/near-run-ahead-compliance`

**Source**: Run-Ahead, article #8

Run-ahead is a time-shifting technique: emulate N+1 frames but only display
the last one, reducing perceived input latency by N frames. Requires
save/load state every frame.

**Compliance checks**:

- [ ] **Prerequisite: deterministic serialization**: Run-ahead requires
  `serialize → run → unserialize` 60 times/sec with zero
  desynchronization. Cross-reference with `/near-serialization-compliance`.

- [ ] **Frame-skip optimization**: Near says each run-ahead frame should
  skip video generation (like frame-skipping) since those frames aren't
  displayed. This reduces overhead from 100% per extra frame to ~40%.
  Verify the emulator can run a frame without generating video output.

- [ ] **Audio handling**: Only the final frame's audio should be output.
  All intermediate frames' audio must be discarded. Verify no audio
  duplication or gaps occur during run-ahead.

- [ ] **Turbo mode interaction**: Near says run-ahead "should be disabled"
  during turbo/fast-forward. Verify this interaction is documented.

- [ ] **Feasibility assessment**: Run-ahead requires the emulator to be
  fast enough to run N+1 frames in one frame's time budget (~16ms). At
  our current ~1.7ms/frame in WASM, we have headroom for ~9× run-ahead.
  Document this as a future capability.

---

### 9. `/near-color-compliance`

**Source**: Color Emulation, article #9

Near provides a gamma ramp for SNES CRT color emulation and the RGB555
bit-expansion algorithm.

**Compliance checks**:

- [ ] **RGB555 expansion**: SNES colors are 15-bit (5 bits per channel).
  Expanding to 8-bit must use the bit-repeat pattern:
  `r8 = r5 << 3 | r5 >> 2`. Verify the framebuffer conversion uses this
  formula, not the naive `r5 << 3` (which makes white = 0xF8F8F8, not
  0xFFFFFF).

- [ ] **Gamma ramp availability**: Near provides a 32-entry gamma ramp
  from Overload that darkens the lower palette while preserving the upper.
  This is optional (an enhancement, not a correctness issue), but if
  color correction is implemented, verify it uses Near's table or an
  equivalent curve.

- [ ] **Color math correctness**: The PPU performs add/subtract blending
  between layers. Verify the blending arithmetic matches hardware behavior
  (saturating add, halved result when half-math is enabled). This is more
  of a PPU audit item but connects to color accuracy.

---

### 10. `/near-alu-compliance`

**Source**: ALU, article #10

Near provides branchless algorithms for ADC, SBC (carry-based, 65816-style),
and SBB (borrow-based) with correct overflow, carry, and flag computation.

**Compliance checks**:

- [ ] **SBC via ADC identity**: For the 65816 (WDC65C816), SBC is defined
  as `ADC(target, ~source, carry)` — NOT `ADC(target, ~source, !carry)`.
  This is a common source of bugs. Verify the emulator's SBC uses the
  correct carry sense.

- [ ] **Overflow flag**: `V = (target ^ result) & (source ^ result)` for
  ADC; `V = (target ^ result) & (source ^ target)` for SBC. These differ!
  Verify both formulas are correct.

- [ ] **Decimal mode**: The 65816 supports BCD (decimal) mode for ADC/SBC.
  Near's article doesn't cover decimal mode, but it is notoriously tricky.
  Verify decimal mode adjusts the result AND the flags correctly (N, Z, C
  are affected; V is undefined on the 65816 in decimal mode).

- [ ] **Carry flag**: Near's formula is `C = (carries ^ overflow) & sign`.
  Verify this matches the emulator's implementation, especially for 16-bit
  mode (M=0 on the 65816) where the sign bit is bit 15, not bit 7.

- [ ] **Half-carry**: The 65816 does not have a half-carry flag (that's
  Z80). Verify no spurious half-carry computation exists.

---

### 11. `/near-hierarchy-compliance`

**Source**: Emulator Hierarchy, article #11

Near describes the evolution from hard-coded → list-based → tree-based
peripheral hierarchies. For a single-system SNES emulator, hard-coded is
acceptable, but the *concepts* matter for extensibility.

**Compliance checks**:

- [ ] **Peripheral abstraction**: If Sufami Turbo, Super Game Boy, or
  BS-X Satellaview support is planned, verify the cartridge loading path
  can accommodate multiple ROM slots. Currently not relevant but should
  be documented as a design constraint.

- [ ] **Controller port model**: Verify controller ports are abstracted
  enough to support standard gamepad, Super Multitap (4 sub-ports),
  Mouse, Super Scope, and Justifier. Even if only gamepad is implemented,
  the port model should not hard-code a single controller type.

- [ ] **Sufami Turbo detection**: Near warns that the Sufami Turbo base
  cartridge shares its game code (`A9PJ`) with "Bishoujo Senshi Sailor
  Moon SuperS." Verify ROM detection does not rely solely on the game code
  field — use ROM size + hash or the `BANDAI SFC-ADX` header string.

---

### 12. `/near-oscillator-compliance`

**Source**: Dynamic Rate Control (oscillator section), article #4; Game Bugs
(Super Bonk), article #6

Near emphasizes that real SNES oscillators are NOT exact: the CPU crystal
drifts, the APU ceramic resonator is even less precise (~32040 Hz vs 32000 Hz),
and both fluctuate with temperature and age.

**Compliance checks**:

- [ ] **APU oscillator frequency**: Verify the emulator uses 32040 Hz (or
  a configurable value) for the APU sample rate, not 32000 Hz. Near says
  "most observations place SNES APU oscillators to be closer to 32040."

- [ ] **Video refresh rate derivation**: NTSC refresh rate is
  `315/88 × 6,000,000 / 1364 / 262 ≈ 60.0985 Hz`, NOT exactly 60 Hz.
  Verify the frame timing does not assume exactly 60 Hz.

- [ ] **Interlace frame timing**: With interlacing, one frame has 263
  scanlines (extra half-line). Refresh rate becomes
  `315/88 × 6,000,000 / 1364 / 525 × 2 ≈ 59.984 Hz`. Verify this is
  handled if interlace mode is ever implemented.

- [ ] **Determinism vs realism**: Our emulator is deterministic (fixed
  oscillator ratio). Near notes this means bugs like Super Bonk's attract
  desync will either always or never occur. Document which behavior we
  produce and the tradeoff.

---

## Orchestration

```
                    /near-compliance-sweep
                  (master orchestrator)
    ┌──────────────────┬──────────────────┐
    │  Threading &     │  Audio &         │
    │  Scheduling      │  Input           │
    ├──────────────────┼──────────────────┤
    │  /near-jit-sync  │  /near-drc       │
    │  /near-serial    │  /near-input-lat │
    │  /near-scheduler │  /near-run-ahead │
    │  /near-oscillator│  /near-auto-joy  │
    └──────────────────┴──────────────────┘
    ┌──────────────────┬──────────────────┐
    │  Video &         │  Hardware &      │
    │  CPU             │  Compatibility   │
    ├──────────────────┼──────────────────┤
    │  /near-color     │  /near-hw-var    │
    │  /near-alu       │  /near-hierarchy │
    └──────────────────┴──────────────────┘
```

### Dependency order

1. `/near-scheduler-compliance` and `/near-oscillator-compliance` first —
   they define the time model everything else depends on.
2. `/near-jit-sync-compliance` — builds on the scheduler.
3. `/near-serialization-compliance` — depends on JIT sync for save state
   correctness.
4. `/near-drc-compliance` and `/near-auto-joypad-compliance` — independent,
   can run in parallel.
5. `/near-alu-compliance` and `/near-color-compliance` — independent,
   can run in parallel.
6. `/near-input-latency-compliance` and `/near-run-ahead-compliance` —
   run-ahead depends on serialization.
7. `/near-hardware-variance-compliance` and `/near-hierarchy-compliance` —
   cross-cutting, run last.

### Interaction with WHAT_WE_NEED.md skills

| Near skill | Overlaps with | Relationship |
|---|---|---|
| `/near-jit-sync` | `/snes-async-audit` | Near skill checks *compliance*; async audit checks *correctness* |
| `/near-auto-joypad` | `/cpu-accuracy-sweep` | Auto-joypad is a CPU/Bus timing concern |
| `/near-alu` | `/cpu-accuracy-sweep` | ALU is a subset of CPU accuracy |
| `/near-color` | `/ppu-audit` | Color math is a subset of PPU correctness |
| `/near-drc` | `/apu-audit` | DRC is the output stage of the audio pipeline |
| `/near-scheduler` | `/timing-sweep` | Scheduler is the foundation of timing |
| `/near-oscillator` | `/timing-sweep` | Oscillator accuracy is a timing concern |
| `/near-serial` | `/snes-async-audit` | Serialization depends on sync model |
| `/near-hierarchy` | `/memory-map-audit` | Peripheral model affects memory mapping |
| `/near-hw-var` | `/rom-compat-sweep` | Hardware variance drives compatibility |
| `/near-input-lat` | `/wasm-perf-profile` | Input latency is a perf/UX concern |
| `/near-run-ahead` | (new capability) | Depends on serialization + perf headroom |

---

## Priority for this emulator

**Immediate** (active bugs or known gaps):
1. `/near-auto-joypad-compliance` — HANDOVER says it completes instantaneously.
   Near's article shows 12 games broken by this.
2. `/near-jit-sync-compliance` — JIT sync is implemented; verify it fully
   matches Near's contract.
3. `/near-drc-compliance` — AudioWorklet is written but untested.
4. `/near-oscillator-compliance` — 32040 vs 32000 Hz matters for DRC.

**Soon** (correctness):
5. `/near-alu-compliance` — SBC carry sense is a classic bug source.
6. `/near-serialization-compliance` — save states work but round-trip
   determinism is untested.
7. `/near-scheduler-compliance` — hybrid scheduler is working but
   never formally verified.

**Later** (features and polish):
8. `/near-color-compliance` — enhancement, not a bug.
9. `/near-input-latency-compliance` — browser input model differs from
   Near's native JIT polling.
10. `/near-run-ahead-compliance` — future capability, blocked on
    serialization verification.
11. `/near-hardware-variance-compliance` — WRAM randomization, etc.
12. `/near-hierarchy-compliance` — only relevant if adding peripherals.
