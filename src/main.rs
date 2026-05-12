/// Native entry point — for testing and debugging outside the browser.
/// Run with: cargo run -- rom/zelda3.smc [--trace] [--dump-frames DIR]

use std::path::Path;
use std::fs;

use zelda_a_link_to_the_past::bus::Bus;
use zelda_a_link_to_the_past::cpu::Cpu;
use zelda_a_link_to_the_past::joypad::*;
use zelda_a_link_to_the_past::rom::Cartridge;

/// Write the PPU framebuffer as a raw PPM image (simple, no deps).
fn dump_frame(fb: &[u32; 256 * 224], path: &str) {
    let mut data = format!("P6\n256 224\n255\n").into_bytes();
    for &px in fb.iter() {
        data.push(((px >> 16) & 0xFF) as u8); // R
        data.push(((px >> 8) & 0xFF) as u8);  // G
        data.push((px & 0xFF) as u8);         // B
    }
    fs::write(path, &data).expect("Failed to write frame");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).map(|s| s.as_str()).unwrap_or("rom/zelda3.smc");
    let trace = args.iter().any(|a| a == "--trace");
    let dump_dir = args.windows(2)
        .find(|w| w[0] == "--dump-frames")
        .map(|w| w[1].clone());

    if let Some(ref dir) = dump_dir {
        fs::create_dir_all(dir).expect("Failed to create dump dir");
        println!("Dumping frames to {dir}/");
    }

    let cart = Cartridge::load(Path::new(rom_path)).expect("Failed to load ROM");
    let mut bus = Bus::new(cart);
    let mut cpu = Cpu::new();
    cpu.trace = trace;
    cpu.reset(&mut bus);

    println!("Running CPU... (press Ctrl+C to stop)");

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

        // Dump every frame if requested
        if let Some(ref dir) = dump_dir {
            dump_frame(&bus.ppu.frame_buffer, &format!("{dir}/frame_{frame:04}.ppm"));
        }

        if frame % 60 == 0 {
            println!("Frame {frame} | PC={:02X}:{:04X}", cpu.pbr, cpu.pc);
        }
        // Phase 1 (0-1200): Start-mash through Nintendo logo + title
        // Phase 2 (~1200): File select appears — press Start to pick slot 1
        //   Since no valid SRAM, this creates a new game → name entry
        // Phase 3 (~2300): Name entry screen
        //   DON'T press A here (it types letters). Navigate to END first.
        //   Cursor starts at 'B' (row 0, col 0). END is row 3, ~col 5.
        // Phase 4: Mash A through cutscene text
        // Phase 5: Walk
        bus.joypad.current = match frame {
            // Mash Start through title
            f if f < 2200 && f % 30 < 4 => BTN_START,
            // Name screen: navigate to END (no A yet!)
            // Down x3 to bottom row
            2350..=2354 => BTN_DOWN,
            2370..=2374 => BTN_DOWN,
            2390..=2394 => BTN_DOWN,
            // Right x5 to reach END
            2410..=2414 => BTN_RIGHT,
            2430..=2434 => BTN_RIGHT,
            2440..=2444 => BTN_RIGHT,
            2450..=2454 => BTN_RIGHT,
            2460..=2464 => BTN_RIGHT,
            // Select END
            2480..=2484 => BTN_A,
            // Now mash A through cutscene (Zelda telepathy + Link house)
            f if f >= 2500 && f < 7000 && f % 10 < 3 => BTN_A,
            // Walk each direction
            7000..=7059 => BTN_DOWN,
            7100..=7159 => BTN_LEFT,
            7200..=7259 => BTN_UP,
            7300..=7359 => BTN_RIGHT,
            7400..=7459 => BTN_DOWN,
            _ => 0,
        };

        // At frame 2550, Link is visible in the cutscene — dump OAM + CGRAM + VRAM
        if frame == 2550 {
            // Dump CGRAM (sprite palettes)
            let cgram_path = dump_dir.as_ref().map(|d| format!("{d}/cgram.bin"))
                .unwrap_or_else(|| "/tmp/cgram.bin".to_string());
            fs::write(&cgram_path, &*bus.ppu.cgram).expect("Failed to write CGRAM");
            println!("Dumped CGRAM ({} bytes) to {cgram_path}", bus.ppu.cgram.len());

            // Dump OAM
            let oam_path = dump_dir.as_ref().map(|d| format!("{d}/oam.bin"))
                .unwrap_or_else(|| "/tmp/oam.bin".to_string());
            fs::write(&oam_path, &*bus.ppu.oam).expect("Failed to write OAM");
            println!("Dumped OAM ({} bytes) to {oam_path}", bus.ppu.oam.len());

            // Dump VRAM
            let vram_path = dump_dir.as_ref().map(|d| format!("{d}/vram.bin"))
                .unwrap_or_else(|| "/tmp/vram.bin".to_string());
            fs::write(&vram_path, &*bus.ppu.vram).expect("Failed to write VRAM");
            println!("Dumped VRAM ({} bytes) to {vram_path}", bus.ppu.vram.len());
            println!("PPU obj_base={:#06X} obj_name_select={:#06X} obj_size={}",
                bus.ppu.obj_base, bus.ppu.obj_name_select, bus.ppu.obj_size);
        }

        if frame >= 7500 {
            println!("Ran 7500 frames. Stopping.");
            break;
        }
    }
}
