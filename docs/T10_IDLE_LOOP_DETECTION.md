# T10 — Idle-Loop Detection for the 65816 CPU

**Status:** Tier 1 implementation behind `idle-skip` Cargo feature (default off).
CPU semantics are correct (framebuffer hash preserved under single-skip cap);
audio hash exhibits a residual divergence from APU chunk-size sensitivity that
needs a separate session of investigation. See §8 "Implementation findings".

**Hard constraint:** must preserve SMW × 600-frame determinism hashes
(`final_fb_hash = 54b3eed74f9f8432`, `final_audio_hash = 62300ecfc4da23e0`)
bit-for-bit. CI gate at `.github/workflows/bench.yml` enforces this on every push.

## 0. Why this matters

`cargo run --release --bin bench rom/smw.smc` shows two opcodes dominating the
SMW dispatch profile:

```
rank  op   name    count       share    cumulative
   1  F0   BEQ     3,455,231   30.56%   30.56%
   2  A5   LDA     3,454,342   30.55%   61.11%
```

That is the canonical 65816 polling shape:

```
loop:  LDA $xx          ; A5 xx     (direct page read of a polled flag byte)
       BEQ loop         ; F0 FD     (branch back if zero)
```

The game is spin-waiting on a WRAM byte that some asynchronous agent — NMI
handler, IRQ handler, HDMA, or auto-joypad read — will eventually flip. Every
iteration costs ~5 master cycles; SMW burns ~3.4M of them per benchmark run.
If we can identify that we're in such a loop and **fast-forward the CPU clock
to the next event that could possibly change the polled byte**, we recover
that 61% of dispatch overhead.

---

## 1. Survey of reference implementations

I cloned and grepped each reference codebase directly. The findings were not
what I expected.

### bsnes (`/tmp/bsnes-probe/bsnes/sfc/cpu/`)

**No idle-loop detection.** `CPU::idle()` in `memory.cpp` is the 65816's
intrinsic internal cycle (per the WDC datasheet), not loop optimisation. The
philosophy is strict cycle-accuracy: every opcode steps the master clock
exactly, and the SMP, PPU, and coprocessors are co-scheduled via libco
threads. byuu's stance was that idle-skip is a correctness hazard and
unnecessary on modern hosts.

### ares (`/tmp/ares-probe/ares/sfc/cpu/memory.cpp`)

Same as bsnes (ares forked from higan). `CPU::idle()`, `CPU::read()`, and
`CPU::wait()` all just compute the cycle count for one memory access. No
pattern detection anywhere in `sfc/cpu/`.

### Mesen2 (`/tmp/mesen-probe/Core/SNES/`)

**No SNES CPU idle-loop detection.** `_skipRender` exists, but that's a PPU
frame-skip for video — unrelated. `SnesCpu::Idle()` is again the 65816
internal cycle. Sour's Mesen2 is event-scheduled but executes every CPU
instruction. The NES core in the same repo is also clean — no NES-style idle
skip carried over.

### snes9x (master, `/tmp/snes9x-probe/`)

**Modern master removed it.** `grep -rn WaitAddress` across the tree finds
matches only in `snapshot.cpp` (reading legacy save-state fields for
backward-compatibility) and nothing else. The 1.53 release also lacks it.

The mechanism *existed* in snes9x 1.39–1.43 era as `CPU.WaitAddress` and
`CPU.WaitByteAddress`, set when a branch instruction noticed it was branching
back a small negative offset to itself; subsequent reads of the same address
would advance `CPU.Cycles` straight to the next H/V event. The code was torn
out (somewhere around the 1.50 refactor that introduced `S9xDoHEventProcessing`)
because the event scheduler made it redundant and it produced subtle bugs in
DSP-1 / SA-1 titles.

### The surprising finding

**None of the four reference SNES emulators currently detect LDA→BEQ idle
loops.** They all just execute every cycle. They get acceptable speed for
three reasons:

1. **Native C++ at 3GHz** runs a 21MHz emulated CPU at well over realtime even
   when 61% of dispatches are polling.
2. **Event-driven scheduling** (`S9xDoHEventProcessing` in snes9x,
   coroutine-yielding scheduler in bsnes/ares, sub-instruction stepping in
   Mesen2) keeps overhead per opcode low.
