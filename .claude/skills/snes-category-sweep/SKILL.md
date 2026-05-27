---
name: snes-category-sweep
argument-hint: "[scope: module path, or 'full' for whole project]"
description: >
  Two-pass categorical analysis and law verification for an SNES emulator codebase (Rust + WASM).
  Pass 1: read-only audit agent analyses the codebase for categorical structure — functors,
  natural transformations, monads, comonads, directed containers (Ahman-Chapman-Uustalu),
  container morphisms (Abbott-Altenkirch-Ghani), Kleisli/co-Kleisli categories, and Yoneda
  embeddings. Writes findings to category.tmp. Pass 2: fix agent reads category.tmp, aligns
  code with its categorical structure, writes property-based law tests, runs them in a
  3-attempt fix loop, then writes CATEGORY_THEORY.md. Extends the generic category-sweep
  with directed container theory: the emulator's address bus, snapshot system, and rendering
  pipeline have comonadic structure that the five directed container laws can verify.
---

# Category Sweep (SNES Emulator — Rust/WASM)

Two-pass categorical analysis. You are the orchestrator — you launch two agents sequentially.

## Pass 1: Categorical Audit Agent (read-only)

Launch an agent with the prompt below. It writes `category.tmp` in the project root (analysis only), then returns its findings as text.

**Do not proceed to Pass 2 until Pass 1 completes and you have confirmed `category.tmp` exists.**

### Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are a mathematician who writes code, analysing an SNES emulator (Rust/WASM) for categorical structure. Your job is not to name-drop monads — it's to find places where the code is fighting its natural categorical structure, and show how aligning with that structure produces simpler, more composable abstractions.

You produce exactly one file: `category.tmp` in the project root. You do NOT write CATEGORY_THEORY.md — that is written by the fix agent after changes are complete.

**Be honest about when category theory doesn't help.** Not every pattern is categorical. Don't force categorical language onto problems that don't have categorical structure.

This analysis extends standard categorical audit with **directed container theory** (Ahman, Chapman, Uustalu 2014; Abbott, Altenkirch, Ghani 2003). Background below.

## Directed Containers: Quick Reference

A **container** is a pair (S, P) where S is a type of shapes and P : S → Type is a family of positions. The associated functor is F(X) = Σ(s : S). (P(s) → X) — "choose a shape, fill every position with data."

A **container morphism** f : (S₁, P₁) → (S₂, P₂) has two components:
- **Forward on shapes**: u : S₁ → S₂ (transforms the structure)
- **Backward on positions**: f_P : P₂(u(s)) → P₁(s) (traces where output data came from)

The positions go backwards. Forward = transformation; backward = provenance.

A **directed container** is a container (S, P) equipped with:
- **Root**: o : Π s. P(s) — each shape has a distinguished position
- **Subshape**: down : Π s. P(s) → S — each position determines a sub-context
- **Embedding**: (+) : Π s. Π p. P(down(s, p)) → P(s) — positions in a subshape map into the global shape

Subject to five laws:
1. down(s, o(s)) = s — subshape at root is the whole shape
2. o(down(s, p)) + p = p (modulo) — root embedding is trivial
3. down(down(s, p), q) = down(s, p + q) — subshapes nest correctly
4. o(s) + p = p — root is identity for embedding
5. (p + q) + r = p + (q + r) — embedding is associative

**Central theorem**: A container is a comonad if and only if it is a directed container.
- extract = read value at root position
- duplicate = at every position, replace value with entire substructure rooted there

**Directed container morphisms** additionally satisfy:
1. extract₂ ∘ φ = extract₁ — roots commute (final answer preserved)
2. duplicate₂ ∘ φ = (φ ⊗ φ) ∘ duplicate₁ — unfolding commutes

## Rules

- **READ-ONLY**: Do NOT edit, create, or delete any source or test files. Write only `category.tmp`.
- Use categorical language precisely. A functor is a functor, not a "kind of like a functor."
- Always ground abstract structure in concrete code locations.

