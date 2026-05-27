---
name: snes-async-audit
argument-hint: "[scope: 'full', 'apu', 'dma', 'ppu', or a specific sync point]"
description: >
  Two-pass audit of inter-chip synchronization in an SNES emulator (Rust + WASM).
  The SNES is three concurrent processors (65816 CPU, SPC700 APU, S-PPU) serialized
  into a single-threaded catch_up loop. This skill audits the serialization strategy
  against Near/byuu's cooperative threading framework (the authoritative reference for
  SNES emulator concurrency). Evaluates: synchronization points, stale-state windows,
  JIT sync opportunities, borrow topology, scheduler design, cooperative serialization
  compatibility, and dynamic rate control for AudioWorklet. Pass 1: read-only audit
  writes async.tmp. Pass 2: fix agent applies ordering fixes, writes synchronization
  contract tests, and produces ASYNC_MODEL.md.
---

# Async Concurrency Audit (SNES Emulator -- Rust/WASM)

Two-pass audit of the emulator's concurrency model. You are the orchestrator.

## Why This Skill Exists

Every other audit skill examines a single chip in isolation. The hardest bugs
live in the spaces *between* chips: the T10 audio divergence (APU sees stale
ports during idle-skip), the DMA-in-bus borrow problem (DMA needs &mut Bus
while Bus owns DMA), the catch_up chunking sensitivity. These are all symptoms
of serializing concurrent hardware into a single thread.

## Authoritative Reference: Near's Emulation Articles

