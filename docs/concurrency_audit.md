# Concurrency Audit: Inter-Chip Synchronization in rust-wasm-snes

**Auditor:** Claude (concurrency sub-agent)
**Date:** 2026-05-27
**Scope:** All inter-chip communication paths; evaluation against Near/byuu's
cooperative threading standard (bsnes/higan/libco).

---

## 1. Concurrency Model Map

### 1.1 Polling Order

The frame loop lives in `src/lib.rs:192` (`run_frame_inner`). The execution
order within each scanline is:

```
for scanline in 0..262:
  1. VBlank/NMI check (scanline == 225)          [lib.rs:194-202]
  2. VBlank end / HDMA init (scanline == 0)       [lib.rs:205-209]
  3. PPU scanline number update                   [lib.rs:212]
  4. V/H IRQ fire check                           [lib.rs:216-228]
  5. Inner CPU step loop:                         [lib.rs:241-260]
     while cpu.cycles < target:
       a. HBlank transition check                 [lib.rs:242-244]
       b. CPU step (one instruction)              [lib.rs:247]
       c. cpu.cycles += elapsed                   [lib.rs:248]
       d. DMA cycle accounting (if pending)       [lib.rs:251-256]
       e. APU catch_up(elapsed)                   [lib.rs:259]
  6. HDMA + PPU render (visible scanlines)        [lib.rs:263-267]
```

### 1.2 How Many Cycles Per Poll

Each CPU `step()` returns one instruction's worth of master cycles (typically
12-42 cycles, i.e., 2-7 CPU cycles x 6 master). The APU is caught up after
every single CPU instruction.

### 1.3 Synchronization Barriers

- **Scanline boundary:** VBlank, NMI, IRQ checks happen between scanlines,
  not within them.
- **HDMA:** Runs once per visible scanline, AFTER the CPU step loop exits.
- **PPU render:** Runs once per visible scanline, AFTER HDMA.
- **DMA:** Runs inline during a CPU write to $420B; cycles added post-hoc.

### 1.4 Comparison to bsnes Cooperative Threading

In bsnes, each chip (CPU, SMP, PPU) is a libco coroutine. Memory access
functions yield to the scheduler. Synchronization happens at the cycle of
the access, not at instruction boundaries. Key differences:

| Aspect | bsnes (cooperative) | This emulator (catch-up) |
|--------|-------------------|--------------------------|
| CPU-APU sync granularity | Per memory access cycle | Per CPU instruction |
| PPU register writes | Take effect at write cycle | Take effect at write cycle (OK) |
| PPU rendering | Cycle-by-cycle pixel pipeline | Once per scanline (batch) |
| HDMA timing | Mid-scanline, cycle-positioned | End of scanline (after CPU loop) |
| IRQ/NMI delivery | Per-cycle check inside chip | Per-instruction check + per-scanline check |
| DMA halts CPU | CPU thread sleeps, others run | CPU halted, APU catch_up added post-hoc |

---

## 2. Synchronization Point Catalog

### 2.1 CPU <-> APU Communication

| Sync Point | Source | Target | Direction | Mechanism | Cycle-Exact? | JIT Sync Candidate? | Risk |
|---|---|---|---|---|---|---|---|
| CPU reads $2140-$2143 | CPU | APU | APU->CPU | `apu.cpu_read()` reads `ports_to_main` [bus.rs:141] | No (stale by up to 1 instruction) | YES (primary) | MEDIUM |
| CPU writes $2140-$2143 | CPU | APU | CPU->APU | `apu.cpu_write()` writes `ports_from_main` [bus.rs:190] | No (APU sees it on next catch_up) | YES (primary) | MEDIUM |
| DMA writes $2140-$217F | DMA | APU | CPU->APU | `apu.cpu_write()` inside DMA loop [bus.rs:365] | No (APU catch_up happens after DMA completes) | Yes | LOW |
| HDMA writes $2140-$217F | HDMA | APU | CPU->APU | `apu.cpu_write()` inside HDMA transfer [bus.rs:514] | No (HDMA runs after CPU step loop) | No (rare target) | LOW |

### 2.2 CPU <-> PPU Communication