## Steps

### 1. Identify Categories

Every codebase has implicit categories. An emulator has several natural ones.

**The Types Category**: Objects are Rust types, morphisms are functions. What structure do the morphisms have?

**The Address Category**: Objects are address spaces (banks, regions), morphisms are memory mappings. The bus dispatch is a morphism.

**The State Category**: Objects are emulator states (CPU registers, PPU registers, WRAM contents), morphisms are state transitions (instructions, scanline renders, DMA transfers).

**The Hardware Category**: Objects are hardware subsystems (CPU, PPU, APU, DMA), morphisms are the communication channels between them (bus reads/writes, I/O ports, interrupt signals).

### 2. Find Containers (S, P)

In an emulator, containers appear as "a structure with indexed positions that hold data."

| Emulator Structure | Shape S | Positions P(s) | Data |
|-------------------|---------|-----------------|------|
| WRAM (128KB) | Unit (one shape) | u17 (addresses 0x00000–0x1FFFF) | u8 bytes |
| VRAM (64KB) | Unit | u16 (word addresses) | u8 bytes |
| OAM (128 sprites) | Unit | u8 (sprite index 0–127) | Sprite attributes |
| CGRAM (256 colors) | Unit | u8 (color index 0–255) | u16 BGR color |
| Opcode table | Unit | u8 (opcode 0–255) | Instruction handler |
| DMA channels | Unit | u8 (channel 0–7) | DmaChannel config |
| LoROM address space | MapMode | (bank: u8, addr: u16) | u8 bytes |
| Frame buffer | Unit | (x: u8, y: u8) where x<256, y<224 | u32 ARGB pixel |
| APU sample buffer | Vec length | usize index | i16 sample |
| BRR ring buffer | Unit | usize (0–11) | i32 decoded sample |
| Echo FIR history | Unit | usize (0–7) | i32 tap value |
| SPC700 address space | Unit | u16 (0x0000–0xFFFF) | u8 byte |

### 3. Find Container Morphisms

Container morphisms have forward-on-shapes and backward-on-positions. In an emulator, the backward direction carries data.

| Morphism | Source Container | Target Container | Forward (shapes) | Backward (positions) |
|----------|-----------------|------------------|-------------------|----------------------|
| Bus read | CPU address (bank, addr) | Mapped hardware | Address decode: (bank, addr) → target | Data flows back: target value → CPU |
| Bus write | CPU (bank, addr, val) | Mapped hardware | Address decode | Acknowledgement (implicit) |
| DMA transfer | A-bus source | B-bus target (PPU/APU) | Source addr → dest register | Data carries from source to dest |
| HDMA | Table in ROM/WRAM | PPU registers | Table entry → register addr | Register value from table data |
| PPU register write | CPU write to $2100–$213F | PPU internal state | Register decode | State update |
| APU port write | CPU write to $2140–$2143 | APU port latch | Port select (addr & 3) | Latch update |
| LoROM mapping | Logical (bank, addr) | Physical ROM offset | `(bank & 0x7F) * 0x8000 + (addr - 0x8000)` | ROM byte at offset |
| Sprite decode | OAM index | Screen pixels | Sprite → tile → pixel chain | Color data back |
| Snapshot save | Live emulator state | Byte blob | Flatten all state → blob | (one-way) |
| Snapshot restore | Byte blob | Live emulator state | Parse blob → structured state | (one-way) |

**Check composition**: Do morphisms compose correctly? Does (DMA source → Bus → PPU register) equal a direct (DMA → PPU) path? The fact that DMA bypasses bus.write() in the current code means the composition may not hold — this is a container morphism that doesn't factor through the bus.

### 4. Find Directed Containers (Comonads)

Look for structures with root + subshape + embedding. These are the comonadic structures.

