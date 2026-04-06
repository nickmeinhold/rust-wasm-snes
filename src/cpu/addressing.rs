/// 65C816 addressing mode resolution.
///
/// Each function reads operand bytes from [PBR:PC], resolves the effective
/// address, and returns (bank, addr) for memory operands. The CPU's PC
/// advances past the operand bytes as a side effect of fetch_byte/fetch_word.

use crate::bus::Bus;
use super::Cpu;

/// Resolved address for memory operands.
#[derive(Debug, Clone, Copy)]
pub struct Addr {
    pub bank: u8,
    pub addr: u16,
}

// ── Immediate ───────────────────────────────────────────────────────────

/// Immediate 8-bit: next byte.
pub fn immediate8(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    cpu.fetch_byte(bus)
}

/// Immediate 16-bit: next two bytes (little-endian).
pub fn immediate16(cpu: &mut Cpu, bus: &mut Bus) -> u16 {
    cpu.fetch_word(bus)
}

// ── Direct Page ─────────────────────────────────────────────────────────

/// Direct Page: DP + d (bank 0).
pub fn direct(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let d = cpu.fetch_byte(bus) as u16;
    Addr { bank: 0, addr: cpu.dp.wrapping_add(d) }
}

/// Direct Page Indexed X: DP + d + X (bank 0).
pub fn direct_x(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let d = cpu.fetch_byte(bus) as u16;
    let x = if cpu.is_x8() { cpu.x & 0xFF } else { cpu.x };
    Addr { bank: 0, addr: cpu.dp.wrapping_add(d).wrapping_add(x) }
}

/// Direct Page Indexed Y: DP + d + Y (bank 0).
pub fn direct_y(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let d = cpu.fetch_byte(bus) as u16;
    let y = if cpu.is_x8() { cpu.y & 0xFF } else { cpu.y };
    Addr { bank: 0, addr: cpu.dp.wrapping_add(d).wrapping_add(y) }
}

/// (Direct): indirect through DP+d, result in DBR.
pub fn direct_indirect(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let base = direct(cpu, bus);
    let lo = bus.read(base.bank, base.addr) as u16;
    let hi = bus.read(base.bank, base.addr.wrapping_add(1)) as u16;
    Addr { bank: cpu.dbr, addr: lo | (hi << 8) }
}

/// (Direct,X): indexed indirect. [DP+d+X] → DBR:ptr.
pub fn direct_x_indirect(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let base = direct_x(cpu, bus);
    let lo = bus.read(base.bank, base.addr) as u16;
    let hi = bus.read(base.bank, base.addr.wrapping_add(1)) as u16;
    Addr { bank: cpu.dbr, addr: lo | (hi << 8) }
}

/// (Direct),Y: indirect indexed. [DP+d] → DBR:ptr + Y.
pub fn direct_indirect_y(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let base = direct(cpu, bus);
    let lo = bus.read(base.bank, base.addr) as u16;
    let hi = bus.read(base.bank, base.addr.wrapping_add(1)) as u16;
    let y = if cpu.is_x8() { cpu.y & 0xFF } else { cpu.y };
    let ptr = (lo | (hi << 8)).wrapping_add(y);
    Addr { bank: cpu.dbr, addr: ptr }
}

/// [Direct]: indirect long. [DP+d] → 3-byte pointer.
pub fn direct_indirect_long(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let base = direct(cpu, bus);
    let lo = bus.read(base.bank, base.addr) as u16;
    let hi = bus.read(base.bank, base.addr.wrapping_add(1)) as u16;
    let bank = bus.read(base.bank, base.addr.wrapping_add(2));
    Addr { bank, addr: lo | (hi << 8) }
}

/// [Direct],Y: indirect long indexed. [DP+d] → 24-bit ptr + Y.
pub fn direct_indirect_long_y(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let base = direct(cpu, bus);
    let lo = bus.read(base.bank, base.addr) as u16;
    let hi = bus.read(base.bank, base.addr.wrapping_add(1)) as u16;
    let bank = bus.read(base.bank, base.addr.wrapping_add(2));
    let y = if cpu.is_x8() { cpu.y & 0xFF } else { cpu.y };
    let ptr = (lo | (hi << 8)).wrapping_add(y);
    // TODO: handle carry into bank byte for cross-bank indexing
    Addr { bank, addr: ptr }
}

// ── Absolute ────────────────────────────────────────────────────────────

