/// SNES PPU (Picture Processing Unit) emulation.
///
/// Handles register writes ($2100-$213F), VRAM/CGRAM/OAM access,
/// and scanline-based rendering for Mode 1 (used by LTTP).

pub mod render;
pub mod color;

/// Per-BG-layer configuration.
#[derive(Clone, Copy, Default)]
pub struct BgLayer {
    /// Tilemap base address in VRAM (word address, shifted left by 1 for byte addr).
    pub tilemap_addr: u16,
    /// Tilemap size: 0=32×32, 1=64×32, 2=32×64, 3=64×64.
    pub tilemap_size: u8,
    /// Character (tile) data base address in VRAM (word address).
    pub chr_addr: u16,
    /// Horizontal scroll (10-bit).
    pub hscroll: u16,
    /// Vertical scroll (10-bit).
    pub vscroll: u16,
    /// Tile size: false=8×8, true=16×16.
    pub tile_size: bool,
}

pub struct Ppu {
    // ── Video RAM ──────────────────────────────────────
    pub vram: Box<[u8; 0x10000]>,  // 64KB
    pub oam: Box<[u8; 0x220]>,     // 544 bytes
    pub cgram: Box<[u8; 0x200]>,   // 512 bytes

    // ── Display control ────────────────────────────────
    pub inidisp: u8,       // $2100: forced blank + brightness
    pub bgmode: u8,        // $2105: BG mode + tile size bits
    pub mosaic: u8,        // $2106
    pub bg: [BgLayer; 4],  // BG1-BG4

    // ── VRAM access ────────────────────────────────────
    pub vram_addr: u16,        // $2116/$2117
    pub vram_increment: u8,    // $2115 (VMAIN)
    pub vram_prefetch: u16,    // Prefetch buffer for reads
    pub vram_remap: u8,        // Address remapping mode from VMAIN

    // ── CGRAM access ─────���─────────────────────────────
    pub cgram_addr: u8,        // $2121
    pub cgram_latch: u8,       // Write latch (low byte)
    pub cgram_flipflop: bool,

    // ── OAM access ─────────────────────────────���───────
    pub oam_addr: u16,         // $2102/$2103
    pub oam_internal_addr: u16,
    pub oam_latch: u8,
    pub oam_flipflop: bool,
    pub obj_size: u8,          // $2101 OBSEL
    pub obj_base: u16,
    pub obj_name_select: u16,

    // ── Scroll latch ────���──────────────────────────────
    pub scroll_latch: u8,      // Previous write for write-twice scroll regs
    // BG offset latch for the Mode 7 / BG scroll shared latch
    pub bghofs_latch: u8,

    // ── Mode 7 ───────────────────────────────────────
    pub m7a: i16,              // $211B — matrix A (cosθ × scale)
    pub m7b: i16,              // $211C — matrix B (sinθ × scale)
    pub m7c: i16,              // $211D — matrix C (-sinθ × scale)
    pub m7d: i16,              // $211E — matrix D (cosθ × scale)
    pub m7x: i16,              // $211F — rotation center X (13-bit signed)
    pub m7y: i16,              // $2120 — rotation center Y (13-bit signed)
    pub m7_latch: u8,          // Shared write-twice latch for M7 registers
    pub m7_hofs: i16,          // $210D also writes M7HOFS (13-bit signed)
    pub m7_vofs: i16,          // $210E also writes M7VOFS (13-bit signed)

    // ── Screen designation ─────────────────────────────
    pub tm: u8,                // $212C main screen
    pub ts: u8,                // $212D sub screen
    pub tmw: u8,               // $212E window mask main
    pub tsw: u8,               // $212F window mask sub

    // ── Color math ──��──────────────────────────────────
    pub cgwsel: u8,            // $2130
    pub cgadsub: u8,           // $2131
    pub fixed_color_r: u8,     // $2132 components
    pub fixed_color_g: u8,
    pub fixed_color_b: u8,

    // ── Window ─────────────────────────────────────────
    pub w1_left: u8,
    pub w1_right: u8,
    pub w2_left: u8,
    pub w2_right: u8,
    pub wbglog: u8,            // $212A
    pub wobjlog: u8,           // $212B
    pub w12sel: u8,            // $2123
    pub w34sel: u8,            // $2124
    pub wobjsel: u8,           // $2125

    // ── Rendering state ────���───────────────────────────
    pub scanline: u16,
    pub frame_buffer: Box<[u32; 256 * 224]>,

