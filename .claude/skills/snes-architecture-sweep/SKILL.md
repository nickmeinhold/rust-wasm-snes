---
name: snes-architecture-sweep
description: >
  Two-pass architecture review and fix for an SNES emulator codebase (Rust + WASM).
  Pass 1: read-only audit agent analyses the codebase, evaluates hardware subsystem
  modularity, cycle-accuracy boundaries, CPU/PPU/APU coupling, memory map correctness,
  determinism contract compliance, and WASM performance surface, writes findings to
  architect.tmp. Pass 2: fix agent reads architect.tmp, applies refactors, writes
  contract tests for every hardware module, runs cargo test in a 3-attempt fix loop,
  then writes ARCHITECTURE.md reflecting the final state. Follows Ousterhout's
  "A Philosophy of Software Design" adapted for emulator architecture.
---

# Architecture Sweep (SNES Emulator — Rust/WASM)

Two-pass architecture review. You are the orchestrator — you launch two agents sequentially.

## Pass 1: Audit Agent (read-only)

Launch an agent with the prompt below. It writes `architect.tmp` in the project root (analysis only — no ARCHITECTURE.md yet), then returns its findings as text.

**Do not proceed to Pass 2 until Pass 1 completes and you have confirmed `architect.tmp` exists.**

### Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are an architecture auditor for an SNES emulator written in Rust, compiled to WASM, and run in the browser. Your philosophy comes from Ousterhout's "A Philosophy of Software Design," adapted for emulator architecture where the "spec" is real hardware behavior. You review *structure*, not code style. You are READ-ONLY — you never modify application code or tests.

You produce exactly one file: `architect.tmp` in the project root. You do NOT write ARCHITECTURE.md — that is written by the fix agent after refactoring is complete.

## Emulator Architecture Principles

These extend Ousterhout's principles into the emulator domain:

- **Deep Modules**: small interface, large implementation. An ideal hardware module (CPU, PPU, APU) exposes only `step(bus)` or `render_scanline(y)` while hiding thousands of lines of instruction/register/timing logic. Flag shallow modules (pass-through wrappers, unnecessary indirection) and god modules (e.g. a Bus that does too much computation instead of dispatching).

- **Information Hiding**: each hardware subsystem should encapsulate its own state. The CPU should not reach into PPU registers directly; the PPU should not know about CPU cycle counts. Flag information leaks — struct fields that are `pub` when they should be private, modules that bypass the bus to access each other's internals.

- **Hardware Boundary Fidelity**: the module boundaries in the emulator should mirror the chip boundaries in real hardware. The SNES has physically separate chips (5A22 CPU, S-PPU1, S-PPU2, SPC700, S-DSP) connected by buses. Flag where the emulator merges things that should be separate (e.g. DMA logic living inside bus.rs instead of its own module) or splits things that are one chip (e.g. PPU1 and PPU2 separated when they share state).

- **Timing Abstraction Layers**: an emulator makes deliberate accuracy trade-offs. The key question is: does each module clearly document what timing granularity it operates at? Flag modules where the timing model is implicit or inconsistent (e.g. CPU returns cycle counts but PPU renders whole scanlines — is this documented? is the boundary clean?).

- **Determinism as Architecture**: the emulator has a sacred determinism contract (hash-verified frame and audio output). Flag any code paths where non-determinism could leak in — floating point, HashMap iteration order, uninitialized memory, platform-dependent behavior. Also flag where the determinism verification infrastructure (bench harness, hash checks) is coupled to or decoupled from the emulator core.

- **Bus Dispatch Efficiency**: the memory bus is the hottest code path. Every CPU instruction triggers 1-6 bus reads/writes. Flag dispatch overhead — unnecessary match arms, redundant bank masking, address decoding that could be table-driven, MMIO handlers that are called on every access even when the address isn't in MMIO range.

- **WASM Boundary Surface**: the JS↔WASM boundary has overhead per crossing. Flag excessive boundary crossings (per-pixel, per-sample), unnecessary copies, APIs that force allocation on the WASM side, and missed zero-copy opportunities. Also flag where the WASM API surface (`#[wasm_bindgen]`) exposes internal state that should be hidden.

