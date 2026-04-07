/// SPC700 CPU emulation — the audio processor's brain.
///
/// An 8-bit CPU running at ~1.024 MHz with its own 64KB address space.
/// Similar to the 6502 but with different opcodes and a few 16-bit operations.
/// Registers: A, X, Y (8-bit), SP (8-bit), PC (16-bit), PSW (flags).

use super::ApuBus;

// PSW flag bits.
const C: u8 = 0x01; // Carry
const Z: u8 = 0x02; // Zero
const I: u8 = 0x04; // Interrupt enable
const H: u8 = 0x08; // Half-carry
const B: u8 = 0x10; // Break (not in PSW reads)
const P: u8 = 0x20; // Direct page select (0=$0000, 1=$0100)
const V: u8 = 0x40; // Overflow
const N: u8 = 0x80; // Negative

pub struct Spc700 {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub psw: u8,
    pub halted: bool,
}

impl Spc700 {
    pub fn new() -> Self {
        Self {
            a: 0, x: 0, y: 0,
            sp: 0xEF,
            pc: 0xFFC0, // IPL ROM entry point
            psw: 0x02,  // Z flag set
            halted: false,
        }
    }

    // ─── Helpers ─────────────────────────────────────────

    fn read_pc(&mut self, bus: &mut ApuBus) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn read_pc16(&mut self, bus: &mut ApuBus) -> u16 {
        let lo = self.read_pc(bus) as u16;
        let hi = self.read_pc(bus) as u16;
        lo | (hi << 8)
    }

    /// Direct page address: dp byte + $0100 if P flag is set.
    fn dp(&self, addr: u8) -> u16 {
        (addr as u16) | if self.psw & P != 0 { 0x100 } else { 0 }
    }

