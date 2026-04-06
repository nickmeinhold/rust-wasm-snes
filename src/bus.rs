/// SNES memory bus — address decoding and hardware register dispatch.
///
/// Every CPU read/write and DMA transfer flows through this module.
/// It decodes the 24-bit address (bank:addr) and routes it to the
/// appropriate component: ROM, WRAM, PPU, APU, DMA, or CPU registers.

use crate::spc700::Apu;
use crate::dma::{self, Dma};
use crate::joypad::Joypad;
use crate::ppu::Ppu;
use crate::rom::Cartridge;

pub struct Bus {
    pub cart: Cartridge,
    pub wram: Box<[u8; 0x20000]>,  // 128KB work RAM
    pub ppu: Ppu,
    pub apu: Apu,
    pub dma: Dma,
    pub joypad: Joypad,

    // ── CPU internal registers ────────────────────────────
    pub nmitimen: u8,    // $4200 — NMI/IRQ enable
    pub htime: u16,      // $4207-$4208
    pub vtime: u16,      // $4209-$420A
    pub hdmaen: u8,      // $420C — HDMA channel enable
    pub memsel: u8,      // $420D — FastROM select

    // ── Math hardware ─────────────────────────────────────
    pub wrmpya: u8,      // $4202
    pub wrmpyb: u8,      // $4203
    pub wrdiv: u16,      // $4204-$4205
    pub wrdivb: u8,      // $4206
    pub rddiv: u16,      // $4214-$4215 (division result)
    pub rdmpy: u16,      // $4216-$4217 (multiplication result)

    // ── WRAM data port ────────────────────────────────────
    pub wram_addr: u32,  // $2181-$2183 (17-bit)

    // ── Timing/status ─────────────────────────────────────
    pub vblank: bool,
    pub hblank: bool,
    pub nmi_flag: bool,  // Set on VBlank, cleared on $4210 read
    pub irq_flag: bool,  // Set on V/H-count match, cleared on $4211 read
    pub auto_joypad_busy: bool,

    pub open_bus: u8,

    /// Pending DMA cycles to add to the CPU cycle count.
    pub pending_dma_cycles: u64,
}

impl Bus {
    pub fn new(cart: Cartridge) -> Self {
        Self {
            cart,
            wram: Box::new([0u8; 0x20000]),
            ppu: Ppu::new(),
            apu: Apu::new(),
            dma: Dma::new(),
            joypad: Joypad::new(),

            nmitimen: 0,
            htime: 0x1FF,
            vtime: 0x1FF,
            hdmaen: 0,
            memsel: 0,

            wrmpya: 0xFF,
            wrmpyb: 0,
            wrdiv: 0xFFFF,
            wrdivb: 0,
            rddiv: 0,
            rdmpy: 0,

            wram_addr: 0,

            vblank: false,
            hblank: false,
            nmi_flag: false,
            irq_flag: false,
            auto_joypad_busy: false,

            open_bus: 0,
            pending_dma_cycles: 0,
        }
    }

    /// Read a byte from the bus. This is the hot path for all CPU reads.
    /// Takes `&mut self` because some register reads have side effects
    /// (flipflops, counters, flag clears).
    pub fn read(&mut self, bank: u8, addr: u16) -> u8 {
        let eb = bank & 0x7F; // Mirror $80-$FF → $00-$7F

        match (eb, addr) {
            // Full WRAM access ($7E-$7F)
            (0x7E, _) => self.wram[addr as usize],
            (0x7F, _) => self.wram[0x10000 + addr as usize],

            // System area banks $00-$3F (and mirrors $80-$BF)
            (0x00..=0x3F, 0x0000..=0x1FFF) => self.wram[addr as usize],
            (0x00..=0x3F, 0x2100..=0x213F) => {
                if addr >= 0x2134 {
                    self.ppu.read_register(addr)
                } else {
                    self.open_bus
                }
            }
            (0x00..=0x3F, 0x2140..=0x217F) => self.apu.cpu_read((addr & 3) as u8),
            (0x00..=0x3F, 0x2180) => { // WMDATA — read from WRAM at wram_addr
                let val = self.wram[self.wram_addr as usize & 0x1FFFF];
                self.wram_addr = (self.wram_addr + 1) & 0x1FFFF;
                val
            }
            (0x00..=0x3F, 0x4016) => 0, // Joypad serial (old-style, unused by LTTP)
            (0x00..=0x3F, 0x4017) => 0,
            (0x00..=0x3F, 0x4200..=0x42FF) => self.read_cpu_register(addr),
            (0x00..=0x3F, 0x4300..=0x437F) => self.dma.read(addr),
            (0x00..=0x3F, 0x8000..=0xFFFF) => self.cart.read(bank, addr),

            // ROM banks $40-$6F
            (0x40..=0x6F, 0x8000..=0xFFFF) => self.cart.read(bank, addr),

            // SRAM banks $70-$7D
            (0x70..=0x7D, 0x0000..=0x7FFF) => {
                let offset = ((eb - 0x70) as usize) * 0x8000 + addr as usize;
                if offset < self.cart.sram.len() {
                    self.cart.sram[offset]
                } else {
                    self.open_bus
                }
            }
            (0x70..=0x7D, 0x8000..=0xFFFF) => self.cart.read(bank, addr),

            _ => self.open_bus,
        }
    }