3. **Cycle-accuracy is a marketing feature.** Any skip is a correctness risk
   they'd rather not own.

Our situation is different: we run in WASM, single-threaded, with a per-opcode
dispatch overhead that's measurably higher than a native C++ switch. The 61%
figure is therefore worth attacking even though "real" emulators don't bother.

**Prior art that's actually useful** comes from elsewhere:

- **GBA emulators** (mGBA, VBA-M): aggressive idle-loop detection because GBA
  software polls the keypad / V-counter constantly.
- **NES emulators** (FCEUX historically): less common, but the pattern is
  identical.
- **DOS PC emulators** (DOSBox `CPU_CYCLES auto`): not strictly idle-skip but
  the same notion of "this BIOS loop is wasting host time, skip ahead."

mGBA's design (see `src/arm/isa-arm.c` and `src/gba/gba.c`) is the closest
template: a small whitelist of known polling-loop shapes per opcode, a
"polled address" register, and a "next event time" hook from the scheduler.
That's the model adapted below.

---

## 2. Pattern detection design

### 2.1 Loop shapes to detect

Start with the dominant shape and expand only after the first gives a measured
win. Each shape is described as "after fetch but before execute, look at the
last N opcodes and the impending branch target."

**Tier 1 — must detect** (covers the SMW 61%):

```
A5 xx        LDA dp        ; direct-page load, polled byte at $00:00xx + DP
F0 FD        BEQ -3        ; branch back exactly to the LDA
```

The signature is: opcode `A5` immediately followed by `F0` with a signed
8-bit offset of `-3` (i.e. PC of BEQ + 2 + (-3) = PC of LDA). Three bytes,
two opcodes, branch lands on itself.

**Tier 2 — common variants** (probably +5–10% on other titles):

| Opcodes | Bytes | Shape | Notes |
|---|---|---|---|
| `A5 xx D0 FD` | 4 | LDA dp / BNE -3 | "wait until nonzero" |
| `AD lo hi F0 FB` | 5 | LDA abs / BEQ -5 | absolute addressing |
| `AD lo hi D0 FB` | 5 | LDA abs / BNE -5 | absolute addressing |
| `B5 xx F0 FD` | 4 | LDA dp,X / BEQ -3 | indexed (rare in polling) |
| `A5 xx 29 mm F0 FB` | 6 | LDA dp / AND #imm / BEQ -5 | masked poll |
| `A5 xx C9 mm F0 FB` | 6 | LDA dp / CMP #imm / BEQ -5 | compare-poll |
| `AC lo hi C0 lo hi F0 F8` | 8 | LDY abs / CPY #imm / BEQ -8 | rarer |

Tier 2 can be deferred. Implement Tier 1, measure, then expand. The histogram
already tells us how much shape coverage we have.

**Branch-to-self (BRA $FE / `80 FE`):** rare in SMW but worth noting — a
forever loop that only an interrupt can break. Treat as a degenerate Tier 1
case; skip directly to the next pending event.

### 2.2 Is the polled address safe to skip?

This is the entire correctness story.

A read is **safe to elide** when reading the same address every cycle would
not change any observable state besides itself. For our bus
(`src/bus.rs:97`), that gives us:

**Safe (pure memory):**
- `$00–$3F:$0000–$1FFF` — low WRAM mirror (the SMW polling target lives here;
  direct-page lands here)
- `$7E–$7F:any` — full WRAM
- `$70–$7D:$0000–$7FFF` — SRAM
- `$00–$3F:$8000–$FFFF`, `$40–$6F:$8000–$FFFF`, etc. — ROM (truly idempotent)

**Unsafe — reading mutates state:**
- `$00–$3F:$2100–$213F` — PPU registers. Several are read-once-then-clear
  (e.g. `$2137 SLHV` latches H/V counters; `$2139/$213A VMDATAREAD` advances
  VRAM address; `$213E/$213F PPU status` have flag bits that reset on read).
- `$00–$3F:$2140–$217F` — APU port mailbox. The CPU side polling these is
  one direction of the SPC handshake. **Reading does not have side effects**
  in our impl (`apu.cpu_read` is pure), but the *value* depends on APU
  cycle-accounting, which is exactly what we're about to fast-forward. See
  §3.3.