| Sync Point | Source | Target | Direction | Mechanism | Cycle-Exact? | JIT Sync Candidate? | Risk |
|---|---|---|---|---|---|---|---|
| CPU writes $2100-$213F | CPU | PPU | CPU->PPU | `ppu.write_register()` [bus.rs:188] | Yes (immediate effect on PPU state) | No | LOW |
| CPU reads $2134-$213F | CPU | PPU | PPU->CPU | `ppu.read_register()` [bus.rs:136] | Partially (scanline counter set at scanline start, not mid-scanline) | No | MEDIUM |
| VRAM writes via $2118/$2119 | CPU | PPU | CPU->PPU | `ppu.write_register()` inside instruction | Yes | No | LOW |
| DMA to PPU ($2118 etc.) | DMA | PPU | CPU->PPU | `ppu.write_register()` inside DMA loop [bus.rs:364] | No (batch, but PPU state updates are immediate) | No | LOW |
| HDMA to PPU ($2100-$213F) | HDMA | PPU | CPU->PPU | `ppu.write_register()` inside HDMA transfer [bus.rs:513] | No (runs after CPU step loop, before render) | No | MEDIUM |

### 2.3 CPU Internal Register Communication

| Sync Point | Source | Target | Direction | Mechanism | Cycle-Exact? | JIT Sync Candidate? | Risk |
|---|---|---|---|---|---|---|---|
| $4210 RDNMI read-clear | CPU | Bus | CPU internal | `read_cpu_register` clears `nmi_flag` [bus.rs:226-229] | Per-instruction | No | LOW |
| $4211 TIMEUP read-clear | CPU | Bus | CPU internal | `read_cpu_register` clears `irq_flag` [bus.rs:231-234] | Per-instruction | No | LOW |
| $4212 HVBJOY read | CPU | Bus | CPU internal | Reads `vblank`, `hblank`, `auto_joypad_busy` [bus.rs:237-241] | HBlank is scanline-granularity, not cycle-exact | No | MEDIUM |
| $4218-$4219 auto-joypad | CPU | Joypad | Joypad->CPU | Direct read of `joypad.current` [bus.rs:248] | Not cycle-exact (auto-read is instantaneous) | No | HIGH |
| $4016 serial joypad read | CPU | Joypad | Joypad->CPU | `joypad.read_serial()` shifts bit [bus.rs:147] | Per-read (state-mutating) | No | LOW |

### 2.4 APU Internal Communication

| Sync Point | Source | Target | Direction | Mechanism | Cycle-Exact? | JIT Sync Candidate? | Risk |
|---|---|---|---|---|---|---|---|
| SPC reads $F4-$F7 | SPC | ApuBus | CPU->SPC | Reads `ports_from_main` [spc700/mod.rs:73] | Per SPC instruction (within run_cycles batch) | N/A | LOW |
| SPC writes $F4-$F7 | SPC | ApuBus | SPC->CPU | Writes `ports_to_main` [spc700/mod.rs:108] | Per SPC instruction | N/A | LOW |
| DSP sample generation | SPC | DSP | SPC->DSP | `generate_sample()` every 32 SPC cycles [spc700/mod.rs:306-310] | Per SPC cycle (internal to run_cycles) | N/A | LOW |
| Timer ticks | Timer | SPC | Timer->SPC | `timer.tick()` every 128/16 SPC cycles [spc700/mod.rs:297-303] | Per SPC cycle | N/A | LOW |

---

## 3. Stale-State Windows

### 3.1 APU Port Reads During CPU Step (MEDIUM risk)

**Location:** `bus.rs:141` — `self.apu.cpu_read((addr & 3) as u8)`

**The problem:** When the CPU reads $2140-$2143, it gets the value in
`ports_to_main` as of the last `apu.catch_up()` call. But `catch_up` happened
after the PREVIOUS CPU instruction, not the current one. The APU may have
written a new port value during the current instruction's execution window
(typically 12-42 master cycles).

**Stale-state window:** 1 CPU instruction (12-42 master cycles = ~0.5-2 SPC
instructions).

**Impact:** The CPU-APU port handshake protocol works by polling: the CPU
writes a command, then polls the APU's response port until it changes. A
0.5-2 SPC instruction lag is negligible for polling loops (they execute
thousands of iterations). However, tight multi-port protocols that depend on
reading a consistent snapshot of ports 0-3 could see inconsistent state.