**Candidate: Bus/Memory as Store Comonad**
```
S = AddressSpace configuration (bank map, MEMSEL, etc.)
P(s) = Valid addresses in configuration s: (bank: u8, addr: u16)
o(s) = "current" address (e.g., CPU's PBR:PC)
down(s, (bank, addr)) = sub-address-space visible from (bank, addr)
    For WRAM: full 128KB visible
    For ROM: the bank's 32KB window
    For MMIO: the register's read-effects
(+) = address composition within sub-space
```
- `extract` = bus.read(current_bank, current_addr)
- `duplicate` = at every address, the full bus state visible from that address

Check the five laws:
1. down(s, o(s)) = s? Is the sub-space at PC the full address space? (Yes, CPU can reach everything)
2-5. Do the embedding laws hold?

**Candidate: Snapshot as Comonad Morphism**
If the Bus is a Store comonad, then snapshot/restore is a comonad morphism:
- Roots commute: extract(restore(snapshot(state))) = extract(state)? This is exactly the determinism contract.
- The sacred hashes (fb=54b3eed74f9f8432, audio=62300ecfc4da23e0) are asserting that extract ∘ restore ∘ snapshot = extract. The comonad law IS the hash invariant.

**Candidate: PPU Scanline as Comonad**
```
S = Scanline configuration (bgmode, scroll, window, color math settings)
P(s) = Pixel positions 0–255
o(s) = leftmost pixel (or: the pixel being rendered)
down(s, x) = rendering context at pixel x (scroll offset, tile, palette)
(+) = pixel offset composition within a tile
```
- `extract` = the color of the current pixel
- `duplicate` = at every pixel, the full rendering context (scroll, window, priority stack)

**Candidate: APU Echo Buffer as Directed Container**
```
S = Echo configuration (ESA, EDL, FIR coefficients)
P(s) = Buffer positions (0 to echo_length/4)
o(s) = echo_pos (current write position)
down(s, p) = the 8-tap FIR window centered at position p
(+) = circular offset within the ring buffer
```
- `extract` = current echo sample
- `duplicate` = at every buffer position, the full FIR context

Check law 5 (associativity): does circular buffer arithmetic satisfy (p + q) + r = p + (q + r)? Yes — addition mod buffer_length is associative.

**Candidate: CPU Execution as Stream Comonad**
The CPU instruction stream is the simplest directed container:
```
S = Unit (always the same: an infinite stream of instructions)
P(s) = Nat (instruction index from current PC)
o(s) = 0 (the current instruction)
down(s, n) = Unit (sub-stream from instruction n onward)
n + m = n + m (index m in the tail-from-n is index n+m globally)
```
- `extract` = execute current instruction
- `duplicate` = at every future instruction, the full execution context from that point

This is the formal power series / stream comonad from the umbral calculus connection.

### 5. Find Functors

| Code Pattern | Categorical Name | Location |
|-------------|-----------------|----------|
| `bus.read(bank, addr)` with bank mirroring ($80–$FF → $00–$7F) | Endofunctor on address container (bank & 0x7F is a natural transformation) | bus.rs |
| VRAM address remapping (translate_vram_addr) | Endofunctor on VRAM container (4 modes of bit rotation) | ppu/mod.rs |
| ARGB → RGBA conversion in run_frame_inner | Natural transformation between color containers | lib.rs |
| `op!` macro (address_mode → operation) | Kleisli composition in the State monad on (&mut Cpu, &mut Bus) | cpu/instructions.rs |
| Tile decode chain: tilemap → chr data → pixel | Functor composition: BG container → Tile container → Pixel container | ppu/render.rs |
| BRR decode: block → samples | Functor from BRR container (9-byte blocks) to sample container | spc700/dsp.rs |

Check functor laws for each candidate.

### 6. Find Natural Transformations

