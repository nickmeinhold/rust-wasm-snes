---
name: near-jit-sync
argument-hint: "[scope: 'full', 'cpu-apu', 'dma', 'scanline-flush']"
description: >
  Read-only audit of JIT synchronization against Near/byuu's cooperative
  threading specification. Writes findings to jit-sync.tmp. Intended to be
  run as part of /near-sweep1 (parallel with near-scheduler, near-oscillator,
  near-serialization), whose fix agent reads all .tmp files.
---

# Near JIT Sync Compliance (`/near-jit-sync`)

Read-only audit. Writes `jit-sync.tmp`. Does NOT modify code.

## Reference: Near's Specification

From `design/cooperative-threading/README.md`:

> "What we've done here is made it so the CPU will keep on running until it
> tries to read from a shared memory region with the APU. Only then will it
> catch up the APU to the CPU before performing the read."

> "the CPU may be hundreds, or even thousands, of instructions ahead in time
> from the APU, simply because the CPU hasn't talked with the APU in a long
> time."

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

You are auditing an SNES emulator's JIT synchronization against Near/byuu's
specification. You are READ-ONLY. You write exactly one file: `jit-sync.tmp`.

Scope: $ARGUMENTS (default: full)

## What to Check

### 1. Sync-on-Access ($2140-$2143)

Read `src/bus.rs`. For every code path that reads or writes the APU port range
($2140-$2143, mirrored at $80-$BF), verify:

- `sync_apu()` is called BEFORE the port value is accessed
- The sync uses the correct cycle delta (master_clock - last_apu_sync)
- The sync is not conditional on any flag that could skip it

Build a table:

| Code Path | File:Line | sync_apu() Before Access? | Correct Delta? |
|-----------|-----------|---------------------------|----------------|

Check mirrors: banks $00-$3F and $80-$BF both map APU ports at $2140-$217F.
Verify both ranges are covered.

### 2. No Sync on Pure Memory

Read `src/bus.rs` read/write dispatch. For ROM, WRAM, and SRAM reads:

- Verify NO call to sync_apu() on these paths
- Check `is_pure_memory()` — does it correctly identify all pure-memory ranges?
- Look for any accidental sync_apu() calls in pure-memory paths

### 3. Deadlock Prevention

Near warns: "you *must* eventually force-switch to the APU to prevent the APU
from deadlocking if the chips never communicate."

Find the end-of-scanline flush. Verify:

- It exists (likely in the frame loop in `src/lib.rs`)
- It runs unconditionally, even if no port access occurred that scanline
- The maximum stale window is bounded (one scanline = 1364 master cycles)

### 4. Context Switch Count

Near claims bsnes syncs CPU↔APU "a few thousand times a second." Estimate
from code structure whether the count is in the right ballpark (~45K-94K/sec),
not millions (which per-instruction sync would produce).

### 5. DMA Sync Points

Read `Bus::execute_general_dma()`. Verify:

- DMA to APU ports triggers sync_apu()
- Mid-DMA periodic sync exists (every N bytes)
- Post-DMA flush exists

### 6. HDMA Sync Points

Read `Bus::hdma_run_scanline()`. Verify:

- sync_apu() is called before HDMA transfers
- HDMA to APU-range registers would trigger sync

### 7. Idle-Skip Interaction

If `idle-skip` feature exists, read `Cpu::try_idle_skip()`. Verify:

- The skip calls catch_up with the correct cycle count
- `last_apu_sync` is updated to prevent double-crediting
- `ports_written_during_run` flag exists and is checked
- Document whether the skip subdivides the catch_up or does it in one call

### 8. Write jit-sync.tmp

Include ALL of:
- Sync-on-access table (all code paths)
- Pure-memory verification (any leaks?)
- Deadlock prevention status
- Context switch count estimate
- DMA/HDMA sync status
- Idle-skip interaction analysis
- Compliance verdict: COMPLIANT / PARTIAL / NON-COMPLIANT for each check
- Recommended fixes (numbered, with effort S/M/L)
```