**Near's JIT sync solution:** Catch up the APU to the exact cycle of the port
access, not the previous instruction boundary. This is both faster (eliminates
unnecessary syncs on ROM reads) and more accurate.

**Proposed fix (Level 1 JIT sync):**
```rust
// In bus.rs read(), replace:
(0x00..=0x3F, 0x2140..=0x217F) => self.apu.cpu_read((addr & 3) as u8),
// With:
(0x00..=0x3F, 0x2140..=0x217F) => {
    // JIT sync: catch up APU to current cycle before reading port
    // (requires passing current cycle count or accumulator)
    self.apu.cpu_read((addr & 3) as u8)
}
```

The challenge is that `bus.read()` doesn't know the current CPU cycle count.
The cycle count is tracked in `Cpu::cycles`, but `bus.read()` only has `&mut Bus`.
This is where Rust's ownership model forces a specific architecture (see Section 5).

### 3.2 APU Port Writes Not Seen Until Next Catch-Up (MEDIUM risk)

**Location:** `bus.rs:190` — `self.apu.cpu_write((addr & 3) as u8, val)`

**The problem:** When the CPU writes $2140-$2143, the value is immediately
stored in `ports_from_main`. But the SPC700 won't execute any instructions
to read that value until the next `apu.catch_up()` call, which happens after
the current CPU instruction completes.

**Impact:** Similar to 3.1 — negligible for polling protocols, but means the
SPC sees port writes slightly late. This is actually the more correct direction
of error: in real hardware, the SPC runs continuously and would read the port
asynchronously. Our lag is on the order of 1 CPU instruction.

### 3.3 HVBJOY ($4212) HBlank Bit — Scanline Granularity (MEDIUM risk)

**Location:** `lib.rs:242-244` — HBlank is set when `cpu.cycles >= hblank_start`

**The problem:** The `hblank` flag transitions from false to true within the
inner CPU step loop, which is correct (per-instruction granularity). However,
`hblank_start` is computed as `target - 68*4 = target - 272` master cycles
before scanline end. The 68-dot approximation is reasonable but not cycle-exact.

**Real hardware:** HBlank begins at dot 274 of 340 (H-counter = 274). The
emulator computes `hblank_start` from master cycles, not dots, which is
approximately correct but may be off by a few cycles depending on the CPU
instruction timing within the scanline.

**Impact:** Games that poll $4212 to detect HBlank start/end for mid-scanline
effects may see the transition at a slightly wrong cycle. Most games use
H-IRQ for precise timing, not $4212 polling.

### 3.4 Auto-Joypad Read Timing (HIGH risk)

**Location:** `lib.rs:201` — `self.bus.auto_joypad_busy = false`
**Location:** `bus.rs:248` — `self.joypad.current as u8` (JOY1L)

**The problem:** Auto-joypad read is set to "not busy" instantaneously at
VBlank start (scanline 225). In real hardware:
1. Auto-joypad read begins at the start of VBlank
2. It takes 4224 master cycles (~3.1 scanlines) to complete
3. During this period, $4212 bit 0 is set (busy)
4. Reading $4218-$421F during this period returns partial/stale data

The emulator sets `auto_joypad_busy = false` immediately, meaning:
- $4212 bit 0 never reads as 1 (auto-read busy)
- $4218-$421F always return `joypad.current` directly, with no serialization
  delay or busy period

**Impact:** This is a known source of bugs in real-game compatibility. Near's
bug documentation specifically calls out auto-joypad timing as breaking games
that read $4218-$421F too early in VBlank. Games that poll $4212 bit 0 to
wait for auto-read completion will see it always clear and proceed immediately,
which happens to work — but games that DON'T wait could read stale data on
real hardware but get correct data from this emulator, masking bugs.

**Proposed fix:** Implement a 4224-master-cycle busy window:
```rust
if scanline == VBLANK_START {
    self.bus.auto_joypad_busy = true;
    // auto_joypad_cycles_remaining = 4224;
    // Decrement in inner step loop; clear busy when zero
}
```

### 3.5 PPU Scanline Counter — Set at Scanline Start Only (LOW risk)

**Location:** `lib.rs:212` — `self.bus.ppu.scanline = scanline`

