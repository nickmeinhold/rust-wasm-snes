/// Native entry point — for testing and debugging outside the browser.
/// Run with: cargo run -- rom/zelda3.smc [--trace]

use std::path::Path;

use zelda_a_link_to_the_past::bus::Bus;
use zelda_a_link_to_the_past::cpu::Cpu;
use zelda_a_link_to_the_past::rom::Cartridge;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).map(|s| s.as_str()).unwrap_or("rom/zelda3.smc");
    let trace = args.iter().any(|a| a == "--trace");

    let cart = Cartridge::load(Path::new(rom_path)).expect("Failed to load ROM");
    let mut bus = Bus::new(cart);
    let mut cpu = Cpu::new();
    cpu.trace = trace;
    cpu.reset(&mut bus);

    println!("Running CPU... (press Ctrl+C to stop)");
    println!("Use --trace for instruction-level logging to stderr");

    // Run for a few frames to test boot sequence.
    let mut frame = 0u64;
    loop {
        for scanline in 0..262u16 {
            if scanline == 225 {
                bus.vblank = true;
                bus.nmi_flag = true;
                if bus.nmitimen & 0x80 != 0 {
                    cpu.nmi_pending = true;
                }
            }
            if scanline == 0 {
                bus.vblank = false;
                bus.nmi_flag = false;
            }

            bus.ppu.scanline = scanline;
            let target = cpu.cycles + 1364;
            while cpu.cycles < target {
                let elapsed = cpu.step(&mut bus);
                cpu.cycles += elapsed;
                if bus.pending_dma_cycles > 0 {
                    cpu.cycles += bus.pending_dma_cycles;
                    bus.pending_dma_cycles = 0;
                }
            }

            bus.apu.catch_up(1364);

            if scanline >= 1 && scanline <= 224 {
                bus.ppu.render_scanline(scanline - 1);
            }
        }

        frame += 1;
        if frame % 60 == 0 {
            println!("Frame {frame} | PC={:02X}:{:04X}", cpu.pbr, cpu.pc);
        }
        if frame >= 300 {
            println!("Ran 300 frames (~5 seconds). Stopping.");
            break;
        }
    }
}
