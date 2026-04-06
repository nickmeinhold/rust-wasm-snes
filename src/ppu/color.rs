/// SNES color conversion.
///
/// The SNES uses 15-bit BGR color: `0bbbbbgg gggrrrrr`.
/// We convert to 32-bit ARGB for the framebuffer.

/// Convert a 15-bit SNES color to 32-bit ARGB, applying master brightness.
/// Brightness is 0-15 (from INIDISP bits 0-3).
pub fn snes_to_argb(color: u16, brightness: u8) -> u32 {
    let r5 = (color & 0x1F) as u32;
    let g5 = ((color >> 5) & 0x1F) as u32;
    let b5 = ((color >> 10) & 0x1F) as u32;

    let bright = brightness as u32;

    // Scale: (5-bit × brightness / 15) → 8-bit
    let r8 = (r5 * bright * 255) / (31 * 15);
    let g8 = (g5 * bright * 255) / (31 * 15);
    let b8 = (b5 * bright * 255) / (31 * 15);

    0xFF000000 | (r8 << 16) | (g8 << 8) | b8
}
