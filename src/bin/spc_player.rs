/// Standalone SPC player — loads .spc files and runs the SPC700+DSP in isolation.
///
/// Produces raw PCM output and diagnostic stats for A/B testing against
/// reference implementations. Bypasses the main CPU entirely.
///
/// Usage: cargo run --bin spc_player -- <file.spc> [--samples N] [--out file.raw]

use std::env;
use std::fs;
use std::io::Write;

use zelda_a_link_to_the_past::spc::SpcFile;
use zelda_a_link_to_the_past::spc700::Apu;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.spc> [--samples N] [--out file.raw]", args[0]);
        eprintln!("  --samples N   Number of stereo samples to generate (default: 32000 = 1 second)");
        eprintln!("  --out FILE    Write raw i16le stereo PCM to FILE");
        std::process::exit(1);
    }

    let spc_path = &args[1];
    let mut num_samples: u32 = 32000; // 1 second at 32 kHz
    let mut out_path: Option<String> = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--samples" => { i += 1; num_samples = args[i].parse().expect("invalid --samples"); }
            "--out" => { i += 1; out_path = Some(args[i].clone()); }
            _ => { eprintln!("Unknown arg: {}", args[i]); std::process::exit(1); }
        }
        i += 1;
    }

    // Parse SPC file.
    let data = fs::read(spc_path).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", spc_path, e);
        std::process::exit(1);
    });
    let spc = SpcFile::parse(&data).unwrap_or_else(|e| {
        eprintln!("Failed to parse SPC: {}", e);
        std::process::exit(1);
    });

    println!("SPC: \"{}\" from \"{}\"", spc.title, spc.game);
    println!("CPU: PC={:04X} A={:02X} X={:02X} Y={:02X} SP={:02X} PSW={:02X}",
             spc.pc, spc.a, spc.x, spc.y, spc.sp, spc.psw);

    // Print initial DSP state.
    let mvol_l = spc.dsp_regs[0x0C] as i8;
    let mvol_r = spc.dsp_regs[0x1C] as i8;
    let kon = spc.dsp_regs[0x4C];
    let koff = spc.dsp_regs[0x5C];
    let flg = spc.dsp_regs[0x6C];
    println!("DSP: MVOL=({},{}) KON={:02X} KOFF={:02X} FLG={:02X} mute={}",
             mvol_l, mvol_r, kon, koff, flg, flg & 0x40 != 0);

    // Print timer/IO state from RAM.
    let control = spc.ram[0xF1];
    println!("IO:  CONTROL={:02X} timers={}{}{} ROM={}",
             control,
             if control & 1 != 0 { "T0 " } else { "" },
             if control & 2 != 0 { "T1 " } else { "" },
             if control & 4 != 0 { "T2 " } else { "" },
             if control & 0x80 != 0 { "on" } else { "off" });
    println!("     T0_target={:02X} T1_target={:02X} T2_target={:02X}",
             spc.ram[0xFA], spc.ram[0xFB], spc.ram[0xFC]);
    println!("     DSP_addr={:02X} ports=[{:02X},{:02X},{:02X},{:02X}]",
             spc.ram[0xF2],
             spc.ram[0xF4], spc.ram[0xF5], spc.ram[0xF6], spc.ram[0xF7]);

    // Load into APU and run.
    let mut apu = Apu::new();
    apu.load_spc(&spc);

    // Run for the requested number of samples.
    // Each stereo sample = 32 SPC700 cycles.
    let spc_cycles = num_samples * 32;
    println!("Running {} SPC cycles ({} stereo samples = {:.2}s)...",
             spc_cycles, num_samples, num_samples as f64 / 32000.0);

    // Clear the debug log that load_spc populated.
    apu.bus.dsp.debug_log.clear();

    // Run in chunks and sample PC to detect tight loops.
    let chunk = spc_cycles / 4;
    for c in 0..4 {
        apu.run_cycles(chunk);
        let endx = apu.bus.dsp.regs[0x7C];
        eprintln!("  chunk {}: PC={:04X} halted={} KON_nz={} ENDX={:02X}",
                  c, apu.cpu.pc, apu.cpu.halted,
                  apu.bus.dsp.kon_nonzero_count, endx);
    }

    // Post-run diagnostics.
    println!("Post: SPC_PC={:04X} halted={} cycles={}", apu.cpu.pc, apu.cpu.halted, apu.cycles);
    println!("      timers_en=[{},{},{}] T0: ctr={} fires={} reads={}",
             apu.bus.timers[0].enabled, apu.bus.timers[1].enabled, apu.bus.timers[2].enabled,
             apu.bus.timers[0].counter, apu.bus.timers[0].fire_count, apu.bus.timers[0].read_count);
    println!("      KON_reg={:02X} KOFF_reg={:02X} KON_writes={} (non-zero={})",
             apu.bus.dsp.regs[0x4C], apu.bus.dsp.regs[0x5C],
             apu.bus.dsp.kon_write_count, apu.bus.dsp.kon_nonzero_count);

    // Dump driver voice state after execution.
    {
        let ram = &apu.bus.ram;
        println!("\nVoice state post-run:");
        for v in 0..8 {
            let addr = 0x31 + v * 2;
            print!("  V{}=$31+{}=${:02X}", v, v*2, ram[addr]);
        }
        println!();
        println!("  $02={:02X} $06={:02X} $0C={:02X} $1D={:02X} $47={:02X}",
                 ram[0x02], ram[0x06], ram[0x0C], ram[0x1D], ram[0x47]);
    }

    // Print DSP write trace.
    if !apu.bus.dsp.debug_log.is_empty() {
        println!("\n── First {} DSP writes (after load) ──", apu.bus.dsp.debug_log.len());
        for line in &apu.bus.dsp.debug_log {
            println!("  {}", line);
        }
    }

    let samples = apu.drain_samples();

    // Analyze output.
    let total = samples.len();
    let non_zero = samples.iter().filter(|&&s| s != 0).count();
    let peak = samples.iter().map(|&s| (s as i32).abs()).max().unwrap_or(0);
    let rms = if total > 0 {
        let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
        (sum_sq / total as f64).sqrt()
    } else {
        0.0
    };

    println!("\n── Results ──────────────────────────────────");
    println!("Total samples: {} ({} stereo pairs)", total, total / 2);
    println!("Non-zero:      {} ({:.1}%)", non_zero, 100.0 * non_zero as f64 / total.max(1) as f64);
    println!("Peak amplitude: {} ({:.1}% of max)", peak, 100.0 * peak as f64 / 32767.0);
    println!("RMS:            {:.1}", rms);

    if non_zero == 0 {
        println!("\n*** ALL ZEROS — audio pipeline is producing silence ***");
    } else if peak < 100 {
        println!("\n*** Very low amplitude — audio may be barely audible ***");
    } else {
        println!("\n*** Audio output detected! ***");
    }

    // Write raw PCM if requested.
    if let Some(path) = out_path {
        let mut f = fs::File::create(&path).expect("Failed to create output file");
        for &s in &samples {
            f.write_all(&s.to_le_bytes()).expect("Write failed");
        }
        println!("Wrote {} bytes to {}", total * 2, path);
        println!("Play with: ffplay -f s16le -ar 32000 -ac 2 {}", path);
    }
}