    /// Write a byte to the bus.
    pub fn write(&mut self, bank: u8, addr: u16, val: u8) {
        let eb = bank & 0x7F;

        match (eb, addr) {
            (0x7E, _) => { self.wram[addr as usize] = val; }
            (0x7F, _) => { self.wram[0x10000 + addr as usize] = val; }

            (0x00..=0x3F, 0x0000..=0x1FFF) => { self.wram[addr as usize] = val; }
            (0x00..=0x3F, 0x2100..=0x213F) => { self.ppu.write_register(addr, val); }
            (0x00..=0x3F, 0x2140..=0x217F) => { self.apu.cpu_write((addr & 3) as u8, val); }
            (0x00..=0x3F, 0x2180) => { // WMDATA
                self.wram[self.wram_addr as usize & 0x1FFFF] = val;
                self.wram_addr = (self.wram_addr + 1) & 0x1FFFF;
            }
            (0x00..=0x3F, 0x2181) => { // WMADDL
                self.wram_addr = (self.wram_addr & 0x1FF00) | val as u32;
            }
            (0x00..=0x3F, 0x2182) => { // WMADDM
                self.wram_addr = (self.wram_addr & 0x100FF) | ((val as u32) << 8);
            }
            (0x00..=0x3F, 0x2183) => { // WMADDH
                self.wram_addr = (self.wram_addr & 0x0FFFF) | (((val & 0x01) as u32) << 16);
            }
            (0x00..=0x3F, 0x4200..=0x42FF) => { self.write_cpu_register(addr, val); }
            (0x00..=0x3F, 0x4300..=0x437F) => { self.dma.write(addr, val); }
            (0x00..=0x3F, 0x8000..=0xFFFF) => {} // ROM — writes ignored

            (0x40..=0x6F, 0x8000..=0xFFFF) => {} // ROM

            (0x70..=0x7D, 0x0000..=0x7FFF) => { // SRAM
                let offset = ((eb - 0x70) as usize) * 0x8000 + addr as usize;
                if offset < self.cart.sram.len() {
                    self.cart.sram[offset] = val;
                }
            }
            (0x70..=0x7D, 0x8000..=0xFFFF) => {} // ROM

            _ => {}
        }
    }

