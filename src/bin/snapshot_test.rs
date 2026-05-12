//! Determinism test for save states (native, no wasm-bindgen).
//!
//! Procedure:
//!   1. Boot ROM and run N warmup frames.
//!   2. Snapshot.
//!   3. Run one more frame, hash framebuffer and capture post-state.  → A
//!   4. Restore the snapshot.
//!   5. Run one more frame, hash framebuffer and capture post-state.  → B
//!   6. Assert A == B (otherwise the snapshot misses some state).

use std::env;
use std::path::Path;

use zelda_a_link_to_the_past::bus::Bus;
use zelda_a_link_to_the_past::cpu::Cpu;
use zelda_a_link_to_the_past::rom::Cartridge;
use zelda_a_link_to_the_past::snapshot::{snapshot_state, restore_state};

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Run one frame. Mirrors `Emulator::run_frame` in lib.rs so the native
/// test sees the same state evolution as the wasm runtime.
fn run_frame(cpu: &mut Cpu, bus: &mut Bus) {
    const MASTER_PER_SCANLINE: u64 = 1364;
    for scanline in 0..262u16 {
        if scanline == 225 {
            bus.vblank = true;
            bus.nmi_flag = true;
            if bus.nmitimen & 0x80 != 0 { cpu.nmi_pending = true; }
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
                2 | 3 => scanline == bus.vtime,
                _ => false,
            };
            if fire { bus.irq_flag = true; cpu.irq_pending = true; }
        }
        let target = cpu.cycles + MASTER_PER_SCANLINE;
        let hblank_start = cpu.cycles + MASTER_PER_SCANLINE - 68 * 4;
        bus.hblank = false;
        while cpu.cycles < target {
            if !bus.hblank && cpu.cycles >= hblank_start { bus.hblank = true; }
            bus.last_write_bank = cpu.pbr;
            bus.last_write_pc = cpu.pc;
            let elapsed = cpu.step(bus);
            cpu.cycles += elapsed;
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
    let _ = bus.apu.drain_samples();
}

fn fb_rgba(bus: &Bus) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(256 * 224 * 4);
    for &pixel in bus.ppu.frame_buffer.iter() {
        rgba.push(((pixel >> 16) & 0xFF) as u8);
        rgba.push(((pixel >> 8) & 0xFF) as u8);
        rgba.push((pixel & 0xFF) as u8);
        rgba.push(255);
    }
    rgba
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rom_path = args.get(1).cloned().unwrap_or_else(|| "rom/smw.smc".to_string());
    let warmup: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(120);

    println!("ROM: {} | warmup frames: {}", rom_path, warmup);

    let cart = Cartridge::load(Path::new(&rom_path)).expect("load ROM");
    let mut bus = Bus::new(cart);
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus);
    let mut frame_count: u64 = 0;

    for _ in 0..warmup {
        run_frame(&mut cpu, &mut bus);
        frame_count += 1;
    }

    let blob = snapshot_state(&cpu, &bus, frame_count);
    println!("snapshot size: {} bytes ({:.2} KB)", blob.len(), blob.len() as f64 / 1024.0);

    // Run +1 frame on path A.
    run_frame(&mut cpu, &mut bus);
    let mut fc_a = frame_count + 1;
    let hash_a = fnv1a64(&fb_rgba(&bus));
    let post_a = snapshot_state(&cpu, &bus, fc_a);

    // Restore and run +1 frame on path B.
    restore_state(&mut cpu, &mut bus, &mut fc_a, &blob).expect("restore");
    run_frame(&mut cpu, &mut bus);
    let hash_b = fnv1a64(&fb_rgba(&bus));
    let post_b = snapshot_state(&cpu, &bus, fc_a + 1);

    println!("FB hash A:        {:016x}", hash_a);
    println!("FB hash B:        {:016x}", hash_b);
    println!("FB match:         {}", hash_a == hash_b);
    println!("post-state match: {} ({} == {} bytes)",
             post_a == post_b, post_a.len(), post_b.len());

    if hash_a != hash_b {
        eprintln!("FAIL: framebuffer differs after restore.");
        std::process::exit(1);
    }
    if post_a != post_b {
        if post_a.len() == post_b.len() {
            for (i, (a, b)) in post_a.iter().zip(post_b.iter()).enumerate() {
                if a != b {
                    eprintln!("first diff at offset {}: {:#04x} vs {:#04x}", i, a, b);
                    break;
                }
            }
        }
        eprintln!("FAIL: post-frame state differs after restore.");
        std::process::exit(1);
    }
    println!("PASS: snapshot/restore is deterministic");
}