    // ── Status ─────────────────────────────────────────
    pub latch_hv: bool,        // H/V latch flag
    pub ophct: u16,            // latched H counter
    pub opvct: u16,            // latched V counter
    pub ophct_flipflop: bool,
    pub opvct_flipflop: bool,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            vram: Box::new([0u8; 0x10000]),
            oam: Box::new([0u8; 0x220]),
            cgram: Box::new([0u8; 0x200]),

            inidisp: 0x80, // Forced blank on reset
            bgmode: 0,
            mosaic: 0,
            bg: [BgLayer::default(); 4],

            vram_addr: 0,
            vram_increment: 1,
            vram_prefetch: 0,
            vram_remap: 0,

            cgram_addr: 0,
            cgram_latch: 0,
            cgram_flipflop: false,

            oam_addr: 0,
            oam_internal_addr: 0,
            oam_latch: 0,
            oam_flipflop: false,
            obj_size: 0,
            obj_base: 0,
            obj_name_select: 0,

            scroll_latch: 0,
            bghofs_latch: 0,

            m7a: 0,
            m7b: 0,
            m7c: 0,
            m7d: 0,
            m7x: 0,
            m7y: 0,
            m7_latch: 0,
            m7_hofs: 0,
            m7_vofs: 0,

            tm: 0,
            ts: 0,
            tmw: 0,
            tsw: 0,

            cgwsel: 0,
            cgadsub: 0,
            fixed_color_r: 0,
            fixed_color_g: 0,
            fixed_color_b: 0,

            w1_left: 0,
            w1_right: 0,
            w2_left: 0,
            w2_right: 0,
            wbglog: 0,
            wobjlog: 0,
            w12sel: 0,
            w34sel: 0,
            wobjsel: 0,

            scanline: 0,
            frame_buffer: Box::new([0u32; 256 * 224]),