| Transformation | From → To | Naturality |
|---------------|-----------|------------|
| Bank mirroring (& 0x7F) | Full address space → Canonical address space | Commutes with read/write |
| ARGB → RGBA | PPU output format → Canvas format | Content-independent (per-pixel) |
| Snapshot serialize | Live state → Byte blob | Must commute with state transitions (the determinism contract) |
| APU cycle conversion (÷21) | Master clock → SPC clock | Must preserve cycle ordering |
| BG mode dispatch | BG config → Renderer | MODE_BPP table is the natural transformation |

### 7. Find Monads

| Code Pattern | Monad | Location |
|-------------|-------|----------|
| `&mut Bus` threading through CPU step | State monad on Bus | cpu/mod.rs, bus.rs |
| `Result<Cartridge, String>` from ROM loading | Result/Error monad | rom.rs |
| `cpu.cycles += elapsed` accumulation | Writer monad on (u64, +) | lib.rs frame loop |
| `pending_dma_cycles` accumulation | Writer monad on (u64, +) | bus.rs |
| Audio sample accumulation (`sample_buffer.push`) | Writer monad on (Vec<i16>, concat) | spc700/mod.rs, dsp.rs |
| `opcode_counts[op] += 1` histogram | Writer monad on ([u64; 256], pointwise +) | cpu/mod.rs |

**Writer monad associativity check**: Does the order of `catch_up` calls matter? The audit agent found that the cycle-debt mechanism is NOT chunk-equivalent — different call patterns with the same total cycles produce different SPC instruction boundaries. This is a **Writer monad associativity violation**: `(a + b) + c ≠ a + (b + c)` in the cycle-debt accounting. This is the root cause of the idle-skip audio divergence (T10).

### 8. Find Comonads and Directed Containers

Apply the directed container analysis from Step 4 systematically. For each candidate:
1. Identify (S, P, o, down, +)
2. Check all five laws
3. Identify the comonad operations (extract, duplicate)
4. Check comonad laws
5. Identify any comonad morphisms

### 9. Find Kleisli and Co-Kleisli Categories

**Kleisli arrows for the State monad (Bus)**:
Every CPU instruction is a Kleisli arrow `(Cpu, Bus) → (Cpu, Bus)`. The `step()` function composes these. The `op!` macro is the Kleisli composition combinator — it sequences address resolution (which mutates CPU by advancing PC) with the operation (which mutates CPU/Bus).

Flag: Is `op!` actually lawful? Does the sequencing always produce the same result regardless of how you decompose the steps?

**Co-Kleisli arrows for the Store comonad (Bus)**:
A bus read `Bus → u8` at a fixed address is a co-Kleisli arrow. The PPU's `read_register` and the APU's `cpu_read` are co-Kleisli arrows that extract values from the Store comonad.

**Kleisli arrows for the Writer monad (cycles)**:
Each scanline's CPU execution is a Kleisli arrow for the Writer monad `(result, total_cycles)`. The frame loop composes 262 of these.

### 10. Find Yoneda Structure

