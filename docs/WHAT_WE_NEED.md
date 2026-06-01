# What We Need: Emulator-Specific Audit Skills

The `code-audit` skill suite targets web apps (API routes, CRUD, migrations, auth). An SNES emulator is a virtual machine — the concerns are fundamentally different. We need audit skills built for emulator correctness, performance, and compatibility.

## Skills to Build

### CPU Accuracy Sweep (`/cpu-accuracy-sweep`)
- **65816 instruction set coverage** — are all opcodes implemented? Are edge cases handled (decimal mode, page-crossing cycle penalties, wrap-around addressing)?
- **Interrupt timing** — NMI, IRQ, BRK priority and latency. Do interrupts fire at the correct cycle?
- **State machine correctness** — the CPU is a fetch-decode-execute state machine. Verify transition completeness and flag behavior (especially the obscure ones: D, V, emulation mode).
- **Two-pass pattern**: audit agent catalogs implemented vs missing opcodes, generates test ROMs or register-level assertions; fix agent fills gaps.

### PPU Audit (`/ppu-audit`)
- **Rendering pipeline** — scanline timing, H-blank/V-blank signals, mid-scanline register changes (raster effects).
- **Mode correctness** — Modes 0-7, especially Mode 7 (affine transforms, rotation, scaling).
- **Sprite handling** — OAM parsing, priority, 8-sprite-per-scanline limit, range/time overflow flags.
- **HDMA** — mid-frame DMA transfers that modify PPU registers per scanline (used heavily by real games).
- **Color math** — add/subtract blending, window masking, fixed color.

### APU/SPC700 Audit (`/apu-audit`)
- **SPC700 instruction accuracy** — separate processor with its own instruction set.
- **Timer correctness** — three hardware timers at different rates (8kHz, 8kHz, 64kHz).
- **DSP emulation** — BRR sample decoding, Gaussian interpolation, echo buffer, noise generator.
- **CPU-APU communication** — the four I/O ports (2140h-2143h) and their synchronization semantics.

### Memory Map Audit (`/memory-map-audit`)
- **Bank switching** — LoROM, HiROM, ExHiROM mappings. Are mirrors correct?
- **Open bus behavior** — reads from unmapped addresses return the last value on the data bus, not zero.
- **MMIO registers** — PPU, APU, DMA, controller ports mapped to the correct addresses with correct read/write semantics.
- **WRAM** — 128KB work RAM access patterns, bank 7E-7F mapping.
- **Special chips** — SA-1, SuperFX, DSP-1, etc. (stretch goal).

### Async Concurrency Audit (`/snes-async-audit`)
- **Inter-chip synchronization** -- the SNES is three concurrent processors (CPU, APU, PPU) serialized into a single-threaded `catch_up` loop. Where does that serialization violate hardware behavior?
- **Stale-state windows** -- when one chip reads another's state, has the source chip been advanced to the correct cycle? Trace every inter-chip read for staleness gaps.
- **Ordering dependencies** -- which chip operations must happen in a specific order? Which commute freely? The pure-memory stratification (`is_pure_memory()`) is the async/sync boundary: pure memory commutes, MMIO doesn't.
- **Idle-skip communication gap** -- during bulk idle-skip (T10), the APU runs `catch_up(N)` in one call. The CPU's intermediate port writes are lost. The APU may depend on those writes for handshake protocols.
- **Borrow topology** -- where does Rust's ownership model force non-hardware execution order? DMA inlined in bus.rs because DMA needs `&mut Bus` while Bus owns DMA. Is this a correctness problem or just an aesthetics one?
- **Catch-up contract** -- is `catch_up(a); catch_up(b) == catch_up(a+b)`? (Yes for APU after cycle_target fix. What about PPU?) Is polling order between chips significant?
- **Async Rust assessment** -- could chips be modeled as coroutines/futures that yield at sync points? Trade-offs for WASM.
- **Phase B interaction** -- how does AudioWorklet (separate thread via SharedArrayBuffer) change the sync model?
- **Connection to categorical findings** -- the distributive law (catch_up chunking), the container morphism (DMA factoring), and the pure-memory stratification all describe different aspects of the concurrency model.

### Timing Sweep (`/timing-sweep`)
- **Cycle-level accuracy** — does the emulator count master clock cycles or approximate?
- **CPU-PPU synchronization** — the PPU runs on a dot clock (5.37MHz) while the CPU runs at ~3.58/2.68/1.78 MHz depending on memory region. Are they properly synchronized?
- **DMA bus conflicts** — DMA halts the CPU. Is this modeled?
- **Alignment** — H/V counter values, interlace vs non-interlace timing.

### WASM Performance Profile (`/wasm-perf-profile`)
- **Hot loop analysis** — the CPU dispatch loop runs millions of times per frame. Is it compiled efficiently?
- **JS-WASM boundary** — how many crossings per frame? Each one has overhead.
- **Memory layout** — is the SNES address space mapped efficiently in WASM linear memory?
- **Rendering path** — pixel buffer transfer to canvas. Double-buffering? Typed arrays?
- **Allocation pressure** — any per-frame allocations that trigger GC?

### ROM Compatibility Sweep (`/rom-compat-sweep`)
- **Header parsing** — LoROM/HiROM detection, checksum validation, region detection.
- **Known-tricky games** — games that rely on hardware quirks (e.g.,Ings that depend on open bus, cycle-exact timing, or undocumented behavior).
- **Test ROM suites** — run against community test ROMs (blargg's, PeterLemon's) and report pass/fail.

## Reusable Patterns from code-audit

These existing patterns transfer directly:

| Pattern | How it applies |
|---------|---------------|
| **Two-pass sweep** (audit agent writes `.tmp`, fix agent repairs) | CPU opcode coverage, memory map completeness |
| **Orchestrator composition** | `/snes-sweep` could compose CPU + PPU + APU + timing audits |
| **`/design-patterns`** | GOF analysis of opcode dispatch (Strategy/Command), rendering pipeline (Template Method), memory mapping (Proxy) |
| **`/state-machine-sweep`** | CPU states, DMA states, PPU mode register |
| **`/category-sweep`** | Functorial structure in address space mapping, natural transformations between ROM layouts |
| **`/dependency-audit`** | Still relevant — Rust crate deps, WASM toolchain versions |

## What Doesn't Transfer

- `/api-contract-audit` — no APIs
- `/migration-review` — no database
- `/crud-sweep` — no CRUD
- `/security-audit` — no user-facing auth or input validation (though ROM parsing is an attack surface)
- `/production-ready` — no deployment infrastructure (though WASM serving has some concerns)
- `/production-sinks` — no server-side event logging

## Orchestrator Design

```
                    /snes-sweep
            (master orchestrator)
    ┌──────────────┬──────────────┐
    │  /snes-cpu   │  /snes-av    │
    │  Accuracy    │  Audio/Video │
    └──────┬───────┴──────┬───────┘
           │              │
    ┌──────┴──────┐┌──────┴──────┐
    │/cpu-accuracy ││ /ppu-audit  │
    │   -sweep     ││             │
    │/memory-map-  ││ /apu-audit  │
    │   audit      ││             │
    │/timing-sweep ││             │
    └──────────────┘└─────────────┘

    ┌─────────────────────────────┐
    │  Cross-cutting              │
    │  /snes-async-audit          │
    │  /wasm-perf-profile         │
    │  /rom-compat-sweep          │
    │  /design-patterns (reused)  │
    │  /state-machine-sweep       │
    │  /dependency-audit (reused) │
    └─────────────────────────────┘
```