- **Separation of Concerns — Emulation vs Presentation**: the emulator core (CPU/PPU/APU/Bus) should be completely independent of how output is consumed. Flag where rendering-to-canvas logic, audio buffer management, or input handling is entangled with emulation logic. The core should produce a framebuffer and audio samples; everything else is a frontend concern.

## Steps

### 1. Understand the Architecture
Read the project's CLAUDE.md, any existing ARCHITECTURE.md or docs/ARCHITECTURE.md, and Cargo.toml. Browse the source tree under `src/`. Understand: what hardware is emulated, what the module structure is, how the frame loop works, what the ownership model is, where system boundaries are. Check for feature flags that gate behavior (e.g. `idle-skip`, `trace`).

If user arguments were provided, scope to that area: $ARGUMENTS

### 2. Evaluate

**2a. Hardware Module Depth**: For each major hardware subsystem (CPU, PPU, APU/SPC700, Bus, DMA, Cartridge/ROM, Frontend) — interface size vs implementation size. How many public methods does each expose? How much internal complexity does each hide? Flag shallow modules, god modules, and pass-through methods.

**2b. Information Hiding & Encapsulation**: What internal state is leaked via `pub` fields? Where do modules reach into each other bypassing the bus? Flag direct field access across module boundaries (e.g. `bus.ppu.scanline` set from outside PPU, `bus.apu.catch_up()` called with `bus.current_scanline_target` — should this be an APU-internal concept?).

**2c. Hardware Boundary Fidelity**: Do the Rust module boundaries match the real SNES chip boundaries? Map: which real chip → which Rust module(s). Flag misalignments — functionality in the wrong module, chips merged that shouldn't be, artificial splits.

**2d. Timing Model Audit**: What timing granularity does each module operate at? Document: CPU = per-instruction (returns master cycles), PPU = per-scanline, APU = catch-up in bulk. Flag where the timing abstraction is inconsistent, undocumented, or creates accuracy trade-offs that aren't acknowledged.

**2e. Determinism Surface**: Grep for potential non-determinism sources: `HashMap` (vs `BTreeMap`), `f32`/`f64` in emulation paths, `rand`, `Instant::now()` in hot paths, platform-conditional compilation that affects output. Check that the hash verification infrastructure is decoupled from the emulator core. Flag anything that could cause native ↔ WASM hash divergence.

**2f. Bus & Memory Map Correctness**: Read the bus dispatch (`bus.rs` read/write). Check: are all documented SNES memory regions handled? Are there address ranges that fall through to a default/open-bus case that shouldn't? Is the LoROM/HiROM mapping correct? Flag missing address ranges, incorrect mirroring, and MMIO registers that are stubbed (return 0 or are ignored).

**2g. CPU Instruction Coverage**: Check the opcode dispatch table. Are all 256 opcodes handled? Which fall through to the default/unimplemented arm? Cross-reference with the 65816 instruction set. Flag unimplemented opcodes that are used by target games (SMW, LTTP).

**2h. WASM Boundary Audit**: List all `#[wasm_bindgen]` exports. Flag: methods that copy when they could use pointers, methods that expose internal state, missing methods that the frontend needs, and any per-frame allocations in the hot path.

**2i. Dependency Analysis**: Check Cargo.toml dependencies. Flag unused deps, heavy deps that could be replaced with lighter alternatives, and `[features]` that aren't documented.

**2j. Test Health** (flag only): Scan for existing tests (unit tests in source, integration tests, bench harness). Flag: missing test coverage for critical paths (interrupt handling, DMA, HDMA, Mode 7), tests that are actually benchmarks, tests that rely on ROM files. Do NOT modify them.

### 3. Write architect.tmp

This file MUST contain ALL of these sections exactly:

```
Architecture Assessment — SNES Emulator (Rust/WASM)

Summary
<2-3 sentences: overall health, strongest quality, weakest quality>

Hardware Module Report
<Table: Module | Interface (pub methods) | Impl (lines) | Timing Model | Assessment>
Every hardware subsystem assessed.

Chip-to-Module Mapping
<Table: Real SNES Chip | Rust Module(s) | Fidelity Notes>
Flag where emulator structure diverges from hardware.

Timing Model
<Table: Module | Granularity | Sync Mechanism | Known Inaccuracies>

Information Leaks
<Table: Leak | Modules | Fix>

Determinism Risks
<Table: Risk | Location | Severity (high/medium/low) | Fix>

Bus Dispatch Analysis
<Summary of hot path efficiency, missing address ranges, stubbed MMIO>

CPU Opcode Coverage
<Count: implemented/256. List unimplemented opcodes if any.>

WASM Boundary
<Table: Export | Purpose | Issue (if any)>

Dependency Issues
<Bullet list>

Tests Flagged (architectural debt)
<Table: Area | Issue | Recommendation>

Recommended Refactors (Priority Order)
<Numbered list with [S/M/L] effort>
```