- `$00–$3F:$2180` — WMDATA: reads from WRAM at `wram_addr` and *increments
  `wram_addr`*. Hard side effect.
- `$00–$3F:$4016/$4017` — joypad serial port: shift register state changes
  on read.
- `$00–$3F:$4210` — RDNMI: reading **clears `nmi_flag`** (line 199–203 of
  `bus.rs`). Catastrophic if elided.
- `$00–$3F:$4211` — TIMEUP: same — read-clear of `irq_flag`.
- `$00–$3F:$4212–$421F` — HVBJOY, joypad auto-read result registers. Their
  *value* is dynamic, but reading is pure.
- `$00–$3F:$4300–$437F` — DMA register block. Pure read; non-dangerous, but
  also never polled.

**Decision rule:** the optimisation triggers **only if the polled address
decodes to WRAM, SRAM, or ROM**. Everything else: bail out, execute the loop
normally.

That decision is cheap: a single `match` on `(bank, addr)` mirroring the read
path. Put it in a helper `bus.is_pure_memory(bank, addr) -> bool` so the rule
lives in one place.

### 2.3 Skip-to target

When we're confident we're idle on a pure-memory address, the polled byte can
only change when **someone writes to it**. Candidates:

1. **NMI handler** writes (most common — the NMI handler clears the flag
   the main loop set, or sets a "frame ready" flag).