    fn push(&mut self, bus: &mut ApuBus, val: u8) {
        bus.write(0x0100 | self.sp as u16, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pop(&mut self, bus: &mut ApuBus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x0100 | self.sp as u16)
    }

    fn push16(&mut self, bus: &mut ApuBus, val: u16) {
        self.push(bus, (val >> 8) as u8);
        self.push(bus, val as u8);
    }

    fn pop16(&mut self, bus: &mut ApuBus) -> u16 {
        let lo = self.pop(bus) as u16;
        let hi = self.pop(bus) as u16;
        lo | (hi << 8)
    }

    fn set_nz(&mut self, val: u8) {
        self.psw = (self.psw & !(N | Z))
            | if val == 0 { Z } else { 0 }
            | (val & 0x80); // N = bit 7
    }

    fn set_nz16(&mut self, val: u16) {
        self.psw = (self.psw & !(N | Z))
            | if val == 0 { Z } else { 0 }
            | if val & 0x8000 != 0 { N } else { 0 };
    }

    // ─── ALU operations ─────────────────────────────────

    fn op_adc(&mut self, a: u8, b: u8) -> u8 {
        let carry = (self.psw & C) as u16;
        let result = a as u16 + b as u16 + carry;
        let r8 = result as u8;
        self.psw = (self.psw & !(C | Z | N | V | H))
            | if result > 0xFF { C } else { 0 }
            | if r8 == 0 { Z } else { 0 }
            | (r8 & 0x80)
            | if (!(a ^ b) & (a ^ r8)) & 0x80 != 0 { V } else { 0 }
            | if ((a & 0x0F) + (b & 0x0F) + carry as u8) > 0x0F { H } else { 0 };
        r8
    }

    fn op_sbc(&mut self, a: u8, b: u8) -> u8 {
        self.op_adc(a, !b)
    }

    fn op_cmp(&mut self, a: u8, b: u8) {
        let result = a as i16 - b as i16;
        self.psw = (self.psw & !(C | Z | N))
            | if result >= 0 { C } else { 0 }
            | if result as u8 == 0 { Z } else { 0 }
            | (result as u8 & 0x80);
    }

    fn op_or(&mut self, a: u8, b: u8) -> u8 { let r = a | b; self.set_nz(r); r }
    fn op_and(&mut self, a: u8, b: u8) -> u8 { let r = a & b; self.set_nz(r); r }
    fn op_eor(&mut self, a: u8, b: u8) -> u8 { let r = a ^ b; self.set_nz(r); r }

    fn op_asl(&mut self, val: u8) -> u8 {
        self.psw = (self.psw & !C) | if val & 0x80 != 0 { C } else { 0 };
        let r = val << 1;
        self.set_nz(r);
        r
    }

    fn op_lsr(&mut self, val: u8) -> u8 {
        self.psw = (self.psw & !C) | (val & 0x01);
        let r = val >> 1;
        self.set_nz(r);
        r
    }

    fn op_rol(&mut self, val: u8) -> u8 {
        let old_c = self.psw & C;
        self.psw = (self.psw & !C) | if val & 0x80 != 0 { C } else { 0 };
        let r = (val << 1) | old_c;
        self.set_nz(r);
        r
    }

    fn op_ror(&mut self, val: u8) -> u8 {
        let old_c = self.psw & C;
        self.psw = (self.psw & !C) | (val & 0x01);
        let r = (val >> 1) | (old_c << 7);
        self.set_nz(r);
        r
    }

    fn branch(&mut self, bus: &mut ApuBus, cond: bool) -> u8 {
        let offset = self.read_pc(bus) as i8;
        if cond {
            self.pc = self.pc.wrapping_add(offset as u16);
            4
        } else {
            2
        }
    }

    // ─── Main step: execute one instruction ─────────────

    pub fn step(&mut self, bus: &mut ApuBus) -> u8 {
        let op = self.read_pc(bus);

        match op {
            // ═══ NOP / Flag operations ═══
            0x00 => 2, // NOP
            0x20 => { self.psw &= !P; 2 }     // CLRP
            0x40 => { self.psw |= P; 2 }       // SETP
            0x60 => { self.psw &= !C; 2 }      // CLRC
            0x80 => { self.psw |= C; 2 }       // SETC
            0xA0 => { self.psw |= I; 3 }       // EI
            0xC0 => { self.psw &= !I; 3 }      // DI
            0xE0 => { self.psw &= !(V | H); 2 } // CLRV
            0xED => { self.psw ^= C; 3 }       // NOTC
            0xEF => { self.halted = true; 3 }   // SLEEP
            0xFF => { self.halted = true; 3 }   // STOP

            // ═══ Branch ═══
            0x10 => self.branch(bus, self.psw & N == 0), // BPL
            0x30 => self.branch(bus, self.psw & N != 0), // BMI
            0x50 => self.branch(bus, self.psw & V == 0), // BVC
            0x70 => self.branch(bus, self.psw & V != 0), // BVS
            0x90 => self.branch(bus, self.psw & C == 0), // BCC
            0xB0 => self.branch(bus, self.psw & C != 0), // BCS
            0xD0 => self.branch(bus, self.psw & Z == 0), // BNE
            0xF0 => self.branch(bus, self.psw & Z != 0), // BEQ
            0x2F => { // BRA (unconditional)
                let offset = self.read_pc(bus) as i8;
                self.pc = self.pc.wrapping_add(offset as u16);
                4
            }

            // ═══ OR A, operand ═══
            0x08 => { let v = self.read_pc(bus); self.a = self.op_or(self.a, v); 2 }
            0x04 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)); self.a = self.op_or(self.a, v); 3 }
            0x05 => { let addr = self.read_pc16(bus); let v = bus.read(addr); self.a = self.op_or(self.a, v); 4 }
            0x06 => { let v = bus.read(self.dp(self.x)); self.a = self.op_or(self.a, v); 3 }
            0x07 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp.wrapping_add(self.x))) as u16 | ((bus.read(self.dp(dp.wrapping_add(self.x).wrapping_add(1))) as u16) << 8); let v = bus.read(addr); self.a = self.op_or(self.a, v); 6 }
            0x14 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp.wrapping_add(self.x))); self.a = self.op_or(self.a, v); 4 }
            0x15 => { let addr = self.read_pc16(bus).wrapping_add(self.x as u16); let v = bus.read(addr); self.a = self.op_or(self.a, v); 5 }
            0x16 => { let addr = self.read_pc16(bus).wrapping_add(self.y as u16); let v = bus.read(addr); self.a = self.op_or(self.a, v); 5 }
            0x17 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp)) as u16 | ((bus.read(self.dp(dp.wrapping_add(1))) as u16) << 8); let v = bus.read(addr.wrapping_add(self.y as u16)); self.a = self.op_or(self.a, v); 6 }
            0x09 => { let ds = self.read_pc(bus); let dd = self.read_pc(bus); let a = bus.read(self.dp(dd)); let b = bus.read(self.dp(ds)); let r = self.op_or(a, b); bus.write(self.dp(dd), r); 6 }
            0x18 => { let imm = self.read_pc(bus); let dp = self.read_pc(bus); let a = bus.read(self.dp(dp)); let r = self.op_or(a, imm); bus.write(self.dp(dp), r); 5 }
            0x19 => { let a = bus.read(self.dp(self.x)); let b = bus.read(self.dp(self.y)); let r = self.op_or(a, b); bus.write(self.dp(self.x), r); 5 }

            // ═══ AND A, operand ═══
            0x28 => { let v = self.read_pc(bus); self.a = self.op_and(self.a, v); 2 }
            0x24 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)); self.a = self.op_and(self.a, v); 3 }
            0x25 => { let addr = self.read_pc16(bus); let v = bus.read(addr); self.a = self.op_and(self.a, v); 4 }
            0x26 => { let v = bus.read(self.dp(self.x)); self.a = self.op_and(self.a, v); 3 }
            0x27 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp.wrapping_add(self.x))) as u16 | ((bus.read(self.dp(dp.wrapping_add(self.x).wrapping_add(1))) as u16) << 8); let v = bus.read(addr); self.a = self.op_and(self.a, v); 6 }
            0x34 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp.wrapping_add(self.x))); self.a = self.op_and(self.a, v); 4 }
            0x35 => { let addr = self.read_pc16(bus).wrapping_add(self.x as u16); let v = bus.read(addr); self.a = self.op_and(self.a, v); 5 }
            0x36 => { let addr = self.read_pc16(bus).wrapping_add(self.y as u16); let v = bus.read(addr); self.a = self.op_and(self.a, v); 5 }
            0x37 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp)) as u16 | ((bus.read(self.dp(dp.wrapping_add(1))) as u16) << 8); let v = bus.read(addr.wrapping_add(self.y as u16)); self.a = self.op_and(self.a, v); 6 }
            0x29 => { let ds = self.read_pc(bus); let dd = self.read_pc(bus); let a = bus.read(self.dp(dd)); let b = bus.read(self.dp(ds)); let r = self.op_and(a, b); bus.write(self.dp(dd), r); 6 }
            0x38 => { let imm = self.read_pc(bus); let dp = self.read_pc(bus); let a = bus.read(self.dp(dp)); let r = self.op_and(a, imm); bus.write(self.dp(dp), r); 5 }
            0x39 => { let a = bus.read(self.dp(self.x)); let b = bus.read(self.dp(self.y)); let r = self.op_and(a, b); bus.write(self.dp(self.x), r); 5 }

            // ═══ EOR A, operand ═══
            0x48 => { let v = self.read_pc(bus); self.a = self.op_eor(self.a, v); 2 }
            0x44 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)); self.a = self.op_eor(self.a, v); 3 }
            0x45 => { let addr = self.read_pc16(bus); let v = bus.read(addr); self.a = self.op_eor(self.a, v); 4 }
            0x46 => { let v = bus.read(self.dp(self.x)); self.a = self.op_eor(self.a, v); 3 }
            0x47 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp.wrapping_add(self.x))) as u16 | ((bus.read(self.dp(dp.wrapping_add(self.x).wrapping_add(1))) as u16) << 8); let v = bus.read(addr); self.a = self.op_eor(self.a, v); 6 }
            0x54 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp.wrapping_add(self.x))); self.a = self.op_eor(self.a, v); 4 }
            0x55 => { let addr = self.read_pc16(bus).wrapping_add(self.x as u16); let v = bus.read(addr); self.a = self.op_eor(self.a, v); 5 }
            0x56 => { let addr = self.read_pc16(bus).wrapping_add(self.y as u16); let v = bus.read(addr); self.a = self.op_eor(self.a, v); 5 }
            0x57 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp)) as u16 | ((bus.read(self.dp(dp.wrapping_add(1))) as u16) << 8); let v = bus.read(addr.wrapping_add(self.y as u16)); self.a = self.op_eor(self.a, v); 6 }
            0x49 => { let ds = self.read_pc(bus); let dd = self.read_pc(bus); let a = bus.read(self.dp(dd)); let b = bus.read(self.dp(ds)); let r = self.op_eor(a, b); bus.write(self.dp(dd), r); 6 }
            0x58 => { let imm = self.read_pc(bus); let dp = self.read_pc(bus); let a = bus.read(self.dp(dp)); let r = self.op_eor(a, imm); bus.write(self.dp(dp), r); 5 }
            0x59 => { let a = bus.read(self.dp(self.x)); let b = bus.read(self.dp(self.y)); let r = self.op_eor(a, b); bus.write(self.dp(self.x), r); 5 }

            // ═══ CMP ═══
            0x68 => { let v = self.read_pc(bus); self.op_cmp(self.a, v); 2 }
            0x64 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)); self.op_cmp(self.a, v); 3 }
            0x65 => { let addr = self.read_pc16(bus); let v = bus.read(addr); self.op_cmp(self.a, v); 4 }
            0x66 => { let v = bus.read(self.dp(self.x)); self.op_cmp(self.a, v); 3 }
            0x67 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp.wrapping_add(self.x))) as u16 | ((bus.read(self.dp(dp.wrapping_add(self.x).wrapping_add(1))) as u16) << 8); let v = bus.read(addr); self.op_cmp(self.a, v); 6 }
            0x74 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp.wrapping_add(self.x))); self.op_cmp(self.a, v); 4 }
            0x75 => { let addr = self.read_pc16(bus).wrapping_add(self.x as u16); let v = bus.read(addr); self.op_cmp(self.a, v); 5 }
            0x76 => { let addr = self.read_pc16(bus).wrapping_add(self.y as u16); let v = bus.read(addr); self.op_cmp(self.a, v); 5 }
            0x77 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp)) as u16 | ((bus.read(self.dp(dp.wrapping_add(1))) as u16) << 8); let v = bus.read(addr.wrapping_add(self.y as u16)); self.op_cmp(self.a, v); 6 }
            0x69 => { let ds = self.read_pc(bus); let dd = self.read_pc(bus); let a = bus.read(self.dp(dd)); let b = bus.read(self.dp(ds)); self.op_cmp(a, b); 6 }
            0x78 => { let imm = self.read_pc(bus); let dp = self.read_pc(bus); let a = bus.read(self.dp(dp)); self.op_cmp(a, imm); 5 }
            0x79 => { let a = bus.read(self.dp(self.x)); let b = bus.read(self.dp(self.y)); self.op_cmp(a, b); 5 }
            // CMP X
            0xC8 => { let v = self.read_pc(bus); self.op_cmp(self.x, v); 2 }
            0x3E => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)); self.op_cmp(self.x, v); 3 }
            0x1E => { let addr = self.read_pc16(bus); let v = bus.read(addr); self.op_cmp(self.x, v); 4 }
            // CMP Y
            0xAD => { let v = self.read_pc(bus); self.op_cmp(self.y, v); 2 }
            0x7E => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)); self.op_cmp(self.y, v); 3 }
            0x5E => { let addr = self.read_pc16(bus); let v = bus.read(addr); self.op_cmp(self.y, v); 4 }

            // ═══ ADC ═══
            0x88 => { let v = self.read_pc(bus); self.a = self.op_adc(self.a, v); 2 }
            0x84 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)); self.a = self.op_adc(self.a, v); 3 }
            0x85 => { let addr = self.read_pc16(bus); let v = bus.read(addr); self.a = self.op_adc(self.a, v); 4 }
            0x86 => { let v = bus.read(self.dp(self.x)); self.a = self.op_adc(self.a, v); 3 }
            0x87 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp.wrapping_add(self.x))) as u16 | ((bus.read(self.dp(dp.wrapping_add(self.x).wrapping_add(1))) as u16) << 8); let v = bus.read(addr); self.a = self.op_adc(self.a, v); 6 }
            0x94 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp.wrapping_add(self.x))); self.a = self.op_adc(self.a, v); 4 }
            0x95 => { let addr = self.read_pc16(bus).wrapping_add(self.x as u16); let v = bus.read(addr); self.a = self.op_adc(self.a, v); 5 }
            0x96 => { let addr = self.read_pc16(bus).wrapping_add(self.y as u16); let v = bus.read(addr); self.a = self.op_adc(self.a, v); 5 }
            0x97 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp)) as u16 | ((bus.read(self.dp(dp.wrapping_add(1))) as u16) << 8); let v = bus.read(addr.wrapping_add(self.y as u16)); self.a = self.op_adc(self.a, v); 6 }
            0x89 => { let ds = self.read_pc(bus); let dd = self.read_pc(bus); let a = bus.read(self.dp(dd)); let b = bus.read(self.dp(ds)); let r = self.op_adc(a, b); bus.write(self.dp(dd), r); 6 }
            0x98 => { let imm = self.read_pc(bus); let dp = self.read_pc(bus); let a = bus.read(self.dp(dp)); let r = self.op_adc(a, imm); bus.write(self.dp(dp), r); 5 }
            0x99 => { let a = bus.read(self.dp(self.x)); let b = bus.read(self.dp(self.y)); let r = self.op_adc(a, b); bus.write(self.dp(self.x), r); 5 }

            // ═══ SBC ═══
            0xA8 => { let v = self.read_pc(bus); self.a = self.op_sbc(self.a, v); 2 }
            0xA4 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)); self.a = self.op_sbc(self.a, v); 3 }
            0xA5 => { let addr = self.read_pc16(bus); let v = bus.read(addr); self.a = self.op_sbc(self.a, v); 4 }
            0xA6 => { let v = bus.read(self.dp(self.x)); self.a = self.op_sbc(self.a, v); 3 }
            0xA7 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp.wrapping_add(self.x))) as u16 | ((bus.read(self.dp(dp.wrapping_add(self.x).wrapping_add(1))) as u16) << 8); let v = bus.read(addr); self.a = self.op_sbc(self.a, v); 6 }
            0xB4 => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp.wrapping_add(self.x))); self.a = self.op_sbc(self.a, v); 4 }
            0xB5 => { let addr = self.read_pc16(bus).wrapping_add(self.x as u16); let v = bus.read(addr); self.a = self.op_sbc(self.a, v); 5 }
            0xB6 => { let addr = self.read_pc16(bus).wrapping_add(self.y as u16); let v = bus.read(addr); self.a = self.op_sbc(self.a, v); 5 }
            0xB7 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp)) as u16 | ((bus.read(self.dp(dp.wrapping_add(1))) as u16) << 8); let v = bus.read(addr.wrapping_add(self.y as u16)); self.a = self.op_sbc(self.a, v); 6 }
            0xA9 => { let ds = self.read_pc(bus); let dd = self.read_pc(bus); let a = bus.read(self.dp(dd)); let b = bus.read(self.dp(ds)); let r = self.op_sbc(a, b); bus.write(self.dp(dd), r); 6 }
            0xB8 => { let imm = self.read_pc(bus); let dp = self.read_pc(bus); let a = bus.read(self.dp(dp)); let r = self.op_sbc(a, imm); bus.write(self.dp(dp), r); 5 }
            0xB9 => { let a = bus.read(self.dp(self.x)); let b = bus.read(self.dp(self.y)); let r = self.op_sbc(a, b); bus.write(self.dp(self.x), r); 5 }

            // ═══ MOV (loads to A) ═══
            0xE8 => { self.a = self.read_pc(bus); self.set_nz(self.a); 2 } // MOV A, #imm
            0xE4 => { let dp = self.read_pc(bus); self.a = bus.read(self.dp(dp)); self.set_nz(self.a); 3 }
            0xE5 => { let addr = self.read_pc16(bus); self.a = bus.read(addr); self.set_nz(self.a); 4 }
            0xE6 => { self.a = bus.read(self.dp(self.x)); self.set_nz(self.a); 3 }
            0xE7 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp.wrapping_add(self.x))) as u16 | ((bus.read(self.dp(dp.wrapping_add(self.x).wrapping_add(1))) as u16) << 8); self.a = bus.read(addr); self.set_nz(self.a); 6 }
            0xF4 => { let dp = self.read_pc(bus); self.a = bus.read(self.dp(dp.wrapping_add(self.x))); self.set_nz(self.a); 4 }
            0xF5 => { let addr = self.read_pc16(bus).wrapping_add(self.x as u16); self.a = bus.read(addr); self.set_nz(self.a); 5 }
            0xF6 => { let addr = self.read_pc16(bus).wrapping_add(self.y as u16); self.a = bus.read(addr); self.set_nz(self.a); 5 }
            0xF7 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp)) as u16 | ((bus.read(self.dp(dp.wrapping_add(1))) as u16) << 8); self.a = bus.read(addr.wrapping_add(self.y as u16)); self.set_nz(self.a); 6 }
            0xBF => { self.a = bus.read(self.dp(self.x)); self.set_nz(self.a); self.x = self.x.wrapping_add(1); 4 } // MOV A, (X)+

            // ═══ MOV (stores from A) ═══
            0xC4 => { let dp = self.read_pc(bus); bus.write(self.dp(dp), self.a); 4 }
            0xC5 => { let addr = self.read_pc16(bus); bus.write(addr, self.a); 5 }
            0xC6 => { bus.write(self.dp(self.x), self.a); 4 }
            0xC7 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp.wrapping_add(self.x))) as u16 | ((bus.read(self.dp(dp.wrapping_add(self.x).wrapping_add(1))) as u16) << 8); bus.write(addr, self.a); 7 }
            0xD4 => { let dp = self.read_pc(bus); bus.write(self.dp(dp.wrapping_add(self.x)), self.a); 5 }
            0xD5 => { let addr = self.read_pc16(bus).wrapping_add(self.x as u16); bus.write(addr, self.a); 6 }
            0xD6 => { let addr = self.read_pc16(bus).wrapping_add(self.y as u16); bus.write(addr, self.a); 6 }
            0xD7 => { let dp = self.read_pc(bus); let addr = bus.read(self.dp(dp)) as u16 | ((bus.read(self.dp(dp.wrapping_add(1))) as u16) << 8); bus.write(addr.wrapping_add(self.y as u16), self.a); 7 }
            0xAF => { bus.write(self.dp(self.x), self.a); self.x = self.x.wrapping_add(1); 4 } // MOV (X)+, A

            // ═══ MOV X ═══
            0xCD => { self.x = self.read_pc(bus); self.set_nz(self.x); 2 } // MOV X, #imm
            0xF8 => { let dp = self.read_pc(bus); self.x = bus.read(self.dp(dp)); self.set_nz(self.x); 3 }
            0xE9 => { let addr = self.read_pc16(bus); self.x = bus.read(addr); self.set_nz(self.x); 4 }
            0xF9 => { let dp = self.read_pc(bus); self.x = bus.read(self.dp(dp.wrapping_add(self.y))); self.set_nz(self.x); 4 }
            0xD8 => { let dp = self.read_pc(bus); bus.write(self.dp(dp), self.x); 4 } // MOV dp, X
            0xC9 => { let addr = self.read_pc16(bus); bus.write(addr, self.x); 5 } // MOV !abs, X
            0xD9 => { let dp = self.read_pc(bus); bus.write(self.dp(dp.wrapping_add(self.y)), self.x); 5 } // MOV dp+Y, X

            // ═══ MOV Y ═══
            0x8D => { self.y = self.read_pc(bus); self.set_nz(self.y); 2 } // MOV Y, #imm
            0xEB => { let dp = self.read_pc(bus); self.y = bus.read(self.dp(dp)); self.set_nz(self.y); 3 }
            0xEC => { let addr = self.read_pc16(bus); self.y = bus.read(addr); self.set_nz(self.y); 4 }
            0xFB => { let dp = self.read_pc(bus); self.y = bus.read(self.dp(dp.wrapping_add(self.x))); self.set_nz(self.y); 4 }
            0xCB => { let dp = self.read_pc(bus); bus.write(self.dp(dp), self.y); 4 } // MOV dp, Y
            0xCC => { let addr = self.read_pc16(bus); bus.write(addr, self.y); 5 } // MOV !abs, Y
            0xDB => { let dp = self.read_pc(bus); bus.write(self.dp(dp.wrapping_add(self.x)), self.y); 5 } // MOV dp+X, Y

            // ═══ MOV register-to-register ═══
            0x7D => { self.a = self.x; self.set_nz(self.a); 2 } // MOV A, X
            0xDD => { self.a = self.y; self.set_nz(self.a); 2 } // MOV A, Y
            0x5D => { self.x = self.a; self.set_nz(self.x); 2 } // MOV X, A
            0xFD => { self.y = self.a; self.set_nz(self.y); 2 } // MOV Y, A
            0x9D => { self.x = self.sp; self.set_nz(self.x); 2 } // MOV X, SP
            0xBD => { self.sp = self.x; 2 } // MOV SP, X

            // ═══ MOV dp, dp / dp, #imm ═══
            0xFA => { let src = self.read_pc(bus); let dst = self.read_pc(bus); let v = bus.read(self.dp(src)); bus.write(self.dp(dst), v); 5 } // MOV dp, dp
            0x8F => { let imm = self.read_pc(bus); let dp = self.read_pc(bus); bus.write(self.dp(dp), imm); 5 } // MOV dp, #imm

            // ═══ INC / DEC ═══
            0xBC => { self.a = self.a.wrapping_add(1); self.set_nz(self.a); 2 } // INC A
            0x3D => { self.x = self.x.wrapping_add(1); self.set_nz(self.x); 2 } // INC X
            0xFC => { self.y = self.y.wrapping_add(1); self.set_nz(self.y); 2 } // INC Y
            0xAB => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)).wrapping_add(1); self.set_nz(v); bus.write(self.dp(dp), v); 4 } // INC dp
            0xAC => { let addr = self.read_pc16(bus); let v = bus.read(addr).wrapping_add(1); self.set_nz(v); bus.write(addr, v); 5 } // INC !abs
            0xBB => { let dp = self.read_pc(bus); let addr = self.dp(dp.wrapping_add(self.x)); let v = bus.read(addr).wrapping_add(1); self.set_nz(v); bus.write(addr, v); 5 } // INC dp+X

            0x9C => { self.a = self.a.wrapping_sub(1); self.set_nz(self.a); 2 } // DEC A
            0x1D => { self.x = self.x.wrapping_sub(1); self.set_nz(self.x); 2 } // DEC X
            0xDC => { self.y = self.y.wrapping_sub(1); self.set_nz(self.y); 2 } // DEC Y
            0x8B => { let dp = self.read_pc(bus); let v = bus.read(self.dp(dp)).wrapping_sub(1); self.set_nz(v); bus.write(self.dp(dp), v); 4 } // DEC dp
            0x8C => { let addr = self.read_pc16(bus); let v = bus.read(addr).wrapping_sub(1); self.set_nz(v); bus.write(addr, v); 5 } // DEC !abs
            0x9B => { let dp = self.read_pc(bus); let addr = self.dp(dp.wrapping_add(self.x)); let v = bus.read(addr).wrapping_sub(1); self.set_nz(v); bus.write(addr, v); 5 } // DEC dp+X

            // ═══ Shift / Rotate ═══
            0x1C => { self.a = self.op_asl(self.a); 2 } // ASL A
            0x0B => { let dp = self.read_pc(bus); let v = self.op_asl(bus.read(self.dp(dp))); bus.write(self.dp(dp), v); 4 }
            0x0C => { let addr = self.read_pc16(bus); let v = self.op_asl(bus.read(addr)); bus.write(addr, v); 5 }
            0x1B => { let dp = self.read_pc(bus); let addr = self.dp(dp.wrapping_add(self.x)); let v = self.op_asl(bus.read(addr)); bus.write(addr, v); 5 }

            0x5C => { self.a = self.op_lsr(self.a); 2 } // LSR A
            0x4B => { let dp = self.read_pc(bus); let v = self.op_lsr(bus.read(self.dp(dp))); bus.write(self.dp(dp), v); 4 }
            0x4C => { let addr = self.read_pc16(bus); let v = self.op_lsr(bus.read(addr)); bus.write(addr, v); 5 }
            0x5B => { let dp = self.read_pc(bus); let addr = self.dp(dp.wrapping_add(self.x)); let v = self.op_lsr(bus.read(addr)); bus.write(addr, v); 5 }

            0x3C => { self.a = self.op_rol(self.a); 2 } // ROL A
            0x2B => { let dp = self.read_pc(bus); let v = self.op_rol(bus.read(self.dp(dp))); bus.write(self.dp(dp), v); 4 }
            0x2C => { let addr = self.read_pc16(bus); let v = self.op_rol(bus.read(addr)); bus.write(addr, v); 5 }
            0x3B => { let dp = self.read_pc(bus); let addr = self.dp(dp.wrapping_add(self.x)); let v = self.op_rol(bus.read(addr)); bus.write(addr, v); 5 }

            0x7C => { self.a = self.op_ror(self.a); 2 } // ROR A
            0x6B => { let dp = self.read_pc(bus); let v = self.op_ror(bus.read(self.dp(dp))); bus.write(self.dp(dp), v); 4 }
            0x6C => { let addr = self.read_pc16(bus); let v = self.op_ror(bus.read(addr)); bus.write(addr, v); 5 }
            0x7B => { let dp = self.read_pc(bus); let addr = self.dp(dp.wrapping_add(self.x)); let v = self.op_ror(bus.read(addr)); bus.write(addr, v); 5 }

            // ═══ 16-bit operations ═══
            0xBA => { // MOVW YA, dp
                let dp = self.read_pc(bus);
                self.a = bus.read(self.dp(dp));
                self.y = bus.read(self.dp(dp.wrapping_add(1)));
                let ya = (self.y as u16) << 8 | self.a as u16;
                self.set_nz16(ya);
                5
            }
            0xDA => { // MOVW dp, YA
                let dp = self.read_pc(bus);
                bus.write(self.dp(dp), self.a);
                bus.write(self.dp(dp.wrapping_add(1)), self.y);
                4
            }
            0x3A => { // INCW dp
                let dp = self.read_pc(bus);
                let lo = bus.read(self.dp(dp));
                let hi = bus.read(self.dp(dp.wrapping_add(1)));
                let val = ((hi as u16) << 8 | lo as u16).wrapping_add(1);
                bus.write(self.dp(dp), val as u8);
                bus.write(self.dp(dp.wrapping_add(1)), (val >> 8) as u8);
                self.set_nz16(val);
                6
            }
            0x1A => { // DECW dp
                let dp = self.read_pc(bus);
                let lo = bus.read(self.dp(dp));
                let hi = bus.read(self.dp(dp.wrapping_add(1)));
                let val = ((hi as u16) << 8 | lo as u16).wrapping_sub(1);
                bus.write(self.dp(dp), val as u8);
                bus.write(self.dp(dp.wrapping_add(1)), (val >> 8) as u8);
                self.set_nz16(val);
                6
            }
            0x5A => { // CMPW YA, dp
                let dp = self.read_pc(bus);
                let lo = bus.read(self.dp(dp)) as u16;
                let hi = bus.read(self.dp(dp.wrapping_add(1))) as u16;
                let mem = hi << 8 | lo;
                let ya = (self.y as u16) << 8 | self.a as u16;
                let result = ya as i32 - mem as i32;
                self.psw = (self.psw & !(C | Z | N))
                    | if result >= 0 { C } else { 0 }
                    | if result as u16 == 0 { Z } else { 0 }
                    | if result as u16 & 0x8000 != 0 { N } else { 0 };
                4
            }
            0x7A => { // ADDW YA, dp
                let dp = self.read_pc(bus);
                let lo = bus.read(self.dp(dp)) as u16;
                let hi = bus.read(self.dp(dp.wrapping_add(1))) as u16;
                let mem = hi << 8 | lo;
                let ya = (self.y as u16) << 8 | self.a as u16;
                let result = ya as u32 + mem as u32;
                let r16 = result as u16;
                self.a = r16 as u8;
                self.y = (r16 >> 8) as u8;
                self.psw = (self.psw & !(C | Z | N | V | H))
                    | if result > 0xFFFF { C } else { 0 }
                    | if r16 == 0 { Z } else { 0 }
                    | if r16 & 0x8000 != 0 { N } else { 0 }
                    | if (!(ya ^ mem) & (ya ^ r16)) & 0x8000 != 0 { V } else { 0 }
                    | if ((ya ^ mem ^ r16) & 0x1000) != 0 { H } else { 0 };
                5
            }
            0x9A => { // SUBW YA, dp
                let dp = self.read_pc(bus);
                let lo = bus.read(self.dp(dp)) as u16;
                let hi = bus.read(self.dp(dp.wrapping_add(1))) as u16;
                let mem = hi << 8 | lo;
                let ya = (self.y as u16) << 8 | self.a as u16;
                let result = ya as i32 - mem as i32;
                let r16 = result as u16;
                self.a = r16 as u8;
                self.y = (r16 >> 8) as u8;
                self.psw = (self.psw & !(C | Z | N | V | H))
                    | if result >= 0 { C } else { 0 }
                    | if r16 == 0 { Z } else { 0 }
                    | if r16 & 0x8000 != 0 { N } else { 0 }
                    | if ((ya ^ mem) & (ya ^ r16)) & 0x8000 != 0 { V } else { 0 }
                    | if ((ya ^ mem ^ r16) & 0x1000) == 0 { H } else { 0 };
                5
            }

            // ═══ Stack ═══
            0x2D => { self.push(bus, self.a); 4 } // PUSH A
            0x4D => { self.push(bus, self.x); 4 } // PUSH X
            0x6D => { self.push(bus, self.y); 4 } // PUSH Y
            0x0D => { self.push(bus, self.psw); 4 } // PUSH PSW
            0xAE => { self.a = self.pop(bus); self.set_nz(self.a); 4 } // POP A
            0xCE => { self.x = self.pop(bus); self.set_nz(self.x); 4 } // POP X
            0xEE => { self.y = self.pop(bus); self.set_nz(self.y); 4 } // POP Y
            0x8E => { self.psw = self.pop(bus); 4 } // POP PSW

            // ═══ CALL / RET ═══
            0x3F => { // CALL !abs
                let addr = self.read_pc16(bus);
                self.push16(bus, self.pc);
                self.pc = addr;
                8
            }
            0x6F => { // RET
                self.pc = self.pop16(bus);
                5
            }
            0x7F => { // RETI
                self.psw = self.pop(bus);
                self.pc = self.pop16(bus);
                6
            }
            0x4F => { // PCALL $xx (push PC, jump to $FFxx)
                let offset = self.read_pc(bus);
                self.push16(bus, self.pc);
                self.pc = 0xFF00 | offset as u16;
                6
            }
            0x0F => { // BRK
                self.push16(bus, self.pc);
                self.push(bus, self.psw);
                self.psw = (self.psw & !I) | B;
                self.pc = bus.read(0xFFDE) as u16 | ((bus.read(0xFFDF) as u16) << 8);
                8
            }

            // ═══ TCALL (table call, 16 entries) ═══
            0x01 | 0x11 | 0x21 | 0x31 | 0x41 | 0x51 | 0x61 | 0x71 |
            0x81 | 0x91 | 0xA1 | 0xB1 | 0xC1 | 0xD1 | 0xE1 | 0xF1 => {
                let n = (op >> 4) as u16;
                let addr = 0xFFDE - n * 2;
                self.push16(bus, self.pc);
                self.pc = bus.read(addr) as u16 | ((bus.read(addr + 1) as u16) << 8);
                8
            }

            // ═══ JMP ═══
            0x5F => { self.pc = self.read_pc16(bus); 3 } // JMP !abs
            0x1F => { // JMP [!abs+X]
                let base = self.read_pc16(bus).wrapping_add(self.x as u16);
                self.pc = bus.read(base) as u16 | ((bus.read(base.wrapping_add(1)) as u16) << 8);
                6
            }

            // ═══ SET1 / CLR1 (bit operations on direct page) ═══
            0x02 | 0x22 | 0x42 | 0x62 | 0x82 | 0xA2 | 0xC2 | 0xE2 => {
                let dp = self.read_pc(bus);
                let bit = (op >> 5) as u8;
                let addr = self.dp(dp);
                let v = bus.read(addr) | (1 << bit);
                bus.write(addr, v);
                4
            }
            0x12 | 0x32 | 0x52 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                let dp = self.read_pc(bus);
                let bit = (op >> 5) as u8;
                let addr = self.dp(dp);
                let v = bus.read(addr) & !(1 << bit);
                bus.write(addr, v);
                4
            }

            // ═══ BBS / BBC (branch if bit set/clear) ═══
            0x03 | 0x23 | 0x43 | 0x63 | 0x83 | 0xA3 | 0xC3 | 0xE3 => {
                let dp = self.read_pc(bus);
                let rel = self.read_pc(bus) as i8;
                let bit = (op >> 5) as u8;
                if bus.read(self.dp(dp)) & (1 << bit) != 0 {
                    self.pc = self.pc.wrapping_add(rel as u16);
                    7
                } else { 5 }
            }
            0x13 | 0x33 | 0x53 | 0x73 | 0x93 | 0xB3 | 0xD3 | 0xF3 => {
                let dp = self.read_pc(bus);
                let rel = self.read_pc(bus) as i8;
                let bit = (op >> 5) as u8;
                if bus.read(self.dp(dp)) & (1 << bit) == 0 {
                    self.pc = self.pc.wrapping_add(rel as u16);
                    7
                } else { 5 }
            }

            // ═══ CBNE (compare and branch if not equal) ═══
            0x2E => { // CBNE dp, rel
                let dp = self.read_pc(bus);
                let rel = self.read_pc(bus) as i8;
                if self.a != bus.read(self.dp(dp)) {
                    self.pc = self.pc.wrapping_add(rel as u16);
                    7
                } else { 5 }
            }
            0xDE => { // CBNE dp+X, rel
                let dp = self.read_pc(bus);
                let rel = self.read_pc(bus) as i8;
                if self.a != bus.read(self.dp(dp.wrapping_add(self.x))) {
                    self.pc = self.pc.wrapping_add(rel as u16);
                    8
                } else { 6 }
            }

            // ═══ DBNZ (decrement and branch if not zero) ═══
            0x6E => { // DBNZ dp, rel
                let dp = self.read_pc(bus);
                let rel = self.read_pc(bus) as i8;
                let addr = self.dp(dp);
                let v = bus.read(addr).wrapping_sub(1);
                bus.write(addr, v);
                if v != 0 {
                    self.pc = self.pc.wrapping_add(rel as u16);
                    7
                } else { 5 }
            }
            0xFE => { // DBNZ Y, rel
                let rel = self.read_pc(bus) as i8;
                self.y = self.y.wrapping_sub(1);
                if self.y != 0 {
                    self.pc = self.pc.wrapping_add(rel as u16);
                    6
                } else { 4 }
            }

            // ═══ TSET1 / TCLR1 ═══
            0x0E => { // TSET1 !abs
                let addr = self.read_pc16(bus);
                let v = bus.read(addr);
                self.set_nz(self.a.wrapping_sub(v)); // Set N/Z from A-v (but don't store)
                bus.write(addr, v | self.a);
                6
            }
            0x4E => { // TCLR1 !abs
                let addr = self.read_pc16(bus);
                let v = bus.read(addr);
                self.set_nz(self.a.wrapping_sub(v));
                bus.write(addr, v & !self.a);
                6
            }

            // ═══ MOV1 (bit transfer) ═══
            0xAA => { // MOV1 C, mem.bit
                let operand = self.read_pc16(bus);
                let addr = operand & 0x1FFF;
                let bit = (operand >> 13) as u8;
                let v = bus.read(addr);
                self.psw = (self.psw & !C) | if v & (1 << bit) != 0 { C } else { 0 };
                4
            }
            0xCA => { // MOV1 mem.bit, C
                let operand = self.read_pc16(bus);
                let addr = operand & 0x1FFF;
                let bit = (operand >> 13) as u8;
                let mut v = bus.read(addr);
                if self.psw & C != 0 { v |= 1 << bit; } else { v &= !(1 << bit); }
                bus.write(addr, v);
                6
            }

            // ═══ NOT1 / OR1 / AND1 / EOR1 ═══
            0xEA => { // NOT1 mem.bit
                let operand = self.read_pc16(bus);
                let addr = operand & 0x1FFF;
                let bit = (operand >> 13) as u8;
                let v = bus.read(addr) ^ (1 << bit);
                bus.write(addr, v);
                5
            }
            0x0A => { // OR1 C, mem.bit
                let operand = self.read_pc16(bus);
                let addr = operand & 0x1FFF;
                let bit = (operand >> 13) as u8;
                if bus.read(addr) & (1 << bit) != 0 { self.psw |= C; }
                5
            }
            0x2A => { // OR1 C, /mem.bit
                let operand = self.read_pc16(bus);
                let addr = operand & 0x1FFF;
                let bit = (operand >> 13) as u8;
                if bus.read(addr) & (1 << bit) == 0 { self.psw |= C; }
                5
            }
            0x4A => { // AND1 C, mem.bit
                let operand = self.read_pc16(bus);
                let addr = operand & 0x1FFF;
                let bit = (operand >> 13) as u8;
                if bus.read(addr) & (1 << bit) == 0 { self.psw &= !C; }
                4
            }
            0x6A => { // AND1 C, /mem.bit
                let operand = self.read_pc16(bus);
                let addr = operand & 0x1FFF;
                let bit = (operand >> 13) as u8;
                if bus.read(addr) & (1 << bit) != 0 { self.psw &= !C; }
                4
            }
            0x8A => { // EOR1 C, mem.bit
                let operand = self.read_pc16(bus);
                let addr = operand & 0x1FFF;
                let bit = (operand >> 13) as u8;
                if bus.read(addr) & (1 << bit) != 0 { self.psw ^= C; }
                5
            }

            // ═══ MUL / DIV / DAA / DAS ═══
            0xCF => { // MUL YA
                let result = self.y as u16 * self.a as u16;
                self.a = result as u8;
                self.y = (result >> 8) as u8;
                self.set_nz(self.y);
                9
            }
            0x9E => { // DIV YA, X
                let ya = (self.y as u16) << 8 | self.a as u16;
                if self.x == 0 {
                    self.psw |= V;
                    self.a = 0xFF;
                    self.y = 0xFF;
                } else {
                    let q = ya / self.x as u16;
                    let r = ya % self.x as u16;
                    self.psw = (self.psw & !V) | if q > 0xFF { V } else { 0 };
                    self.a = q as u8;
                    self.y = r as u8;
                }
                self.set_nz(self.a);
                12
            }
            0xDF => { // DAA A
                if self.psw & C != 0 || self.a > 0x99 {
                    self.a = self.a.wrapping_add(0x60);
                    self.psw |= C;
                }
                if self.psw & H != 0 || (self.a & 0x0F) > 0x09 {
                    self.a = self.a.wrapping_add(0x06);
                }
                self.set_nz(self.a);
                3
            }
            0xBE => { // DAS A
                if self.psw & C == 0 || self.a > 0x99 {
                    self.a = self.a.wrapping_sub(0x60);
                    self.psw &= !C;
                }
                if self.psw & H == 0 || (self.a & 0x0F) > 0x09 {
                    self.a = self.a.wrapping_sub(0x06);
                }
                self.set_nz(self.a);
                3
            }

            // ═══ XCN (exchange nibbles) ═══
            0x9F => {
                self.a = (self.a >> 4) | (self.a << 4);
                self.set_nz(self.a);
                5
            }

            // Catch unimplemented opcodes
            _ => {
                // Unimplemented opcode — treat as NOP to avoid hangs.
                // In debug builds this could log a warning.
                #[cfg(debug_assertions)]
                eprintln!("SPC700: unimplemented opcode ${:02X} at PC=${:04X}", op, self.pc.wrapping_sub(1));
                2
            }
        }
    }
}
