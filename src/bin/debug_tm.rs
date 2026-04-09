/// Debug tool: loads SMW ROM, runs frames, and logs every TM ($212C) write
/// with the CPU address that wrote it.
///
/// Usage: cargo run --bin debug_tm -- rom/smw.smc [frames]

use std::cell::Cell;
use std::env;
use std::fs;
use std::io::Write;

use zelda_a_link_to_the_past::rom::Cartridge;
use zelda_a_link_to_the_past::bus::Bus;
use zelda_a_link_to_the_past::cpu::Cpu;
use std::path::Path;

const MASTER_CYCLES_PER_SCANLINE: u64 = 1364;
const SCANLINES_PER_FRAME: u16 = 262;
const VBLANK_START: u16 = 225;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <rom.smc> [frames]", args[0]);
        std::process::exit(1);
    }

    let rom_path = &args[1];
    let num_frames: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(600);

    let cart = Cartridge::load(Path::new(rom_path)).unwrap();
    let mut bus = Bus::new(cart);
    let mut cpu = Cpu::new();

    // Reset vector
    let reset_lo = bus.read(0x00, 0xFFFC);
    let reset_hi = bus.read(0x00, 0xFFFD);
    cpu.pc = (reset_lo as u16) | ((reset_hi as u16) << 8);
    cpu.pbr = 0;

    let mut last_tm: u8 = 0;
    let mut frame_count: u32 = 0;
    let mut pre_0d9d: u8 = 0;
    let mut pre_3e: u8 = 0;
    let mut last_bg1sc: u8 = 0xFF;

    // Watchpoint/trace state (previously unsafe static mut)
    let ow_tracing = Cell::new(false);
    let ow_count = Cell::new(0u32);
    let last_game_mode = Cell::new(0xFFu8);
    let m08_tracing = Cell::new(false);
    let m08_count = Cell::new(0u32);
    let last_0d7c = Cell::new(0u16);

    // Simulate button press for Start at frame 300 (auto-start the game)
    let start_frame = 180;

    for frame in 0..num_frames {
        frame_count = frame;

        // Press Start around frame 180 to get past title screen
        if frame == start_frame {
            bus.joypad.current |= 0x1000; // Start
            eprintln!("--- Frame {}: Pressing Start ---", frame);
        }
        if frame == start_frame + 5 {
            bus.joypad.current &= !0x1000;
        }
        // Press buttons to advance through transition modes
        // $16 checks "newly pressed" — must release then press, not hold
        let game_mode = bus.wram[0x0100];
        if game_mode == 0x07 {
            bus.joypad.current = 0x1000; // Start only (mode 07 checks $15/$17)
        } else if game_mode >= 0x08 && game_mode < 0x0E {
            // Toggle A button every other frame so it registers as "newly pressed"
            if frame % 2 == 0 {
                bus.joypad.current = 0x0080; // A
            } else {
                bus.joypad.current = 0;
            }
        } else if game_mode >= 0x0E {
            bus.joypad.current = 0;
        }

        for scanline in 0..SCANLINES_PER_FRAME {
            if scanline == VBLANK_START {
                bus.vblank = true;
                bus.nmi_flag = true;
                if bus.nmitimen & 0x80 != 0 {
                    cpu.nmi_pending = true;
                }
                bus.auto_joypad_busy = false;
            }

            if scanline == 0 {
                bus.vblank = false;
                bus.nmi_flag = false;
                bus.hdma_init_frame();
            }

            bus.ppu.scanline = scanline;

            let irq_mode = (bus.nmitimen >> 4) & 0x03;
            if irq_mode != 0 && !bus.irq_flag {
                let fire = match irq_mode {
                    1 => true,
                    2 => scanline == bus.vtime,
                    3 => scanline == bus.vtime,
                    _ => false,
                };
                if fire {
                    bus.irq_flag = true;
                    cpu.irq_pending = true;
                }
            }

            let target = cpu.cycles + MASTER_CYCLES_PER_SCANLINE;
            let hblank_start = cpu.cycles + MASTER_CYCLES_PER_SCANLINE - 68 * 4;
            bus.hblank = false;

            while cpu.cycles < target {
                if !bus.hblank && cpu.cycles >= hblank_start {
                    bus.hblank = true;
                }

                // Track PC for write breakpoints
                let pre_pc = cpu.pc;
                let pre_pbr = cpu.pbr;
                let pre_tm = bus.ppu.tm;
                let pre_wram0 = bus.wram[0];

                bus.last_write_bank = pre_pbr;
                bus.last_write_pc = pre_pc;

                // Targeted overworld trace: start when mode 07 handler is entered
                if !ow_tracing.get() && frame >= 400 && cpu.pc == 0x9C64 && cpu.pbr == 0x00
                    && bus.wram[0x0100] == 0x07 {
                    ow_tracing.set(true);
                    eprintln!("=== OVERWORLD TRACE START frame={} ===", frame);
                }
                if ow_tracing.get() && ow_count.get() < 500000 {
                    let op = bus.read(cpu.pbr, cpu.pc);
                    eprintln!("PC:{:02X}:{:04X} OP:{:02X} A:{:04X} X:{:04X} Y:{:04X} SP:{:04X} P:{:02X} DP:{:04X} DB:{:02X} E:{}",
                        cpu.pbr, cpu.pc, op, cpu.a, cpu.x, cpu.y, cpu.sp,
                        cpu.p.to_byte(cpu.emulation), cpu.dp, cpu.dbr,
                        if cpu.emulation { 1 } else { 0 });
                    ow_count.set(ow_count.get() + 1);
                    if ow_count.get() >= 500000 {
                        eprintln!("=== OVERWORLD TRACE DONE ({} instructions) ===", ow_count.get());
                    }
                }

                let elapsed = cpu.step(&mut bus);
                cpu.cycles += elapsed;

                // Check if TM changed
                if bus.ppu.tm != pre_tm {
                    eprintln!(
                        "Frame {:4} Scan {:3} | TM: {:02X} -> {:02X} (BG2={}) | PC={:02X}:{:04X}",
                        frame, scanline, pre_tm, bus.ppu.tm,
                        bus.ppu.tm & 2 != 0, pre_pbr, pre_pc
                    );
                }

                // Trace instructions around BGMODE computation at 05:8570
                if cpu.pbr == 0x05 && cpu.pc == 0x8570 && !cpu.trace {
                    eprintln!("=== BGMODE TRACE (frame {} game={:02X}) $00={:02X} $3E={:02X} ===",
                        frame, bus.wram[0x0100], bus.wram[0], bus.wram[0x3E]);
                }
                // Watchpoint: WRAM $0100 (game mode) changes
                let cur_mode = bus.wram[0x0100];
                if cur_mode != last_game_mode.get() {
                    eprintln!("  GAME_MODE {:02X} -> {:02X} frame={} scan={} PC={:02X}:{:04X}",
                        last_game_mode.get(), cur_mode, frame, scanline, pre_pbr, pre_pc);
                    last_game_mode.set(cur_mode);
                }
                // Trace mode 08 handler for one frame
                if cpu.pc == 0x9CD1 && cpu.pbr == 0x00 && cur_mode == 0x08 && !m08_tracing.get() && frame == 365 {
                    m08_tracing.set(true);
                    eprintln!("  MODE08 buttons: $15={:02X} $16={:02X} $17={:02X} $18={:02X} joy={:04X} $DA2={:02X} $DA4={:02X} $DA0={:02X}",
                        bus.wram[0x15], bus.wram[0x16], bus.wram[0x17], bus.wram[0x18],
                        bus.joypad.current, bus.wram[0x0DA2], bus.wram[0x0DA4], bus.wram[0x0DA0]);
                }
                if m08_tracing.get() && m08_count.get() < 200 {
                    let op = bus.read(cpu.pbr, cpu.pc);
                    eprintln!("  M08[{:3}] {:02X}:{:04X} op={:02X} A={:04X} X={:04X} Y={:04X} P={:02X}",
                        m08_count.get(), cpu.pbr, cpu.pc, op, cpu.a, cpu.x, cpu.y,
                        cpu.p.to_byte(cpu.emulation));
                    m08_count.set(m08_count.get() + 1);
                    if cpu.pc == 0x806B && cpu.pbr == 0x00 && m08_count.get() > 5 {
                        m08_tracing.set(false);
                    }
                }
                // Start tracing 30 instructions before we expect to hit 8570
                // The code starts around 05:8540
                if cpu.pbr == 0x05 && cpu.pc == 0x853F && frame >= 210 && frame <= 220 {
                    let ptr_lo = bus.wram[0x65] as u32;
                    let ptr_hi = bus.wram[0x66] as u32;
                    let ptr_bank = bus.wram[0x67] as u32;
                    let ptr = (ptr_bank << 16) | (ptr_hi << 8) | ptr_lo;
                    let effective = ptr.wrapping_add(cpu.y as u32);
                    let val = bus.read((effective >> 16) as u8, effective as u16);
                    eprintln!("  LDA [$65],Y: ptr={:06X} Y={:04X} eff={:06X} val={:02X} (A will get {:02X})",
                        ptr, cpu.y, effective, val, val);
                }

                // Watchpoint: WRAM $0D7C (16-bit) — the VRAM transfer pointer
                let cur_0d7c = (bus.wram[0x0D7C] as u16) | ((bus.wram[0x0D7D] as u16) << 8);
                if cur_0d7c != last_0d7c.get() {
                    eprintln!("Frame {:4} Scan {:3} | $0D7C: {:04X} -> {:04X} | PC={:02X}:{:04X} A={:04X} X={:04X} Y={:04X}",
                        frame, scanline, last_0d7c.get(), cur_0d7c, pre_pbr, pre_pc, cpu.a, cpu.x, cpu.y);
                    last_0d7c.set(cur_0d7c);
                }

                // Track BG1SC changes
                {
                    let raw_bg1sc = ((bus.ppu.bg[0].tilemap_addr >> 8) as u8 & 0xFC) | bus.ppu.bg[0].tilemap_size;
                    if raw_bg1sc != last_bg1sc {
                        eprintln!(
                            "Frame {:4} Scan {:3} | BG1SC: {:02X} -> {:02X} (tmap={:04X} size={}) | PC={:02X}:{:04X}",
                            frame, scanline, last_bg1sc, raw_bg1sc,
                            bus.ppu.bg[0].tilemap_addr, bus.ppu.bg[0].tilemap_size,
                            pre_pbr, pre_pc
                        );
                        last_bg1sc = raw_bg1sc;
                    }
                }

                // Watchpoint: RAM $0D9D (SMW's TM shadow variable)
                let new_0d9d = bus.wram[0x0D9D];
                if new_0d9d != pre_0d9d {
                    eprintln!(
                        "Frame {:4} Scan {:3} | RAM $0D9D: {:02X} -> {:02X} (BG2={}) | PC={:02X}:{:04X} A={:02X} X={:02X} Y={:02X}",
                        frame, scanline, pre_0d9d, new_0d9d,
                        new_0d9d & 2 != 0, pre_pbr, pre_pc,
                        cpu.a as u8, cpu.x as u8, cpu.y as u8
                    );
                    pre_0d9d = new_0d9d;
                }

                // Watchpoint: WRAM $003E (BGMODE shadow — NMI copies to $2105)
                let new_3e = bus.wram[0x3E];
                if new_3e != pre_3e {
                    eprintln!(
                        "Frame {:4} Scan {:3} | WRAM $3E: {:02X} -> {:02X} (mode={} bg3hi={}) | PC={:02X}:{:04X} A={:04X} X={:04X} Y={:04X} P={:02X}",
                        frame, scanline, pre_3e, new_3e,
                        new_3e & 7, (new_3e >> 3) & 1, pre_pbr, pre_pc,
                        cpu.a, cpu.x, cpu.y, cpu.p.to_byte(cpu.emulation)
                    );
                    pre_3e = new_3e;
                }

                if bus.pending_dma_cycles > 0 {
                    let dma = bus.pending_dma_cycles;
                    cpu.cycles += dma;
                    bus.apu.catch_up(dma as u32);
                    bus.pending_dma_cycles = 0;
                }

                bus.apu.catch_up(elapsed as u32);
            }

            if scanline >= 1 && scanline <= 224 {
                bus.hdma_run_scanline();
                bus.ppu.render_scanline(scanline - 1);
            }
        }

        // Log state every 60 frames
        if frame % 60 == 0 {
            let game_mode = bus.wram[0x0100];
            let sub_mode = bus.wram[0x0DB4]; // SMW overworld sub-state
            eprintln!(
                "=== Frame {} | TM={:02X} | bgmode={:02X}(m{} bg3hi={}) wram3E={:02X} bright={} | game={:02X} sub={:02X} | hscr={} vscr={} | PC={:02X}:{:04X} ===",
                frame, bus.ppu.tm,
                bus.ppu.bgmode, bus.ppu.bgmode & 7, (bus.ppu.bgmode >> 3) & 1,
                bus.wram[0x3E],
                bus.ppu.inidisp & 0x0F,
                game_mode, sub_mode,
                bus.ppu.bg[0].hscroll, bus.ppu.bg[0].vscroll,
                cpu.pbr, cpu.pc
            );
        }
    }

    // --- Second pass: focused VRAM tracking during overworld init ---
    eprintln!("\n=== Pass 2: VRAM tracking during overworld init ===");
    let cart2 = Cartridge::load(Path::new(rom_path)).unwrap();
    let mut bus2 = Bus::new(cart2);
    let mut cpu2 = Cpu::new();
    let reset_lo2 = bus2.read(0x00, 0xFFFC);
    let reset_hi2 = bus2.read(0x00, 0xFFFD);
    cpu2.pc = (reset_lo2 as u16) | ((reset_hi2 as u16) << 8);
    cpu2.pbr = 0;

    // Run to frame 210 (just before overworld init)
    for frame in 0..210 {
        if frame == start_frame { bus2.joypad.current |= 0x1000; }
        if frame == start_frame + 5 { bus2.joypad.current &= !0x1000; }
        for scanline in 0..SCANLINES_PER_FRAME {
            if scanline == VBLANK_START {
                bus2.vblank = true; bus2.nmi_flag = true;
                if bus2.nmitimen & 0x80 != 0 { cpu2.nmi_pending = true; }
                bus2.auto_joypad_busy = false;
            }
            if scanline == 0 { bus2.vblank = false; bus2.nmi_flag = false; bus2.hdma_init_frame(); }
            bus2.ppu.scanline = scanline;
            let irq_mode = (bus2.nmitimen >> 4) & 0x03;
            if irq_mode != 0 && !bus2.irq_flag {
                let fire = match irq_mode { 1 => true, 2 => scanline == bus2.vtime, 3 => scanline == bus2.vtime, _ => false };
                if fire { bus2.irq_flag = true; cpu2.irq_pending = true; }
            }
            let target = cpu2.cycles + MASTER_CYCLES_PER_SCANLINE;
            bus2.hblank = false;
            while cpu2.cycles < target {
                bus2.last_write_bank = cpu2.pbr; bus2.last_write_pc = cpu2.pc;
                let elapsed = cpu2.step(&mut bus2);
                cpu2.cycles += elapsed;
                if bus2.pending_dma_cycles > 0 { let d = bus2.pending_dma_cycles; cpu2.cycles += d; bus2.apu.catch_up(d as u32); bus2.pending_dma_cycles = 0; }
                bus2.apu.catch_up(elapsed as u32);
            }
            if scanline >= 1 && scanline <= 224 { bus2.hdma_run_scanline(); bus2.ppu.render_scanline(scanline - 1); }
        }
    }

    eprintln!("VMAIN state at frame 210: increment={:02X} remap={} step={} inc_after_high={}",
        bus2.ppu.vram_increment, bus2.ppu.vram_remap,
        match bus2.ppu.vram_increment & 0x03 { 0=>1, 1=>32, _=>128 },
        bus2.ppu.vram_increment & 0x80 != 0);

    // Snapshot VRAM
    let vram_before = bus2.ppu.vram.clone();

    // Run frames 210-250 (overworld init happens here)
    for frame in 210..250 {
        for scanline in 0..SCANLINES_PER_FRAME {
            if scanline == VBLANK_START {
                bus2.vblank = true; bus2.nmi_flag = true;
                if bus2.nmitimen & 0x80 != 0 { cpu2.nmi_pending = true; }
                bus2.auto_joypad_busy = false;
            }
            if scanline == 0 { bus2.vblank = false; bus2.nmi_flag = false; bus2.hdma_init_frame(); }
            bus2.ppu.scanline = scanline;
            let irq_mode = (bus2.nmitimen >> 4) & 0x03;
            if irq_mode != 0 && !bus2.irq_flag {
                let fire = match irq_mode { 1 => true, 2 => scanline == bus2.vtime, 3 => scanline == bus2.vtime, _ => false };
                if fire { bus2.irq_flag = true; cpu2.irq_pending = true; }
            }
            let target = cpu2.cycles + MASTER_CYCLES_PER_SCANLINE;
            bus2.hblank = false;
            while cpu2.cycles < target {
                bus2.last_write_bank = cpu2.pbr; bus2.last_write_pc = cpu2.pc;
                let elapsed = cpu2.step(&mut bus2);
                cpu2.cycles += elapsed;
                if bus2.pending_dma_cycles > 0 { let d = bus2.pending_dma_cycles; cpu2.cycles += d; bus2.apu.catch_up(d as u32); bus2.pending_dma_cycles = 0; }
                bus2.apu.catch_up(elapsed as u32);
            }
            if scanline >= 1 && scanline <= 224 { bus2.hdma_run_scanline(); bus2.ppu.render_scanline(scanline - 1); }
        }
    }

    // Compare VRAM
    let mut changed_regions: Vec<(usize, usize)> = Vec::new();
    let mut in_region = false;
    let mut region_start = 0;
    for i in 0..0x10000 {
        let changed = bus2.ppu.vram[i] != vram_before[i];
        if changed && !in_region { region_start = i; in_region = true; }
        if !changed && in_region { changed_regions.push((region_start, i)); in_region = false; }
    }
    if in_region { changed_regions.push((region_start, 0x10000)); }

    let total_changed: usize = changed_regions.iter().map(|(s,e)| e - s).sum();
    eprintln!("VRAM changes during frames 210-250: {} bytes in {} regions", total_changed, changed_regions.len());
    for (start, end) in &changed_regions {
        eprintln!("  VRAM 0x{:04X}-0x{:04X} ({} bytes)", start, end - 1, end - start);
    }

    // Check the BG1 chr region specifically (chr=0x0000, tile data starts at byte 0)
    let chr_nonzero = bus2.ppu.vram[0..0x4000].iter().filter(|&&b| b != 0).count();
    eprintln!("\nBG1 chr region (0x0000-0x3FFF): {} non-zero bytes out of 16384", chr_nonzero);

    // Check tile 248 specifically
    let t248_start = 248 * 32;
    let t248_data = &bus2.ppu.vram[t248_start..t248_start+32];
    let t248_nz = t248_data.iter().filter(|&&b| b != 0).count();
    eprintln!("Tile 248 (0x{:04X}-0x{:04X}): {} non-zero bytes", t248_start, t248_start+31, t248_nz);

    eprintln!("\n=== Final state after {} frames ===", frame_count);
    eprintln!("TM={:02X} (BG2={})", bus.ppu.tm, bus.ppu.tm & 2 != 0);

    // Probe a row of pixels on BG1 to see what's there
    eprintln!("\n=== BG1 pixel probe at y=120 (every 8px) ===");
    for x in (0..256).step_by(8) {
        let info = bus.ppu.probe_bg_pixel(x, 120);
        eprintln!("  {}", info);
    }

    // Also dump BG1 and BG3 scroll and tilemap info
    let bg1 = &bus.ppu.bg[0];
    eprintln!("\nBG1: tilemap={:04X} chr={:04X} hscroll={} vscroll={} tile_size={} size={}",
        bg1.tilemap_addr, bg1.chr_addr, bg1.hscroll, bg1.vscroll, bg1.tile_size, bg1.tilemap_size);
    let bg3 = &bus.ppu.bg[2];
    eprintln!("BG3: tilemap={:04X} chr={:04X} hscroll={} vscroll={} tile_size={} size={}",
        bg3.tilemap_addr, bg3.chr_addr, bg3.hscroll, bg3.vscroll, bg3.tile_size, bg3.tilemap_size);

    // Check BG3 tile 0 — 2bpp = 16 bytes per tile
    let bg3_chr_byte = (bg3.chr_addr as usize) * 2; // word addr to byte addr
    let t0_data = &bus.ppu.vram[bg3_chr_byte..bg3_chr_byte+16];
    let t0_nonzero = t0_data.iter().filter(|&&b| b != 0).count();
    eprintln!("BG3 tile 0 (byte {:04X}): {} non-zero bytes {:02X?}", bg3_chr_byte, t0_nonzero, &t0_data[..8]);

    // Check BG3 tilemap — what are the first 32 entries?
    let bg3_tmap_byte = (bg3.tilemap_addr as usize) * 2;
    let mut bg3_tiles = String::new();
    let mut bg3_nonzero = 0usize;
    for i in 0..32usize {
        let lo = bus.ppu.vram[bg3_tmap_byte + i*2] as u16;
        let hi = bus.ppu.vram[bg3_tmap_byte + i*2 + 1] as u16;
        let tile = (lo | (hi << 8)) & 0x3FF;
        if tile != 0 { bg3_nonzero += 1; }
        if i < 16 { bg3_tiles.push_str(&format!("{:3X} ", tile)); }
    }
    eprintln!("BG3 tilemap row 0: {} (non-zero: {}/32)", bg3_tiles.trim(), bg3_nonzero);

    // Check BG3 chr tiles referenced by tilemap
    eprintln!("\n=== BG3 chr tiles (2bpp, 16 bytes each) ===");
    let bg3_chr_base = (bus.ppu.bg[2].chr_addr as usize) * 2;
    for tile_num in [0u16, 38, 55, 56, 57, 58, 59, 65, 66, 252] {
        let tile_off = bg3_chr_base + (tile_num as usize) * 16;
        let tile_data = &bus.ppu.vram[tile_off..tile_off+16];
        let nz = tile_data.iter().filter(|&&b| b != 0).count();
        eprintln!("  Tile {:3}: {} non-zero bytes ({})",
            tile_num, nz, if nz == 0 { "TRANSPARENT" } else { "opaque" });
    }

    // Count ALL non-zero BG3 tilemap entries across all 4 screens
    let mut bg3_total_nz = 0usize;
    for i in 0..4096usize {
        let off = bg3_tmap_byte + i * 2;
        if off + 1 >= bus.ppu.vram.len() { break; }
        let lo = bus.ppu.vram[off];
        let hi = bus.ppu.vram[off + 1];
        if lo != 0 || hi != 0 { bg3_total_nz += 1; }
    }
    eprintln!("BG3 tilemap total: {}/4096 non-zero entries", bg3_total_nz);

    // Dump CGRAM palettes used by BG1 tiles
    eprintln!("\n=== CGRAM palettes 4 and 5 (used by BG1 tiles) ===");
    for pal in 4..6u16 {
        let base = (pal * 16) as usize;
        let mut colors = String::new();
        for i in 0..16usize {
            let idx = (base + i) * 2;
            let lo = bus.ppu.cgram[idx];
            let hi = bus.ppu.cgram[idx + 1];
            let color = (lo as u16) | ((hi as u16) << 8);
            let r = color & 0x1F;
            let g = (color >> 5) & 0x1F;
            let b = (color >> 10) & 0x1F;
            if i > 0 { colors.push(' '); }
            colors.push_str(&format!("{:04X}", color));
        }
        eprintln!("  Pal {}: {}", pal, colors);
    }
    // Also dump palettes 0-7 summary (first non-zero entry in each)
    eprintln!("\n=== All 8 BG palettes (first non-zero color) ===");
    for pal in 0..8u16 {
        let base = (pal * 16) as usize;
        let mut non_zero_count = 0;
        let mut first_color = 0u16;
        for i in 0..16usize {
            let idx = (base + i) * 2;
            let color = (bus.ppu.cgram[idx] as u16) | ((bus.ppu.cgram[idx + 1] as u16) << 8);
            if color != 0 {
                non_zero_count += 1;
                if first_color == 0 { first_color = color; }
            }
        }
        eprintln!("  Pal {}: {}/16 non-zero, first={:04X}", pal, non_zero_count, first_color);
    }

    // Dump first 4 rows of BG1 tilemap entries (word addresses)
    let tmap_base = bus.ppu.bg[0].tilemap_addr as usize;
    eprintln!("\n=== BG1 tilemap dump (first 4 rows, word addr {:04X}) ===", tmap_base);
    for row in 0..4u16 {
        let mut tiles = String::new();
        for col in 0..32u16 {
            let word_addr = tmap_base + (row * 32 + col) as usize;
            let byte_addr = word_addr * 2;
            let lo = bus.ppu.vram[byte_addr] as u16;
            let hi = bus.ppu.vram[byte_addr + 1] as u16;
            let entry = lo | (hi << 8);
            let tile = entry & 0x03FF;
            if col > 0 { tiles.push(' '); }
            tiles.push_str(&format!("{:3X}", tile));
        }
        eprintln!("  row {:2}: {}", row, tiles);
    }

    // Also dump tilemap at the scroll position the probe reads from
    let scroll_tile_y = (120 + bus.ppu.bg[0].vscroll as usize) / 8;
    let screen_y = (scroll_tile_y >> 5) & 1;
    let map_y = scroll_tile_y & 0x1F;
    eprintln!("\n=== BG1 tilemap at scroll position (tile_y={} screen_y={} map_y={}) ===", scroll_tile_y, screen_y, map_y);
    let screen_offset = match bus.ppu.bg[0].tilemap_size {
        0 => 0,
        1 => 0,
        2 => screen_y * 0x400,
        3 => screen_y * 0x800,
        _ => 0,
    };
    let row_base = tmap_base + screen_offset + map_y * 32;
    let mut tiles = String::new();
    for col in 0..32usize {
        let word_addr = row_base + col;
        let byte_addr = word_addr * 2;
        if byte_addr + 1 < bus.ppu.vram.len() {
            let lo = bus.ppu.vram[byte_addr] as u16;
            let hi = bus.ppu.vram[byte_addr + 1] as u16;
            let entry = lo | (hi << 8);
            let tile = entry & 0x03FF;
            if col > 0 { tiles.push(' '); }
            tiles.push_str(&format!("{:3X}", tile));
        }
    }
    eprintln!("  row {:2}: {}", map_y, tiles);

    // Raw byte dump of first row of tilemap (show both low and high bytes)
    eprintln!("\n=== Raw bytes at BG1 tilemap word 0x2000 (first 16 entries) ===");
    for i in 0..16usize {
        let word_addr = tmap_base + i;
        let byte_addr = word_addr * 2;
        let lo = bus.ppu.vram[byte_addr];
        let hi = bus.ppu.vram[byte_addr + 1];
        eprintln!("  word {:04X} byte {:04X}: lo={:02X} hi={:02X} (entry={:04X} tile={})",
            word_addr, byte_addr, lo, hi, (lo as u16) | ((hi as u16) << 8),
            ((lo as u16) | ((hi as u16) << 8)) & 0x3FF);
    }

    // Also check Screen 2 where the real tiles are
    eprintln!("\n=== Raw bytes at Screen 2 row 7 (word 0x28E0, where probe reads) ===");
    for i in 0..32usize {
        let word_addr = 0x2800 + 7 * 32 + i;
        let byte_addr = word_addr * 2;
        let lo = bus.ppu.vram[byte_addr];
        let hi = bus.ppu.vram[byte_addr + 1];
        let entry = (lo as u16) | ((hi as u16) << 8);
        let tile = entry & 0x3FF;
        if tile != 248 {
            eprintln!("  word {:04X} byte {:04X}: lo={:02X} hi={:02X} tile={} pal={} <--- NON-248",
                word_addr, byte_addr, lo, hi, tile, (entry >> 10) & 7);
        }
    }

    // Check: scan ALL 4 screens for non-248 entries
    eprintln!("\n=== Scanning all 4 tilemap screens for non-248 tiles ===");
    for screen in 0..4usize {
        let screen_base = tmap_base + screen * 0x400;
        let mut non_248 = 0;
        let mut first_non_248 = None;
        for i in 0..1024usize {
            let word_addr = screen_base + i;
            let byte_addr = word_addr * 2;
            if byte_addr + 1 >= bus.ppu.vram.len() { break; }
            let lo = bus.ppu.vram[byte_addr] as u16;
            let hi = bus.ppu.vram[byte_addr + 1] as u16;
            let tile = (lo | (hi << 8)) & 0x3FF;
            if tile != 248 {
                non_248 += 1;
                if first_non_248.is_none() {
                    first_non_248 = Some((word_addr, tile));
                }
            }
        }
        let first_str = match first_non_248 {
            Some((addr, tile)) => format!(" first=word {:04X} tile={}", addr, tile),
            None => String::new(),
        };
        eprintln!("  Screen {} (word {:04X}-{:04X}): {}/1024 non-248{}",
            screen, screen_base, screen_base + 0x3FF, non_248, first_str);
    }

    // Count non-248 tiles in entire BG1 tilemap area (all 4 screens)
    let mut non_248_count = 0;
    let mut total_entries = 0;
    for i in 0..4096usize {
        let word_addr = tmap_base + i;
        let byte_addr = word_addr * 2;
        if byte_addr + 1 >= bus.ppu.vram.len() { break; }
        let lo = bus.ppu.vram[byte_addr] as u16;
        let hi = bus.ppu.vram[byte_addr + 1] as u16;
        let tile = (lo | (hi << 8)) & 0x03FF;
        total_entries += 1;
        if tile != 248 { non_248_count += 1; }
    }
    eprintln!("\nBG1 tilemap: {}/{} entries are non-tile-248", non_248_count, total_entries);

    // Dump Screen 2 row-by-row occupancy (where tiles actually are)
    eprintln!("\n=== Screen 2 row occupancy (word 0x2800) ===");
    let s2_base = tmap_base + 0x800;
    for row in 0..32u16 {
        let mut filled: Vec<u16> = Vec::new();
        for col in 0..32u16 {
            let word_addr = s2_base as u16 + row * 32 + col;
            let byte_addr = (word_addr as usize) * 2;
            let lo = bus.ppu.vram[byte_addr] as u16;
            let hi = bus.ppu.vram[byte_addr + 1] as u16;
            let tile = (lo | (hi << 8)) & 0x3FF;
            if tile != 248 { filled.push(col); }
        }
        if !filled.is_empty() {
            eprintln!("  row {:2}: {} tiles, cols {:?}", row, filled.len(),
                if filled.len() > 8 { format!("{}-{}", filled[0], filled[filled.len()-1]) }
                else { format!("{:?}", filled) });
        }
    }

    // Dump VRAM and WRAM for comparison
    {
        std::fs::write("reference/our_vram.bin", &*bus.ppu.vram).unwrap();
        std::fs::write("reference/our_wram.bin", &bus.wram[..]).unwrap();
        eprintln!("VRAM and WRAM saved");
    }

    // Dump framebuffers: with bgmode fix, BG1-only, BG3-only
    let saved_tm = bus.ppu.tm;
    let saved_bgmode = bus.ppu.bgmode;

    // Render with bg3hi forced OFF (Mode 1, no BG3 priority)
    bus.ppu.bgmode = 0x01;
    for scan in 1..=224u16 { bus.ppu.render_scanline(scan - 1); }
    let ppm_path = "debug_frame.ppm";
    let mut f = fs::File::create(ppm_path).unwrap();
    write!(f, "P6\n256 224\n255\n").unwrap();
    for pixel in bus.ppu.frame_buffer.iter() {
        let r = ((pixel >> 16) & 0xFF) as u8;
        let g = ((pixel >> 8) & 0xFF) as u8;
        let b = (pixel & 0xFF) as u8;
        f.write_all(&[r, g, b]).unwrap();
    }
    eprintln!("\nFramebuffer saved to {}", ppm_path);

    // BG1-only render
    bus.ppu.tm = 0x01; // BG1 only
    for scan in 1..=224u16 { bus.ppu.render_scanline(scan - 1); }
    let ppm_bg1 = "debug_frame_bg1.ppm";
    {
        let mut f = fs::File::create(ppm_bg1).unwrap();
        write!(f, "P6\n256 224\n255\n").unwrap();
        for pixel in bus.ppu.frame_buffer.iter() {
            let r = ((pixel >> 16) & 0xFF) as u8;
            let g = ((pixel >> 8) & 0xFF) as u8;
            let b = (pixel & 0xFF) as u8;
            f.write_all(&[r, g, b]).unwrap();
        }
    }
    eprintln!("BG1-only saved to {}", ppm_bg1);

    // BG3-only render
    bus.ppu.tm = 0x04; // BG3 only
    for scan in 1..=224u16 { bus.ppu.render_scanline(scan - 1); }
    let ppm_bg3 = "debug_frame_bg3.ppm";
    {
        let mut f = fs::File::create(ppm_bg3).unwrap();
        write!(f, "P6\n256 224\n255\n").unwrap();
        for pixel in bus.ppu.frame_buffer.iter() {
            let r = ((pixel >> 16) & 0xFF) as u8;
            let g = ((pixel >> 8) & 0xFF) as u8;
            let b = (pixel & 0xFF) as u8;
            f.write_all(&[r, g, b]).unwrap();
        }
    }
    eprintln!("BG3-only saved to {}", ppm_bg3);

    // Restore
    bus.ppu.tm = saved_tm;
    bus.ppu.bgmode = saved_bgmode;
}