/// Absolute: DBR:addr.
pub fn absolute(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let addr = cpu.fetch_word(bus);
    Addr { bank: cpu.dbr, addr }
}

/// Absolute,X: DBR:addr+X.
pub fn absolute_x(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let addr = cpu.fetch_word(bus);
    let x = if cpu.is_x8() { cpu.x & 0xFF } else { cpu.x };
    Addr { bank: cpu.dbr, addr: addr.wrapping_add(x) }
}

/// Absolute,Y: DBR:addr+Y.
pub fn absolute_y(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let addr = cpu.fetch_word(bus);
    let y = if cpu.is_x8() { cpu.y & 0xFF } else { cpu.y };
    Addr { bank: cpu.dbr, addr: addr.wrapping_add(y) }
}

/// (Absolute): JMP indirect. [00:addr] → 16-bit pointer. PBR unchanged.
pub fn absolute_indirect(cpu: &mut Cpu, bus: &mut Bus) -> u16 {
    let addr = cpu.fetch_word(bus);
    let lo = bus.read(0x00, addr) as u16;
    let hi = bus.read(0x00, addr.wrapping_add(1)) as u16;
    lo | (hi << 8)
}

/// (Absolute,X): JMP indexed indirect. [PBR:addr+X] → 16-bit pointer.
pub fn absolute_x_indirect(cpu: &mut Cpu, bus: &mut Bus) -> u16 {
    let addr = cpu.fetch_word(bus);
    let x = if cpu.is_x8() { cpu.x & 0xFF } else { cpu.x };
    let eff = addr.wrapping_add(x);
    let lo = bus.read(cpu.pbr, eff) as u16;
    let hi = bus.read(cpu.pbr, eff.wrapping_add(1)) as u16;
    lo | (hi << 8)
}

/// [Absolute]: JML indirect long. [00:addr] → 24-bit pointer.
pub fn absolute_indirect_long(cpu: &mut Cpu, bus: &mut Bus) -> (u8, u16) {
    let addr = cpu.fetch_word(bus);
    let lo = bus.read(0x00, addr) as u16;
    let hi = bus.read(0x00, addr.wrapping_add(1)) as u16;
    let bank = bus.read(0x00, addr.wrapping_add(2));
    (bank, lo | (hi << 8))
}

// ── Absolute Long ───────────────────────────────────────────────────────

/// Long: bank:addr (3-byte operand).
pub fn long(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let addr = cpu.fetch_word(bus);
    let bank = cpu.fetch_byte(bus);
    Addr { bank, addr }
}

/// Long,X: bank:addr+X.
pub fn long_x(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let addr = cpu.fetch_word(bus);
    let bank = cpu.fetch_byte(bus);
    let x = if cpu.is_x8() { cpu.x & 0xFF } else { cpu.x };
    Addr { bank, addr: addr.wrapping_add(x) }
}

// ── Stack Relative ──────────────────────────────────────────────────────

/// Stack Relative: SP + d (bank 0).
pub fn stack_relative(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let d = cpu.fetch_byte(bus) as u16;
    Addr { bank: 0, addr: cpu.sp.wrapping_add(d) }
}

/// (Stack Relative),Y: [SP+d] → DBR:ptr + Y.
pub fn stack_relative_indirect_y(cpu: &mut Cpu, bus: &mut Bus) -> Addr {
    let base = stack_relative(cpu, bus);
    let lo = bus.read(base.bank, base.addr) as u16;
    let hi = bus.read(base.bank, base.addr.wrapping_add(1)) as u16;
    let y = if cpu.is_x8() { cpu.y & 0xFF } else { cpu.y };
    let ptr = (lo | (hi << 8)).wrapping_add(y);
    Addr { bank: cpu.dbr, addr: ptr }
}

// ── Relative ────────────────────────────────────────────────────────────

/// Relative 8-bit branch offset. Returns the target PC.
pub fn relative8(cpu: &mut Cpu, bus: &mut Bus) -> u16 {
    let offset = cpu.fetch_byte(bus) as i8;
    cpu.pc.wrapping_add(offset as u16)
}

/// Relative 16-bit branch offset (BRL). Returns the target PC.
pub fn relative16(cpu: &mut Cpu, bus: &mut Bus) -> u16 {
    let offset = cpu.fetch_word(bus) as i16;
    cpu.pc.wrapping_add(offset as u16)
}
