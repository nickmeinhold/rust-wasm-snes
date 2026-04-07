/// SPC700 APU (Audio Processing Unit) emulation.
///
/// The SNES APU is an independent subsystem: an SPC700 CPU running at ~1.024 MHz
/// with 64KB RAM, a DSP for audio synthesis, and 3 timers. It communicates with
/// the main 65816 CPU only through 4 bidirectional I/O ports ($2140-$2143).
///
/// On reset, the IPL ROM at $FFC0-$FFFF runs a boot loader that accepts program
/// uploads from the main CPU. The game uploads its music driver, which then takes
/// over and communicates via the ports for play/stop/tempo commands.

pub mod cpu;
pub mod dsp;
pub mod timers;

use cpu::Spc700;
use dsp::Dsp;
use timers::Timer;

/// SPC700 IPL boot ROM — 64 bytes mapped at $FFC0-$FFFF.
///
/// This program initializes the stack, signals readiness ($AA/$BB on ports 0-1),
/// waits for the main CPU to send $CC, then enters a data transfer loop to
/// receive and store the game's music driver code into APU RAM.
const IPL_ROM: [u8; 64] = [
    0xCD, 0xEF, 0xBD, 0xE8, 0x00, 0xC6, 0x1D, 0xD0,
    0xFC, 0x8F, 0xAA, 0xF4, 0x8F, 0xBB, 0xF5, 0x78,
    0xCC, 0xF4, 0xD0, 0xFB, 0x2F, 0x19, 0xEB, 0xF4,
    0xD0, 0xFC, 0x7E, 0xF4, 0xD0, 0x0B, 0xE4, 0xF5,
    0xCB, 0xF4, 0xD7, 0x00, 0xFC, 0xD0, 0xF3, 0xAB,
    0x01, 0x10, 0xEF, 0x7E, 0xF4, 0x10, 0xEB, 0xBA,
    0xF6, 0xDA, 0x00, 0xBA, 0xF4, 0xC4, 0xF4, 0xDD,
    0x5D, 0xD0, 0xDB, 0x1F, 0x00, 0x00, 0xC0, 0xFF,
];

/// The APU bus: SPC700-visible memory (RAM + I/O + DSP + timers).
///
/// Separated from the CPU to satisfy Rust's borrow checker — the CPU
/// borrows `&mut ApuBus` during `step()` while being a sibling field.
pub struct ApuBus {
    /// 64KB APU RAM.
    pub ram: Box<[u8; 65536]>,
    /// S-DSP (Digital Signal Processor).
    pub dsp: Dsp,
    /// Three timers: T0, T1 (8 kHz), T2 (64 kHz).
    pub timers: [Timer; 3],
    /// Ports written by main CPU ($2140-$2143 → SPC reads $F4-$F7).
    pub ports_from_main: [u8; 4],
    /// Ports written by SPC ($F4-$F7 → main CPU reads $2140-$2143).
    pub ports_to_main: [u8; 4],
    /// Whether IPL ROM is mapped at $FFC0-$FFFF.
    pub rom_enabled: bool,
}

impl ApuBus {
    pub fn new() -> Self {
        Self {
            ram: Box::new([0u8; 65536]),
            dsp: Dsp::new(),
            timers: [Timer::new(256), Timer::new(256), Timer::new(32)],
            ports_from_main: [0; 4],
            ports_to_main: [0xAA, 0xBB, 0, 0],
            rom_enabled: true,
        }
    }

    /// Read a byte from the SPC700 address space.
    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x00F0 => 0,
            0x00F1 => 0,
            0x00F2 => self.dsp.addr_reg,
            0x00F3 => self.dsp.read(self.dsp.addr_reg),
            0x00F4..=0x00F7 => self.ports_from_main[(addr - 0xF4) as usize],
            0x00F8..=0x00F9 => self.ram[addr as usize],
            0x00FA..=0x00FC => 0, // Timer targets are write-only
            0x00FD => self.timers[0].read_counter(),
            0x00FE => self.timers[1].read_counter(),
            0x00FF => self.timers[2].read_counter(),
            0xFFC0..=0xFFFF if self.rom_enabled => IPL_ROM[(addr - 0xFFC0) as usize],
            _ => self.ram[addr as usize],
        }
    }

    /// Write a byte to the SPC700 address space.
    pub fn write(&mut self, addr: u16, val: u8) {
        // Always write to RAM (even for I/O addresses — hardware does this).
        self.ram[addr as usize] = val;

        match addr {
            0x00F0 => {} // TEST register (ignore)
            0x00F1 => { // CONTROL
                self.rom_enabled = val & 0x80 != 0;
                self.timers[0].enabled = val & 0x01 != 0;
                self.timers[1].enabled = val & 0x02 != 0;
                self.timers[2].enabled = val & 0x04 != 0;
                if val & 0x10 != 0 {
                    self.ports_from_main[0] = 0;
                    self.ports_from_main[1] = 0;
                }
                if val & 0x20 != 0 {
                    self.ports_from_main[2] = 0;
                    self.ports_from_main[3] = 0;
                }
            }
            0x00F2 => self.dsp.addr_reg = val,
            0x00F3 => self.dsp.write(self.dsp.addr_reg, val),
            0x00F4..=0x00F7 => {
                self.ports_to_main[(addr - 0xF4) as usize] = val;
            }
            0x00FA => self.timers[0].target = if val == 0 { 256 } else { val as u16 },
            0x00FB => self.timers[1].target = if val == 0 { 256 } else { val as u16 },
            0x00FC => self.timers[2].target = if val == 0 { 256 } else { val as u16 },
            _ => {}
        }
    }
}