    /// Read CPU internal registers ($4200-$42FF).
    fn read_cpu_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4210 => { // RDNMI
                let val = if self.nmi_flag { 0x80 } else { 0x00 } | 0x02; // CPU version
                self.nmi_flag = false;
                val
            }
            0x4211 => { // TIMEUP — IRQ flag (read-clear)
                let val = if self.irq_flag { 0x80 } else { 0x00 };
                self.irq_flag = false;
                val
            }
            0x4212 => { // HVBJOY — VBlank/HBlank/auto-joypad status
                let mut val = 0u8;
                if self.vblank { val |= 0x80; }
                if self.hblank { val |= 0x40; }
                if self.auto_joypad_busy { val |= 0x01; }
                val
            }
            0x4214 => self.rddiv as u8,          // RDDIVL
            0x4215 => (self.rddiv >> 8) as u8,    // RDDIVH
            0x4216 => self.rdmpy as u8,           // RDMPYL
            0x4217 => (self.rdmpy >> 8) as u8,    // RDMPYH
            0x4218 => self.joypad.current as u8,   // JOY1L
            0x4219 => (self.joypad.current >> 8) as u8, // JOY1H
            0x421A..=0x421F => 0, // JOY2-4 (unused)
            _ => self.open_bus,
        }
    }

    /// Write CPU internal registers ($4200-$42FF).
    fn write_cpu_register(&mut self, addr: u16, val: u8) {
        match addr {
            0x4200 => { self.nmitimen = val; }
            0x4201 => {} // WRIO — programmable I/O port (ignore)
            0x4202 => { self.wrmpya = val; }
            0x4203 => { // WRMPYB — writing this triggers multiplication
                self.wrmpyb = val;
                self.rdmpy = self.wrmpya as u16 * val as u16;
            }
            0x4204 => { self.wrdiv = (self.wrdiv & 0xFF00) | val as u16; }
            0x4205 => { self.wrdiv = (self.wrdiv & 0x00FF) | ((val as u16) << 8); }
            0x4206 => { // WRDIVB — writing this triggers division
                self.wrdivb = val;
                if val != 0 {
                    self.rddiv = self.wrdiv / val as u16;
                    self.rdmpy = self.wrdiv % val as u16;
                } else {
                    self.rddiv = 0xFFFF;
                    self.rdmpy = self.wrdiv;
                }
            }
            0x4207 => { self.htime = (self.htime & 0x100) | val as u16; }
            0x4208 => { self.htime = (self.htime & 0x0FF) | (((val & 0x01) as u16) << 8); }
            0x4209 => { self.vtime = (self.vtime & 0x100) | val as u16; }
            0x420A => { self.vtime = (self.vtime & 0x0FF) | (((val & 0x01) as u16) << 8); }
            0x420B => { // MDMAEN — trigger general DMA
                self.execute_general_dma(val);
            }
            0x420C => { self.hdmaen = val; }
            0x420D => { self.memsel = val; }
            _ => {}
        }
    }

    /// Execute general DMA for all enabled channels.
    /// Inlined here to avoid borrow-checker issues with closures over `self`.
    fn execute_general_dma(&mut self, enable_mask: u8) {
        use crate::dma::DMA_TRANSFER_PATTERNS;

        let mut total_cycles: u64 = 0;

        for ch_idx in 0..8u8 {
            if enable_mask & (1 << ch_idx) == 0 { continue; }

            let mode = (self.dma.channels[ch_idx as usize].control & 0x07) as usize;
            let direction = self.dma.channels[ch_idx as usize].control & 0x80 != 0;
            let fixed_a = self.dma.channels[ch_idx as usize].control & 0x08 != 0;
            let decrement_a = self.dma.channels[ch_idx as usize].control & 0x10 != 0;
            let dest_base = 0x2100u16 + self.dma.channels[ch_idx as usize].dest as u16;
            let transfer_size = dma::DMA_TRANSFER_SIZES[mode];
            let pattern = DMA_TRANSFER_PATTERNS[mode];

            let mut remaining = if self.dma.channels[ch_idx as usize].size == 0 {
                0x10000u32
            } else {
                self.dma.channels[ch_idx as usize].size as u32
            };
            let mut unit_idx: u8 = 0;

            while remaining > 0 {
                let b_addr = dest_base + pattern[unit_idx as usize] as u16;
                let a_bank = self.dma.channels[ch_idx as usize].src_bank;
                let a_addr = self.dma.channels[ch_idx as usize].src_addr;

                if direction {
                    // B → A: read from PPU registers, write to WRAM/ROM
                    let val = self.read(0x00, b_addr); // B-bus is bank 0
                    self.write(a_bank, a_addr, val);
                } else {
                    // A → B: read from ROM/WRAM, write to PPU registers
                    let val = self.read(a_bank, a_addr);
                    // B-bus write goes directly to the target register
                    match b_addr {
                        0x2100..=0x213F => self.ppu.write_register(b_addr, val),
                        0x2140..=0x217F => self.apu.cpu_write((b_addr & 3) as u8, val),
                        0x2180 => {
                            self.wram[self.wram_addr as usize & 0x1FFFF] = val;
                            self.wram_addr = (self.wram_addr + 1) & 0x1FFFF;
                        }
                        _ => {}
                    }
                }

                if !fixed_a {
                    if decrement_a {
                        self.dma.channels[ch_idx as usize].src_addr =
                            self.dma.channels[ch_idx as usize].src_addr.wrapping_sub(1);
                    } else {
                        self.dma.channels[ch_idx as usize].src_addr =
                            self.dma.channels[ch_idx as usize].src_addr.wrapping_add(1);
                    }
                }

                unit_idx = (unit_idx + 1) % transfer_size;
                remaining -= 1;
                total_cycles += 8;
            }

            self.dma.channels[ch_idx as usize].size = 0;
        }

        self.pending_dma_cycles += total_cycles;
    }

    /// Initialize HDMA channels at the start of each frame (scanline 0).
    /// Reads the first table entry for each enabled HDMA channel.
    pub fn hdma_init_frame(&mut self) {
        if self.hdmaen == 0 { return; }

        for ch in 0..8u8 {
            if self.hdmaen & (1 << ch) == 0 {
                self.dma.channels[ch as usize].hdma_terminated = true;
                continue;
            }

            let c = &mut self.dma.channels[ch as usize];
            // Initialize table pointer from source address
            c.hdma_addr = c.src_addr;
            c.hdma_terminated = false;
            c.hdma_do_transfer = true;
        }

        // Load first table entry for each channel (needs bus access)
        for ch in 0..8u8 {
            if self.hdmaen & (1 << ch) == 0 { continue; }
            if self.dma.channels[ch as usize].hdma_terminated { continue; }
            self.hdma_load_entry(ch);
        }
    }

    /// Load the next HDMA table entry for a channel.
    fn hdma_load_entry(&mut self, ch: u8) {
        let idx = ch as usize;
        let bank = self.dma.channels[idx].src_bank;
        let addr = self.dma.channels[idx].hdma_addr;

        // Read line count byte
        let line_count = self.read(bank, addr);
        self.dma.channels[idx].hdma_addr = addr.wrapping_add(1);

        if line_count == 0 {
            self.dma.channels[idx].hdma_terminated = true;
            return;
        }

        self.dma.channels[idx].hdma_line_counter = line_count;
        self.dma.channels[idx].hdma_do_transfer = true;

        // For indirect mode, read 16-bit data address from table
        let indirect = self.dma.channels[idx].control & 0x40 != 0;
        if indirect {
            let tbl_addr = self.dma.channels[idx].hdma_addr;
            let lo = self.read(bank, tbl_addr) as u16;
            let hi = self.read(bank, tbl_addr.wrapping_add(1)) as u16;
            self.dma.channels[idx].size = lo | (hi << 8); // indirect addr stored in size field
            self.dma.channels[idx].hdma_addr = tbl_addr.wrapping_add(2);
        }
    }

    /// Execute HDMA transfers for one scanline.
    /// Called at the start of each visible scanline (0-224).
    pub fn hdma_run_scanline(&mut self) {
        if self.hdmaen == 0 { return; }

        for ch in 0..8u8 {
            if self.hdmaen & (1 << ch) == 0 { continue; }
            let idx = ch as usize;
            if self.dma.channels[idx].hdma_terminated { continue; }

            // Transfer data if flagged
            if self.dma.channels[idx].hdma_do_transfer {
                self.hdma_transfer(ch);
            }

            // Decrement line counter (bits 0-6 only)
            let counter = self.dma.channels[idx].hdma_line_counter;
            let new_count = (counter & 0x80) | ((counter & 0x7F).wrapping_sub(1) & 0x7F);
            self.dma.channels[idx].hdma_line_counter = new_count;

            // If counter reached 0, load next entry
            if new_count & 0x7F == 0 {
                self.hdma_load_entry(ch);
            } else {
                // Continuous mode (bit 7): transfer every line
                // Repeat mode: don't transfer until next entry
                self.dma.channels[idx].hdma_do_transfer = counter & 0x80 != 0;
            }
        }
    }

    /// Transfer data bytes for one HDMA channel on this scanline.
    fn hdma_transfer(&mut self, ch: u8) {
        use crate::dma::{DMA_TRANSFER_PATTERNS, DMA_TRANSFER_SIZES};

        let idx = ch as usize;
        let mode = (self.dma.channels[idx].control & 0x07) as usize;
        let indirect = self.dma.channels[idx].control & 0x40 != 0;
        let dest_base = 0x2100u16 + self.dma.channels[idx].dest as u16;
        let transfer_size = DMA_TRANSFER_SIZES[mode];
        let pattern = DMA_TRANSFER_PATTERNS[mode];

        for i in 0..transfer_size {
            let b_addr = dest_base + pattern[i as usize] as u16;

            let val = if indirect {
                // Read from indirect address (bank from $43x7, addr from $43x5-x6)
                let data_bank = self.dma.channels[idx].hdma_indirect_bank;
                let data_addr = self.dma.channels[idx].size;
                let v = self.read(data_bank, data_addr);
                self.dma.channels[idx].size = data_addr.wrapping_add(1);
                v
            } else {
                // Direct mode: read from HDMA table (bank from $43x4, addr from $43x8-x9)
                let bank = self.dma.channels[idx].src_bank;
                let addr = self.dma.channels[idx].hdma_addr;
                let v = self.read(bank, addr);
                self.dma.channels[idx].hdma_addr = addr.wrapping_add(1);
                v
            };

            // Write to B-bus register
            match b_addr {
                0x2100..=0x213F => self.ppu.write_register(b_addr, val),
                0x2140..=0x217F => self.apu.cpu_write((b_addr & 3) as u8, val),
                0x2180 => {
                    self.wram[self.wram_addr as usize & 0x1FFFF] = val;
                    self.wram_addr = (self.wram_addr + 1) & 0x1FFFF;
                }
                _ => {}
            }
        }
    }
}
