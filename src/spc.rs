/// .SPC file parser — loads SPC700 state snapshots for standalone playback.
///
/// Format: 66,048 bytes containing a complete snapshot of the SPC700 subsystem:
/// header (0x00), CPU registers (0x25), 64KB RAM (0x100), DSP registers (0x10100).

/// Parsed SPC file — everything needed to restore the APU to a playable state.
pub struct SpcFile {
    pub title: String,
    pub game: String,
    pub pc: u16,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub psw: u8,
    pub sp: u8,
    pub ram: Box<[u8; 65536]>,
    pub dsp_regs: [u8; 128],
}

impl SpcFile {
    /// Parse an SPC file from raw bytes.
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 0x10180 {
            return Err(format!("SPC file too small: {} bytes (need >= 0x10180)", data.len()));
        }

        // Validate header signature.
        let sig = &data[0..27];
        if !sig.starts_with(b"SNES-SPC700") {
            return Err("Invalid SPC header signature".into());
        }

        // CPU registers at 0x25-0x2B.
        let pc = u16::from_le_bytes([data[0x25], data[0x26]]);
        let a = data[0x27];
        let x = data[0x28];
        let y = data[0x29];
        let psw = data[0x2A];
        let sp = data[0x2B];

        // ID666 metadata (text format, best-effort).
        let title = String::from_utf8_lossy(&data[0x2E..0x4E]).trim_end_matches('\0').to_string();
        let game = String::from_utf8_lossy(&data[0x4E..0x6E]).trim_end_matches('\0').to_string();

        // 64KB RAM at offset 0x100.
        let mut ram = Box::new([0u8; 65536]);
        ram.copy_from_slice(&data[0x100..0x10100]);

        // 128 DSP registers at offset 0x10100.
        let mut dsp_regs = [0u8; 128];
        dsp_regs.copy_from_slice(&data[0x10100..0x10180]);

        Ok(SpcFile { title, game, pc, a, x, y, psw, sp, ram, dsp_regs })
    }
}