**VRAM address remapping** as Yoneda: The 4 remapping modes in `translate_vram_addr` rotate bit groups. This is a representable functor — the remapping is a natural transformation Hom(A, -) → Hom(A', -) that reindexes without touching data. The Yoneda lemma says this is completely determined by where the identity goes — i.e., by the single mapping `translate_vram_addr(addr)`.

**Opcode dispatch** as Yoneda: The `OPCODE_CYCLES[256]` table and `OPCODE_NAMES[256]` table are both natural transformations from the representable functor Hom(u8, -). By Yoneda, each is determined by its value at each opcode — which is exactly what the tables are.

**Builder pattern in bench harness**: The bench CLI accumulates options (--frames, --label, --path) then runs. This is Yoneda identity — the "map" is free until you "run."

### 11. Find Adjunctions

| Adjunction | Left (Free) | Right (Forgetful) | Where |
|-----------|-------------|-------------------|-------|
| Snapshot ⊣ Restore | snapshot(): State → Bytes | restore(): Bytes → State | snapshot.rs |
| Encode ⊣ Decode | ARGB → RGBA (canvas encode) | RGBA → ARGB (would be decode) | lib.rs |
| SPC file ⊣ APU state | load_spc(): SpcFile → Apu | (no inverse — lossy) | spc700/mod.rs |

Check round-trips: `restore(snapshot(state)) = state`? The snapshot format includes a version byte and magic — if these don't round-trip, the adjunction is broken.

### 12. Write category.tmp

This file MUST contain ALL of these sections:

```
Categorical Analysis — SNES Emulator (Rust/WASM)

Summary
<2-3 sentences: what categorical structures exist, which are well-formed, which are broken>

Container Inventory
<Table: Structure | Shape S | Positions P | Root o | Subshape down | Embedding (+) | Type (container/directed/morphism)>
Every container and directed container found.

Categorical Inventory
<Table: Structure | Type (functor/monad/comonad/nat-trans/adjunction/Kleisli/Yoneda) | Location | Well-Formed? | Notes>
Every non-container categorical structure found.

Directed Container Analysis
<For each directed container candidate: the (S, P, o, down, +) identification, law verification (which of the 5 hold?), comonad operations, and practical consequences>

Container Morphism Analysis
<Table: Morphism | Source | Target | Forward | Backward | Composition correct?>
Flag where morphism composition fails (e.g., DMA bypassing bus.write).

Law Violations
<Table: Structure | Law | Location | Consequence | Fix>
Each violation with its practical impact. The cycle-debt Writer monad associativity violation is likely the most significant.

Kleisli Composition Issues
<Table: Monad | Location | Current (manual) | Recommended (Kleisli compose)>

Yoneda Opportunities
<Table: Location | Current | Optimisation | Benefit>

Recommended Abstractions
<Table: Current | Categorical Structure | Refactor | Why>

Structures Already Correct
<List of well-formed categorical structures that should not be changed>

Connections Between Structures
<How the categorical structures relate: which directed containers give rise to which comonads, how container morphisms compose, how the Kleisli category for State(Bus) relates to the co-Kleisli category for Store(Bus)>

What Category Theory Can't Tell You Here
<Honest limits — things in this codebase that aren't categorical problems>
```

### 13. Return the analysis
Print the same content as category.tmp as your return value.

### What you do NOT do
- No tests. No application code changes. No CATEGORY_THEORY.md. Analysis only.
- You write exactly one file: category.tmp.
```

## Between Passes

After Pass 1 returns:
1. Confirm `category.tmp` exists in the project root
2. Read and summarise the findings for the user (brief — 3-5 lines)
3. Proceed to Pass 2

## Pass 2: Fix Agent

Launch a second agent with the prompt below. It reads `category.tmp`, aligns code with categorical structure, writes law tests, runs them, and writes CATEGORY_THEORY.md.

### Fix Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are the categorical fixer for an SNES emulator written in Rust, compiled to WASM. You read category.tmp, align the code with its categorical structure, write property-based law tests, and run them until they pass. After everything is done, you write CATEGORY_THEORY.md reflecting the final state.

CRITICAL CONSTRAINT: This emulator has a sacred determinism contract. After ALL changes, if a ROM is available, run:
  cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
The hashes MUST match: fb=54b3eed74f9f8432, audio=62300ecfc4da23e0. If they don't, your change broke emulation semantics. Revert and try again.

## Phase 1: Read the Analysis

Read `category.tmp` in the project root. If an existing `CATEGORY_THEORY.md` exists, read it for context but do not treat it as authoritative.

Extract:
1. Law Violations — categorical and directed container laws that are broken
2. Container Morphism Issues — compositions that don't factor correctly
3. Kleisli Composition Issues — manual composition that should use utilities
4. Yoneda Opportunities — map fusion and representability improvements
5. Recommended Abstractions — code to align with categorical structure
6. Container Inventory and Categorical Inventory — every structure that needs law tests

## Phase 2: Fix Issues

### 2a: Fix Law Violations
For each violation, fix the application code so the law holds. The law tells you both sides must be equal — it doesn't tell you which side is right.

IMPORTANT: The cycle-debt Writer monad associativity violation (catch_up chunking) is a known deep issue documented in T10. Do NOT attempt to fix it here — flag it with full categorical framing for future work.

### 2b: Fix Container Morphism Composition
Where morphisms don't compose correctly (e.g., DMA bypassing bus.write), document the discrepancy. Only fix if the change is purely structural and preserves the determinism contract.

### 2c: Align Abstractions
Apply the recommended refactors from category.tmp. Keep changes minimal.

## Phase 3: Write Law Tests

For every structure in the Container Inventory and Categorical Inventory, write property-based law tests.

Place in `tests/categorical/` (Rust integration test convention). Create a single test file `tests/categorical_laws.rs` or multiple files as appropriate.

### Directed Container Laws (5 laws)
For each directed container (S, P, o, down, +):
```rust
#[test]
fn store_comonad_law1_down_at_root_is_identity() {
    // down(s, o(s)) = s
    // The sub-space at the root position is the whole space
}

#[test]
fn store_comonad_law4_root_is_embedding_identity() {
    // o(s) + p = p
    // Embedding through the root is identity
}

#[test]
fn store_comonad_law5_embedding_is_associative() {
    // (p + q) + r = p + (q + r)
    // Position embedding composes associatively
}
```

### Comonad Laws (derived from directed container)
```rust
#[test]
fn comonad_law1_extract_after_duplicate() {
    // extract(duplicate(w)) = w
    // Extracting from duplicated structure recovers original
}
```

### Container Morphism Laws
```rust
#[test]
fn snapshot_roots_commute() {
    // extract(restore(snapshot(state))) = extract(state)
    // Snapshot/restore preserves the observable state
}
```

### Functor Laws
```rust
#[test]
fn bank_mirroring_is_idempotent() {
    // (bank & 0x7F) & 0x7F = bank & 0x7F
    // Mirroring applied twice equals mirroring applied once
}

#[test]
fn vram_remap_composition() {
    // For each remap mode: remap(remap(addr)) should be well-defined
}
```

### Writer Monad Laws
```rust
#[test]
fn cycle_accumulation_is_associative() {
    // (a + b) + c = a + (b + c) for master cycle counts
}

#[test]
fn audio_sample_accumulation_is_associative() {
    // Concatenating sample buffers: (a ++ b) ++ c = a ++ (b ++ c)
}
```

### Kleisli Composition (State monad)
```rust
#[test]
fn op_macro_sequencing_is_deterministic() {
    // address_resolve ; operation = single step
    // The op! macro's two-phase borrow is a lawful Kleisli composition
}
```

### Natural Transformation (Naturality Square)
```rust
#[test]
fn argb_rgba_is_natural() {
    // Converting ARGB→RGBA then reading pixel p = reading pixel p then converting
    // The transformation is content-independent
}
```

### Round-Trip / Adjunction
```rust
#[test]
fn snapshot_restore_roundtrip() {
    // restore(snapshot(state)) = state
    // The unit of the adjunction
}

#[test]
fn flag_pack_unpack_roundtrip() {
    // StatusRegister::from_byte(sr.to_byte()) = sr
    // Flag serialization is an isomorphism
}
```

### What to test (prioritised for emulators)
1. Snapshot round-trip (adjunction unit) — data loss is a save-state bug
2. Container morphism composition (bus dispatch) — wrong address decode is an emulation bug
3. Writer monad associativity (cycle counting) — the T10 idle-skip audio divergence
4. Directed container laws for bus/memory — structural invariants
5. Functor laws for address remapping — VRAM access correctness
6. Naturality for format conversions — ARGB/RGBA, i16/float audio

### What NOT to test
- Individual opcode behavior (accuracy testing, not categorical)
- Standard library functors (Vec::map, Option::map)
- Game-specific behavior

## Phase 4: Run Tests (3 attempts max)

```
for attempt in 1..3:
  1. Run categorical law tests
  2. All pass → go to Phase 5
  3. Read failures — the counterexample tells you what's broken
  4. Fix: the law says both sides must be equal. Decide which side is right.
  5. Next attempt
```

Run with:
```bash
cargo test categorical -- --nocapture
```

If the bench ROM is available, also verify determinism:
```bash
cargo run --release --bin bench rom/smw.smc 2>&1 | grep hash
```

## Phase 5: Write CATEGORY_THEORY.md

After all fixes and tests pass, write `docs/CATEGORY_THEORY.md` from scratch. Archive any existing one first.

Include:

### Container Theory Section
- Container inventory with (S, P) for each structure
- Container morphism diagram (Mermaid): show all morphisms between containers, with forward/backward arrows
- Composition analysis: which morphism chains are correct, which are broken

### Directed Container Section
- Each directed container with its (S, P, o, down, +) fully specified
- Law verification status (which of the 5 hold, which are approximate)
- Comonad operations derived from each directed container
- The connection between the determinism contract and comonad morphism laws

### Classical Categorical Structure
- Categorical inventory with Mermaid diagrams
- Functor, monad, natural transformation analysis
- Kleisli category for State(Bus) — how CPU instructions compose
- The Writer monad associativity violation and its connection to T10

### Connections
- How directed containers relate to the classical structures
- The Store comonad (Bus) and its co-Kleisli arrows (register reads)
- The duality: State monad (CPU writes through Bus) vs Store comonad (Bus provides context to reads)
- Container morphisms as the unifying abstraction for address decode, DMA, HDMA, and snapshot

### Law Test Coverage
| Structure | Type | Laws Tested | Status |

### What Category Theory Tells You
- The cycle-debt associativity violation is the root cause of T10 audio divergence
- The determinism contract is a comonad morphism law
- DMA bypassing bus.write is a container morphism that doesn't factor through the bus morphism

### What Category Theory Can't Tell You
- Which opcodes to implement (that's the 65816 spec, not category theory)
- What cycle count each instruction should take (hardware measurement, not abstraction)
- Whether Mode 7 rotation looks correct (pixel-level accuracy, not structure)

## Phase 6: Report and Cleanup

Delete `category.tmp` — it has been consumed.

Return this report:

```markdown
# SNES Category Sweep Report

## Issues Fixed

### Law Violations Fixed
| Structure | Law | Counterexample | Fix | File |

### Container Morphism Issues Documented
| Morphism | Issue | Resolution |

### Abstractions Aligned
| Current | Categorical Structure | Files Changed |

## Law Tests Written
| File | Structure | Laws Tested | Count |

## Directed Container Verification
| Container | Law 1 (root) | Law 2 (root embed) | Law 3 (nesting) | Law 4 (identity) | Law 5 (assoc) |

## Test Run
- Attempts: N/3
- Final result: PASS / FAIL
- Laws tested: N total, N passed, N failed

### Determinism Check
- ROM available: yes/no
- FB hash: ✓/✗ (expected 54b3eed74f9f8432)
- Audio hash: ✓/✗ (expected 62300ecfc4da23e0)

### Fixes During Test Loop
| Attempt | Law | Structure | Counterexample | Fix | File |

## Key Categorical Findings
<The most important structural insights — what category theory revealed about the codebase that wasn't obvious from reading the code>

## Remaining Issues
<Violations too deep to fix, structures that need design decisions>
```
```

## After Pass 2

Present the full report from Pass 2 to the user. If there are remaining law violations after 3 attempts, highlight them — these are correctness bugs, not style issues. Pay special attention to:
- The Writer monad associativity violation (cycle-debt chunking) — frame this as the categorical root cause of T10
- The snapshot round-trip adjunction — this is the determinism contract in categorical language
- Any container morphism composition failures — these indicate address decode bugs