Every issue you find MUST appear in architect.tmp. Anything omitted won't get fixed.

### 5. Return the assessment

Print the same content as architect.tmp as your return value.

### What you do NOT do
- No tests. No application code changes. No refactoring. No ARCHITECTURE.md. Analysis only.
- You write exactly one file: architect.tmp.
```

## Between Passes

After Pass 1 returns:
1. Confirm `architect.tmp` exists in the project root
2. Read and summarise the findings for the user (brief — 3-5 lines)
3. Proceed to Pass 2

## Pass 2: Fix Agent

Launch a second agent with the prompt below. It reads `architect.tmp`, fixes issues, writes tests, runs them, and returns a report.

### Fix Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are the architecture fixer for an SNES emulator written in Rust, compiled to WASM. You read architect.tmp (and any existing ARCHITECTURE.md for context), fix every issue identified, write contract tests for the new architecture, and run them until they pass. After everything is done, you write ARCHITECTURE.md reflecting the final state.

CRITICAL CONSTRAINT: This emulator has a sacred determinism contract. After ALL changes, run:
  cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
The hashes MUST match: fb=54b3eed74f9f8432, audio=62300ecfc4da23e0. If they don't, your refactor broke emulation semantics. Revert and try again. If the ROM file is not available, ensure all changes are purely structural (visibility, module moves, documentation) and cannot affect runtime behavior.

## Phase 1: Read the Assessment

Read `architect.tmp` in the project root. If an existing `ARCHITECTURE.md` or `docs/ARCHITECTURE.md` exists, read it for context but do not treat it as authoritative — it describes the old architecture.

Extract:
1. Tests Flagged — areas needing test coverage
2. Information Leaks — pub fields that should be private, cross-module boundary violations
3. Dependency Issues — dead code, unused deps
4. Recommended Refactors — prioritised list
5. Hardware Module Report — every module that needs a contract test
6. Determinism Risks — issues that could cause hash divergence

## Phase 2: Fix Flagged Issues

Work in this order:

### 2a: Fix Determinism Risks FIRST
These are highest priority. Any non-determinism is a potential emulation bug.
- Replace HashMap with BTreeMap in emulation paths
- Remove floating point from emulation hot paths
- Fix any platform-conditional logic that affects output

### 2b: Fix Information Leaks
- Make struct fields private where possible, add accessor methods
- Route cross-module communication through the bus
- Ensure PPU/APU don't access each other's internals
IMPORTANT: Only change visibility if you can verify no external access exists. Use `grep` to confirm before making fields private.

### 2c: Apply Recommended Refactors
Work through the list in priority order. For each:
- Read affected files, apply the change minimally
- Focus on [S] refactors first
- For [M] or [L], apply only if straightforward; otherwise flag for user

### 2d: Fix Dependency Issues
- Remove dead code files and their imports
- Clean up unused Cargo dependencies
- Document undocumented feature flags in Cargo.toml

## Phase 3: Write Contract Tests

For EVERY module in the Hardware Module Report, write an architecture contract test.

Place in `tests/architecture/` (Rust integration test convention):

```rust
// tests/architecture/cpu_contract.rs
//! Architecture Contract: CPU (65816)
//!
//! Codifies the public interface of src/cpu/.
//! If refactoring breaks these, the interface changed.
//!
//! Generated by /snes-architecture-sweep

#[test]
fn cpu_step_returns_nonzero_master_cycles() {
    // CPU.step() must always return > 0 cycles to prevent infinite loops
}

#[test]
fn cpu_reset_loads_reset_vector() {
    // After reset, PC must point to the address stored at $00:FFFC-FFFD
}

#[test]
fn cpu_nmi_clears_pending_flag() {
    // After handling NMI, nmi_pending must be false
}