This audit is grounded in Near/byuu's emulation articles
(https://github.com/higan-emu/emulation-articles), the definitive work on
emulator concurrency. Near created bsnes/higan, the most accurate SNES emulator
ever built, and wrote libco (a userspace cooperative threading library)
specifically because the catch_up pattern cannot handle mid-instruction
synchronization correctly.

The key articles and their relevance:

### Cooperative Threading (design/cooperative-threading)

Near identifies three approaches to serializing parallel hardware:

1. **Preemptive OS threads**: Fail because kernel transitions are too expensive
   for the tens of millions of context switches per emulated second.

2. **State machines**: Work but produce exponentially complex code. A CPU with
   instruction cycles, bus hold delays, and subcycles needs nested
   `switch(cycle) { switch(subcycle) { ... } }` for every opcode. Near:
   "a massive increase in technical complexity."

3. **Cooperative threading**: Each chip runs in its own coroutine with its own
   stack. Complex state machines collapse back to natural sequential code.
   Yields happen implicitly in memory access functions:

   ```cpp
   void CPU::wait(uint clockCycles) {
       apuCounter += clockCycles;
       while(apuCounter > 0) yield(apu);
   }
   ```

   The CPU automatically yields to the APU whenever it's ahead. The scheduler
   is implicit in the counter arithmetic.

**The JIT synchronization optimization** (the most important insight for us):

> "What we've done here is made it so the CPU will keep on running until it
> tries to read from a shared memory region with the APU. Only then will it
> catch up the APU to the CPU before performing the read. If either the CPU
> doesn't need to synchronize to the APU often, *or vice versa*, then the
> number of context switches per second drops *dramatically*. In the case of
> the SNES, my emulator bsnes only has to synchronize the CPU and APU a few
> thousand times a second, instead of millions of times a second."

This is directly applicable: our emulator catches up the APU on every CPU
instruction. JIT sync would only catch up on $2140-$2143 access, reducing
synchronization overhead by 1000x while *improving* correctness (the APU
runs to the exact cycle of the port access, not to an approximate batch
boundary).

**The performance trade-off:**

> "In my admittedly non-scientific personal observations, if you can emulate a
> processor using a single-level state machine, it tends to run faster than a
> cooperative-threaded approach by a good margin. Whereas when you get into two
> (or even three or more) levels of state machines... the cooperative threading
> model comes out as the clear winner on performance."

Our SPC700 is a single-level state machine (fetch-decode-execute). The 65816
has two levels (instruction + addressing mode). The PPU has three levels
(frame, scanline, pixel). This suggests cooperative threading is net positive
for the full system even if the SPC700 alone might be faster as a state machine.

### Cooperative Serialization (design/cooperative-serialization)

Save states are the hard problem for cooperative threading. Near defines three
methods:

**Method 1 -- Fast Synchronization**: Disable scheduler, let each thread run to
its entry point. Fast but breaks determinism:

> "The above code breaks determinism by allowing the CPU to call `bus.read(PC)`
> even though it might be ahead of the APU, and the APU may write to the address
> that PC points at."

Near identifies a subtle deeper problem with JIT sync:

> "what if we know the CPU was reading from ROM that cannot change, or from a
> memory range that the APU has no access to?... the CPU may be hundreds, or
> even thousands, of instructions ahead in time from the APU... So what happens
> when you allow the CPU to complete that one single read is that, in rare
> cases, that read may happen thousands of instructions ahead of the APU, which
> is a *far* more serious transgression of synchronization."

**This directly explains our T10 audio divergence.** When idle-skip fast-forwards
the CPU through a polling loop, the APU catch-up becomes a single large chunk.
The CPU may be thousands of instructions ahead. The APU doesn't see intermediate
port states. Near hit exactly this problem.

**Method 2 -- Strict Synchronization**: Detect when advancing one thread
desynchronizes another, retry. Correct but may not terminate:

> "the more threads we emulate, and the more those threads talk to each other,
> the less likely this method will be to ever finish."

**Method 3 -- Hibernation**: Serialize native stacks directly. Not portable to
disk (ASLR), but deterministic. Ideal for rewind and run-ahead.

**Recommendation**: Disk saves use Method 1 + fallback to Method 2. Rewind /
run-ahead use Method 3. Near's 12+ years of bsnes experience:

> "I have never had a single report from a user where a manual save state
> (using methods 1 and 2) have resulted in failure."

Our current snapshot system is essentially Method 1 without cooperative
threading. The audit should assess whether our save state determinism is
affected by the catch-up model.

### Schedulers (design/schedulers)

Two approaches to tracking relative time:

**Relative schedulers** (bsnes): One signed 64-bit integer per chip pair.
CPU steps N clocks -> subtract `N * SMP_frequency` from `cpu_smp`. SMP steps
N clocks -> add `N * CPU_frequency`. Counter >= 0 means CPU is ahead.
- Simple, fast, perfect for SNES (3-4 chips)
- Scales as O(processors^2) -- bad for Sega CD but fine for SNES
- 64-bit gives 17,474 seconds of headroom

**Absolute schedulers** (higan): Each thread has a 64-bit unsigned timestamp
with attosecond precision (`Second = 2^63 - 1`). Periodically normalize by
subtracting the minimum. N:N relationships without per-pair tracking.

Our emulator uses `cycle_target: u64` (absolute, monotonically increasing)
for the APU. The audit should assess whether this is the right choice and
whether we need per-pair relative tracking for CPU<->APU vs CPU<->PPU.

### Dynamic Rate Control (audio/dynamic-rate-control)

The solution to AudioWorklet timing. SNES audio and video cannot be perfectly
synchronized statically because:
- CPU oscillator: ~21.477 MHz (varies by manufacturing tolerance)
- APU oscillator: ~24.607 MHz (not ~24.576 MHz as commonly assumed)
- Host audio clock: 48kHz (browser-controlled, independent of emulation)
- Host display: 60Hz (may drift from SNES's ~60.098Hz)

Near's solution: **continuously adjust the resampling ratio** to keep the audio
buffer approximately half-full:

```
dynamicFrequency = ((1.0 - maxDelta) + 2.0 * fillLevel * maxDelta)
                   * outputFrequency
```

With `maxDelta = 0.005` (0.5% max pitch distortion), this produces inaudible
stretching that prevents both buffer underflow (pops) and overflow (latency
growth). This is directly applicable to our Phase B AudioWorklet design (T13).

### Emulation Bugs: SNES (emulation-bugs/snes)

Near documents specific games broken by synchronization errors:
- Wolverine, World Masters Golf, SpellCraft: auto-joypad polling timing
- Several games: HVBJOY register ($4212) timing sensitivity

These are compatibility targets for the sync audit.

## The SNES Hardware Concurrency Model

The SNES is an inherently async system:
- **CPU** (65816): 3.58 MHz (FastROM) / 2.68 MHz / 1.79 MHz depending on region
- **APU** (SPC700 + S-DSP): 1.024 MHz, independent clock, independent oscillator
- **PPU** (S-PPU1 + S-PPU2): 5.37 MHz dot clock, scanline-driven
- **DMA/HDMA**: steals CPU bus cycles, halts CPU during transfer

Communication channels:
- **APU I/O ports** ($2140-$2143 CPU side, $F4-$F7 APU side): 4-byte
  bidirectional mailbox, asynchronous. This is the critical sync point.
- **PPU registers** ($2100-$213F): CPU writes, PPU reads on its own clock
- **DMA**: bulk transfer that freezes the CPU, mediates A-bus <-> B-bus
- **HDMA**: per-scanline DMA that runs during H-blank
- **NMI/IRQ**: asynchronous interrupt signals from PPU to CPU

The emulator serializes all of this into `run_frame()` -> scanline loop ->
`cpu.step()` + `apu.catch_up()` + `ppu.render_scanline()`. This audit examines
whether that serialization preserves observable behavior, using Near's framework
as the benchmark for what "correct" looks like.

## Rust-Specific Considerations

### Stackful vs Stackless Coroutines

Near explicitly notes that C++20 stackless coroutines are unsuitable:

> "Probably the biggest limitation for it is that C++20 coroutines are
> stackless, which vastly limits their potential for anything of even moderate
> complexity."

Rust's `async/await` is also stackless. For the higan cooperative threading
model, we need **stackful** coroutines. Options:

| Approach | Crate | Stable? | WASM? | Notes |
|----------|-------|---------|-------|-------|
| Stackful coroutines | `corosensei` | Yes | No (x86/ARM only) | Closest to libco. 4.3ns/switch. Used by wasmtime. |
| Nightly coroutines | `#[coroutine]` | No | Yes (stackless) | Works for simple chips, breaks for deep nesting |
| Manual state machine | (hand-rolled) | Yes | Yes | Current approach. Correct but complex. |
| OS threads + sync | `std::thread` | Yes | No (WASM is single-threaded) | Not viable for browser target |

**The WASM constraint is the key blocker.** `corosensei` doesn't support WASM.
Nightly `#[coroutine]` is stackless. The full higan model may not be portable
to our browser target. The audit must assess whether JIT synchronization within
the current catch-up model can achieve most of the correctness benefits without
requiring stackful coroutines.

### The Borrow Topology Problem

Rust's ownership model enforces a specific serialization structure:

```
Emulator owns:
  Bus owns:
    PPU (exclusive)
    APU (exclusive)
    DMA (exclusive -- but DMA logic is in Bus methods)
    Joypad (exclusive)
    WRAM (inline array)
  CPU (exclusive, separate from Bus)
```

In cooperative threading, each chip owns its own state and communicates through
message-passing or shared-with-synchronization. In our model, the Bus owns
everything, and `&mut Bus` is threaded through CPU execution. This creates:

- **DMA in bus**: DMA needs `&mut Bus` to read/write while Bus owns DMA.
  Solved by inlining DMA into bus.rs (186 lines in the wrong module).
- **APU catch_up**: CPU step has `&mut Bus`, APU is inside Bus. Solved by
  calling `bus.apu.catch_up()` directly after step.
- **PPU render**: Same ownership issue. Solved by rendering at scanline boundary.

The cooperative threading model would restructure this: each chip owns its state,
the scheduler owns the communication channels. The audit should assess whether
this restructuring is worth the effort for correctness vs whether JIT sync
within the current structure is sufficient.

## Connection to Categorical Findings

The category sweep found:
1. **Pure-memory stratification**: `is_pure_memory()` separates the bus into a
   directed container (comonad) on pure memory and an indexed state monad on
   MMIO. This is exactly the JIT sync boundary -- pure memory accesses don't
   need synchronization, MMIO accesses do.
2. **Distributive law**: `catch_up(a+b) = catch_up(a); catch_up(b)` holds for
   cycle counting (after the cycle_target fix). This means catch_up chunking
   doesn't matter for *timing*. But it may matter for *communication*.
3. **Container morphism**: DMA was factored through bus.write(). This ensures
   DMA and CPU writes take the same synchronization path.

The async audit extends these: the categorical structure tells you *which*
operations commute. The async audit asks *does the execution order respect that?*

## Pass 1: Async Audit Agent (read-only)

Launch an agent with the prompt below. It writes `async.tmp` in the project root.

**Do not proceed to Pass 2 until Pass 1 completes and you have confirmed `async.tmp` exists.**

### Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are a concurrency auditor for an SNES emulator written in Rust, compiled to
WASM. The SNES has three independent processors (CPU, APU, PPU) that the emulator
serializes into a single thread. Your job is to find where that serialization
violates the concurrent behavior of real hardware.

Your audit is grounded in Near/byuu's cooperative threading framework — the
definitive work on SNES emulator concurrency. Near created bsnes/higan and wrote
libco specifically because the catch_up pattern cannot handle mid-instruction
synchronization correctly. You should evaluate this emulator's catch_up model
against Near's standard, identify where it falls short, and propose the minimal
fixes that close the gap.

You produce exactly one file: `async.tmp` in the project root. You are READ-ONLY.

## Essential Context from Near's Articles

### The Three Synchronization Approaches

Near identifies three ways to serialize parallel hardware:

1. **State machines**: Explicit cycle tracking. Works but produces nested
   switch/case hell for multi-level timing (instruction → bus hold → subcycle).
   Our emulator uses this for the SPC700.

2. **Cooperative threading (libco)**: Each chip is a coroutine with its own
   stack. Yields happen in memory access functions. Natural code, no state
   machines. bsnes uses this. NOT available in WASM (needs stackful coroutines).

3. **Catch-up (our model)**: Run one chip ahead, then batch-advance the others.
   Simpler than cooperative threading but introduces stale-state windows.

### JIT Synchronization (the key optimization)

Near's most important insight for catch-up emulators: don't synchronize every
cycle. Only synchronize when accessing shared memory. For SNES CPU↔APU, this
means only on $2140-$2143 port access. Drops context switches from millions
to thousands per second while IMPROVING correctness — the APU runs to the
exact cycle of the port access.

Our emulator catches up the APU after every CPU instruction. This is both
slower (unnecessary syncs on ROM/WRAM reads) and less accurate (batch boundary
doesn't align with port access cycle) than JIT sync. The audit should measure
the gap.

### The Serialization Problem (explains T10 audio)

Near on JIT sync's save-state problem:

"what if we know the CPU was reading from ROM that cannot change... the CPU
may be hundreds, or even thousands, of instructions ahead in time from the
APU... So what happens when you allow the CPU to complete that one single read
is that, in rare cases, that read may happen thousands of instructions ahead
of the APU."

This is EXACTLY the T10 idle-skip problem. When the CPU skips N idle cycles,
it's thousands of instructions ahead of the APU. The APU catch_up happens as
one bulk call. Intermediate CPU→APU port writes that would have occurred in the
unskipped path are lost. Games using port handshake protocols are affected.

### Scheduler Design

Near defines two scheduler types:
- **Relative** (bsnes): signed i64 per chip pair. Simple, perfect for SNES.
- **Absolute** (higan): u64 timestamp per chip, attosecond precision.

Our emulator uses cycle_target (u64, absolute, monotonic) for the APU. The
audit should assess whether this is the right model.

### Dynamic Rate Control (for AudioWorklet)

Near's solution to audio/video sync drift: continuously adjust the resampling
ratio to keep the audio buffer half-full. Formula:

  dynamicFrequency = ((1.0 - maxDelta) + 2.0 * fillLevel * maxDelta) * outputFrequency

With maxDelta = 0.005 (0.5% max pitch distortion). Key fact: the real SNES APU
oscillator runs at ~32040 * 768 Hz (~24.607 MHz), not the commonly assumed
32000 * 768 Hz (~24.576 MHz). This 0.125% frequency error causes buffer
underflow every ~10 seconds if not corrected.

This directly applies to our T13 AudioWorklet work.

## Steps

### 1. Map the Concurrency Model

Read the project's CLAUDE.md, docs/ARCHITECTURE.md, docs/CATEGORY_THEORY.md,
and docs/T10_IDLE_LOOP_DETECTION.md for context.

If user arguments were provided, scope to that area: $ARGUMENTS

Read the frame loop in `src/lib.rs` (the `run_frame_inner` / `run_frame`
methods). Map:
- What is the polling order? (CPU step, then APU catch_up, then PPU render?)
- How many cycles does each poll advance?
- Where are the synchronization barriers? (scanline boundaries, V-blank, etc.)
- How does this compare to bsnes's cooperative threading model?

### 2. Catalog Synchronization Points

For each pair of communicating chips, find every synchronization point in the
code. A sync point is a place where one chip reads or writes state owned by
another.

Build a table:

| Sync Point | Source Chip | Target Chip | Direction | Mechanism | Cycle-Exact? | JIT Sync Candidate? |
|------------|-------------|-------------|-----------|-----------|-------------|---------------------|
| APU port write | CPU | APU | CPU -> APU | bus.write($2140-$2143) | ? | YES -- critical |
| APU port read | CPU | APU | APU -> CPU | bus.read($2140-$2143) | ? | YES -- critical |
| PPU reg write | CPU | PPU | CPU -> PPU | bus.write($2100-$213F) | ? | Yes (mid-scanline) |
| NMI signal | PPU | CPU | PPU -> CPU | bus.nmi_flag | ? | No (scanline boundary) |
| DMA trigger | CPU | DMA | CPU -> DMA | bus.write($420B) | ? | No (instant) |
| HDMA | PPU timing | DMA -> PPU | PPU clock -> DMA -> PPU regs | ? | No (H-blank) |
| Auto-joypad | Controller | CPU | external -> CPU | $4218-$421F | ? | Near's bug list! |
| ...etc | | | | | | |

The "JIT Sync Candidate?" column is the key output: which sync points should
trigger a forced catch_up of the target chip? Near's JIT optimization says:
only sync at shared memory boundaries. For SNES, the critical boundary is
the CPU↔APU port region ($2140-$2143).

### 3. Identify Stale-State Windows

For each sync point, determine whether there's a window where one chip could
read stale state from another. Trace the execution:

1. When does chip A write the value?
2. When does chip B's catch_up advance past that write?
3. Is there a gap where B could read the old value?

The most important windows to check:
- **APU port reads during CPU step**: when the APU executes a MOVW from port
  $F4-$F7, does it see the CPU's most recent write to $2140-$2143?
- **PPU register state during mid-scanline writes**: if the CPU writes BGXHOFS
  at cycle 100 of a scanline, does the PPU see it for the remaining pixels?
- **NMI timing**: NMI fires at scanline 225. Is bus.nmi_flag set at the exact
  cycle, or at the scanline boundary?
- **HDMA setup**: HDMA channels are loaded during init (scanline 0). Are the
  source addresses read at the right cycle?
- **DMA during catch_up**: if DMA triggers mid-instruction, when does the APU
  see the DMA'd data?
- **Auto-joypad polling** ($4218-$421F): Near's bug list shows this breaks
  Wolverine, World Masters Golf, SpellCraft when timing is wrong. Check our
  implementation against the known failure modes.

### 4. Audit the Idle-Skip Communication Gap (T10)

This is the most important section. Read `src/cpu/mod.rs` (the `try_idle_skip`
method) and trace what happens to inter-chip communication during a skip.

Near's serialization article describes exactly this failure mode:

"the CPU may be hundreds, or even thousands, of instructions ahead in time
from the APU... this is a far more serious transgression of synchronization."

Trace specifically:
1. What does the CPU's polling loop look like? (LDA $xx / BEQ back)
2. In the unskipped path, does the CPU write to any MMIO during the loop?
   (No -- idle-skip is gated on is_pure_memory(). But the APU may be writing
   to its ports expecting the CPU to read them.)
3. Does the APU's behavior depend on the CPU reading $2140-$2143?
   (Yes: many games use port handshaking where the APU waits for the CPU to
   acknowledge before proceeding to the next transfer.)
4. During idle-skip, the APU runs catch_up(N) in one call. In the unskipped
   path, the APU would have been polled after every CPU instruction (~18
   master cycles). The APU might have written to its output ports expecting
   the CPU to read between polls. Those reads never happen during skip.
5. Is the pure-memory gate sufficient? The CPU doesn't touch MMIO during the
   loop, but the *APU* may be in the middle of a handshake that depends on
   the CPU's read timing. The idle-skip is safe for CPU→APU communication
   but potentially unsafe for APU→CPU handshakes that are timing-dependent.

Assess: does the idle-skip need to be refined? Possible fixes:
- Sub-divide the bulk catch_up into chunks aligned to APU port write boundaries
- Track whether the APU has pending port writes and break the skip early
- Accept the inaccuracy (most games don't handshake during idle loops)

### 5. Audit the Borrow Topology

Map the ownership graph and identify every place where Rust's borrow rules
force a specific execution order:

| Constraint | What Wants Concurrent Access | Current Solution | Hardware Reality |
|-----------|----------------------------|------------------|-----------------|
| DMA in bus | DMA needs &mut Bus while Bus owns DMA | DMA inlined in bus.rs | DMA is a separate chip |
| HDMA in bus | Same as DMA | Inlined | Separate DMA chip |
| APU catch_up | CPU step has &mut Bus, APU inside Bus | Call catch_up after step | Truly concurrent |
| PPU render | CPU step has &mut Bus, PPU inside Bus | Render at scanline boundary | Truly concurrent |

For each constraint, assess: does the forced ordering matter? If the PPU
rendered *during* CPU execution instead of at the scanline boundary, would
the output change? (Yes: mid-scanline register writes.)

Near's cooperative threading model eliminates these constraints by giving each
chip its own coroutine stack. Assess whether Rust can achieve the same with:
- Split borrows (e.g., `Bus` split into `BusMemory` + `BusPpu` + `BusApu`)
- Interior mutability (RefCell, but adds runtime cost)
- Message passing (channels, but WASM is single-threaded)
- Stackful coroutines via corosensei (not WASM-compatible)

### 6. Assess the Cooperative Threading Migration Path

Evaluate three levels of migration toward Near's model:

**Level 0 (current):** Catch-up after every CPU instruction. Simple. Stale
during instruction execution. No JIT sync.

**Level 1 (JIT sync within catch-up):** Only catch up the APU when CPU
accesses $2140-$2143. Insert `bus.apu.catch_up(cycles_elapsed_since_last_sync)`
in the bus read/write dispatch for the APU port range. Keeps the current
architecture, adds one conditional per bus access, dramatically improves
sync accuracy. **This is the recommended first step.**

**Level 2 (cooperative threading):** Each chip is a corosensei coroutine.
The scheduler runs the chip with the minimum clock. Chips yield on every
memory access. Full higan model. Maximum accuracy. **Not WASM-compatible.**
Would require the native bench to use corosensei while the WASM build falls
back to Level 1. Assess whether this dual-path is worth maintaining.

**Level 2b (cooperative threading via async):** Each chip is a Rust `async fn`
that `.await`s on bus accesses. Single-threaded executor (no tokio). This is
stackless — it works for flat dispatch loops but breaks for deeply nested
addressing modes. Assess feasibility for the 65816 (2-level: instruction +
addressing) and SPC700 (1-level: flat decode).

For each level, assess:
- Correctness improvement (which stale-state windows does it close?)
- Performance impact (context switch overhead vs reduced unnecessary syncs)
- Implementation effort
- WASM compatibility
- Save state compatibility (Near's Method 1/2/3)

### 7. Evaluate Dynamic Rate Control for AudioWorklet

Read `docs/PHASE_B_PLAN.md` if it exists, and `docs/OPEN_TASKS.md`. The Phase B
plan moves audio to an AudioWorklet (separate thread via SharedArrayBuffer).

Near's DRC formula solves the fundamental problem: the emulator produces audio
at the SNES's true sample rate (~32040 Hz), but the browser consumes it at
48000 Hz. Static resampling drifts because the two clocks are independent.

Assess:
- Ring buffer design: APU writes samples, AudioWorklet reads them.
  SharedArrayBuffer with Atomics for the read/write pointers.
- Fill-level monitoring: AudioWorklet reports its buffer fill level back
  to the main thread (via Atomics or postMessage).
- Rate adjustment: The emulator adjusts its effective APU sample rate based
  on fill level. Near's formula with maxDelta=0.005 limits pitch distortion
  to inaudible 0.5%.
- Interaction with catch_up: If the AudioWorklet runs independently, the APU
  must produce samples ahead of time. This changes catch_up semantics — the
  APU must run speculatively, not lazily.
- The 32040 vs 32000 Hz distinction: if we assume 32000 Hz, we'll underflow
  every ~10 seconds. Use the correct ~32040 Hz base frequency.

### 8. Write async.tmp

This file MUST contain ALL of these sections:

```
Async Concurrency Audit -- SNES Emulator (Rust/WASM)

Summary
<2-3 sentences: current sync model health, biggest risk, most promising fix>

Near's Framework Assessment
<How does this emulator's catch-up model compare to bsnes's cooperative
threading? Where on the Level 0/1/2 spectrum is it? What's achievable?>

Concurrency Model Map
<Diagram (ASCII or Mermaid) showing: chips, communication channels, polling
order, cycle budgets per poll, sync points>

Synchronization Point Inventory
<Table: Sync Point | Chips | Direction | Mechanism | Cycle-Exact? | JIT Sync Candidate? | Risk>
Every inter-chip communication.

Stale-State Windows
<Table: Window | Duration (cycles) | Trigger | Observable Effect | Severity>
Every window where a chip could read stale data from another.

Idle-Skip Communication Analysis (T10)
<Detailed trace of what happens to inter-chip communication during idle-skip.
What ports are written, what handshakes are broken, what the APU expects.
Reference Near's serialization article on the "thousands of instructions ahead"
problem.>

Borrow Topology
<Table: Constraint | What Conflicts | Current Solution | Hardware Reality | Fix>
Every place where Rust ownership forces non-hardware ordering.

Scheduler Assessment
<Current model vs Near's relative vs absolute scheduler. Is cycle_target the
right abstraction? Should we track CPU↔APU and CPU↔PPU separately?>

Catch-Up Contract
<For each catch_up call: is it associative? commutative? idempotent?
Under what conditions does ordering matter? Connection to the categorical
distributive law.>

Cooperative Threading Migration Path
<Level 0/1/2/2b assessment. What's achievable in WASM? What requires native-
only code? Is the dual-path worth it?>

Dynamic Rate Control Assessment
<Near's DRC formula applicability to our AudioWorklet design. The 32040 Hz
correction. Ring buffer design. Fill-level feedback loop.>

Ordering Dependencies
<Table: Operation A | Operation B | Must A happen before B? | Why? | Currently Correct?>
Every ordering constraint between chip operations.

Recommended Fixes (Priority Order)
<Numbered list with [S/M/L] effort and risk assessment>

What This Audit Cannot Determine
<Honest limits -- things that require hardware testing, game-specific
knowledge, or running the ROM to resolve>
```

Every issue MUST appear in async.tmp. Anything omitted won't get fixed.

### What you do NOT do
- No tests. No application code changes. No documentation. Analysis only.
- You write exactly one file: async.tmp.
```

## Between Passes

After Pass 1 returns:
1. Confirm `async.tmp` exists in the project root
2. Read and summarise the findings for the user (brief -- 3-5 lines)
3. Proceed to Pass 2

## Pass 2: Fix Agent

Launch a second agent with the prompt below.

### Fix Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are the concurrency fixer for an SNES emulator written in Rust, compiled to
WASM. You read async.tmp, fix synchronization ordering issues, write inter-chip
contract tests, and produce ASYNC_MODEL.md.

Your fixes should move the emulator toward Near/byuu's JIT synchronization model
— the pragmatic sweet spot between the current catch-up-every-instruction model
and full cooperative threading (which isn't WASM-compatible).

CRITICAL CONSTRAINT: This emulator has a sacred determinism contract. After ALL
changes, if a ROM is available, run:
  cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
The hashes MUST match: fb=54b3eed74f9f8432, audio=62300ecfc4da23e0.

IMPORTANT: Hash changes from JIT sync fixes may indicate IMPROVED accuracy (the
emulator is now synchronizing at the correct cycle). If hashes change, investigate
whether the new output is more correct before reverting. Document the change.

## Phase 1: Read the Assessment

Read `async.tmp`. Also read docs/ARCHITECTURE.md and docs/CATEGORY_THEORY.md
for context on the module structure and categorical findings.

Extract:
1. Stale-State Windows -- priority ordering by severity
2. JIT Sync Candidates -- which sync points should trigger forced catch_up
3. Ordering Dependencies that are currently violated
4. Borrow Topology fixes that don't require major restructuring
5. Catch-Up Contract properties that need enforcement
6. Recommended Fixes in priority order

## Phase 2: Fix Issues

### 2a: Implement JIT Synchronization for CPU↔APU (Level 1)

This is the highest-value fix. In the bus read/write dispatch for $2140-$2143:

```rust
// In bus.rs, read dispatch:
0x2140..=0x2143 => {
    // JIT sync: catch up APU to current cycle before reading port
    self.apu.catch_up(cycles_since_last_apu_sync);
    self.apu.cpu_read((addr & 3) as u8)
}
```

This requires tracking how many cycles have elapsed since the last APU sync.
The simplest approach: track a `apu_sync_debt: u32` on Bus that accumulates
cycles from cpu.step() and is consumed by catch_up calls.

IMPORTANT: This changes the APU's sync granularity from "every CPU instruction"
to "only at port access." This is MORE accurate (matches hardware) but will
change the audio hash. The new hash should be closer to reference recordings.

### 2b: Fix Ordering Violations
Where the audit found operations happening in the wrong order, fix the ordering.

### 2c: Fix Stale-State Windows
For each stale-state window with severity > LOW:
- Insert synchronization (catch_up) at the narrowest possible point
- Document why the synchronization is needed
- If the fix is too invasive, flag it for user decision

### 2d: Document the Catch-Up Contract
Add doc comments to each catch_up method specifying:
- Associativity: is catch_up(a); catch_up(b) == catch_up(a+b)?
- Commutativity: does the ordering of catch_up calls between chips matter?
- Idempotency: is catch_up(0) a no-op?
- Pre-conditions: what state must be consistent before calling?
- Near's framework: which serialization method (1/2/3) does this correspond to?

## Phase 3: Write Synchronization Contract Tests

Place in `tests/async_contracts/`. These tests verify inter-chip synchronization
properties.

### JIT Sync Tests
```rust
#[test]
fn apu_port_write_visible_after_jit_sync() {
    // CPU writes $2140, then reads $2140 after JIT sync
    // The read must reflect the write
}

#[test]
fn apu_port_read_triggers_catch_up() {
    // Verify that reading $2140-$2143 forces APU catch_up
    // to the current cycle, not just the last instruction boundary
}

#[test]
fn non_apu_read_does_not_trigger_catch_up() {
    // Reading WRAM or ROM should NOT trigger APU catch_up
    // (this is the JIT optimization — only sync at shared memory)
}
```

### Catch-Up Algebraic Properties
```rust
#[test]
fn catch_up_is_associative() {
    // catch_up(a); catch_up(b) produces identical APU state to catch_up(a+b)
    // This is the distributive law from the categorical analysis
}

#[test]
fn catch_up_zero_is_identity() {
    // catch_up(0) must not change any APU state
}

#[test]
fn catch_up_is_monotone() {
    // After catch_up(n), apu.cycles >= previous apu.cycles + n
    // (modulo SPC instruction granularity overshoot — now absorbed by
    // cycle_target, so this should hold exactly)
}
```

### Idle-Skip Sync Tests
```rust
#[test]
fn idle_skip_preserves_apu_port_state() {
    // Set up an idle loop on a pure-memory address
    // Write a known value to APU ports before the loop
    // After idle-skip, APU ports must reflect the pre-loop state
}

#[test]
fn idle_skip_apu_cycle_count_matches_unskipped() {
    // Run the same frame with and without idle-skip
    // APU cycle counts must match
}
```

### DMA Synchronization Tests
```rust
#[test]
fn dma_and_cpu_write_same_path() {
    // Write the same byte to the same PPU register via DMA and via CPU
    // The PPU state must be identical either way
    // (This is the container morphism factoring from the category sweep)
}
```

### Auto-Joypad Timing Tests
```rust
#[test]
fn auto_joypad_registers_valid_during_vblank() {
    // $4218-$421F should contain valid data after auto-read completes
    // Near's bug list shows this breaks real games when wrong
}
```

### What NOT to test
- Game-specific handshake protocols (too many, need ROM)
- Sub-cycle PPU timing (scanline-level is the current granularity)
- AudioWorklet thread safety (Phase B, not yet implemented)
- Full cooperative threading scheduler (Level 2, future work)

## Phase 4: Run Tests (3 attempts max)

```
for attempt in 1..3:
  1. Run sync contract tests
  2. All pass -> go to Phase 5
  3. Read failures, diagnose
  4. Fix: is the test wrong (expecting hardware behavior the emulator
     intentionally doesn't model) or is the code wrong?
  5. Next attempt
```

Run with:
```bash
cargo test async_contracts -- --nocapture
```

If the bench ROM is available, verify determinism:
```bash
cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
```

## Phase 5: Write ASYNC_MODEL.md

After all fixes and tests pass, write `docs/ASYNC_MODEL.md` from scratch.

Include:

### The SNES Concurrency Model
- What runs concurrently on real hardware (CPU, APU, PPU clocks)
- How the emulator serializes this (catch_up pattern)
- Near's cooperative threading as the gold standard
- Our position on the Level 0/1/2 spectrum

### Near's Framework Applied
- Cooperative threading: what we'd gain, what blocks us (WASM)
- JIT synchronization: what we implemented, what it fixed
- Cooperative serialization: how our snapshot system relates to Methods 1/2/3
- Dynamic rate control: design for AudioWorklet (T13)
- The 32040 Hz correction

### Synchronization Point Map
- Mermaid sequence diagram: one scanline showing CPU steps, APU catch_ups,
  PPU render, with JIT sync points marked
- Table of all sync points with cycle-exactness status
- Before/after comparison: catch-up-every-instruction vs JIT sync

### The Catch-Up Contract
- Formal properties: associativity, monotonicity, zero-identity
- Per-chip: CPU (executor), APU (catch_up with JIT sync), PPU (scanline)
- Connection to the categorical distributive law
- Near's scheduler design: relative vs absolute, our choice and why

### Stale-State Analysis
- Known windows with severity and observability
- The idle-skip communication gap (T10) — framed in Near's terms
- Which games are affected (from Near's emulation-bugs/snes)

### The Pure-Memory Boundary (connecting all three sweeps)
- Architecture: module boundary between WRAM/ROM and MMIO
- Category theory: comonad/monad stratification via `is_pure_memory()`
- Async: the JIT sync boundary (pure memory = no sync, MMIO = sync)
- Idle-skip safety: pure memory polls are safe, MMIO polls are not

### Borrow Topology
- Ownership diagram (Mermaid)
- Where Rust ownership forces non-hardware ordering
- How cooperative threading would restructure this
- Proposed split-borrow design for Level 1.5

### Dynamic Rate Control Design
- Near's DRC formula adapted for AudioWorklet
- Ring buffer: SharedArrayBuffer, Atomics read/write pointers
- Fill-level feedback: AudioWorklet -> main thread
- The 32040 Hz base frequency
- maxDelta = 0.005 (0.5% pitch, inaudible)

### Cooperative Threading Roadmap
- Level 0 (current) -> Level 1 (JIT sync) -> Level 2 (corosensei, native-only)
- What each level buys in accuracy
- WASM constraints and dual-path considerations
- corosensei as the Rust equivalent of libco

### Test Coverage
| Test | Property | Status |

### Remaining Issues
<Sync problems that require Level 2, hardware testing, or game-specific work>

## Phase 6: Report and Cleanup

Delete `async.tmp`.

Return this report:

```markdown
# SNES Async Concurrency Audit Report

## Concurrency Model Health
<1-2 sentences: overall assessment, position on Near's spectrum>

## Near's Framework Assessment
| Aspect | bsnes (gold standard) | Our emulator | Gap |

## JIT Sync Implementation
| Sync Point | Before (cycle-exact?) | After (cycle-exact?) | Hash Impact |

## Stale-State Windows Found
| Window | Chips | Duration | Severity | Fixed? |

## Ordering Violations Found
| Operation A | Operation B | Expected Order | Actual Order | Fixed? |

## Synchronization Fixes Applied
| Fix | Files Changed | Hash Impact |

## Catch-Up Contract
| Chip | Associative | Monotone | Zero-Identity |
| APU  | yes/no      | yes/no   | yes/no        |
| PPU  | yes/no      | yes/no   | yes/no        |

## Sync Contract Tests Written
| File | Property | Count |

## Test Run
- Attempts: N/3
- Final result: PASS / FAIL
- Tests: N total, N passed, N failed

### Determinism Check
- ROM available: yes/no
- FB hash: check/cross (expected 54b3eed74f9f8432)
- Audio hash: check/cross (expected 62300ecfc4da23e0)
- If changed: assessment of whether new hashes are more accurate

## Dynamic Rate Control Design
<Summary of DRC for AudioWorklet, ready for T13 implementation>

## Cooperative Threading Roadmap
| Level | Description | WASM? | Effort | Accuracy Gain |

## Key Findings
<What did the async lens + Near's framework reveal that the architecture
and category sweeps didn't?>

## Remaining Issues
<Problems requiring Level 2, hardware testing, or game-specific work>
```
```

## After Pass 2

Present the full report. Highlight:
- The JIT sync implementation and its impact on hashes
- The idle-skip communication gap framed in Near's terms
- Dynamic rate control design for AudioWorklet
- The cooperative threading roadmap (Level 0 -> 1 -> 2)
- Any stale-state windows that affect real games (from Near's bug list)
- Connection to the pure-memory stratification (the thread linking all three sweeps)
