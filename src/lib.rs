/// SNES Emulator — Zelda: A Link to the Past
///
/// A web-based SNES emulator compiled to WASM. The emulation core runs
/// in Rust; the browser provides the display (Canvas), input (keyboard),
/// and timing (requestAnimationFrame).

pub mod apu;
pub mod bus;
pub mod cpu;
pub mod dma;
pub mod joypad;
pub mod ppu;
pub mod rom;
pub mod spc700;

use bus::Bus;
use cpu::Cpu;
use rom::Cartridge;
use wasm_bindgen::prelude::*;

const MASTER_CYCLES_PER_SCANLINE: u64 = 1364;
const SCANLINES_PER_FRAME: u16 = 262;
const VISIBLE_SCANLINES: u16 = 224;
const VBLANK_START: u16 = 225;

/// The emulator state, held across animation frames.
#[wasm_bindgen]
pub struct Emulator {
    cpu: Cpu,
    bus: Bus,
    frame_count: u64,
}

#[wasm_bindgen]
impl Emulator {
    /// Create a new emulator from ROM data.
    #[wasm_bindgen(constructor)]
    pub fn new(rom_data: &[u8]) -> Result<Emulator, JsValue> {
        console_error_panic_hook::set_once();

        // Detect and strip copier header.
        let rom = if rom_data.len() % 1024 == 512 {
            log("Detected 512-byte copier header, stripping...");
            rom_data[512..].to_vec()
        } else {
            rom_data.to_vec()
        };

        // Parse header (simplified — we know it's LoROM LTTP).
        let title = if rom.len() > 0x7FD4 {
            String::from_utf8_lossy(&rom[0x7FC0..0x7FD5]).trim().to_string()
        } else {
            "Unknown".to_string()
        };
        log(&format!("ROM loaded: \"{title}\" ({} KB)", rom.len() / 1024));

        let ram_size_code = if rom.len() > 0x7FD8 { rom[0x7FD8] } else { 0 };
        let ram_size = if ram_size_code == 0 { 0 } else { 1024usize << ram_size_code };

        let cart = Cartridge {
            rom,
            sram: vec![0u8; ram_size],
            title,
            map_mode: rom::MapMode::LoROM,
            rom_size: 0, // Not needed at runtime
            ram_size,
            country: 0,
            version: 0,
            checksum: 0,
            checksum_complement: 0,
        };

        let mut bus = Bus::new(cart);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);