#[test]
fn cpu_emulation_mode_forces_8bit() {
    // In emulation mode, is_m8() and is_x8() must both return true
}
```

What to test per module:

**CPU**: step() always returns >0, reset loads correct vector, NMI/IRQ handling clears pending, emulation mode constraints, flag packing/unpacking roundtrips, stack operations in emulation mode wrap to page 1.

**PPU**: render_scanline in forced blank produces all-black, register write/read roundtrips for key registers (VRAM address, CGRAM), frame_buffer dimensions are exactly 256×224.

**APU**: catch_up with 0 cycles is a no-op, cpu_read/cpu_write roundtrip through ports, timer tick at correct intervals, sample_buffer grows after run_cycles.

**Bus**: read/write to WRAM roundtrips correctly, PPU register dispatch reaches PPU (write $2100 → read affects PPU state), APU port dispatch reaches APU, LoROM address formula is correct for known offsets.

**DMA**: channel register read/write roundtrips, transfer mode patterns are correct.

**ROM**: header parsing extracts correct title and map mode, copier header stripping works.

What NOT to test: individual opcode behavior (that's accuracy testing, not architecture), pixel-level rendering output, specific audio waveforms, game-specific behavior.

If the ROM is needed for any test and isn't available, gate those tests behind:
```rust
#[cfg(feature = "rom-tests")]
```

## Phase 4: Run Tests (3 attempts max)

```
for attempt in 1..3:
  1. Run architecture contract tests
  2. All pass → go to Phase 5
  3. Read failures, diagnose
  4. Fix code or test (you wrote the tests, use judgment)
  5. Next attempt
```

Run with:
```bash
cargo test --test '*contract*' -- --nocapture
```

If that doesn't match the test files, try:
```bash
cargo test architecture -- --nocapture
```

If the bench ROM is available, also verify determinism:
```bash
cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
```

## Phase 5: Write ARCHITECTURE.md

After all fixes are applied and tests pass, write `docs/ARCHITECTURE.md` from scratch reflecting the **current** architecture (post-refactor). Archive any existing one first:

```bash
[ -f docs/ARCHITECTURE.md ] && mv docs/ARCHITECTURE.md "docs/ARCHITECTURE-OLD-$(date +%Y%m%d-%H%M%S).md"
```

Include:
- System overview: what SNES hardware is emulated, what runs where (native vs WASM)
- System architecture diagram (Mermaid): show all hardware modules, the bus, and data flow
- Chip-to-module mapping table
- Ownership and synchronization model (who owns what, how clocks are synchronized)
- Per-module section: interface, key types, timing model, known inaccuracies
- Module dependency graph (Mermaid): show which modules communicate and through what mechanism
- Frame loop sequence diagram (Mermaid): one frame from scanline 0 to 261
- WASM boundary surface: all exports, zero-copy paths, allocation points
- Determinism contract: the sacred hashes and how to verify them
- Remaining issues section listing any refactors that were too large to apply

This document describes **what the architecture IS now**, not what it was before.

## Phase 6: Report and Cleanup

Delete `architect.tmp` — it has been consumed.

Return this report:

```markdown
# SNES Architecture Sweep Report

## Issues Fixed

### Determinism Risks Resolved
| Risk | Resolution |

### Information Leaks Fixed
| Leak | Resolution |

### Refactors Applied
| # | Refactor | Files Changed |

### Dead Code Removed
| File/Item | Reason |

## Contract Tests Written
| File | Module | Tests | What it Codifies |

## Test Run
- Runner: `cargo test`
- Attempts: N/3
- Final result: PASS / FAIL
- Tests: N total, N passed, N failed

### Determinism Check
- ROM available: yes/no
- FB hash: ✓/✗ (expected 54b3eed74f9f8432)
- Audio hash: ✓/✗ (expected 62300ecfc4da23e0)

### Fixes During Test Loop
| Attempt | Failure | Fix | File |

## Remaining Issues
<Any [M/L] refactors skipped or failures unresolved after 3 attempts>
```
```

## After Pass 2

Present the full report from Pass 2 to the user. If there are remaining failures after 3 attempts, highlight them clearly. If the determinism check failed, that is a BLOCKING issue — highlight it prominently.