            latch_hv: false,
            ophct: 0,
            opvct: 0,
            ophct_flipflop: false,
            opvct_flipflop: false,
        }
    }

    /// Get the VRAM increment step size from the VMAIN register.
    fn vram_step(&self) -> u16 {
        match self.vram_increment & 0x03 {
            0 => 1,
            1 => 32,
            2 | 3 => 128,
            _ => unreachable!(),
        }
    }

    /// Apply VRAM address translation/remapping.
    fn translate_vram_addr(&self, addr: u16) -> u16 {
        match self.vram_remap {
            0 => addr,
            1 => {
                // Remap: aaaaaaaaBBBccccc → aaaaaaaacccccBBB
                (addr & 0xFF00) | ((addr & 0x001F) << 3) | ((addr >> 5) & 7)
            }
            2 => {
                // Remap: aaaaaaaBBBcccccc → aaaaaaaccccccBBB
                (addr & 0xFE00) | ((addr & 0x003F) << 3) | ((addr >> 6) & 7)
            }
            3 => {
                // Remap: aaaaaaBBBccccccc → aaaaaacccccccBBB
                (addr & 0xFC00) | ((addr & 0x007F) << 3) | ((addr >> 7) & 7)
            }
            _ => addr,
        }
    }

    /// Handle a write to a PPU register ($2100-$213F).
    pub fn write_register(&mut self, addr: u16, val: u8) {
        match addr {
            0x2100 => { self.inidisp = val; }
            0x2101 => { // OBSEL
                self.obj_size = (val >> 5) & 0x07;
                self.obj_base = ((val & 0x07) as u16) << 13;
                self.obj_name_select = (((val >> 3) & 0x03) as u16 + 1) << 12;
            }
            0x2102 => { // OAMADDL
                self.oam_addr = (self.oam_addr & 0x0200) | ((val as u16) << 1);
                self.oam_internal_addr = self.oam_addr;
                self.oam_flipflop = false;
            }
            0x2103 => { // OAMADDH
                self.oam_addr = (self.oam_addr & 0x01FE) | (((val & 0x01) as u16) << 9);
                self.oam_internal_addr = self.oam_addr;
                self.oam_flipflop = false;
            }
            0x2104 => { // OAMDATA
                if self.oam_internal_addr >= 0x200 {
                    // High table
                    let idx = self.oam_internal_addr as usize & 0x21F;
                    if idx < self.oam.len() {
                        self.oam[idx] = val;
                    }
                    self.oam_internal_addr = self.oam_internal_addr.wrapping_add(1) & 0x3FF;
                } else if !self.oam_flipflop {
                    self.oam_latch = val;
                    self.oam_flipflop = true;
                } else {
                    let idx = self.oam_internal_addr as usize & 0x1FE;
                    if idx + 1 < self.oam.len() {
                        self.oam[idx] = self.oam_latch;
                        self.oam[idx + 1] = val;
                    }
                    self.oam_internal_addr = self.oam_internal_addr.wrapping_add(2) & 0x3FF;
                    self.oam_flipflop = false;
                }
            }
            0x2105 => { // BGMODE
                self.bgmode = val;
                self.bg[0].tile_size = val & 0x10 != 0;
                self.bg[1].tile_size = val & 0x20 != 0;
                self.bg[2].tile_size = val & 0x40 != 0;
                self.bg[3].tile_size = val & 0x80 != 0;
            }
            0x2106 => { self.mosaic = val; }
            0x2107 => { // BG1SC
                self.bg[0].tilemap_addr = ((val & 0xFC) as u16) << 8;
                self.bg[0].tilemap_size = val & 0x03;
            }
            0x2108 => { // BG2SC
                self.bg[1].tilemap_addr = ((val & 0xFC) as u16) << 8;
                self.bg[1].tilemap_size = val & 0x03;
            }
            0x2109 => { // BG3SC
                self.bg[2].tilemap_addr = ((val & 0xFC) as u16) << 8;
                self.bg[2].tilemap_size = val & 0x03;
            }
            0x210A => { // BG4SC
                self.bg[3].tilemap_addr = ((val & 0xFC) as u16) << 8;
                self.bg[3].tilemap_size = val & 0x03;
            }
            0x210B => { // BG12NBA — BG1/BG2 character data address
                self.bg[0].chr_addr = ((val & 0x0F) as u16) << 12;
                self.bg[1].chr_addr = ((val >> 4) as u16) << 12;
            }
            0x210C => { // BG34NBA
                self.bg[2].chr_addr = ((val & 0x0F) as u16) << 12;
                self.bg[3].chr_addr = ((val >> 4) as u16) << 12;
            }
            // BG scroll registers — write-twice with latch
            0x210D => { // BG1HOFS + M7HOFS
                self.bg[0].hscroll = ((val as u16) << 8) | (self.scroll_latch as u16 & 0xF8)
                    | (self.bghofs_latch as u16 & 0x07);
                self.scroll_latch = val;
                self.bghofs_latch = val;
                // Mode 7 HOFS: 13-bit signed from M7 latch
                let raw = ((val as u16) << 8) | self.m7_latch as u16;
                self.m7_hofs = ((raw << 3) as i16) >> 3; // Sign-extend from 13 bits
                self.m7_latch = val;
            }
            0x210E => { // BG1VOFS + M7VOFS
                self.bg[0].vscroll = ((val as u16) << 8) | self.scroll_latch as u16;
                self.scroll_latch = val;
                // Mode 7 VOFS: 13-bit signed from M7 latch
                let raw = ((val as u16) << 8) | self.m7_latch as u16;
                self.m7_vofs = ((raw << 3) as i16) >> 3;
                self.m7_latch = val;
            }
            0x210F => { // BG2HOFS
                self.bg[1].hscroll = ((val as u16) << 8) | (self.scroll_latch as u16 & 0xF8)
                    | (self.bghofs_latch as u16 & 0x07);
                self.scroll_latch = val;
                self.bghofs_latch = val;
            }
            0x2110 => { // BG2VOFS
                self.bg[1].vscroll = ((val as u16) << 8) | self.scroll_latch as u16;
                self.scroll_latch = val;
            }
            0x2111 => { // BG3HOFS
                self.bg[2].hscroll = ((val as u16) << 8) | (self.scroll_latch as u16 & 0xF8)
                    | (self.bghofs_latch as u16 & 0x07);
                self.scroll_latch = val;
                self.bghofs_latch = val;
            }
            0x2112 => { // BG3VOFS
                self.bg[2].vscroll = ((val as u16) << 8) | self.scroll_latch as u16;
                self.scroll_latch = val;
            }
            0x2113 => { // BG4HOFS
                self.bg[3].hscroll = ((val as u16) << 8) | (self.scroll_latch as u16 & 0xF8)
                    | (self.bghofs_latch as u16 & 0x07);
                self.scroll_latch = val;
                self.bghofs_latch = val;
            }
            0x2114 => { // BG4VOFS
                self.bg[3].vscroll = ((val as u16) << 8) | self.scroll_latch as u16;
                self.scroll_latch = val;
            }
            // Mode 7 matrix registers — all use shared write-twice latch
            0x211B => { // M7A
                self.m7a = ((val as u16) << 8 | self.m7_latch as u16) as i16;
                self.m7_latch = val;
            }
            0x211C => { // M7B
                self.m7b = ((val as u16) << 8 | self.m7_latch as u16) as i16;
                self.m7_latch = val;
            }
            0x211D => { // M7C
                self.m7c = ((val as u16) << 8 | self.m7_latch as u16) as i16;
                self.m7_latch = val;
            }
            0x211E => { // M7D
                self.m7d = ((val as u16) << 8 | self.m7_latch as u16) as i16;
                self.m7_latch = val;
            }
            0x211F => { // M7X — rotation center X (13-bit signed)
                let raw = ((val as u16) << 8) | self.m7_latch as u16;
                self.m7x = ((raw << 3) as i16) >> 3;
                self.m7_latch = val;
            }
            0x2120 => { // M7Y — rotation center Y (13-bit signed)
                let raw = ((val as u16) << 8) | self.m7_latch as u16;
                self.m7y = ((raw << 3) as i16) >> 3;
                self.m7_latch = val;
            }
            0x2115 => { // VMAIN
                self.vram_increment = val & 0x03;
                self.vram_remap = (val >> 2) & 0x03;
                // Bit 7: increment after high byte (1) or low byte (0)
                // Stored in bit 7 of vram_increment for convenience
                if val & 0x80 != 0 {
                    self.vram_increment |= 0x80;
                }
            }
            0x2116 => { // VMADDL
                self.vram_addr = (self.vram_addr & 0xFF00) | val as u16;
                // Prefetch on address change
                let translated = self.translate_vram_addr(self.vram_addr);
                let byte_addr = (translated as usize) * 2;
                if byte_addr + 1 < self.vram.len() {
                    self.vram_prefetch =
                        self.vram[byte_addr] as u16 | ((self.vram[byte_addr + 1] as u16) << 8);
                }
            }
            0x2117 => { // VMADDH
                self.vram_addr = (self.vram_addr & 0x00FF) | ((val as u16) << 8);
                let translated = self.translate_vram_addr(self.vram_addr);
                let byte_addr = (translated as usize) * 2;
                if byte_addr + 1 < self.vram.len() {
                    self.vram_prefetch =
                        self.vram[byte_addr] as u16 | ((self.vram[byte_addr + 1] as u16) << 8);
                }
            }
            0x2118 => { // VMDATAL — write low byte
                let translated = self.translate_vram_addr(self.vram_addr);
                let byte_addr = (translated as usize) * 2;
                if byte_addr < self.vram.len() {
                    self.vram[byte_addr] = val;
                }
                // Increment after low byte write if VMAIN bit 7 is 0
                if self.vram_increment & 0x80 == 0 {
                    self.vram_addr = self.vram_addr.wrapping_add(self.vram_step());
                }
            }
            0x2119 => { // VMDATAH — write high byte
                let translated = self.translate_vram_addr(self.vram_addr);
                let byte_addr = (translated as usize) * 2 + 1;
                if byte_addr < self.vram.len() {
                    self.vram[byte_addr] = val;
                }
                // Increment after high byte write if VMAIN bit 7 is 1
                if self.vram_increment & 0x80 != 0 {
                    self.vram_addr = self.vram_addr.wrapping_add(self.vram_step());
                }
            }
            0x2121 => { // CGADD
                self.cgram_addr = val;
                self.cgram_flipflop = false;
            }
            0x2122 => { // CGDATA
                let idx = (self.cgram_addr as usize) * 2;
                if !self.cgram_flipflop {
                    self.cgram_latch = val;
                    self.cgram_flipflop = true;
                } else {
                    if idx + 1 < self.cgram.len() {
                        self.cgram[idx] = self.cgram_latch;
                        self.cgram[idx + 1] = val & 0x7F; // Only 15 bits used
                    }
                    self.cgram_addr = self.cgram_addr.wrapping_add(1);
                    self.cgram_flipflop = false;
                }
            }
            0x2123 => { self.w12sel = val; }
            0x2124 => { self.w34sel = val; }
            0x2125 => { self.wobjsel = val; }
            0x2126 => { self.w1_left = val; }
            0x2127 => { self.w1_right = val; }
            0x2128 => { self.w2_left = val; }
            0x2129 => { self.w2_right = val; }
            0x212A => { self.wbglog = val; }
            0x212B => { self.wobjlog = val; }
            0x212C => { self.tm = val; }
            0x212D => { self.ts = val; }
            0x212E => { self.tmw = val; }
            0x212F => { self.tsw = val; }
            0x2130 => { self.cgwsel = val; }
            0x2131 => { self.cgadsub = val; }
            0x2132 => { // COLDATA — fixed color
                let intensity = val & 0x1F;
                if val & 0x20 != 0 { self.fixed_color_r = intensity; }
                if val & 0x40 != 0 { self.fixed_color_g = intensity; }
                if val & 0x80 != 0 { self.fixed_color_b = intensity; }
            }
            0x2133 => { /* SETINI — interlace/overscan, mostly ignore */ }
            _ => {} // Unmapped or unimplemented
        }

    }

    /// Handle a read from a PPU register ($2134-$213F).
    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x2134 => { // MPYL — low byte of M7A × M7B
                let result = (self.m7a as i32).wrapping_mul(self.m7b as i32 >> 8);
                result as u8
            }
            0x2135 => { // MPYM — mid byte
                let result = (self.m7a as i32).wrapping_mul(self.m7b as i32 >> 8);
                (result >> 8) as u8
            }
            0x2136 => { // MPYH — high byte
                let result = (self.m7a as i32).wrapping_mul(self.m7b as i32 >> 8);
                (result >> 16) as u8
            }
            0x2137 => { // SLHV — latch H/V counter
                self.latch_hv = true;
                self.ophct = 0; // Approximate — real hardware latches exact dot
                self.opvct = self.scanline;
                self.ophct_flipflop = false;
                self.opvct_flipflop = false;
                0 // open bus
            }
            0x2138 => { // OAMDATAREAD
                let idx = self.oam_internal_addr as usize;
                let val = if idx < self.oam.len() { self.oam[idx] } else { 0 };
                self.oam_internal_addr = self.oam_internal_addr.wrapping_add(1) & 0x3FF;
                val
            }
            0x2139 => { // VMDATALREAD — read low byte of prefetch
                let val = self.vram_prefetch as u8;
                if self.vram_increment & 0x80 == 0 {
                    let translated = self.translate_vram_addr(self.vram_addr);
                    let byte_addr = (translated as usize) * 2;
                    if byte_addr + 1 < self.vram.len() {
                        self.vram_prefetch = self.vram[byte_addr] as u16
                            | ((self.vram[byte_addr + 1] as u16) << 8);
                    }
                    self.vram_addr = self.vram_addr.wrapping_add(self.vram_step());
                }
                val
            }
            0x213A => { // VMDATAHREAD
                let val = (self.vram_prefetch >> 8) as u8;
                if self.vram_increment & 0x80 != 0 {
                    let translated = self.translate_vram_addr(self.vram_addr);
                    let byte_addr = (translated as usize) * 2;
                    if byte_addr + 1 < self.vram.len() {
                        self.vram_prefetch = self.vram[byte_addr] as u16
                            | ((self.vram[byte_addr + 1] as u16) << 8);
                    }
                    self.vram_addr = self.vram_addr.wrapping_add(self.vram_step());
                }
                val
            }
            0x213B => { // CGDATAREAD
                let idx = (self.cgram_addr as usize) * 2;
                let val = if !self.cgram_flipflop {
                    self.cgram_flipflop = true;
                    if idx < self.cgram.len() { self.cgram[idx] } else { 0 }
                } else {
                    self.cgram_flipflop = false;
                    let v = if idx + 1 < self.cgram.len() { self.cgram[idx + 1] } else { 0 };
                    self.cgram_addr = self.cgram_addr.wrapping_add(1);
                    v
                };
                val
            }
            0x213C => { // OPHCT
                let val = if !self.ophct_flipflop {
                    self.ophct_flipflop = true;
                    self.ophct as u8
                } else {
                    self.ophct_flipflop = false;
                    (self.ophct >> 8) as u8
                };
                val
            }
            0x213D => { // OPVCT
                let val = if !self.opvct_flipflop {
                    self.opvct_flipflop = true;
                    self.opvct as u8
                } else {
                    self.opvct_flipflop = false;
                    (self.opvct >> 8) as u8
                };
                val
            }
            0x213E => 0x01, // STAT77 — PPU1 version
            0x213F => { // STAT78 — PPU2 version + interlace
                self.ophct_flipflop = false;
                self.opvct_flipflop = false;
                0x01 // PPU2 version, NTSC
            }
            _ => 0, // Open bus
        }
    }
}