/// Complete APU: SPC700 CPU + bus (RAM/DSP/timers/ports).
pub struct Apu {
    pub cpu: Spc700,
    pub bus: ApuBus,
    /// SPC700 cycle counter (for synchronization with main CPU).
    pub cycles: u64,
    /// Fractional cycle accumulator for precise main→SPC timing.
    pub cycle_frac: u32,
    /// DSP sample counter (one stereo sample per 32 SPC cycles).
    dsp_counter: u32,
    /// Audio output buffer (interleaved stereo i16: L, R, L, R, ...).
    pub sample_buffer: Vec<i16>,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            cpu: Spc700::new(),
            bus: ApuBus::new(),
            cycles: 0,
            cycle_frac: 0,
            dsp_counter: 0,
            sample_buffer: Vec::with_capacity(2048),
        }
    }

    /// Load state from a parsed SPC file for standalone playback.
    pub fn load_spc(&mut self, spc: &crate::spc::SpcFile) {
        // Restore CPU registers.
        self.cpu.pc = spc.pc;
        self.cpu.a = spc.a;
        self.cpu.x = spc.x;
        self.cpu.y = spc.y;
        self.cpu.psw = spc.psw;
        self.cpu.sp = spc.sp;
        self.cpu.halted = false;

        // Load 64KB RAM.
        self.bus.ram.copy_from_slice(&*spc.ram);

        // Restore DSP registers (write through the DSP's write() method
        // so that voice state, echo length, etc. are properly initialized).
        // Skip ENDX (0x7C) — it's read-only on real hardware; writing clears it.
        for i in 0..128u8 {
            if i == 0x7C { continue; }
            self.bus.dsp.write(i, spc.dsp_regs[i as usize]);
        }

        // Replay I/O register state from RAM so timers and ports initialize.
        // The SPC file stores the last-written values at $F0-$FF in the RAM dump,
        // but raw copy_from_slice doesn't trigger the I/O side effects.
        let control = spc.ram[0xF1];
        self.bus.rom_enabled = control & 0x80 != 0;
        self.bus.timers[0].enabled = control & 0x01 != 0;
        self.bus.timers[1].enabled = control & 0x02 != 0;
        self.bus.timers[2].enabled = control & 0x04 != 0;

        // Timer targets ($FA-$FC).
        let t0 = spc.ram[0xFA];
        let t1 = spc.ram[0xFB];
        let t2 = spc.ram[0xFC];
        self.bus.timers[0].target = if t0 == 0 { 256 } else { t0 as u16 };
        self.bus.timers[1].target = if t1 == 0 { 256 } else { t1 as u16 };
        self.bus.timers[2].target = if t2 == 0 { 256 } else { t2 as u16 };

        // DSP address register.
        self.bus.dsp.addr_reg = spc.ram[0xF2];

        // Reset timing.
        self.cycles = 0;
        self.cycle_frac = 0;
        self.dsp_counter = 0;
        self.sample_buffer.clear();
    }

    /// Run the APU for the given number of SPC700 cycles.
    pub fn run_cycles(&mut self, target_cycles: u32) {
        let end_cycle = self.cycles + target_cycles as u64;
        while self.cycles < end_cycle {
            // Execute one SPC700 instruction (each takes multiple cycles).
            let inst_cycles = if !self.cpu.halted {
                self.cpu.step(&mut self.bus) as u64
            } else {
                1 // Advance time even when halted
            };

            // Tick timers and DSP for each cycle consumed by this instruction.
            for _ in 0..inst_cycles {
                let c = self.cycles;
                if c % 128 == 0 {
                    self.bus.timers[0].tick();
                    self.bus.timers[1].tick();
                }
                if c % 16 == 0 {
                    self.bus.timers[2].tick();
                }

                self.dsp_counter += 1;
                if self.dsp_counter >= 32 {
                    self.dsp_counter = 0;
                    let (left, right) = self.bus.dsp.generate_sample(&mut self.bus.ram);
                    self.sample_buffer.push(left);
                    self.sample_buffer.push(right);
                }

                self.cycles += 1;
            }
        }
    }

    /// Run APU for the equivalent of `master_cycles` main CPU master clocks.
    /// Uses a fractional accumulator for precise timing (main clock / 21 ≈ SPC clock).
    pub fn catch_up(&mut self, master_cycles: u32) {
        // SPC700 clock = master clock / 21 (approximately).
        // Use fixed-point: accumulate master cycles, divide by 21.
        self.cycle_frac += master_cycles;
        let spc_cycles = self.cycle_frac / 21;
        self.cycle_frac %= 21;
        if spc_cycles > 0 {
            self.run_cycles(spc_cycles);
        }
    }

    /// Main CPU reads from $2140-$2143.
    pub fn cpu_read(&self, port: u8) -> u8 {
        self.bus.ports_to_main[port as usize & 3]
    }

    /// Main CPU writes to $2140-$2143.
    pub fn cpu_write(&mut self, port: u8, val: u8) {
        self.bus.ports_from_main[port as usize & 3] = val;
    }

    /// Drain the audio sample buffer, returning all accumulated samples.
    pub fn drain_samples(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.sample_buffer)
    }
}
