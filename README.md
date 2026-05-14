# Rust/WASM SNES Emulator

A cycle-accurate Super Nintendo emulator written in Rust, compiled to WebAssembly, running in the browser.

**[→ Live demo](https://nickmeinhold.github.io/rust-wasm-snes/)** &nbsp;·&nbsp; pick a game from the in-browser ROM picker (746 titles via Internet Archive, served through a Cloudflare CORS proxy)

To my knowledge, this is the only Rust/WASM SNES emulator running in a browser.

## What's actually interesting here

There are plenty of SNES emulators in C, C++, and Rust. There are very few that:

1. **Run in the browser** as WebAssembly without a desktop runtime
2. **Are audio-verified against a reference implementation** rather than tuned by ear
3. **Use a trace-oracle debugging methodology** to ground every CPU/APU change in measurable behaviour

Those three properties are what made this project worth building. Everything else is in service of them.

## Audio: verified to 1.01× amplitude

The S-DSP (the SNES sound chip) is famously hard to get right. Most Rust SNES emulator efforts are unverified or "sounds OK." This implementation:

- Decodes BRR (Bit-Rate Reduction) audio four samples at a time, with correct KON delay and echo write-back
- Uses cycle-debt accounting in the SPC700 sub-CPU so timing overshoot doesn't compound
- Compares output against [blargg's snes_spc reference](https://www.slack.net/~ant/libs/audio.html) using **principal-component analysis** of waveforms

Result: 1.01× amplitude vs reference, well within the [Hafter audio threshold](https://en.wikipedia.org/wiki/Just-noticeable_difference) (0.25 dB / 1°). The PCA comparison script lives in `reference/principal_component_compare.py`; the trace-diff tool in `reference/diff_trace.py`.

## Trace-oracle debugging

The hardest bugs in this codebase weren't found by reading code — they were found by *diffing execution traces against a reference emulator*.

The methodology:

1. Add feature-gated trace logging to the CPU/APU (`cargo build --features trace` etc.)
2. Run the same ROM in [Mesen2](https://www.mesen.ca/) (high-accuracy reference) with its built-in Trace Logger
3. Diff the two traces; the first divergence is the bug

This caught two SPC700 bugs in <5 minutes that would have taken days otherwise:
- **MUL YA Z-flag**: Z came from Y register only, should come from the full 16-bit result
- **POP A/X/Y flag-spurious-write**: Unlike the 6502's PLA, SPC700's POP does *not* modify N/Z flags. This bug killed every audio sequencer.

The 65C816 main CPU went through the same process across 100,000-instruction comparisons — proven clean.

## Architecture

```
┌─────────────────┐    ┌──────────┐
│  65C816 CPU     │◄──►│   PPU    │  Picture Processing Unit
│  (all 256 ops)  │    │ (modes   │  - Mode 0/1/3/7 supported
│                 │    │  partial)│  - Per-layer rendering for debug
└────────┬────────┘    └──────────┘
         │
         │   bus.rs (memory map, LoROM)
         │
┌────────▼────────┐    ┌──────────┐
│  SPC700 CPU     │◄──►│  S-DSP   │  Sound DSP
│  (all 256 ops)  │    │          │  - 8 voices, BRR decode
│  cycle-debt     │    │          │  - ADSR/GAIN envelope
│  timing         │    │          │  - Gaussian interpolation
└─────────────────┘    └──────────┘  - 8-tap FIR echo
```

- **CPU**: `src/cpu/` — every 65C816 opcode, master-cycle accurate (×6 for SNES base clock)
- **APU**: `src/spc700/` — SPC700 sub-CPU plus S-DSP, with `cargo build --features trace` for execution logging
- **PPU**: `src/ppu/` — partial mode coverage (1/3 work, 7 partial); per-layer framebuffer dump for debugging tilemap/priority issues
- **Joypad**: `src/joypad.rs` — both `$4218`/`$4219` auto-read and `$4016`/`$4017` serial protocol (the latter required for SMW overworld input)

## What runs

| Game | Status |
|------|--------|
| Super Mario World | Boots; overworld renders; mode-07 input chain has a known stall (see issues) |
| The Legend of Zelda: A Link to the Past | Overworld + Mode 7 working |
| Mega Man X | Boots and plays; audio broken |
| Super Metroid | Reaches region-lockout screen |
| Super Mario Kart | Black screen — needs DSP-1 coprocessor |
| Chrono Trigger | Needs HiROM support |

## Determinism contract

For SMW × 600 frames at default reset state, the framebuffer + audio hashes are
locked:

| | |
|---|---|
| `final_fb_hash` | `54b3eed74f9f8432` |
| `final_audio_hash` | `62300ecfc4da23e0` |

These are bit-identical across native (x86_64) and browser (wasm32 in
Chromium). Any change that flips a hash is by definition a semantic change.
The `bench/` harness produces these on every run; `bench/compare.js` prints ✓/✗.
CI (`.github/workflows/bench.yml`) enforces both on every push — see
`bench/README.md` for the harness, `docs/T10_IDLE_LOOP_DETECTION.md` for an
example of how the contract caught a real regression mid-optimization.

## Save states

`src/snapshot.rs` serializes full emulator state (CPU + bus + PPU + APU + SRAM)
to a length-prefixed binary blob, ~484 KB per snapshot. Hand-rolled little-endian
format, no serde dependency. Magic + version checked on restore. Round-trip
determinism validated by `cargo run --bin snapshot_test`.

## Build

```bash
# Native (debug tooling, headless ROM runners, audio comparison, bench)
cargo build --release
cargo run --release --bin bench rom/smw.smc
cargo run --bin debug_tm -- path/to/rom.smc

# WebAssembly (browser)
wasm-pack build --target web
python3 web/serve.py
# open http://localhost:8090

# Phase B worker scaffold (off-main-thread emulation; experimental)
# open http://localhost:8090/index-phase-b.html — requires the COOP/COEP headers
# that serve.py adds; a plain `python -m http.server` will silently disable SAB

# With execution tracing (for debugging against a reference emulator)
cargo build --release --features trace

# With the experimental idle-loop optimization (default off; CPU semantics correct
# but audio diverges — see docs/T10_IDLE_LOOP_DETECTION.md §8)
cargo build --release --features idle-skip
```

## Reference tooling

- `reference/snes_spc/` — blargg's snes_spc (gitignored; clone separately for audio comparison)
- `reference/principal_component_compare.py` — PCA-based waveform comparison
- `reference/compare_waveforms.py` — direct waveform diff
- `reference/diff_trace.py` — execution-trace diff against Mesen2 traces
- `~/Applications/Mesen2/` — high-accuracy reference emulator with Lua scripting and Trace Logger

## Known issues

- SMW overworld stalls in mode 07 — the `$4016` joypad serial protocol fix is correct in the debug runner but the WASM build still doesn't transition past the title in some cases. Tracked.
- MMX audio broken (different SPC pattern than tested games).
- Crate name is still `zelda-a-link-to-the-past` from when LTTP was the only working game. Rename pending.

## Credits

- [blargg's snes_spc](https://www.slack.net/~ant/libs/audio.html) — audio reference implementation
- [Mesen2](https://www.mesen.ca/) — execution-trace reference and Lua-scriptable debugging
- [Internet Archive](https://archive.org/) — ROM library accessed via CORS proxy
- [SNES Development Manual](https://archive.org/details/SNESDevManual) — hardware reference

Built across ten development sessions in early 2026. Audio watermarking expertise applied via [Tirkel, Meinhold et al. (SIN 2011)](https://dl.acm.org/doi/10.1145/2070425.2070451).

## License

MIT. ROM files are *not* included — bring your own; the in-browser ROM picker uses publicly-archived material from the Internet Archive.
