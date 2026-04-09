/// SNES joypad emulation — auto-read ($4218) and serial ($4016) modes.
///
/// Button layout (matches auto-read register $4218/$4219):
/// Bit: 15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
///       B  Y Sel St  ↑  ↓  ←  →  A  X  L  R  -  -  -  -
///
/// Serial reads via $4016/$4017 return one bit per read in the same
/// order (B first, then Y, Select, Start, …, R, then 1s).

pub const BTN_B: u16      = 0x8000;
pub const BTN_Y: u16      = 0x4000;
pub const BTN_SELECT: u16 = 0x2000;
pub const BTN_START: u16  = 0x1000;
pub const BTN_UP: u16     = 0x0800;
pub const BTN_DOWN: u16   = 0x0400;
pub const BTN_LEFT: u16   = 0x0200;
pub const BTN_RIGHT: u16  = 0x0100;
pub const BTN_A: u16      = 0x0080;
pub const BTN_X: u16      = 0x0040;
pub const BTN_L: u16      = 0x0020;
pub const BTN_R: u16      = 0x0010;

pub struct Joypad {
    /// Current button state (auto-read result for pad 1).
    pub current: u16,

    /// Latched button state for serial reads ($4016/$4017).
    latched: u16,
    /// Bit index for the next serial read (0–15, then returns 1).
    bit_index: u8,
    /// Whether the strobe line is currently high.
    strobe: bool,
}

impl Joypad {
    pub fn new() -> Self {
        Self {
            current: 0,
            latched: 0,
            bit_index: 0,
            strobe: false,
        }
    }

    /// Write to $4016 — controls the strobe line.
    /// Setting bit 0 high latches the current button state.
    /// Clearing bit 0 releases the strobe and resets the serial counter.
    pub fn write_strobe(&mut self, val: u8) {
        let new_strobe = val & 1 != 0;
        if self.strobe && !new_strobe {
            // Falling edge: latch buttons and reset counter
            self.latched = self.current;
            self.bit_index = 0;
            #[cfg(not(target_arch = "wasm32"))]
            if self.current != 0 {
                eprintln!("  JOYPAD strobe: latched {:04X}", self.current);
            }
        }
        self.strobe = new_strobe;
    }

    /// Read from $4016 — returns one bit of player 1 data (bit 0).
    /// Bits are returned MSB-first (B, Y, Select, Start, …, R).
    /// After all 16 bits, returns 1.
    pub fn read_serial(&mut self) -> u8 {
        if self.strobe {
            // While strobe is high, always return current state of B button
            return ((self.current >> 15) & 1) as u8;
        }

        if self.bit_index >= 16 {
            return 1;
        }

        let bit = (self.latched >> (15 - self.bit_index)) & 1;
        self.bit_index += 1;
        bit as u8
    }
}
