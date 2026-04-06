/// 65C816 instruction execution — all 256 opcodes.
///
/// Each opcode handler reads operands, performs the operation, updates flags,
/// and returns the cycle count. The width of immediate operands and ALU
/// operations depends on the M flag (accumulator) or X flag (index registers).

use super::addressing::{self as addr, Addr};
use super::tables::OPCODE_CYCLES;
use super::Cpu;
use crate::bus::Bus;

/// Resolve addressing mode into a local, then call the helper.
/// This avoids borrowing `cpu` twice (once for addr resolution, once for helper).
macro_rules! op {
    ($fn:ident, $addr:expr, $cpu:expr, $bus:expr, $cy:expr) => {{
        let a = $addr;
        $fn($cpu, $bus, a);
        $cy
    }};
}

/// Execute a single opcode. Returns CPU cycle count.
pub fn execute(cpu: &mut Cpu, bus: &mut Bus, opcode: u8) -> u8 {
    let base_cycles = OPCODE_CYCLES[opcode as usize];

    match opcode {
        // ════════════════════════════════════════════════════════════════
        // LDA — Load Accumulator
        // ════════════════════════════════════════════════════════════════
        0xA9 => { // LDA #imm
            if cpu.is_m8() {
                let val = addr::immediate8(cpu, bus);
                cpu.a = (cpu.a & 0xFF00) | val as u16;
                cpu.update_nz8(val);
            } else {
                let val = addr::immediate16(cpu, bus);
                cpu.a = val;
                cpu.update_nz16(val);
            }
            base_cycles
        }
        0xA5 => { op!(lda, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0xB5 => { op!(lda, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0xAD => { op!(lda, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0xBD => { op!(lda, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }
        0xB9 => { op!(lda, addr::absolute_y(cpu, bus), cpu, bus, base_cycles) }
        0xA1 => { op!(lda, addr::direct_x_indirect(cpu, bus), cpu, bus, base_cycles) }
        0xB1 => { op!(lda, addr::direct_indirect_y(cpu, bus), cpu, bus, base_cycles) }
        0xB2 => { op!(lda, addr::direct_indirect(cpu, bus), cpu, bus, base_cycles) }
        0xA7 => { op!(lda, addr::direct_indirect_long(cpu, bus), cpu, bus, base_cycles) }
        0xB7 => { op!(lda, addr::direct_indirect_long_y(cpu, bus), cpu, bus, base_cycles) }
        0xAF => { op!(lda, addr::long(cpu, bus), cpu, bus, base_cycles) }
        0xBF => { op!(lda, addr::long_x(cpu, bus), cpu, bus, base_cycles) }
        0xA3 => { op!(lda, addr::stack_relative(cpu, bus), cpu, bus, base_cycles) }
        0xB3 => { op!(lda, addr::stack_relative_indirect_y(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // LDX — Load Index X
        // ════════════════════════════════════════════════════════════════
        0xA2 => { // LDX #imm
            if cpu.is_x8() {
                let val = addr::immediate8(cpu, bus);
                cpu.x = val as u16;
                cpu.update_nz8(val);
            } else {
                let val = addr::immediate16(cpu, bus);
                cpu.x = val;
                cpu.update_nz16(val);
            }
            base_cycles
        }
        0xA6 => { op!(ldx, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0xB6 => { op!(ldx, addr::direct_y(cpu, bus), cpu, bus, base_cycles) }
        0xAE => { op!(ldx, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0xBE => { op!(ldx, addr::absolute_y(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // LDY — Load Index Y
        // ════════════════════════════════════════════════════════════════
        0xA0 => { // LDY #imm
            if cpu.is_x8() {
                let val = addr::immediate8(cpu, bus);
                cpu.y = val as u16;
                cpu.update_nz8(val);
            } else {
                let val = addr::immediate16(cpu, bus);
                cpu.y = val;
                cpu.update_nz16(val);
            }
            base_cycles
        }
        0xA4 => { op!(ldy, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0xB4 => { op!(ldy, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0xAC => { op!(ldy, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0xBC => { op!(ldy, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // STA — Store Accumulator
        // ════════════════════════════════════════════════════════════════
        0x85 => { op!(sta, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x95 => { op!(sta, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x8D => { op!(sta, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x9D => { op!(sta, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }
        0x99 => { op!(sta, addr::absolute_y(cpu, bus), cpu, bus, base_cycles) }
        0x81 => { op!(sta, addr::direct_x_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x91 => { op!(sta, addr::direct_indirect_y(cpu, bus), cpu, bus, base_cycles) }
        0x92 => { op!(sta, addr::direct_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x87 => { op!(sta, addr::direct_indirect_long(cpu, bus), cpu, bus, base_cycles) }
        0x97 => { op!(sta, addr::direct_indirect_long_y(cpu, bus), cpu, bus, base_cycles) }
        0x8F => { op!(sta, addr::long(cpu, bus), cpu, bus, base_cycles) }
        0x9F => { op!(sta, addr::long_x(cpu, bus), cpu, bus, base_cycles) }
        0x83 => { op!(sta, addr::stack_relative(cpu, bus), cpu, bus, base_cycles) }
        0x93 => { op!(sta, addr::stack_relative_indirect_y(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // STX — Store Index X
        // ════════════════════════════════════════════════════════════════
        0x86 => { op!(stx, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x96 => { op!(stx, addr::direct_y(cpu, bus), cpu, bus, base_cycles) }
        0x8E => { op!(stx, addr::absolute(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // STY — Store Index Y
        // ════════════════════════════════════════════════════════════════
        0x84 => { op!(sty, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x94 => { op!(sty, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x8C => { op!(sty, addr::absolute(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // STZ — Store Zero
        // ════════════════════════════════════════════════════════════════
        0x64 => { op!(stz, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x74 => { op!(stz, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x9C => { op!(stz, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x9E => { op!(stz, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // AND — Logical AND
        // ════════════════════════════════════════════════════════════════
        0x29 => { // AND #imm
            if cpu.is_m8() {
                let val = addr::immediate8(cpu, bus);
                let result = (cpu.a as u8) & val;
                cpu.a = (cpu.a & 0xFF00) | result as u16;
                cpu.update_nz8(result);
            } else {
                let val = addr::immediate16(cpu, bus);
                cpu.a &= val;
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0x25 => { op!(and, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x35 => { op!(and, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x2D => { op!(and, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x3D => { op!(and, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }
        0x39 => { op!(and, addr::absolute_y(cpu, bus), cpu, bus, base_cycles) }
        0x21 => { op!(and, addr::direct_x_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x31 => { op!(and, addr::direct_indirect_y(cpu, bus), cpu, bus, base_cycles) }
        0x32 => { op!(and, addr::direct_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x27 => { op!(and, addr::direct_indirect_long(cpu, bus), cpu, bus, base_cycles) }
        0x37 => { op!(and, addr::direct_indirect_long_y(cpu, bus), cpu, bus, base_cycles) }
        0x2F => { op!(and, addr::long(cpu, bus), cpu, bus, base_cycles) }
        0x3F => { op!(and, addr::long_x(cpu, bus), cpu, bus, base_cycles) }
        0x23 => { op!(and, addr::stack_relative(cpu, bus), cpu, bus, base_cycles) }
        0x33 => { op!(and, addr::stack_relative_indirect_y(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // ORA — Logical OR
        // ════════════════════════════════════════════════════════════════
        0x09 => { // ORA #imm
            if cpu.is_m8() {
                let val = addr::immediate8(cpu, bus);
                let result = (cpu.a as u8) | val;
                cpu.a = (cpu.a & 0xFF00) | result as u16;
                cpu.update_nz8(result);
            } else {
                let val = addr::immediate16(cpu, bus);
                cpu.a |= val;
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0x05 => { op!(ora, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x15 => { op!(ora, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x0D => { op!(ora, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x1D => { op!(ora, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }
        0x19 => { op!(ora, addr::absolute_y(cpu, bus), cpu, bus, base_cycles) }
        0x01 => { op!(ora, addr::direct_x_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x11 => { op!(ora, addr::direct_indirect_y(cpu, bus), cpu, bus, base_cycles) }
        0x12 => { op!(ora, addr::direct_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x07 => { op!(ora, addr::direct_indirect_long(cpu, bus), cpu, bus, base_cycles) }
        0x17 => { op!(ora, addr::direct_indirect_long_y(cpu, bus), cpu, bus, base_cycles) }
        0x0F => { op!(ora, addr::long(cpu, bus), cpu, bus, base_cycles) }
        0x1F => { op!(ora, addr::long_x(cpu, bus), cpu, bus, base_cycles) }
        0x03 => { op!(ora, addr::stack_relative(cpu, bus), cpu, bus, base_cycles) }
        0x13 => { op!(ora, addr::stack_relative_indirect_y(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // EOR — Exclusive OR
        // ════════════════════════════════════════════════════════════════
        0x49 => { // EOR #imm
            if cpu.is_m8() {
                let val = addr::immediate8(cpu, bus);
                let result = (cpu.a as u8) ^ val;
                cpu.a = (cpu.a & 0xFF00) | result as u16;
                cpu.update_nz8(result);
            } else {
                let val = addr::immediate16(cpu, bus);
                cpu.a ^= val;
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0x45 => { op!(eor, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x55 => { op!(eor, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x4D => { op!(eor, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x5D => { op!(eor, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }
        0x59 => { op!(eor, addr::absolute_y(cpu, bus), cpu, bus, base_cycles) }
        0x41 => { op!(eor, addr::direct_x_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x51 => { op!(eor, addr::direct_indirect_y(cpu, bus), cpu, bus, base_cycles) }
        0x52 => { op!(eor, addr::direct_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x47 => { op!(eor, addr::direct_indirect_long(cpu, bus), cpu, bus, base_cycles) }
        0x57 => { op!(eor, addr::direct_indirect_long_y(cpu, bus), cpu, bus, base_cycles) }
        0x4F => { op!(eor, addr::long(cpu, bus), cpu, bus, base_cycles) }
        0x5F => { op!(eor, addr::long_x(cpu, bus), cpu, bus, base_cycles) }
        0x43 => { op!(eor, addr::stack_relative(cpu, bus), cpu, bus, base_cycles) }
        0x53 => { op!(eor, addr::stack_relative_indirect_y(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // ADC — Add with Carry
        // ════════════════════════════════════════════════════════════════
        0x69 => { // ADC #imm
            if cpu.is_m8() {
                let val = addr::immediate8(cpu, bus);
                adc8(cpu, val);
            } else {
                let val = addr::immediate16(cpu, bus);
                adc16(cpu, val);
            }
            base_cycles
        }
        0x65 => { op!(adc_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x75 => { op!(adc_mem, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x6D => { op!(adc_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x7D => { op!(adc_mem, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }
        0x79 => { op!(adc_mem, addr::absolute_y(cpu, bus), cpu, bus, base_cycles) }
        0x61 => { op!(adc_mem, addr::direct_x_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x71 => { op!(adc_mem, addr::direct_indirect_y(cpu, bus), cpu, bus, base_cycles) }
        0x72 => { op!(adc_mem, addr::direct_indirect(cpu, bus), cpu, bus, base_cycles) }
        0x67 => { op!(adc_mem, addr::direct_indirect_long(cpu, bus), cpu, bus, base_cycles) }
        0x77 => { op!(adc_mem, addr::direct_indirect_long_y(cpu, bus), cpu, bus, base_cycles) }
        0x6F => { op!(adc_mem, addr::long(cpu, bus), cpu, bus, base_cycles) }
        0x7F => { op!(adc_mem, addr::long_x(cpu, bus), cpu, bus, base_cycles) }
        0x63 => { op!(adc_mem, addr::stack_relative(cpu, bus), cpu, bus, base_cycles) }
        0x73 => { op!(adc_mem, addr::stack_relative_indirect_y(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // SBC — Subtract with Carry (borrow)
        // ════════════════════════════════════════════════════════════════
        0xE9 => { // SBC #imm
            if cpu.is_m8() {
                let val = addr::immediate8(cpu, bus);
                sbc8(cpu, val);
            } else {
                let val = addr::immediate16(cpu, bus);
                sbc16(cpu, val);
            }
            base_cycles
        }
        0xE5 => { op!(sbc_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0xF5 => { op!(sbc_mem, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0xED => { op!(sbc_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0xFD => { op!(sbc_mem, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }
        0xF9 => { op!(sbc_mem, addr::absolute_y(cpu, bus), cpu, bus, base_cycles) }
        0xE1 => { op!(sbc_mem, addr::direct_x_indirect(cpu, bus), cpu, bus, base_cycles) }
        0xF1 => { op!(sbc_mem, addr::direct_indirect_y(cpu, bus), cpu, bus, base_cycles) }
        0xF2 => { op!(sbc_mem, addr::direct_indirect(cpu, bus), cpu, bus, base_cycles) }
        0xE7 => { op!(sbc_mem, addr::direct_indirect_long(cpu, bus), cpu, bus, base_cycles) }
        0xF7 => { op!(sbc_mem, addr::direct_indirect_long_y(cpu, bus), cpu, bus, base_cycles) }
        0xEF => { op!(sbc_mem, addr::long(cpu, bus), cpu, bus, base_cycles) }
        0xFF => { op!(sbc_mem, addr::long_x(cpu, bus), cpu, bus, base_cycles) }
        0xE3 => { op!(sbc_mem, addr::stack_relative(cpu, bus), cpu, bus, base_cycles) }
        0xF3 => { op!(sbc_mem, addr::stack_relative_indirect_y(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // CMP — Compare Accumulator
        // ════════════════════════════════════════════════════════════════
        0xC9 => { // CMP #imm
            if cpu.is_m8() {
                let val = addr::immediate8(cpu, bus);
                cmp8(cpu, cpu.a as u8, val);
            } else {
                let val = addr::immediate16(cpu, bus);
                cmp16(cpu, cpu.a, val);
            }
            base_cycles
        }
        0xC5 => { op!(cmp_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0xD5 => { op!(cmp_mem, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0xCD => { op!(cmp_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0xDD => { op!(cmp_mem, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }
        0xD9 => { op!(cmp_mem, addr::absolute_y(cpu, bus), cpu, bus, base_cycles) }
        0xC1 => { op!(cmp_mem, addr::direct_x_indirect(cpu, bus), cpu, bus, base_cycles) }
        0xD1 => { op!(cmp_mem, addr::direct_indirect_y(cpu, bus), cpu, bus, base_cycles) }
        0xD2 => { op!(cmp_mem, addr::direct_indirect(cpu, bus), cpu, bus, base_cycles) }
        0xC7 => { op!(cmp_mem, addr::direct_indirect_long(cpu, bus), cpu, bus, base_cycles) }
        0xD7 => { op!(cmp_mem, addr::direct_indirect_long_y(cpu, bus), cpu, bus, base_cycles) }
        0xCF => { op!(cmp_mem, addr::long(cpu, bus), cpu, bus, base_cycles) }
        0xDF => { op!(cmp_mem, addr::long_x(cpu, bus), cpu, bus, base_cycles) }
        0xC3 => { op!(cmp_mem, addr::stack_relative(cpu, bus), cpu, bus, base_cycles) }
        0xD3 => { op!(cmp_mem, addr::stack_relative_indirect_y(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // CPX — Compare X
        // ════════════════════════════════════════════════════════════════
        0xE0 => { // CPX #imm
            if cpu.is_x8() {
                let val = addr::immediate8(cpu, bus);
                cmp8(cpu, cpu.x as u8, val);
            } else {
                let val = addr::immediate16(cpu, bus);
                cmp16(cpu, cpu.x, val);
            }
            base_cycles
        }
        0xE4 => { op!(cpx_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0xEC => { op!(cpx_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // CPY — Compare Y
        // ════════════════════════════════════════════════════════════════
        0xC0 => { // CPY #imm
            if cpu.is_x8() {
                let val = addr::immediate8(cpu, bus);
                cmp8(cpu, cpu.y as u8, val);
            } else {
                let val = addr::immediate16(cpu, bus);
                cmp16(cpu, cpu.y, val);
            }
            base_cycles
        }
        0xC4 => { op!(cpy_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0xCC => { op!(cpy_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // BIT — Bit Test
        // ════════════════════════════════════════════════════════════════
        0x89 => { // BIT #imm — only sets Z flag, not N/V
            if cpu.is_m8() {
                let val = addr::immediate8(cpu, bus);
                cpu.p.z = (cpu.a as u8) & val == 0;
            } else {
                let val = addr::immediate16(cpu, bus);
                cpu.p.z = cpu.a & val == 0;
            }
            base_cycles
        }
        0x24 => { op!(bit, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x34 => { op!(bit, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x2C => { op!(bit, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x3C => { op!(bit, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // INC / DEC — Increment / Decrement (memory and accumulator)
        // ════════════════════════════════════════════════════════════════
        0x1A => { // INC A
            if cpu.is_m8() {
                let val = (cpu.a as u8).wrapping_add(1);
                cpu.a = (cpu.a & 0xFF00) | val as u16;
                cpu.update_nz8(val);
            } else {
                cpu.a = cpu.a.wrapping_add(1);
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0xE6 => { op!(inc_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0xF6 => { op!(inc_mem, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0xEE => { op!(inc_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0xFE => { op!(inc_mem, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }

        0x3A => { // DEC A
            if cpu.is_m8() {
                let val = (cpu.a as u8).wrapping_sub(1);
                cpu.a = (cpu.a & 0xFF00) | val as u16;
                cpu.update_nz8(val);
            } else {
                cpu.a = cpu.a.wrapping_sub(1);
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0xC6 => { op!(dec_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0xD6 => { op!(dec_mem, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0xCE => { op!(dec_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0xDE => { op!(dec_mem, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }

        0xE8 => { // INX
            if cpu.is_x8() {
                cpu.x = (cpu.x & 0xFF00) | ((cpu.x as u8).wrapping_add(1)) as u16;
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.x = cpu.x.wrapping_add(1);
                cpu.update_nz16(cpu.x);
            }
            base_cycles
        }
        0xCA => { // DEX
            if cpu.is_x8() {
                cpu.x = (cpu.x & 0xFF00) | ((cpu.x as u8).wrapping_sub(1)) as u16;
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.x = cpu.x.wrapping_sub(1);
                cpu.update_nz16(cpu.x);
            }
            base_cycles
        }
        0xC8 => { // INY
            if cpu.is_x8() {
                cpu.y = (cpu.y & 0xFF00) | ((cpu.y as u8).wrapping_add(1)) as u16;
                cpu.update_nz8(cpu.y as u8);
            } else {
                cpu.y = cpu.y.wrapping_add(1);
                cpu.update_nz16(cpu.y);
            }
            base_cycles
        }
        0x88 => { // DEY
            if cpu.is_x8() {
                cpu.y = (cpu.y & 0xFF00) | ((cpu.y as u8).wrapping_sub(1)) as u16;
                cpu.update_nz8(cpu.y as u8);
            } else {
                cpu.y = cpu.y.wrapping_sub(1);
                cpu.update_nz16(cpu.y);
            }
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // ASL — Arithmetic Shift Left
        // ════════════════════════════════════════════════════════════════
        0x0A => { // ASL A
            if cpu.is_m8() {
                let val = cpu.a as u8;
                cpu.p.c = val & 0x80 != 0;
                let result = val << 1;
                cpu.a = (cpu.a & 0xFF00) | result as u16;
                cpu.update_nz8(result);
            } else {
                cpu.p.c = cpu.a & 0x8000 != 0;
                cpu.a <<= 1;
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0x06 => { op!(asl_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x16 => { op!(asl_mem, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x0E => { op!(asl_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x1E => { op!(asl_mem, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // LSR — Logical Shift Right
        // ════════════════════════════════════════════════════════════════
        0x4A => { // LSR A
            if cpu.is_m8() {
                let val = cpu.a as u8;
                cpu.p.c = val & 0x01 != 0;
                let result = val >> 1;
                cpu.a = (cpu.a & 0xFF00) | result as u16;
                cpu.update_nz8(result);
            } else {
                cpu.p.c = cpu.a & 0x0001 != 0;
                cpu.a >>= 1;
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0x46 => { op!(lsr_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x56 => { op!(lsr_mem, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x4E => { op!(lsr_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x5E => { op!(lsr_mem, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // ROL — Rotate Left
        // ════════════════════════════════════════════════════════════════
        0x2A => { // ROL A
            if cpu.is_m8() {
                let val = cpu.a as u8;
                let carry_in = cpu.p.c as u8;
                cpu.p.c = val & 0x80 != 0;
                let result = (val << 1) | carry_in;
                cpu.a = (cpu.a & 0xFF00) | result as u16;
                cpu.update_nz8(result);
            } else {
                let carry_in = cpu.p.c as u16;
                cpu.p.c = cpu.a & 0x8000 != 0;
                cpu.a = (cpu.a << 1) | carry_in;
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0x26 => { op!(rol_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x36 => { op!(rol_mem, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x2E => { op!(rol_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x3E => { op!(rol_mem, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // ROR — Rotate Right
        // ════════════════════════════════════════════════════════════════
        0x6A => { // ROR A
            if cpu.is_m8() {
                let val = cpu.a as u8;
                let carry_in = if cpu.p.c { 0x80u8 } else { 0 };
                cpu.p.c = val & 0x01 != 0;
                let result = (val >> 1) | carry_in;
                cpu.a = (cpu.a & 0xFF00) | result as u16;
                cpu.update_nz8(result);
            } else {
                let carry_in = if cpu.p.c { 0x8000u16 } else { 0 };
                cpu.p.c = cpu.a & 0x0001 != 0;
                cpu.a = (cpu.a >> 1) | carry_in;
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0x66 => { op!(ror_mem, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x76 => { op!(ror_mem, addr::direct_x(cpu, bus), cpu, bus, base_cycles) }
        0x6E => { op!(ror_mem, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x7E => { op!(ror_mem, addr::absolute_x(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // TSB / TRB — Test and Set/Reset Bits
        // ════════════════════════════════════════════════════════════════
        0x04 => { op!(tsb, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x0C => { op!(tsb, addr::absolute(cpu, bus), cpu, bus, base_cycles) }
        0x14 => { op!(trb, addr::direct(cpu, bus), cpu, bus, base_cycles) }
        0x1C => { op!(trb, addr::absolute(cpu, bus), cpu, bus, base_cycles) }

        // ════════════════════════════════════════════════════════════════
        // Branches
        // ════════════════════════════════════════════════════════════════
        0x80 => { cpu.pc = addr::relative8(cpu, bus); base_cycles + 1 }  // BRA
        0x82 => { cpu.pc = addr::relative16(cpu, bus); base_cycles }     // BRL
        0xF0 => { // BEQ
            let target = addr::relative8(cpu, bus);
            if cpu.p.z { cpu.pc = target; base_cycles + 1 } else { base_cycles }
        }
        0xD0 => { // BNE
            let target = addr::relative8(cpu, bus);
            if !cpu.p.z { cpu.pc = target; base_cycles + 1 } else { base_cycles }
        }
        0xB0 => { // BCS
            let target = addr::relative8(cpu, bus);
            if cpu.p.c { cpu.pc = target; base_cycles + 1 } else { base_cycles }
        }
        0x90 => { // BCC
            let target = addr::relative8(cpu, bus);
            if !cpu.p.c { cpu.pc = target; base_cycles + 1 } else { base_cycles }
        }
        0x30 => { // BMI
            let target = addr::relative8(cpu, bus);
            if cpu.p.n { cpu.pc = target; base_cycles + 1 } else { base_cycles }
        }
        0x10 => { // BPL
            let target = addr::relative8(cpu, bus);
            if !cpu.p.n { cpu.pc = target; base_cycles + 1 } else { base_cycles }
        }
        0x70 => { // BVS
            let target = addr::relative8(cpu, bus);
            if cpu.p.v { cpu.pc = target; base_cycles + 1 } else { base_cycles }
        }
        0x50 => { // BVC
            let target = addr::relative8(cpu, bus);
            if !cpu.p.v { cpu.pc = target; base_cycles + 1 } else { base_cycles }
        }

        // ════════════════════════════════════════════════════════════════
        // Jumps and Calls
        // ════════════════════════════════════════════════════════════════
        0x4C => { // JMP abs
            cpu.pc = cpu.fetch_word(bus);
            base_cycles
        }
        0x6C => { // JMP (abs) — indirect
            cpu.pc = addr::absolute_indirect(cpu, bus);
            base_cycles
        }
        0x7C => { // JMP (abs,X)
            cpu.pc = addr::absolute_x_indirect(cpu, bus);
            base_cycles
        }
        0x5C => { // JML long
            let (bank, a) = cpu.fetch_long(bus);
            cpu.pbr = bank;
            cpu.pc = a;
            base_cycles
        }
        0xDC => { // JML [abs] — indirect long
            let (bank, a) = addr::absolute_indirect_long(cpu, bus);
            cpu.pbr = bank;
            cpu.pc = a;
            base_cycles
        }
        0x20 => { // JSR abs
            let target = cpu.fetch_word(bus);
            // Push PC-1 (return address minus 1, RTS will add 1)
            cpu.push_word(bus, cpu.pc.wrapping_sub(1));
            cpu.pc = target;
            base_cycles
        }
        0xFC => { // JSR (abs,X)
            let target = addr::absolute_x_indirect(cpu, bus);
            cpu.push_word(bus, cpu.pc.wrapping_sub(1));
            cpu.pc = target;
            base_cycles
        }
        0x22 => { // JSL long
            let new_addr = cpu.fetch_word(bus);
            let new_bank = cpu.fetch_byte(bus);
            // Push PBR, then PC-1
            cpu.push_byte(bus, cpu.pbr);
            cpu.push_word(bus, cpu.pc.wrapping_sub(1));
            cpu.pbr = new_bank;
            cpu.pc = new_addr;
            base_cycles
        }
        0x60 => { // RTS
            let pc = cpu.pull_word(bus);
            cpu.pc = pc.wrapping_add(1);
            base_cycles
        }
        0x6B => { // RTL
            let pc = cpu.pull_word(bus);
            let bank = cpu.pull_byte(bus);
            cpu.pc = pc.wrapping_add(1);
            cpu.pbr = bank;
            base_cycles
        }
        0x40 => { // RTI
            let p = cpu.pull_byte(bus);
            cpu.p.from_byte(p, cpu.emulation);
            let pc = cpu.pull_word(bus);
            cpu.pc = pc;
            if !cpu.emulation {
                cpu.pbr = cpu.pull_byte(bus);
            }
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // Stack Push / Pull
        // ════════════════════════════════════════════════════════════════
        0x48 => { // PHA
            if cpu.is_m8() {
                cpu.push_byte(bus, cpu.a as u8);
            } else {
                cpu.push_word(bus, cpu.a);
            }
            base_cycles
        }
        0x68 => { // PLA
            if cpu.is_m8() {
                let val = cpu.pull_byte(bus);
                cpu.a = (cpu.a & 0xFF00) | val as u16;
                cpu.update_nz8(val);
            } else {
                cpu.a = cpu.pull_word(bus);
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0xDA => { // PHX
            if cpu.is_x8() {
                cpu.push_byte(bus, cpu.x as u8);
            } else {
                cpu.push_word(bus, cpu.x);
            }
            base_cycles
        }
        0xFA => { // PLX
            if cpu.is_x8() {
                let val = cpu.pull_byte(bus);
                cpu.x = val as u16;
                cpu.update_nz8(val);
            } else {
                cpu.x = cpu.pull_word(bus);
                cpu.update_nz16(cpu.x);
            }
            base_cycles
        }
        0x5A => { // PHY
            if cpu.is_x8() {
                cpu.push_byte(bus, cpu.y as u8);
            } else {
                cpu.push_word(bus, cpu.y);
            }
            base_cycles
        }
        0x7A => { // PLY
            if cpu.is_x8() {
                let val = cpu.pull_byte(bus);
                cpu.y = val as u16;
                cpu.update_nz8(val);
            } else {
                cpu.y = cpu.pull_word(bus);
                cpu.update_nz16(cpu.y);
            }
            base_cycles
        }
        0x08 => { // PHP
            cpu.push_byte(bus, cpu.p.to_byte(cpu.emulation));
            base_cycles
        }
        0x28 => { // PLP
            let val = cpu.pull_byte(bus);
            let old_x = cpu.p.x;
            cpu.p.from_byte(val, cpu.emulation);
            // When X flag transitions from 0→1, high bytes of X/Y are zeroed.
            if !old_x && cpu.p.x {
                cpu.x &= 0xFF;
                cpu.y &= 0xFF;
            }
            base_cycles
        }
        0x8B => { // PHB — push data bank register
            cpu.push_byte(bus, cpu.dbr);
            base_cycles
        }
        0xAB => { // PLB — pull data bank register
            cpu.dbr = cpu.pull_byte(bus);
            cpu.update_nz8(cpu.dbr);
            base_cycles
        }
        0x0B => { // PHD — push direct page register
            cpu.push_word(bus, cpu.dp);
            base_cycles
        }
        0x2B => { // PLD — pull direct page register
            cpu.dp = cpu.pull_word(bus);
            cpu.update_nz16(cpu.dp);
            base_cycles
        }
        0x4B => { // PHK — push program bank register
            cpu.push_byte(bus, cpu.pbr);
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // PEA / PEI / PER — Push Effective Address
        // ════════════════════════════════════════════════════════════════
        0xF4 => { // PEA abs — push 16-bit immediate
            let val = cpu.fetch_word(bus);
            cpu.push_word(bus, val);
            base_cycles
        }
        0xD4 => { // PEI (dp) — push indirect
            let a = addr::direct(cpu, bus);
            let lo = bus.read(a.bank, a.addr) as u16;
            let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
            cpu.push_word(bus, lo | (hi << 8));
            base_cycles
        }
        0x62 => { // PER — push effective relative
            let offset = cpu.fetch_word(bus) as i16;
            let val = cpu.pc.wrapping_add(offset as u16);
            cpu.push_word(bus, val);
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // Register Transfers
        // ════════════════════════════════════════════════════════════════
        0xAA => { // TAX
            if cpu.is_x8() {
                cpu.x = (cpu.x & 0xFF00) | (cpu.a & 0xFF);
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.x = cpu.a;
                cpu.update_nz16(cpu.x);
            }
            base_cycles
        }
        0x8A => { // TXA
            if cpu.is_m8() {
                cpu.a = (cpu.a & 0xFF00) | (cpu.x & 0xFF);
                cpu.update_nz8(cpu.a as u8);
            } else {
                cpu.a = cpu.x;
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0xA8 => { // TAY
            if cpu.is_x8() {
                cpu.y = (cpu.y & 0xFF00) | (cpu.a & 0xFF);
                cpu.update_nz8(cpu.y as u8);
            } else {
                cpu.y = cpu.a;
                cpu.update_nz16(cpu.y);
            }
            base_cycles
        }
        0x98 => { // TYA
            if cpu.is_m8() {
                cpu.a = (cpu.a & 0xFF00) | (cpu.y & 0xFF);
                cpu.update_nz8(cpu.a as u8);
            } else {
                cpu.a = cpu.y;
                cpu.update_nz16(cpu.a);
            }
            base_cycles
        }
        0x9A => { // TXS — transfer X to stack pointer (no flags!)
            if cpu.emulation {
                cpu.sp = 0x0100 | (cpu.x & 0xFF);
            } else {
                cpu.sp = cpu.x;
            }
            base_cycles
        }
        0xBA => { // TSX — transfer stack pointer to X
            if cpu.is_x8() {
                cpu.x = cpu.sp & 0xFF;
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.x = cpu.sp;
                cpu.update_nz16(cpu.x);
            }
            base_cycles
        }
        0x1B => { // TCS — transfer 16-bit A to SP (no flags!)
            cpu.sp = cpu.a;
            if cpu.emulation {
                cpu.sp = 0x0100 | (cpu.sp & 0xFF);
            }
            base_cycles
        }
        0x3B => { // TSC — transfer SP to 16-bit A
            cpu.a = cpu.sp;
            cpu.update_nz16(cpu.a);
            base_cycles
        }
        0x9B => { // TXY
            if cpu.is_x8() {
                cpu.y = (cpu.y & 0xFF00) | (cpu.x & 0xFF);
                cpu.update_nz8(cpu.y as u8);
            } else {
                cpu.y = cpu.x;
                cpu.update_nz16(cpu.y);
            }
            base_cycles
        }
        0xBB => { // TYX
            if cpu.is_x8() {
                cpu.x = (cpu.x & 0xFF00) | (cpu.y & 0xFF);
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.x = cpu.y;
                cpu.update_nz16(cpu.x);
            }
            base_cycles
        }
        0x5B => { // TCD — transfer 16-bit A to Direct Page
            cpu.dp = cpu.a;
            cpu.update_nz16(cpu.dp);
            base_cycles
        }
        0x7B => { // TDC — transfer Direct Page to 16-bit A
            cpu.a = cpu.dp;
            cpu.update_nz16(cpu.a);
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // Flag instructions
        // ════════════════════════════════════════════════════════════════
        0x18 => { cpu.p.c = false; base_cycles } // CLC
        0x38 => { cpu.p.c = true; base_cycles }  // SEC
        0x58 => { cpu.p.i = false; base_cycles } // CLI
        0x78 => { cpu.p.i = true; base_cycles }  // SEI
        0xD8 => { cpu.p.d = false; base_cycles } // CLD
        0xF8 => { cpu.p.d = true; base_cycles }  // SED
        0xB8 => { cpu.p.v = false; base_cycles } // CLV

        // ════════════════════════════════════════════════════════════════
        // REP / SEP — Reset/Set processor status bits
        // ════════════════════════════════════════════════════════════════
        0xC2 => { // REP — clear bits in P
            let mask = cpu.fetch_byte(bus);
            let p = cpu.p.to_byte(cpu.emulation) & !mask;
            let old_x = cpu.p.x;
            cpu.p.from_byte(p, cpu.emulation);
            // Transitioning X from 1→0 doesn't clear high bytes
            // (only 0→1 does)
            let _ = old_x;
            base_cycles
        }
        0xE2 => { // SEP — set bits in P
            let mask = cpu.fetch_byte(bus);
            let p = cpu.p.to_byte(cpu.emulation) | mask;
            let old_x = cpu.p.x;
            cpu.p.from_byte(p, cpu.emulation);
            // Transitioning X from 0→1 zeroes high bytes of X and Y.
            if !old_x && cpu.p.x {
                cpu.x &= 0xFF;
                cpu.y &= 0xFF;
            }
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // XCE — Exchange Carry and Emulation flags
        // ════════════════════════════════════════════════════════════════
        0xFB => {
            let old_carry = cpu.p.c;
            cpu.p.c = cpu.emulation;
            cpu.emulation = old_carry;
            if cpu.emulation {
                // Entering emulation mode: force 8-bit, high bytes zeroed
                cpu.p.m = true;
                cpu.p.x = true;
                cpu.x &= 0xFF;
                cpu.y &= 0xFF;
                cpu.sp = 0x0100 | (cpu.sp & 0xFF);
            }
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // XBA — Exchange B and A (high and low bytes of accumulator)
        // ════════════════════════════════════════════════════════════════
        0xEB => {
            cpu.a = (cpu.a >> 8) | (cpu.a << 8);
            // N and Z based on the NEW low byte (A), regardless of M flag
            cpu.update_nz8(cpu.a as u8);
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // Block Move — MVN (Move Next) / MVP (Move Previous)
        // ════════════════════════════════════════════════════════════════
        0x54 => { // MVN — move block negative direction (ascending)
            let dst_bank = cpu.fetch_byte(bus);
            let src_bank = cpu.fetch_byte(bus);
            cpu.dbr = dst_bank;

            // Move one byte per "iteration" but loop until C wraps
            let src = bus.read(src_bank, cpu.x);
            bus.write(dst_bank, cpu.y, src);
            cpu.x = cpu.x.wrapping_add(1);
            cpu.y = cpu.y.wrapping_add(1);
            cpu.a = cpu.a.wrapping_sub(1);

            if cpu.a != 0xFFFF {
                cpu.pc = cpu.pc.wrapping_sub(3); // Re-execute this instruction
            }
            base_cycles
        }
        0x44 => { // MVP — move block positive direction (descending)
            let dst_bank = cpu.fetch_byte(bus);
            let src_bank = cpu.fetch_byte(bus);
            cpu.dbr = dst_bank;

            let src = bus.read(src_bank, cpu.x);
            bus.write(dst_bank, cpu.y, src);
            cpu.x = cpu.x.wrapping_sub(1);
            cpu.y = cpu.y.wrapping_sub(1);
            cpu.a = cpu.a.wrapping_sub(1);

            if cpu.a != 0xFFFF {
                cpu.pc = cpu.pc.wrapping_sub(3);
            }
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // Miscellaneous
        // ════════════════════════════════════════════════════════════════
        0xEA => { base_cycles } // NOP
        0x42 => { // WDM — reserved, acts as 2-byte NOP on SNES
            let _ = cpu.fetch_byte(bus);
            base_cycles
        }
        0xDB => { // STP — stop the processor
            cpu.stopped = true;
            base_cycles
        }
        0xCB => { // WAI — wait for interrupt
            cpu.waiting = true;
            base_cycles
        }

        // ════════════════════════════════════════════════════════════════
        // BRK / COP — Software interrupts
        // ════════════════════════════════════════════════════════════════
        0x00 => { // BRK
            let _ = cpu.fetch_byte(bus); // signature byte (ignored)
            if cpu.emulation {
                cpu.push_word(bus, cpu.pc);
                cpu.push_byte(bus, cpu.p.to_byte(true) | 0x10); // Set B flag
                cpu.p.i = true;
                cpu.p.d = false;
                let lo = bus.read(0x00, 0xFFFE) as u16;
                let hi = bus.read(0x00, 0xFFFF) as u16;
                cpu.pc = lo | (hi << 8);
            } else {
                cpu.push_byte(bus, cpu.pbr);
                cpu.push_word(bus, cpu.pc);
                cpu.push_byte(bus, cpu.p.to_byte(false));
                cpu.p.i = true;
                cpu.p.d = false;
                cpu.pbr = 0;
                let lo = bus.read(0x00, 0xFFE6) as u16;
                let hi = bus.read(0x00, 0xFFE7) as u16;
                cpu.pc = lo | (hi << 8);
            }
            base_cycles
        }
        0x02 => { // COP
            let _ = cpu.fetch_byte(bus);
            if cpu.emulation {
                cpu.push_word(bus, cpu.pc);
                cpu.push_byte(bus, cpu.p.to_byte(true));
                cpu.p.i = true;
                cpu.p.d = false;
                let lo = bus.read(0x00, 0xFFF4) as u16;
                let hi = bus.read(0x00, 0xFFF5) as u16;
                cpu.pc = lo | (hi << 8);
            } else {
                cpu.push_byte(bus, cpu.pbr);
                cpu.push_word(bus, cpu.pc);
                cpu.push_byte(bus, cpu.p.to_byte(false));
                cpu.p.i = true;
                cpu.p.d = false;
                cpu.pbr = 0;
                let lo = bus.read(0x00, 0xFFE4) as u16;
                let hi = bus.read(0x00, 0xFFE5) as u16;
                cpu.pc = lo | (hi << 8);
            }
            base_cycles
        }

        #[allow(unreachable_patterns)]
        _ => {
            eprintln!(
                "UNIMPLEMENTED opcode: ${:02X} at {:02X}:{:04X}",
                opcode, cpu.pbr, cpu.pc.wrapping_sub(1)
            );
            base_cycles.max(2) // Don't stall; advance
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Helper functions for memory-addressed operations
// ═══════════════════════════════════════════════════════════════════════════

fn read_m(cpu: &Cpu, bus: &mut Bus, a: Addr) -> u16 {
    if cpu.is_m8() {
        bus.read(a.bank, a.addr) as u16
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }
}

fn write_m(cpu: &Cpu, bus: &mut Bus, a: Addr, val: u16) {
    if cpu.is_m8() {
        bus.write(a.bank, a.addr, val as u8);
    } else {
        bus.write(a.bank, a.addr, val as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (val >> 8) as u8);
    }
}

fn read_x(cpu: &Cpu, bus: &mut Bus, a: Addr) -> u16 {
    if cpu.is_x8() {
        bus.read(a.bank, a.addr) as u16
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }
}

// ── Load helpers ────────────────────────────────────────────────────────

fn lda(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_m(cpu, bus, a);
    if cpu.is_m8() {
        cpu.a = (cpu.a & 0xFF00) | (val & 0xFF);
        cpu.update_nz8(val as u8);
    } else {
        cpu.a = val;
        cpu.update_nz16(val);
    }
}

fn ldx(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_x(cpu, bus, a);
    if cpu.is_x8() {
        cpu.x = val & 0xFF;
        cpu.update_nz8(val as u8);
    } else {
        cpu.x = val;
        cpu.update_nz16(val);
    }
}

fn ldy(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_x(cpu, bus, a);
    if cpu.is_x8() {
        cpu.y = val & 0xFF;
        cpu.update_nz8(val as u8);
    } else {
        cpu.y = val;
        cpu.update_nz16(val);
    }
}

// ── Store helpers ───────────────────────────────────────────────────────

fn sta(cpu: &Cpu, bus: &mut Bus, a: Addr) {
    write_m(cpu, bus, a, cpu.a);
}

fn stx(cpu: &Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_x8() {
        bus.write(a.bank, a.addr, cpu.x as u8);
    } else {
        bus.write(a.bank, a.addr, cpu.x as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (cpu.x >> 8) as u8);
    }
}

fn sty(cpu: &Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_x8() {
        bus.write(a.bank, a.addr, cpu.y as u8);
    } else {
        bus.write(a.bank, a.addr, cpu.y as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (cpu.y >> 8) as u8);
    }
}

fn stz(cpu: &Cpu, bus: &mut Bus, a: Addr) {
    write_m(cpu, bus, a, 0);
}

// ── ALU helpers ─────────────────────────────────────────────────────────

fn and(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_m(cpu, bus, a);
    if cpu.is_m8() {
        let result = (cpu.a as u8) & (val as u8);
        cpu.a = (cpu.a & 0xFF00) | result as u16;
        cpu.update_nz8(result);
    } else {
        cpu.a &= val;
        cpu.update_nz16(cpu.a);
    }
}

fn ora(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_m(cpu, bus, a);
    if cpu.is_m8() {
        let result = (cpu.a as u8) | (val as u8);
        cpu.a = (cpu.a & 0xFF00) | result as u16;
        cpu.update_nz8(result);
    } else {
        cpu.a |= val;
        cpu.update_nz16(cpu.a);
    }
}

fn eor(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_m(cpu, bus, a);
    if cpu.is_m8() {
        let result = (cpu.a as u8) ^ (val as u8);
        cpu.a = (cpu.a & 0xFF00) | result as u16;
        cpu.update_nz8(result);
    } else {
        cpu.a ^= val;
        cpu.update_nz16(cpu.a);
    }
}

fn adc8(cpu: &mut Cpu, val: u8) {
    let a = cpu.a as u8;
    let carry = cpu.p.c as u8;
    let result = a as u16 + val as u16 + carry as u16;
    cpu.p.c = result > 0xFF;
    cpu.p.v = (!(a ^ val) & (a ^ result as u8)) & 0x80 != 0;
    let result8 = result as u8;
    cpu.a = (cpu.a & 0xFF00) | result8 as u16;
    cpu.update_nz8(result8);
}

fn adc16(cpu: &mut Cpu, val: u16) {
    let a = cpu.a;
    let carry = cpu.p.c as u16;
    let result = a as u32 + val as u32 + carry as u32;
    cpu.p.c = result > 0xFFFF;
    cpu.p.v = (!(a ^ val) & (a ^ result as u16)) & 0x8000 != 0;
    cpu.a = result as u16;
    cpu.update_nz16(cpu.a);
}

fn adc_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_m(cpu, bus, a);
    if cpu.is_m8() { adc8(cpu, val as u8); } else { adc16(cpu, val); }
}

fn sbc8(cpu: &mut Cpu, val: u8) {
    // SBC is ADC with complement
    adc8(cpu, !val);
}

fn sbc16(cpu: &mut Cpu, val: u16) {
    adc16(cpu, !val);
}

fn sbc_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_m(cpu, bus, a);
    if cpu.is_m8() { sbc8(cpu, val as u8); } else { sbc16(cpu, val); }
}

fn cmp8(cpu: &mut Cpu, reg: u8, val: u8) {
    let result = reg as i16 - val as i16;
    cpu.p.c = reg >= val;
    cpu.p.z = reg == val;
    cpu.p.n = (result as u8) & 0x80 != 0;
}

fn cmp16(cpu: &mut Cpu, reg: u16, val: u16) {
    let result = reg as i32 - val as i32;
    cpu.p.c = reg >= val;
    cpu.p.z = reg == val;
    cpu.p.n = (result as u16) & 0x8000 != 0;
}

fn cmp_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_m(cpu, bus, a);
    if cpu.is_m8() { cmp8(cpu, cpu.a as u8, val as u8); }
    else { cmp16(cpu, cpu.a, val); }
}

fn cpx_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_x(cpu, bus, a);
    if cpu.is_x8() { cmp8(cpu, cpu.x as u8, val as u8); }
    else { cmp16(cpu, cpu.x, val); }
}

fn cpy_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_x(cpu, bus, a);
    if cpu.is_x8() { cmp8(cpu, cpu.y as u8, val as u8); }
    else { cmp16(cpu, cpu.y, val); }
}

fn bit(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    let val = read_m(cpu, bus, a);
    if cpu.is_m8() {
        let v = val as u8;
        cpu.p.z = (cpu.a as u8) & v == 0;
        cpu.p.n = v & 0x80 != 0;
        cpu.p.v = v & 0x40 != 0;
    } else {
        cpu.p.z = cpu.a & val == 0;
        cpu.p.n = val & 0x8000 != 0;
        cpu.p.v = val & 0x4000 != 0;
    }
}

// ── Shift/rotate memory helpers ─────────────────────────────────────────

fn asl_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_m8() {
        let val = bus.read(a.bank, a.addr);
        cpu.p.c = val & 0x80 != 0;
        let result = val << 1;
        bus.write(a.bank, a.addr, result);
        cpu.update_nz8(result);
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        let val = lo | (hi << 8);
        cpu.p.c = val & 0x8000 != 0;
        let result = val << 1;
        bus.write(a.bank, a.addr, result as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (result >> 8) as u8);
        cpu.update_nz16(result);
    }
}

fn lsr_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_m8() {
        let val = bus.read(a.bank, a.addr);
        cpu.p.c = val & 0x01 != 0;
        let result = val >> 1;
        bus.write(a.bank, a.addr, result);
        cpu.update_nz8(result);
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        let val = lo | (hi << 8);
        cpu.p.c = val & 0x0001 != 0;
        let result = val >> 1;
        bus.write(a.bank, a.addr, result as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (result >> 8) as u8);
        cpu.update_nz16(result);
    }
}

fn rol_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_m8() {
        let val = bus.read(a.bank, a.addr);
        let carry_in = cpu.p.c as u8;
        cpu.p.c = val & 0x80 != 0;
        let result = (val << 1) | carry_in;
        bus.write(a.bank, a.addr, result);
        cpu.update_nz8(result);
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        let val = lo | (hi << 8);
        let carry_in = cpu.p.c as u16;
        cpu.p.c = val & 0x8000 != 0;
        let result = (val << 1) | carry_in;
        bus.write(a.bank, a.addr, result as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (result >> 8) as u8);
        cpu.update_nz16(result);
    }
}

fn ror_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_m8() {
        let val = bus.read(a.bank, a.addr);
        let carry_in = if cpu.p.c { 0x80u8 } else { 0 };
        cpu.p.c = val & 0x01 != 0;
        let result = (val >> 1) | carry_in;
        bus.write(a.bank, a.addr, result);
        cpu.update_nz8(result);
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        let val = lo | (hi << 8);
        let carry_in = if cpu.p.c { 0x8000u16 } else { 0 };
        cpu.p.c = val & 0x0001 != 0;
        let result = (val >> 1) | carry_in;
        bus.write(a.bank, a.addr, result as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (result >> 8) as u8);
        cpu.update_nz16(result);
    }
}

fn tsb(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_m8() {
        let val = bus.read(a.bank, a.addr);
        cpu.p.z = (cpu.a as u8) & val == 0;
        bus.write(a.bank, a.addr, val | cpu.a as u8);
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        let val = lo | (hi << 8);
        cpu.p.z = cpu.a & val == 0;
        let result = val | cpu.a;
        bus.write(a.bank, a.addr, result as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (result >> 8) as u8);
    }
}

fn trb(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_m8() {
        let val = bus.read(a.bank, a.addr);
        cpu.p.z = (cpu.a as u8) & val == 0;
        bus.write(a.bank, a.addr, val & !(cpu.a as u8));
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        let val = lo | (hi << 8);
        cpu.p.z = cpu.a & val == 0;
        let result = val & !cpu.a;
        bus.write(a.bank, a.addr, result as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (result >> 8) as u8);
    }
}

fn inc_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_m8() {
        let val = bus.read(a.bank, a.addr).wrapping_add(1);
        bus.write(a.bank, a.addr, val);
        cpu.update_nz8(val);
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        let val = (lo | (hi << 8)).wrapping_add(1);
        bus.write(a.bank, a.addr, val as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (val >> 8) as u8);
        cpu.update_nz16(val);
    }
}

fn dec_mem(cpu: &mut Cpu, bus: &mut Bus, a: Addr) {
    if cpu.is_m8() {
        let val = bus.read(a.bank, a.addr).wrapping_sub(1);
        bus.write(a.bank, a.addr, val);
        cpu.update_nz8(val);
    } else {
        let lo = bus.read(a.bank, a.addr) as u16;
        let hi = bus.read(a.bank, a.addr.wrapping_add(1)) as u16;
        let val = (lo | (hi << 8)).wrapping_sub(1);
        bus.write(a.bank, a.addr, val as u8);
        bus.write(a.bank, a.addr.wrapping_add(1), (val >> 8) as u8);
        cpu.update_nz16(val);
    }
}
