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

    /// Last CPU PC before a write (for write breakpoint logging).
    pub last_write_bank: u8,
    pub last_write_pc: u16,

    /// Master-cycle deadline of the current scanline. Set by the frame loop
    /// before each scanline's inner step loop, read by the CPU's idle-loop
    /// fast path to bound forward skips.
    pub current_scanline_target: u64,

    // ── JIT sync state (Level 1) ─────────────────────────────
    //
    // Near/byuu's insight: synchronize the APU only when the CPU accesses
    // the shared I/O ports ($2140-$2143), not after every instruction. This
    // is both more accurate (sync happens at the cycle of the access) and
    // faster (eliminates ~95% of catch_up calls for instructions that don't
    // touch APU ports).

    /// Running master-cycle counter within the current scanline.  Updated
    /// by the frame loop after each `cpu.step()` returns.
    pub master_clock: u64,

    /// Master-cycle value at which the APU was last caught up.  The delta
    /// `master_clock - last_apu_sync` is the number of master cycles the
    /// APU still owes when a port access forces a sync.
    pub last_apu_sync: u64,
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
            last_write_bank: 0,
            last_write_pc: 0,
            current_scanline_target: 0,
            master_clock: 0,
            last_apu_sync: 0,
        }
    }

    /// Returns true if reads from (bank, addr) have no side effects AND the
    /// address cannot be mutated except by NMI/IRQ/HDMA. WRAM, SRAM, and ROM
    /// qualify; I/O registers ($2100-$5FFF) do not — many have read-clear or
    /// read-advance behaviour ($4210 RDNMI, $2180 WMDATA, $2139 VMDATAREAD).
    ///
    /// Used by the CPU idle-loop fast path to decide whether the polled byte
    /// of an LDA→BEQ spin-wait is safe to elide.
    pub fn is_pure_memory(&self, bank: u8, addr: u16) -> bool {
        let eb = bank & 0x7F;
        matches!(
            (eb, addr),
            (0x7E, _)                                 // full WRAM
            | (0x7F, _)                               // full WRAM (upper)
            | (0x00..=0x3F, 0x0000..=0x1FFF)          // WRAM low mirror
            | (0x00..=0x3F, 0x8000..=0xFFFF)          // LoROM cart
            | (0x40..=0x6F, 0x8000..=0xFFFF)          // ROM banks $40-$6F
            | (0x70..=0x7D, 0x0000..=0x7FFF)          // SRAM
            | (0x70..=0x7D, 0x8000..=0xFFFF)          // ROM
        )
    }

    /// Force-synchronize the APU to the current master clock cycle.
    ///
    /// Called on CPU reads/writes to the APU I/O ports ($2140-$2143) so the
    /// SPC700 has executed up to the exact cycle of the access. This is the
    /// core of Near/byuu's "JIT sync" model: instead of catching up the APU
    /// after every CPU instruction, we only sync on shared-memory access.
    ///
    /// The delta `master_clock - last_apu_sync` is always non-negative and
    /// represents the master cycles the APU hasn't yet consumed.
    #[inline]
    pub fn sync_apu(&mut self) {
        debug_assert!(self.master_clock >= self.last_apu_sync,
            "master_clock {} < last_apu_sync {} — clock inversion",
            self.master_clock, self.last_apu_sync);
        let delta = self.master_clock - self.last_apu_sync;
        if delta > 0 {
            self.apu.catch_up(delta as u32);
            self.last_apu_sync = self.master_clock;
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
            (0x00..=0x3F, 0x2140..=0x217F) => {
                // JIT sync: catch up APU to the exact cycle of this port read.
                self.sync_apu();
                self.apu.cpu_read((addr & 3) as u8)
            }
            (0x00..=0x3F, 0x2180) => { // WMDATA — read from WRAM at wram_addr
                let val = self.wram[self.wram_addr as usize & 0x1FFFF];
                self.wram_addr = (self.wram_addr + 1) & 0x1FFFF;
                val
            }
            (0x00..=0x3F, 0x4016) => self.joypad.read_serial(),
            (0x00..=0x3F, 0x4017) => 0, // Player 2 — not implemented
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
            (0x00..=0x3F, 0x2100..=0x213F) => {
                #[cfg(target_arch = "wasm32")]
                if addr == 0x212C && val != self.ppu.tm {
                    web_sys::console::log_1(
                        &format!("TM write: {:02X} -> {:02X} from {:02X}:{:04X}",
                            self.ppu.tm, val, self.last_write_bank, self.last_write_pc).into()
                    );
                }
                self.ppu.write_register(addr, val);
            }
            (0x00..=0x3F, 0x2140..=0x217F) => {
                // JIT sync: catch up APU before the CPU writes new port data,
                // so the SPC700 processes any pending instructions first.
                self.sync_apu();
                self.apu.cpu_write((addr & 3) as u8, val);
            }
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
            (0x00..=0x3F, 0x4016) => { self.joypad.write_strobe(val); }
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
            0x4218 => self.joypad.read_auto() as u8,   // JOY1L
            0x4219 => (self.joypad.read_auto() >> 8) as u8, // JOY1H
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
                #[cfg(feature = "vram-trace")]
                eprintln!("  $420B write val={:02X} from PC={:02X}:{:04X} wram[0]={:02X}",
                    val, self.last_write_bank, self.last_write_pc, self.wram[0]);
                self.execute_general_dma(val);
            }
            0x420C => {
                #[cfg(feature = "vram-trace")]
                if val != self.hdmaen {
                    // Log which channels are enabled and their targets
                    let mut targets = String::new();
                    for ch in 0..8u8 {
                        if val & (1 << ch) != 0 {
                            let dest = 0x2100u16 + self.dma.channels[ch as usize].dest as u16;
                            targets.push_str(&format!(" ch{}→${:04X}", ch, dest));
                        }
                    }
                    eprintln!("  HDMAEN: {:02X} -> {:02X}{}", self.hdmaen, val, targets);
                }
                self.hdmaen = val;
            }
            0x420D => { self.memsel = val; }
            _ => {}
        }
    }

    /// Execute general DMA for all enabled channels.
    /// Inlined here to avoid borrow-checker issues with closures over `self`.
    fn execute_general_dma(&mut self, enable_mask: u8) {
        use crate::dma::DMA_TRANSFER_PATTERNS;

        let mut total_cycles: u64 = 0;
        let mut dma_cycles_since_sync: u32 = 0;

        for ch_idx in 0..8u8 {
            if enable_mask & (1 << ch_idx) == 0 { continue; }

            #[cfg(feature = "vram-trace")]
            {
                let ch = &self.dma.channels[ch_idx as usize];
                let dest = 0x2100u16 + ch.dest as u16;
                let mode = ch.control & 0x07;
                let size = if ch.size == 0 { 0x10000u32 } else { ch.size as u32 };
                let game_mode = self.wram[0x0100];
                let fixed = ch.control & 0x08 != 0;
                let src_val = if ch.src_bank == 0x00 && (ch.src_addr as usize) < self.wram.len() {
                    self.wram[ch.src_addr as usize]
                } else { 0xFF };
                eprintln!("  DMA ch{} mode={} dest=${:04X} src={:02X}:{:04X} size={} dir={} fixed={} src[0]={:02X} vram={:04X} vmain={:02X} game={:02X} scan={}",
                    ch_idx, mode, dest, ch.src_bank, ch.src_addr, size,
                    if ch.control & 0x80 != 0 { "B→A" } else { "A→B" },
                    fixed, src_val,
                    self.ppu.vram_addr, self.ppu.vram_increment, game_mode,
                    self.ppu.scanline);
            }

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
                    // B → A: read from B-bus, write to A-bus
                    let val = self.read(0x00, b_addr);
                    self.write(a_bank, a_addr, val);
                } else {
                    // A → B: read from A-bus, write to B-bus
                    // Route through self.write() so DMA and CPU writes
                    // take the same code path (container morphism factors
                    // through the Bus: T_DMA → T_BUS → T_PPU).
                    let val = self.read(a_bank, a_addr);
                    self.write(0x00, b_addr, val);
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

                // Mid-DMA APU sync: on real hardware the APU continues
                // running during DMA. Sync every 128 master cycles (16
                // bytes) so timers and DSP sample generation stay in step.
                dma_cycles_since_sync += 8;
                if dma_cycles_since_sync >= 128 {
                    self.master_clock += dma_cycles_since_sync as u64;
                    self.sync_apu();
                    dma_cycles_since_sync = 0;
                }
            }

            self.dma.channels[ch_idx as usize].size = 0;
        }

        // Credit any remaining DMA cycles not yet synced.
        if dma_cycles_since_sync > 0 {
            self.master_clock += dma_cycles_since_sync as u64;
            self.sync_apu();
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
    ///
    /// NOTE: This currently runs AFTER the end-of-scanline APU flush in
    /// run_frame_inner(), so the sync_apu() call below always sees delta=0
    /// and is effectively a no-op. On real hardware, HDMA runs during H-blank
    /// at the START of a scanline (before CPU execution), not after.
    /// Moving HDMA to run before the CPU step loop would make this pre-sync
    /// meaningful and improve timing accuracy for games that use HDMA to
    /// write APU ports.
    pub fn hdma_run_scanline(&mut self) {
        if self.hdmaen == 0 { return; }
        // Flush pending APU cycles before HDMA transfers begin.
        // HDMA runs during H-blank; the APU should be caught up to this
        // point so any HDMA writes to APU ports see correct state.
        // (Currently a no-op — see doc comment above.)
        self.sync_apu();

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

            // Write to B-bus register — route through self.write() so
            // HDMA and CPU writes take the same code path.
            self.write(0x00, b_addr, val);
        }
    }
}