        Ok(Emulator {
            cpu,
            bus,
            frame_count: 0,
        })
    }

    /// Run one complete frame (262 scanlines). Returns the framebuffer as
    /// RGBA bytes suitable for ImageData.
    pub fn run_frame(&mut self) -> Vec<u8> {
        for scanline in 0..SCANLINES_PER_FRAME {
            // VBlank start
            if scanline == VBLANK_START {
                self.bus.vblank = true;
                self.bus.nmi_flag = true;
                if self.bus.nmitimen & 0x80 != 0 {
                    self.cpu.nmi_pending = true;
                }
                // Auto-joypad read happens at VBlank start
                self.bus.auto_joypad_busy = false;
            }

            // VBlank end / new frame
            if scanline == 0 {
                self.bus.vblank = false;
                self.bus.nmi_flag = false;
                // Initialize HDMA channels at the start of each frame
                self.bus.hdma_init_frame();
            }

            // Tell the PPU which scanline we're on (for V-counter latching).
            self.bus.ppu.scanline = scanline;

            // V/H-count IRQ: fires once when position matches, cleared by $4211 read.
            let irq_mode = (self.bus.nmitimen >> 4) & 0x03;
            if irq_mode != 0 && !self.bus.irq_flag {
                let fire = match irq_mode {
                    1 => true, // H-count: fire once per scanline (H position approximated)
                    2 => scanline == self.bus.vtime, // V-count: fire when scanline matches
                    3 => scanline == self.bus.vtime, // V+H: fire when V matches (H approximated)
                    _ => false,
                };
                if fire {
                    self.bus.irq_flag = true;
                    self.cpu.irq_pending = true;
                }
            }

            // Run CPU and APU in lockstep. The APU must keep up with the CPU
            // so that port reads/writes see timely responses — otherwise the
            // boot handshake deadlocks (CPU polls for a response the APU
            // hasn't had cycles to produce yet).
            // HBlank occurs during the last ~68 master cycles of each scanline.
            let target = self.cpu.cycles + MASTER_CYCLES_PER_SCANLINE;
            let hblank_start = self.cpu.cycles + MASTER_CYCLES_PER_SCANLINE - 68 * 4;
            self.bus.hblank = false;
            while self.cpu.cycles < target {
                if !self.bus.hblank && self.cpu.cycles >= hblank_start {
                    self.bus.hblank = true;
                }
                let elapsed = self.cpu.step(&mut self.bus);
                self.cpu.cycles += elapsed;

                // Add any DMA cycles
                if self.bus.pending_dma_cycles > 0 {
                    let dma = self.bus.pending_dma_cycles;
                    self.cpu.cycles += dma;
                    self.bus.apu.catch_up(dma as u32);
                    self.bus.pending_dma_cycles = 0;
                }

                // Run APU for the same number of master cycles.
                self.bus.apu.catch_up(elapsed as u32);
            }

            // Render visible scanlines
            if scanline >= 1 && scanline <= VISIBLE_SCANLINES {
                // Run HDMA before rendering each scanline
                self.bus.hdma_run_scanline();
                self.bus.ppu.render_scanline(scanline - 1);
            }
        }

        self.frame_count += 1;

        // Convert framebuffer to RGBA bytes for Canvas ImageData.
        // Our framebuffer is ARGB u32, Canvas wants RGBA u8.
        let fb = &self.bus.ppu.frame_buffer;
        let mut rgba = Vec::with_capacity(256 * 224 * 4);
        for &pixel in fb.iter() {
            rgba.push(((pixel >> 16) & 0xFF) as u8); // R
            rgba.push(((pixel >> 8) & 0xFF) as u8);  // G
            rgba.push((pixel & 0xFF) as u8);          // B
            rgba.push(255);                            // A
        }
        rgba
    }

    /// Set a button state. `button` is a SNES button mask, `pressed` is the state.
    pub fn set_button(&mut self, button: u16, pressed: bool) {
        if pressed {
            self.bus.joypad.current |= button;
        } else {
            self.bus.joypad.current &= !button;
        }
    }

    /// Get the current frame count.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Dump rich emulator state to console — PPU, OAM, game state from WRAM.
    pub fn dump_full_state(&self) -> String {
        let ppu = &self.bus.ppu;
        let mode = ppu.bgmode & 0x07;
        let forced_blank = ppu.inidisp & 0x80 != 0;
        let brightness = ppu.inidisp & 0x0F;
        let tm = ppu.tm;

        // Count active sprites (non-zero Y position or visible on screen)
        let mut active_sprites = 0u32;
        let mut sprite_summary = String::new();
        for i in 0..128 {
            let y = ppu.oam[i * 4 + 1];
            let x_lo = ppu.oam[i * 4] as u16;
            let hi_byte = ppu.oam[512 + i / 4];
            let hi_shift = (i % 4) * 2;
            let x_hi = (hi_byte >> hi_shift) & 0x01;
            let x = (x_hi as u16) << 8 | x_lo;
            let tile = ppu.oam[i * 4 + 2];

            // Skip sprites that are off-screen (X=256+ and small, or Y=224+)
            if y < 224 && x < 256 && tile != 0 {
                active_sprites += 1;
                if active_sprites <= 5 {
                    let attr = ppu.oam[i * 4 + 3];
                    let pri = (attr >> 4) & 3;
                    let pal = (attr >> 1) & 7;
                    sprite_summary.push_str(&format!(
                        " [spr{}: ({},{}) tile={:02X} pri={} pal={}]",
                        i, x, y, tile, pri, pal
                    ));
                }
            }
        }

        // Read interesting game state from WRAM
        let game_mode = self.bus.wram[0x10];
        let sub_mode = self.bus.wram[0x11];
        let link_x = u16::from_le_bytes([self.bus.wram[0x22], self.bus.wram[0x23]]);
        let link_y = u16::from_le_bytes([self.bus.wram[0x20], self.bus.wram[0x21]]);

        // Check if framebuffer has any visible content
        let fb_nonblack = ppu.frame_buffer.iter().filter(|&&p| p != 0xFF000000).count();

        // APU state
        let apu = &self.bus.apu;
        let spc_pc = apu.cpu.pc;
        let spc_halted = apu.cpu.halted;
        let ports_to = &apu.bus.ports_to_main;
        let ports_from = &apu.bus.ports_from_main;
        let spc_cycles = apu.cycles;
        let sample_buf_len = apu.sample_buffer.len();
        // Count active DSP voices (env_phase != Off)
        let active_voices = (0..8u8).filter(|&i| {
            // Check KON register and if voice has been keyed
            apu.bus.dsp.regs[0x4C] & (1 << i) != 0 || apu.bus.dsp.regs[0x7C] & (1 << i) != 0
        }).count();

        format!(
            "PPU: mode={} blank={} bright={} TM={:02X} OBSEL_size={} | \
             BG1: map={:04X} chr={:04X} scr=({},{}) | \
             TM bits: BG1={} BG2={} BG3={} BG4={} OBJ={} | \
             Sprites: {} active{} | \
             Game: mode=${:02X} sub=${:02X} link=({},{}) | \
             OBJ base={:04X} namesel={:04X} | \
             FB: {} visible px | PC={:02X}:{:04X} | \
             APU: spc_pc={:04X} halted={} cycles={} samples={} voices={} \
             ports_out=[{:02X},{:02X},{:02X},{:02X}] ports_in=[{:02X},{:02X},{:02X},{:02X}]",
            mode, forced_blank, brightness, tm, ppu.obj_size,
            ppu.bg[0].tilemap_addr, ppu.bg[0].chr_addr,
            ppu.bg[0].hscroll & 0x3FF, ppu.bg[0].vscroll & 0x3FF,
            tm & 1, (tm >> 1) & 1, (tm >> 2) & 1, (tm >> 3) & 1, (tm >> 4) & 1,
            active_sprites, sprite_summary,
            game_mode, sub_mode, link_x, link_y,
            ppu.obj_base, ppu.obj_name_select,
            fb_nonblack,
            self.cpu.pbr, self.cpu.pc,
            spc_pc, spc_halted, spc_cycles, sample_buf_len, active_voices,
            ports_to[0], ports_to[1], ports_to[2], ports_to[3],
            ports_from[0], ports_from[1], ports_from[2], ports_from[3],
        )
    }

    /// Dump PPU state to console for debugging.
    pub fn dump_ppu_state(&self) -> String {
        let ppu = &self.bus.ppu;
        let mode = ppu.bgmode & 0x07;
        let forced_blank = ppu.inidisp & 0x80 != 0;
        let brightness = ppu.inidisp & 0x0F;
        let tm = ppu.tm;

        // Check if VRAM has any non-zero data
        let vram_nonzero = ppu.vram.iter().filter(|&&b| b != 0).count();
        let cgram_nonzero = ppu.cgram.iter().filter(|&&b| b != 0).count();

        // Check framebuffer for non-black pixels
        let fb_nonblack = ppu.frame_buffer.iter().filter(|&&p| p != 0xFF000000 && p != 0).count();

        format!(
            "PPU: mode={} blank={} bright={} TM={:02X} | BG1: map={:04X} chr={:04X} scroll=({},{}) | BG2: map={:04X} chr={:04X} | BG3: map={:04X} chr={:04X} | VRAM: {} nonzero | CGRAM: {} nonzero | FB: {} non-black pixels | PC={:02X}:{:04X}",
            mode, forced_blank, brightness, tm,
            ppu.bg[0].tilemap_addr, ppu.bg[0].chr_addr, ppu.bg[0].hscroll, ppu.bg[0].vscroll,
            ppu.bg[1].tilemap_addr, ppu.bg[1].chr_addr,
            ppu.bg[2].tilemap_addr, ppu.bg[2].chr_addr,
            vram_nonzero, cgram_nonzero, fb_nonblack,
            self.cpu.pbr, self.cpu.pc,
        )
    }

    /// Get audio samples generated during the last frame.
    /// Returns interleaved stereo i16 samples (L, R, L, R, ...) at 32 kHz.
    pub fn get_audio_samples(&mut self) -> Vec<i16> {
        self.bus.apu.drain_samples()
    }

    /// Dump DSP voice state for audio debugging.
    pub fn dump_dsp_voices(&self) -> String {
        self.bus.apu.bus.dsp.dump_voices()
    }

    /// Drain DSP debug log entries.
    pub fn drain_dsp_debug(&mut self) -> String {
        let log = self.bus.apu.bus.dsp.debug_log.join("\n");
        self.bus.apu.bus.dsp.debug_log.clear();
        log
    }

    /// Enable/disable CPU trace logging.
    pub fn set_trace(&mut self, enabled: bool) {
        self.cpu.trace = enabled;
    }
}

/// Log to the browser console.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}