**The problem:** The PPU's scanline counter is set once at the start of each
scanline, before the CPU step loop. If the CPU reads $213D (OPVCT) mid-scanline
via $2137 (SLHV latch), the latched V-counter will be the value set at scanline
start, which is correct. But the H-counter ($213C OPHCT) is set to 0 at latch
time (`ppu.rs:508`), which is wrong — it should reflect the CPU's current
horizontal position within the scanline.

**Impact:** Games using SLHV for lightgun input or other position-dependent
reads will get incorrect H-counter values. This is a PPU accuracy issue rather
than a concurrency issue per se.

### 3.6 HDMA Timing — After CPU Step Loop (MEDIUM risk)

**Location:** `lib.rs:265` — `self.bus.hdma_run_scanline()`

**The problem:** HDMA transfers run AFTER the CPU step loop exits and BEFORE
PPU rendering. On real hardware, HDMA transfers occur at the start of HBlank
(around dot 278), while the CPU is still executing. The transfers steal CPU
cycles and the PPU sees the register changes immediately.

In this emulator:
1. CPU runs the entire scanline (1364 master cycles)
2. Then HDMA runs (modifying PPU registers)
3. Then PPU renders the scanline

This means HDMA register changes take effect just before rendering, which is
approximately correct for end-of-scanline HDMA. But HDMA that modifies scroll
registers or window positions will be off by ~68 dots (the HBlank period)
relative to real hardware.

**Impact:** Mid-frame scroll effects and raster effects driven by HDMA will
be shifted by a fraction of a scanline. For Mode 7 HDMA (common in racing
games), this can cause visible seams.

### 3.7 DMA Does Not Advance APU During Transfer (LOW risk)

**Location:** `bus.rs:308-393` — `execute_general_dma`

**The problem:** During general DMA, the CPU is halted and the DMA engine
transfers data. The total DMA cycles are accumulated in `pending_dma_cycles`
and then credited to both `cpu.cycles` and `apu.catch_up` AFTER the DMA
completes [lib.rs:251-256].

