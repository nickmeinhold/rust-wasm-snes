/// SNES DMA (Direct Memory Access) — 8 channels.
///
/// General DMA halts the CPU and bulk-transfers data between the A-bus
/// (ROM/WRAM) and B-bus (PPU/APU registers at $2100-$21FF).

#[derive(Clone, Copy, Default)]
pub struct DmaChannel {
    /// $43x0 — Control: direction, mode, address mode.
    /// Bit 6 = HDMA indirect mode. Bits 0-2 = transfer mode.
    pub control: u8,
    /// $43x1 — B-bus destination register ($00-$FF → $2100 + dest).
    pub dest: u8,
    /// $43x2-$43x3 — A-bus source address (HDMA: table start address).
    pub src_addr: u16,
    /// $43x4 — A-bus source bank (HDMA: table bank).
    pub src_bank: u8,
    /// $43x5-$43x6 — Transfer size. In HDMA mode: indirect data address.
    pub size: u16,
    /// $43x7 — HDMA indirect bank.
    pub hdma_indirect_bank: u8,
    /// $43x8-$43x9 — HDMA table current address (runtime).
    pub hdma_addr: u16,
    /// $43xA — HDMA line counter (runtime).
    pub hdma_line_counter: u8,
    /// $43xB — Unused.
    pub unused: u8,

    // ── HDMA runtime state (not mapped to registers) ─────
    /// Whether this channel has reached a $00 terminator.
    pub hdma_terminated: bool,
    /// Whether to transfer data on the current scanline.
    pub hdma_do_transfer: bool,
}

pub struct Dma {
    pub channels: [DmaChannel; 8],
}

impl Dma {
    pub fn new() -> Self {
        Self {
            channels: [DmaChannel::default(); 8],
        }
    }

    /// Read a DMA register ($4300-$437F).
    pub fn read(&self, addr: u16) -> u8 {
        let ch = ((addr >> 4) & 0x07) as usize;
        let reg = addr & 0x0F;
        let c = &self.channels[ch];

        match reg {
            0x0 => c.control,
            0x1 => c.dest,
            0x2 => c.src_addr as u8,
            0x3 => (c.src_addr >> 8) as u8,
            0x4 => c.src_bank,
            0x5 => c.size as u8,
            0x6 => (c.size >> 8) as u8,
            0x7 => c.hdma_indirect_bank,
            0x8 => c.hdma_addr as u8,
            0x9 => (c.hdma_addr >> 8) as u8,
            0xA => c.hdma_line_counter,
            0xB | 0xF => c.unused,
            _ => 0,
        }
    }

    /// Write a DMA register ($4300-$437F).
    pub fn write(&mut self, addr: u16, val: u8) {
        let ch = ((addr >> 4) & 0x07) as usize;
        let reg = addr & 0x0F;
        let c = &mut self.channels[ch];

        match reg {
            0x0 => c.control = val,
            0x1 => c.dest = val,
            0x2 => c.src_addr = (c.src_addr & 0xFF00) | val as u16,
            0x3 => c.src_addr = (c.src_addr & 0x00FF) | ((val as u16) << 8),
            0x4 => c.src_bank = val,
            0x5 => c.size = (c.size & 0xFF00) | val as u16,
            0x6 => c.size = (c.size & 0x00FF) | ((val as u16) << 8),
            0x7 => c.hdma_indirect_bank = val,
            0x8 => c.hdma_addr = (c.hdma_addr & 0xFF00) | val as u16,
            0x9 => c.hdma_addr = (c.hdma_addr & 0x00FF) | ((val as u16) << 8),
            0xA => c.hdma_line_counter = val,
            0xB | 0xF => c.unused = val,
            _ => {}
        }
    }
}

/// B-bus register offsets for each transfer unit, by mode.
/// Each mode defines a pattern of B-bus register offsets per transfer unit.
pub const DMA_TRANSFER_PATTERNS: [[u8; 4]; 8] = [
    [0, 0, 0, 0], // Mode 0: 1 register
    [0, 1, 0, 1], // Mode 1: 2 registers (e.g., VMDATAL/H)
    [0, 0, 0, 0], // Mode 2: 1 register, write twice
    [0, 0, 1, 1], // Mode 3: 2 registers, write twice each
    [0, 1, 2, 3], // Mode 4: 4 registers
    [0, 1, 0, 1], // Mode 5: same as 1 (alternate interpretation)
    [0, 0, 0, 0], // Mode 6: same as 2
    [0, 0, 1, 1], // Mode 7: same as 3
];

/// Transfer lengths per mode.
pub const DMA_TRANSFER_SIZES: [u8; 8] = [1, 2, 2, 4, 4, 4, 2, 4];

/// Execute general DMA for all enabled channels.
/// This is called when $420B is written.
/// Returns the number of master cycles consumed.
pub fn execute_dma(
    channels: &mut [DmaChannel; 8],
    enable_mask: u8,
    // We need closures for bus read/write since DMA goes through the bus.
    mut read_a: impl FnMut(u8, u16) -> u8,
    mut write_b: impl FnMut(u16, u8),
    mut read_b: impl FnMut(u16) -> u8,
    mut write_a: impl FnMut(u8, u16, u8),
) -> u64 {
    let mut total_cycles: u64 = 0;

    for ch_idx in 0..8 {
        if enable_mask & (1 << ch_idx) == 0 { continue; }

        let mode = (channels[ch_idx].control & 0x07) as usize;
        let direction = channels[ch_idx].control & 0x80 != 0; // true = B→A
        let fixed_a = channels[ch_idx].control & 0x08 != 0;
        let decrement_a = channels[ch_idx].control & 0x10 != 0;

        let dest_base = 0x2100u16 + channels[ch_idx].dest as u16;
        let transfer_size = DMA_TRANSFER_SIZES[mode];
        let pattern = &DMA_TRANSFER_PATTERNS[mode];

        let mut remaining = if channels[ch_idx].size == 0 { 0x10000u32 } else { channels[ch_idx].size as u32 };
        let mut unit_idx: u8 = 0;

        while remaining > 0 {
            let b_addr = dest_base + pattern[unit_idx as usize] as u16;
            let a_bank = channels[ch_idx].src_bank;
            let a_addr = channels[ch_idx].src_addr;

            if direction {
                // B → A (read from PPU, write to WRAM/ROM)
                let val = read_b(b_addr);
                write_a(a_bank, a_addr, val);
            } else {
                // A → B (read from ROM/WRAM, write to PPU)
                let val = read_a(a_bank, a_addr);
                write_b(b_addr, val);
            }

            // Adjust A-bus address.
            if !fixed_a {
                if decrement_a {
                    channels[ch_idx].src_addr = channels[ch_idx].src_addr.wrapping_sub(1);
                } else {
                    channels[ch_idx].src_addr = channels[ch_idx].src_addr.wrapping_add(1);
                }
            }

            unit_idx = (unit_idx + 1) % transfer_size;
            remaining -= 1;
            total_cycles += 8; // 8 master cycles per byte
        }

        channels[ch_idx].size = 0;
    }

    total_cycles
}
