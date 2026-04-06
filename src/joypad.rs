/// SNES joypad emulation (auto-read mode).
///
/// Button layout:
/// Bit: 15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
///       B  Y Sel St  ↑  ↓  ←  →  A  X  L  R  -  -  -  -

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
}

impl Joypad {
    pub fn new() -> Self {
        Self { current: 0 }
    }
}