On real hardware, the APU continues running during DMA (it's independent).
In this emulator, the APU is frozen during DMA execution and then bulk-caught-up.
For large transfers (e.g., 64KB VRAM fill), this could be thousands of master
cycles where the APU doesn't run.

**Impact:** The APU's `run_cycles` with cycle_debt handles the bulk catch-up
correctly in terms of total cycles executed. The timing of DSP sample
generation and timer ticks within that window may differ from real hardware.
For typical DMA transfers (256-4096 bytes, 2048-32768 master cycles), this
is ~1-16ms of APU time batched into one call. The cycle_debt mechanism in
`spc700/mod.rs:282-316` handles this, but the instruction-boundary timing
within the batch differs from continuous execution.

---

## 4. Idle-Skip Communication Gap (T10)

### 4.1 The Mechanism

**Location:** `cpu/mod.rs:165-263` — `try_idle_skip`

When idle-skip fires, the following happens:
1. Pattern `A5 xx F0 FC` detected at current PC [cpu/mod.rs:199-208]
2. Polled address verified as pure memory [cpu/mod.rs:212]
3. Skip budget computed: `scanline_target - cycles - 80` [cpu/mod.rs:218-223]
4. A register and N/Z flags pre-loaded [cpu/mod.rs:233-236]
5. APU caught up in simulated 18-cycle chunks [cpu/mod.rs:244-250]
6. `cpu.cycles` advanced by skip amount [cpu/mod.rs:251]
7. Returns 0 (cycles already accounted for) [cpu/mod.rs:262]

### 4.2 The Communication Gap

During the skipped span, the CPU would normally execute ~N iterations of
`LDA dp / BEQ loop`. Each iteration takes ~36 master cycles (18+18). In the
unskipped path:

1. Each LDA/BEQ pair triggers `apu.catch_up(18)` twice
2. Between iterations, the outer loop checks HBlank transition
3. The APU runs ~0.85 SPC cycles per catch_up(18) call (18/21)
4. Port handshake happens at natural instruction boundaries

In the skipped path:
1. APU is caught up in a tight loop of `catch_up(18)` calls [cpu/mod.rs:244-248]
2. No CPU instructions execute (no port reads or writes)
3. If the APU writes to `ports_to_main` during this window, NO CPU code
   runs to read those ports

**Key insight from Near:** "what if we know the CPU was reading from ROM that
cannot change... the CPU may be hundreds, or even thousands, of instructions
ahead in time from the APU."

The T10 skip is the converse: the CPU is NOT ahead — both CPU and APU cycles
are advanced together via the chunked catch_up. But CPU->APU port WRITES that
would normally happen during the unskipped path are missing. If the NMI handler
(which breaks the idle loop) writes to APU ports $2140-$2143, those writes
happen at the same cycle regardless of skip. The issue is that:

**The SPC700 sees a different chunk distribution of catch_up calls.**

### 4.3 Root Cause of Audio Divergence

**Location:** `spc700/mod.rs:281-317` — `run_cycles`

The `cycle_debt` mechanism:
```rust
self.cycle_debt += target_cycles as i64;
while self.cycle_debt > 0 {
    let inst_cycles = self.cpu.step(&mut self.bus) as i64;
    self.cycle_debt -= inst_cycles;
    for _ in 0..inst_cycles { /* tick timers, sample DSP */ }
}
```

This is NOT chunk-equivalent. Consider two scenarios delivering 36 total
SPC cycles:

**Scenario A** (unskipped): `catch_up(18)` then `catch_up(18)`
- Call 1: debt = 0 + 18/21 ~= 0 SPC cycles (frac accumulates)
  Actually: `cycle_frac += 18; spc_cycles = 18/21 = 0; frac = 18`
- Call 2: `cycle_frac = 18 + 18 = 36; spc_cycles = 36/21 = 1; frac = 15`
  So 1 SPC cycle executed, debt goes to 1 - (instruction_cost), where
  instruction_cost might be 2-8 SPC cycles.

**Scenario B** (chunked differently): `catch_up(36)` at once
- `cycle_frac += 36; spc_cycles = 36/21 = 1; frac = 15`
  Same result! Actually this specific case IS equivalent.

The real divergence comes from the fractional accumulator state. After many
iterations, the sequence of `cycle_frac % 21` values differs between:
- 600 calls of `catch_up(18)` (frac sequence: 18, 15, 12, 9, 6, 3, 0, 18, ...)
- 300 calls of `catch_up(36)` (frac sequence: 15, 9, 3, 18, 12, 6, 0, 15, ...)

These sequences trigger `run_cycles(spc_cycles)` at different points, which
means SPC instructions execute at different cycle boundaries, which means DSP
samples are generated at slightly different SPC cycle counts.

**This is the fundamental mechanism of the T10 audio divergence.** The
`catch_up` function's integer division `cycle_frac / 21` is not distributive:
`floor((a+b)/21) != floor(a/21) + floor(b/21)` in general.

### 4.4 Additional Risk: Port Communication During Skip

If the game's idle loop polls a WRAM byte that is set by the NMI handler, AND
the NMI handler also writes to APU ports $2140-$2143 as part of its music
driver communication, then:

- In the unskipped path: CPU executes NMI, writes APU ports, APU catch_up
  happens after each NMI instruction, SPC sees port data incrementally.
- In the skipped path: Same — NMI fires normally after skip ends. The NMI
  handler's APU port writes happen at the same cycle.

This specific case is actually safe because the skip ends BEFORE the NMI fires,
and normal execution resumes. The NMI handler runs identically in both paths.

The UNSAFE case would be: a skip that crosses an NMI boundary. The current
implementation bounds skips at the scanline target [cpu/mod.rs:218], and NMI
fires between scanlines [lib.rs:194-199], so this cannot happen. This is a
correct design decision.

---

## 5. Borrow Topology Audit

### 5.1 Ownership Graph

```
Emulator
  +-- cpu: Cpu              (owns all CPU registers)
  +-- bus: Bus              (owns all shared state)
       +-- cart: Cartridge   (ROM + SRAM)
       +-- wram: [u8; 128K] (main work RAM)
       +-- ppu: Ppu          (video state + VRAM/OAM/CGRAM)
       +-- apu: Apu          (audio state)
       |    +-- cpu: Spc700   (SPC700 registers)
       |    +-- bus: ApuBus   (APU RAM + DSP + timers + ports)
       +-- dma: Dma          (DMA channel registers)
       +-- joypad: Joypad    (button state)
       +-- (scalar registers: nmitimen, htime, vtime, etc.)
```

### 5.2 Critical Borrow Constraint: CPU Cannot Access Bus During Step

`Cpu::step(&mut self, bus: &mut Bus) -> u64` takes `&mut Bus` as a parameter
rather than owning it. This means:

1. The CPU borrows the Bus mutably during `step()`
2. The frame loop in `lib.rs` owns both `cpu` and `bus` as separate fields
3. After `step()` returns, the frame loop can access both independently

This forces the execution model where `apu.catch_up()` happens AFTER
`cpu.step()` returns, not during it. In bsnes, the cooperative threading model
allows the CPU's memory access function to yield to the APU mid-instruction.
Rust's borrow checker makes this impossible without unsafe code or interior
mutability.

### 5.3 JIT Sync Blocked by Borrow Topology

To implement JIT sync (catch up APU on port access), we need to call
`self.apu.catch_up(cycles_since_last_sync)` from inside `Bus::read()`.
This is possible because `apu` is a field of `Bus` and `Bus::read(&mut self)`
has mutable access. But we need to know "cycles since last sync," which lives
in `Cpu::cycles` — and the CPU is borrowed by its own `step()` method.

**Solutions, in order of invasiveness:**

1. **Pass cycle info into Bus:** Add `bus.current_cycle: u64` and update it
   at each CPU memory access. `Bus::read` can then compute `current_cycle -
   bus.last_apu_sync_cycle` and call `apu.catch_up()`. This is the cleanest
   Level 1 JIT sync approach.

2. **Interior mutability on Apu:** Use `RefCell<Apu>` to allow catch_up calls
   from multiple contexts. Adds runtime overhead and complexity.

3. **Split Bus into sub-structs:** Separate APU-accessible state from
   PPU-accessible state. Allows finer-grained borrows but major refactor.

### 5.4 DMA Borrow Workaround

**Location:** `bus.rs:308-393` — `execute_general_dma`

The DMA implementation is inlined into `Bus` as a method rather than using the
standalone `dma::execute_dma` function (which exists at `dma.rs:118-177` but
is unused). The comment at `bus.rs:307` explains: "Inlined here to avoid
borrow-checker issues with closures over `self`."

The standalone version uses closures (`read_a`, `write_b`, etc.) which would
require splitting the mutable borrow of `Bus` — Rust won't allow passing
`&mut self.cart` and `&mut self.ppu` as separate closure captures when `self`
is already borrowed. The inlined version works because method calls on `self`
fields within a method body are fine (Rust's borrow checker understands
field-level borrows within a method).

---

## 6. Cooperative Threading Migration Assessment

### Level 0: Current State (Catch-Up, Per-Instruction)

**What it is:** CPU runs one instruction, then APU catches up for the same
number of master cycles. PPU renders per-scanline.

**Accuracy:** Adequate for most games. APU port communication has ~1 instruction
lag. PPU is scanline-accurate, not cycle-accurate. HDMA timing is off by
~68 dots.

**Performance:** Good baseline. The per-instruction APU catch_up is actually
more frequent than necessary — most instructions don't touch APU ports.

### Level 1: JIT Sync (Recommended Next Step)

**What it is:** Only synchronize the APU when the CPU accesses ports $2140-$2143.
All other CPU instructions run without APU sync. When a port access occurs,
catch up the APU to the exact cycle of the access.

**Implementation path:**
1. Add `bus.master_cycle_counter: u64` — updated in the frame loop after each
   `cpu.step()` call.
2. Add `bus.last_apu_sync_cycle: u64` — records when APU was last caught up.
3. In `Bus::read` and `Bus::write` for $2140-$217F:
   ```rust
   let delta = self.master_cycle_counter - self.last_apu_sync_cycle;
   if delta > 0 {
       self.apu.catch_up(delta as u32);
       self.last_apu_sync_cycle = self.master_cycle_counter;
   }
   ```
4. At end of each scanline, catch up APU for any remaining cycles.

**Benefits:**
- MORE accurate than current per-instruction sync (APU is at the exact cycle
  of the port access, not the previous instruction boundary)
- FASTER: eliminates ~95% of catch_up calls (only ~2-5 port accesses per
  scanline vs ~60-100 instructions per scanline)
- Fixes the T10 audio divergence: during idle-skip, there are no port accesses,
  so no catch_up calls happen. The bulk catch_up at scanline end delivers all
  cycles in one call — which is IDENTICAL to what JIT sync would do in the
  unskipped case (no port accesses in the idle loop either)

**Risk:** LOW. The only behavior change is WHEN `catch_up` is called within a
scanline. Total cycles per scanline are identical. Port values are more accurate.

**Estimated effort:** ~50 lines of code. Add two u64 fields to Bus, modify
read/write for $2140-$217F, add end-of-scanline flush.

### Level 2: Cooperative Threading via corosensei

**What it is:** Each chip runs as a stackful coroutine using the `corosensei`
crate (a Rust equivalent of libco). Memory access functions yield to a
scheduler.

**Status:** NOT feasible for WASM. `corosensei` requires stack switching,
which WASM's structured control flow does not support. The `wasm32` target
would need Asyncify or the WASM stack switching proposal (Phase 2 as of 2026).
Native builds could use it.

### Level 2b: Cooperative Threading via async/await

**What it is:** Each chip is an async task. Memory access functions `.await`
a scheduler future. A single-threaded executor polls all tasks.

**Status:** Partially feasible in WASM. Rust's `async/await` compiles to
state machines, which WASM can run. But the overhead is significant:
- Every memory access becomes a potential yield point (poll/resume cycle)
- State machine codegen bloats WASM binary
- Cannot yield from deeply nested instruction handlers without making the
  entire call chain async

**Assessment:** Not recommended. The async transform changes the code structure
profoundly and adds overhead on the hot path. JIT sync (Level 1) provides
better accuracy than per-instruction catch-up with LESS overhead, not more.

### Migration Recommendation

**Level 1 JIT sync is the clear winner.** It is:
- Simpler than the current per-instruction model (fewer catch_up calls)
- More accurate (sync at port-access cycle, not instruction boundary)
- Faster (95% fewer catch_up invocations)
- Fixes T10 audio divergence as a side effect
- ~50 lines of code
- No architectural change needed

Level 2/2b should be deferred until WASM gets stack switching support or until
a game requires sub-instruction synchronization accuracy.

---

## 7. Dynamic Rate Control for AudioWorklet

### 7.1 Near's DRC Formula

```
dynamicFrequency = ((1.0 - maxDelta) + 2.0 * fillLevel * maxDelta) * outputFrequency
```

Where:
- `maxDelta = 0.005` (0.5% maximum pitch distortion)
- `fillLevel` = current audio buffer fill level as fraction (0.0 = empty, 1.0 = full)
- `outputFrequency` = system audio output rate (typically 44100 or 48000 Hz)

When buffer is half-full (`fillLevel = 0.5`): `dynamicFrequency = outputFrequency` (no adjustment).
When empty (`fillLevel = 0.0`): `dynamicFrequency = 0.995 * outputFrequency` (slow down to fill).
When full (`fillLevel = 1.0`): `dynamicFrequency = 1.005 * outputFrequency` (speed up to drain).

### 7.2 Application to Phase B AudioWorklet

The Phase B design (per `PHASE_B_PLAN.md`) routes audio through a Web Worker
to an AudioWorklet. The AudioWorklet's `process()` callback pulls samples from
a SharedArrayBuffer ring buffer.

**DRC integration point:** The AudioWorklet process callback knows:
- How many samples are in the ring buffer (fillLevel)
- The output sample rate (from AudioContext)

It should compute `dynamicFrequency` and resample accordingly. The resampling
ratio is `apuOutputRate / dynamicFrequency` where `apuOutputRate` is the
emulator's native output rate.

### 7.3 Critical Frequency Correction

Near documents that the real SNES APU oscillator runs at approximately
**32040 Hz** (× 768 = ~24.607 MHz), NOT the commonly assumed 32000 Hz.
The emulator currently generates samples at:

- DSP sample rate: 1 sample every 32 SPC cycles [spc700/mod.rs:306]
- SPC clock: master_cycles / 21 [spc700/mod.rs:325]
- Master cycles per frame: 1364 × 262 = 357,368
- SPC cycles per frame: 357,368 / 21 ≈ 17,017.5
- Samples per frame: 17,017.5 / 32 ≈ 531.8 stereo pairs
- At 60 fps: 531.8 × 60 ≈ 31,909 Hz

This is ~0.4% below the real 32040 Hz. Without DRC, this means the audio
buffer will slowly drain (underflow every ~250 frames ≈ 4.2 seconds, producing
audible pops).

**Recommendation:** The AudioWorklet should:
1. Set `apuOutputRate = 32040` (not 32000) as the nominal input rate
2. Apply Near's DRC formula with `maxDelta = 0.005`
3. Use linear interpolation for resampling (cubic is better but the 0.5%
   adjustment makes the difference inaudible)

### 7.4 DRC and T10 Interaction

With JIT sync (Level 1), the T10 idle-skip becomes audio-safe. The DRC
formula handles any residual sample rate variation from the skip. The buffer
fill level self-corrects: if the skip produces slightly fewer or more samples
in a burst, DRC adjusts the resampling ratio to compensate. This is exactly
the scenario Near designed DRC for.

---

## 8. Summary of Findings

### Critical Issues (should fix)

| # | Issue | Location | Impact | Fix |
|---|---|---|---|---|
| C1 | Auto-joypad has no busy period | lib.rs:201 | Masks timing bugs; breaks games that don't poll $4212 | Implement 4224-cycle busy window |
| C2 | APU sync is per-instruction, not per-port-access | lib.rs:259, bus.rs:141,190 | Sub-optimal accuracy AND performance | Level 1 JIT sync (~50 LOC) |

### Important Issues (should fix, lower priority)

| # | Issue | Location | Impact | Fix |
|---|---|---|---|---|
| I1 | HDMA runs after CPU step loop | lib.rs:265 | Raster effects off by ~68 dots | Move HDMA into HBlank check inside step loop |
| I2 | T10 audio divergence | cpu/mod.rs:244-250 | Idle-skip breaks audio determinism | JIT sync (C2) fixes this as side effect |
| I3 | DMA freezes APU | bus.rs:308-393 | Batched APU catch-up during DMA | Add periodic APU catch_up inside DMA loop |
| I4 | H-count IRQ fires per-scanline, not at H-position | lib.rs:219 | H-IRQ timing is scanline-granular | Track H-position within scanline |

### Low-Priority Issues (nice to have)

| # | Issue | Location | Impact | Fix |
|---|---|---|---|---|
| L1 | SLHV latch H-counter always 0 | ppu/mod.rs:508 | Lightgun and H-counter reads wrong | Compute H-position from CPU cycle within scanline |
| L2 | PPU rendering is per-scanline | ppu/render.rs | No mid-scanline register change effects | Would require cycle-accurate PPU (massive effort) |
| L3 | `cycle_debt` is not chunk-equivalent | spc700/mod.rs:282-316 | Different chunk sequences produce different timing | Refactor to precise cycle counter (large effort; JIT sync makes this unnecessary for T10) |

### Recommended Priority Order

1. **C2: Level 1 JIT sync** — ~50 LOC, fixes C2 + I2 simultaneously, improves
   both accuracy and performance. This is the single highest-value change.
2. **C1: Auto-joypad busy period** — ~20 LOC, needed for game compatibility.
3. **I1: HDMA in HBlank** — ~30 LOC, fixes raster effect timing.
4. **I3: DMA APU catch-up** — ~10 LOC, add periodic sync inside DMA loop.
5. **I4: H-IRQ position tracking** — ~40 LOC, requires tracking dot position.

### The Punch Line

This emulator's catch-up model is fundamentally sound. The two-processor SNES
(ignoring the PPU, which only communicates via register writes and scanline
rendering) requires exactly one synchronization point between CPU and APU:
the four I/O ports at $2140-$2143. Near's JIT sync insight — synchronize
only on shared memory access — maps perfectly to this architecture.

The current per-instruction catch_up is both slower and less accurate than JIT
sync. Implementing JIT sync is a ~50 line change that simultaneously:
- Eliminates ~95% of `catch_up` calls (performance win)
- Makes port reads cycle-exact (accuracy win)
- Fixes the T10 idle-skip audio divergence (unblocks the idle-skip feature)
- Simplifies the DRC integration for AudioWorklet (T13)

This is the rare optimization that improves every metric at once. It should be
the next thing built.