2. **IRQ handler** writes (H/V-count IRQ, much less common).
3. **HDMA transfer** writes (HDMA can target WRAM via `$2180` indirection).
4. **Auto-joypad read** writes `$4218–$421F`, but those are I/O so excluded.
5. **DMA transfer** writes (initiated by the polling code itself? no — by
   definition we're not executing any non-poll code).
6. **APU port write back from SPC700** — the SPC writes `$2140–$2143` mirrors
   from the APU side. Again I/O, excluded by §2.2.

For Tier 1 (pure WRAM poll), the **only realistic mutator is the next
scheduled NMI or IRQ**. The scanline-based frame loop in `lib.rs:181`
schedules these at deterministic points:

- NMI fires when `scanline == VBLANK_START` (line 184) and `nmitimen & 0x80`
- IRQ fires at the configured V/H match (lines 206–218)
- HDMA writes happen at `hdma_run_scanline` (line 252)

So the **skip target is: advance `cpu.cycles` to the start of the next
scheduled NMI / IRQ / HDMA-write event, whichever comes first**, then resume
normal execution. Conservatively: jump to the next scanline boundary.
Maximally: jump to whichever specific event is next, computed from `nmitimen`,
`vtime`, and `hdmaen`.

Start conservatively. **Skip target v1 = next scanline boundary.** This
preserves all scanline-aligned state transitions (HBLANK, HDMA fire,
V-counter increments) — and that's the granularity at which the determinism
hash is computed. Going finer-grained is a v2 optimisation.

---

## 3. Determinism preservation argument

The determinism contract requires that after 600 frames of SMW the framebuffer
and audio sample stream hash to the published values. The skip mechanism
preserves this if and only if, for each skip event:

> The CPU and bus state at the resume point is byte-identical to the state
> that would result from executing every cycle of the polling loop normally.

### 3.1 CPU register state

In a pure poll loop on WRAM, the loop body reads memory and updates N/Z flags.
At resume:

- `A` ends with the value of the polled byte at resume time. If the byte
  didn't change during the skip, `A` is identical to the value it had on entry
  to the skip. If it did change (some HDMA or interrupt wrote), the skip target
  is exactly the cycle at which that write happened, and a single more iteration
  would produce the new `A`. Either way, **`A` at resume equals the polled byte
  at resume cycle.** Match.
- `N`, `Z` flags: derived from `A`. Match.
- `PC`: back at the LDA, ready to re-fetch. Match.
- `cycles`: advanced to the scheduled-event cycle. By construction, matches
  the cycle that the un-skipped emulator would have hit when an interrupt
  finally fired (give or take one loop iteration of slop — see §3.4).

### 3.2 Bus / WRAM / PPU / DMA state

Nothing in the loop body writes to anywhere. WRAM, VRAM, OAM, CGRAM, DMA
registers, joypad shift state: all identical because nothing touched them.

The polled byte itself: identical because (a) loop body doesn't write it, and
(b) the skip *enables* exactly the same NMI/IRQ/HDMA-driven writes that the
un-skipped loop would have allowed — we resume at the same cycle.

### 3.3 APU cycle accounting

This is the subtle one. Look at `lib.rs:246`:

```rust
self.bus.apu.catch_up(elapsed as u32);
```

After each CPU step, the APU is advanced by `elapsed` master cycles. If we
skip 1000 cycles of CPU work, **the APU must still be told about those 1000
cycles**, otherwise the SPC700 runs late and the audio hash drifts.

The skip mechanism MUST account for elapsed APU cycles on every skip:

```
skip_cycles = next_event_cycle - cpu.cycles
cpu.cycles  = next_event_cycle
bus.apu.catch_up(skip_cycles as u32)
```

Equivalently: the existing scanline loop already does this naturally if the
skip target is "end of the current scanline" — at that point the outer
`while self.cpu.cycles < target` loop exits, the next scanline's `catch_up`
will be called on the *next* CPU step (because `elapsed` is computed from a
single `cpu.step()`). To preserve audio determinism, **the skip must
synthesise an APU catch-up call for exactly the skipped master cycles**, no
more, no less. Treat the skip as if it were one giant `cpu.step()` that
consumed those cycles.

### 3.4 The "one extra iteration" question

If a real interrupt would have fired in the middle of an LDA fetch, the
65816 finishes the current instruction before taking the interrupt. After a
skip, we resume at the LDA *just before* the cycle the interrupt fires, do
one final LDA+BEQ iteration (now the byte has changed, so BEQ falls through —
or NMI fires between LDA and BEQ), and the interrupt is taken at the same
master-cycle as it would have been without the skip.

This is the only way to keep state byte-identical: **the skip target must
land us at the start of an instruction, with enough cycle budget that the
emulator's normal interrupt-pending check (line 169–180 of `cpu/mod.rs`) will
take the interrupt at the right cycle.** Skipping *past* the interrupt boundary
is forbidden.

Concretely, skip to `next_event_cycle - 1_instruction_worth` (≈ 30 master
cycles) is the safe form. The remaining one-or-two loop iterations execute
normally, and the interrupt fires at exactly the cycle it would have. The
hash is preserved.

### 3.5 What if we mis-classify the loop?

If the pattern detector fires on something that *isn't* actually a pure poll
(e.g. the loop body has a hidden side effect we missed), the skip will produce
a divergent state and the determinism hash will change.

There is no rollback. The defence is conservatism: §2.2 restricts the
optimisation to LDA-from-WRAM-or-SRAM-or-ROM only, and §2.1 starts with the
single tightest pattern shape (LDA dp / BEQ -3). False positives in that
narrow window require something genuinely pathological (a polling target in
WRAM that's also a DMA destination *and* an HDMA destination on the same
scanline — and even that case is fine, because the skip respects scanline
boundaries).

If a hash regression is discovered post-merge: gate the optimisation behind a
Cargo feature flag (`idle-skip`), default on, and turn it off if a specific
game trips it.

---

## 4. Implementation sketch

### 4.1 Where the detection lives

In `Cpu::step` (`src/cpu/mod.rs:154`), after the existing interrupt-check
block but **before** opcode fetch. If we're idle-skipping, we don't even need
to execute the LDA — we know what state it would produce.

```text
fn step(&mut self, bus) -> u64 {
    if self.stopped { ... }
    if self.waiting { ... }       // existing WAI handling
    if self.nmi_pending { ... }
    if self.irq_pending { ... }

    // NEW: idle-loop fast path
    if let Some(skip_cycles) = self.try_idle_skip(bus) {
        bus.apu.catch_up(skip_cycles as u32);
        return skip_cycles;
    }

    let opcode = self.fetch_byte(bus);
    ...
}
```

`try_idle_skip` does:

1. Look at the three bytes at `PBR:PC` — must be `A5 xx F0 FD` (Tier 1).
2. Compute the polled address: `00:00xx + DP`.
3. Call `bus.is_pure_memory(bank, addr)`. If not, return `None`.
4. Compute the next-event cycle (next scanline boundary is fine for v1).
5. Compute `skip = next_event_cycle - cpu.cycles - safety_margin`.
   `safety_margin` ≈ 30 master cycles (one LDA+BEQ iteration).
6. If `skip < some_minimum` (say 5 iterations worth ≈ 150 master cycles),
   don't bother — return `None`. Avoids thrashing on near-event edges.
7. Otherwise, advance `cpu.cycles` by `skip`, set `A` to
   `bus.read(bank, addr)` to keep flags consistent, update N/Z, return `Some(skip)`.

PC is *not* advanced. We resume at the LDA on the next call to `step`, the
remaining few iterations run normally, and the interrupt fires at the correct
boundary.

### 4.2 State to add to `Cpu`

Minimal:
- `pub idle_skip_hits: u64` — for the diagnostic histogram, mirroring
  `opcode_counts`. Lets bench prove the optimisation fired.

Optional (for Tier 2 detection):
- A small ring-buffer of last-N opcodes — but for Tier 1 we just peek at
  PBR:PC, no history needed. Keep `Cpu` lean.

### 4.3 Bus surface

Add to `src/bus.rs`:

```text
impl Bus {
    /// Returns true if reads from (bank, addr) have no side effects AND
    /// the address cannot be mutated except by NMI / IRQ / HDMA.
    pub fn is_pure_memory(&self, bank: u8, addr: u16) -> bool { ... }
}
```

Matches the §2.2 rule: WRAM mirrors, full WRAM, SRAM, ROM → true; everything
else → false. One `match` arm per region, cheap.

### 4.4 Skip-target computation

For v1, the skip target is "end of current scanline." That's known to the
outer loop in `lib.rs:228` but not currently exposed to `Cpu::step`. Either:

- Pass the per-scanline `target` cycle into `step` as a parameter (mild API
  churn), or
- Stash `current_scanline_target: u64` on `Bus` when entering the inner
  while-loop. `Cpu::step` reads it.

Prefer the second — less plumbing.

For v2, compute the true next-event time:
```
next_nmi = if nmitimen & 0x80 != 0 { cycles_until_vblank_start } else { u64::MAX };
next_irq = if irq_mode != 0 { cycles_until_v_match } else { u64::MAX };
next_hdma = cycles_until_next_scanline; // any active hdmaen
target = min(next_nmi, next_irq, next_hdma)
```

### 4.5 Rollback story

There is none. If detection is wrong, the hash changes and CI catches it.
Strategy: keep the rule narrow, ship it behind a Cargo feature flag for the
first PR, default-on, and document the flag in `bench/README.md`.

---

## 5. Validation plan

### 5.1 Mandatory gates

- `cargo run --release --bin bench rom/smw.smc` → `final_fb_hash` must equal
  `54b3eed74f9f8432`, `final_audio_hash` must equal `62300ecfc4da23e0`.
- Browser bench via `bench-cli.js` — same hashes from WASM build.
- CI workflow `.github/workflows/bench.yml` is the source of truth.

### 5.2 Additional ROMs to spot-check

- **Zelda 3 (LTTP)** — heavy NMI-driven main loop, different polling pattern.
- **F-Zero** — IRQ-driven mid-screen mode changes; tests IRQ skip target.
- **Super Metroid** — uses HDMA aggressively for window/colour effects;
  tests HDMA-as-mutator path.
- **Yoshi's Island (SA-1)** — SA-1 coprocessor is not implemented here, but
  the main CPU still polls; good "doesn't break anything weird" test.

For each: capture a 600-frame hash before the change, ensure unchanged after.
This is cheap — add them as `bench/baselines/<rom>.json` entries.

### 5.3 Unit tests for the pattern detector

Direct tests on `Cpu::try_idle_skip` (or whatever the function ends up
named):

- Empty WRAM at the polled address → skip fires, returns `Some(positive)`.
- Polled address is `$4210` → skip does **not** fire (returns `None`).
- Polled address is `$2180` → skip does **not** fire.
- Loop body is `LDA $00 ; NOP ; BEQ -3` (three-byte body, wrong shape) →
  skip does **not** fire.
- `next_event_cycle - cpu.cycles < safety_margin` → skip does **not** fire.

These run in <1ms each and catch every classification bug at the unit level.

### 5.4 Worst-case scenarios

- **False positive:** the LDA-from-WRAM pattern hits a byte that some other
  agent (DMA from polled code itself? No — by definition no non-poll code is
  running) mutates within the loop. Mitigated by restricting to pure-memory
  addresses, capping skip at scanline granularity.
- **False negative:** SMW happens to use one of the Tier 2 shapes for half
  its polling. Mitigated by measurement — if Tier 1 alone halves the
  histogram share, Tier 2 is a follow-up; if Tier 1 barely moves it, time to
  expand pattern coverage before merging.
- **APU drift:** §3.3 — solved by accounting `skip_cycles` to
  `apu.catch_up`. The audio hash is the canary: if it changes, the catch-up
  is wrong.
- **Race with HDMA writing the polled byte:** HDMA fires on scanline
  boundaries; skip target is scanline boundary; the HDMA write happens
  *after* resume. Same as un-skipped. Safe.

---

## 6. Estimated effort and risk

**Effort:** 1–2 sessions for a Tier-1-only implementation behind a feature
flag, including unit tests and ROM hash validation. The code is small (~80
lines spread across `cpu/mod.rs` and `bus.rs`); the time goes into proving
correctness, not writing logic.

**Risk ranking:**

1. **APU catch-up off-by-one** — most likely failure mode. Audio hash will
   diverge by a couple of samples. Easy to spot, easy to fix.
2. **Skip-target too aggressive** — landing past an interrupt-fire cycle.
   Framebuffer hash diverges. Fix: shrink `safety_margin`.
3. **Polled address mis-classified as pure** — say, missing a PPU register
   range. Catastrophic but easy to find by bisecting which ROM regresses.
4. **Loop-shape false positive** — some non-polling code that happens to
   match `A5 xx F0 FD`. Genuinely possible (it's only three bytes), but for
   it to matter the polled byte would have to be one that *changes due to
   code we'd be running*, which means the loop wasn't tight in the first
   place. Skip wouldn't fire long enough to matter.

**Expected upside:** if we capture all of SMW's 3.4M LDA+BEQ pairs (~50% of
all dispatches), and each skip elides ~10 iterations on average, that's
~17M instruction dispatches eliminated per 600-frame run. At a measured
~200ns per dispatch in WASM, that's ~3.4 seconds of host time saved across
the 600-frame run — roughly **10–30% wall-clock speedup**. The "10–100×
on the polling fraction" framing from the task description is correct in
isolation, but the overall frame-time win is bounded by everything else
(PPU rendering, APU mixing, bus dispatch overhead on non-polling code).

**Worst-case downside:** if it doesn't work, we revert. The Cargo feature
flag means the cost of carrying broken code is zero — it never compiles in.

---

## 7. Open questions for Nick

1. **Feature flag or always-on?** I'd default to feature-flagged in the first
   PR, flip to always-on once we have hash-clean runs on the four ROMs in §5.2.
2. **Tier 2 patterns in the first PR or follow-up?** I'd defer them. The
   histogram tells us if it matters; we shouldn't carry unmeasured complexity.
3. **Do we ever want to skip *forward across scanline boundaries*?** That's
   v2, and it's where IRQ-driven loops would benefit most. But it requires
   exposing the full event scheduler to `Cpu::step`, which is a bigger
   refactor. Defer until we've proven v1.

---

## 8. Implementation findings (2026-05-13)

Tier-1 implementation landed behind the `idle-skip` Cargo feature, default off.
The detection works as designed: 88,597 hits per 600-frame SMW run, 52% of
total frame master cycles skipped, +11% wall-clock perf. CPU semantics are
preserved — but audio determinism is not, even with extensive APU chunking
compensation.

### What works

- **Pattern detection** — fires on the canonical `A5 xx F0 FC` shape (note:
  the design doc had `FD` here; correct offset is `-4 = 0xFC` per the
  emulator's `relative8` semantics in `src/cpu/addressing.rs:184`, which
  uses PC-after-fetch as the base).
- **Pure-memory address gate** — `Bus::is_pure_memory()` correctly rejects
  all I/O regions (verified: setting `MIN_SKIP = u64::MAX` yields zero hits
  and bit-identical hashes).
- **CPU/framebuffer state preservation** — verified empirically by capping
  total hits to one (~792 master cycles skipped): `final_fb_hash` matches
  reference `54b3eed74f9f8432` bit-for-bit. The pre-loaded A register,
  N/Z flags, and PC-not-advanced strategy from §4.1 step 7 reproduce the
  unskipped state exactly.

### What doesn't work yet

- **Audio hash diverges from frame 1.** Even with the chunk-simulation
  fix (replaying the skipped span as N calls of `apu.catch_up(18)` to
  mimic the unskipped 18-master-cycle-per-step pattern), `final_audio_hash`
  is wrong. With one capped skip and chunk sim:
  - reference: `62300ecfc4da23e0`
  - actual:    `cd08a0fd6e31c868`
- **fb_hash diverges with many skips.** With the feature fully on (88K
  hits), fb_hash also drifts to `cfcd078d948adbf7`. The path: audio drift
  →  SPC writes to `bus.ports_to_main` shift in time → CPU reads of
  `$2140-$2143` see different values → branches diverge → eventually a
  PPU register write differs → framebuffer hash flips. The fb divergence
  is downstream of the audio divergence; fixing audio fixes both.

### Why naive chunk simulation isn't enough

The SPC700's `Apu::run_cycles` (`src/spc700/mod.rs:281`) uses a cycle-debt
mechanism:

```rust
self.cycle_debt += target_cycles as i64;
while self.cycle_debt > 0 {
    let inst = self.cpu.step(...);
    self.cycle_debt -= inst;
    for _ in 0..inst { /* tick timers, DSP samples */ }
}
```

This is **not** chunk-equivalent in general. A 4-cycle SPC instruction
runs as long as debt > 0 at instruction start. Many small calls produce
the same total work as one big call, but the **instruction-boundary
timing** within the SPC's run differs: when debt becomes negative after
one inst, subsequent small calls may or may not push debt back to
positive depending on the chunk sequence. Mathematically equivalent
total cycles can still produce different SPC instruction interleaving,
and the DSP samples written during `for _ in 0..inst` happen at slightly
different SPC cycle counts.

**This is the failure mode the design doc §6 risk 1 anticipated**, and
it's deeper than off-by-one APU catch-up accounting. It's an emulation
correctness vs. host-perf tradeoff that the existing emulator already
makes (per the inline comment at `spc700/mod.rs:185-188`: "prevents
overshoot amplification when catch_up is called frequently with small
cycle counts"). Our optimization exposes it.

### Possible fixes (for the next session)

1. **Make run_cycles chunk-deterministic.** Eliminate `cycle_debt` and
   instead use a precise SPC-cycle counter that runs *exactly* N cycles
   per call (possibly mid-instruction). This is a refactor of the SPC700
   timing model, not a small fix. Reference: bsnes's libco-based
   coroutine scheduler, where the SPC yields after every cycle.
2. **Match the existing chunk distribution exactly.** During a skip,
   call `apu.catch_up` with the same sequence of values that the
   unskipped path would have produced — not just `18 × N`, but the
   specific sequence including the LDA-vs-BEQ alternation and any
   timer/HDMA-induced extra catch_ups in the outer loop. Brittle.
3. **Detect across full scanline boundaries instead.** Skip to the
   end of the next scheduled event (NMI/IRQ/HDMA), not just the next
   scanline. This widens the skip but doesn't help with chunking.
4. **Accept the new hash and re-baseline.** If the audio output is
   *semantically* correct (audio still sounds right; the divergence is
   sub-perceptual), accept it and document the new reference hashes.
   The PCA comparison harness in `reference/principal_component_compare.py`
   could prove "audibly equivalent" even if bit-different.

Option 4 is the most pragmatic but only works if the audio quality is
preserved. Option 1 is the principled fix but is a multi-session refactor.

### Performance achieved (feature on, hashes broken)

- Native bench: 575 → 626 emulated_fps (+8.9%)
- Frame mean: 1738 → 1597 µs (-8.1%)
- 52% of master cycles fast-forwarded
- 88,597 hits in 600 frames (~148 hits/frame)

The win is real; the determinism contract isn't. Worth picking back up.
