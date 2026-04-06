/// SNES ROM / Cartridge loader.
///
/// Handles .smc files (with optional 512-byte copier header), parses the
/// internal ROM header at the LoROM offset ($7FC0), and exposes the raw
/// ROM data for memory mapping.

use std::fmt;
use std::fs;
use std::path::Path;

const COPIER_HEADER_SIZE: usize = 512;
const LOROM_HEADER_OFFSET: usize = 0x7FC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapMode {
    LoROM,
    HiROM,
}

pub struct Cartridge {
    /// Raw ROM data with copier header stripped.
    pub rom: Vec<u8>,
    /// Battery-backed SRAM (8KB for LTTP).
    pub sram: Vec<u8>,
    pub title: String,
    pub map_mode: MapMode,
    pub rom_size: usize,
    pub ram_size: usize,
    pub country: u8,
    pub version: u8,
    pub checksum: u16,
    pub checksum_complement: u16,
}

impl Cartridge {
    pub fn load(path: &Path) -> Result<Self, String> {
        let data = fs::read(path).map_err(|e| format!("Failed to read ROM: {e}"))?;

        // Detect and strip copier header.
        // If file size mod 1024 == 512, there's a 512-byte copier header.
        let rom = if data.len() % 1024 == COPIER_HEADER_SIZE {
            println!(
                "Detected {COPIER_HEADER_SIZE}-byte copier header, stripping..."
            );
            data[COPIER_HEADER_SIZE..].to_vec()
        } else {
            data
        };

        if rom.len() < LOROM_HEADER_OFFSET + 64 {
            return Err(format!(
                "ROM too small ({} bytes) to contain internal header",
                rom.len()
            ));
        }

        // Parse internal header at LoROM offset $7FC0.
        let h = &rom[LOROM_HEADER_OFFSET..];

        let title = String::from_utf8_lossy(&h[0..21]).trim().to_string();

        let map_byte = h[0x15]; // offset $7FD5 relative to $7FC0
        let map_mode = if map_byte & 0x01 == 0 {
            MapMode::LoROM
        } else {
            MapMode::HiROM
        };

        let rom_size_code = h[0x17]; // $7FD7
        let rom_size = 1024 << rom_size_code; // 2^N KB

        let ram_size_code = h[0x18]; // $7FD8
        let ram_size = if ram_size_code == 0 {
            0
        } else {
            1024 << ram_size_code
        };

        let country = h[0x19]; // $7FD9
        let version = h[0x1B]; // $7FDB

        let checksum_complement = u16::from_le_bytes([h[0x1C], h[0x1D]]); // $7FDC
        let checksum = u16::from_le_bytes([h[0x1E], h[0x1F]]); // $7FDE

        // SRAM — LTTP uses 8KB battery save.
        let sram = vec![0u8; ram_size];

        let cart = Self {
            rom,
            sram,
            title,
            map_mode,
            rom_size,
            ram_size,
            country,
            version,
            checksum,
            checksum_complement,
        };

        println!("{cart}");

        // Verify checksum complement.
        if checksum.wrapping_add(checksum_complement) != 0xFFFF {
            println!(
                "WARNING: checksum + complement = {:#06X} (expected 0xFFFF)",
                checksum.wrapping_add(checksum_complement)
            );
        }

        Ok(cart)
    }

    /// Read a byte from ROM using the LoROM offset formula.
    /// `bank` is the effective bank (already masked to 0-$7F).
    /// `addr` must be in $8000..$FFFF.
    pub fn read(&self, bank: u8, addr: u16) -> u8 {
        let offset = ((bank & 0x7F) as usize) * 0x8000 + (addr as usize - 0x8000);
        if offset < self.rom.len() {
            self.rom[offset]
        } else {
            0 // Open bus for out-of-range reads
        }
    }
}

impl fmt::Display for Cartridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ROM: \"{}\" | {:?} | {}KB ROM | {}KB SRAM | v{} | checksum: {:#06X}",
            self.title,
            self.map_mode,
            self.rom_size / 1024,
            self.ram_size / 1024,
            self.version,
            self.checksum,
        )
    }
}
