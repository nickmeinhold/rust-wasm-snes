/// Opcode metadata tables for the 65C816.

/// Human-readable opcode names for trace logging (indexed by opcode byte).
pub static OPCODE_NAMES: [&str; 256] = [
    // 0x00-0x0F
    "BRK", "ORA", "COP", "ORA", "TSB", "ORA", "ASL", "ORA",
    "PHP", "ORA", "ASL", "PHD", "TSB", "ORA", "ASL", "ORA",
    // 0x10-0x1F
    "BPL", "ORA", "ORA", "ORA", "TRB", "ORA", "ASL", "ORA",
    "CLC", "ORA", "INC", "TCS", "TRB", "ORA", "ASL", "ORA",
    // 0x20-0x2F
    "JSR", "AND", "JSL", "AND", "BIT", "AND", "ROL", "AND",
    "PLP", "AND", "ROL", "PLD", "BIT", "AND", "ROL", "AND",
    // 0x30-0x3F
    "BMI", "AND", "AND", "AND", "BIT", "AND", "ROL", "AND",
    "SEC", "AND", "DEC", "TSC", "BIT", "AND", "ROL", "AND",
    // 0x40-0x4F
    "RTI", "EOR", "WDM", "EOR", "MVP", "EOR", "LSR", "EOR",
    "PHA", "EOR", "LSR", "PHK", "JMP", "EOR", "LSR", "EOR",
    // 0x50-0x5F
    "BVC", "EOR", "EOR", "EOR", "MVN", "EOR", "LSR", "EOR",
    "CLI", "EOR", "PHY", "TCD", "JML", "EOR", "LSR", "EOR",
    // 0x60-0x6F
    "RTS", "ADC", "PER", "ADC", "STZ", "ADC", "ROR", "ADC",
    "PLA", "ADC", "ROR", "RTL", "JMP", "ADC", "ROR", "ADC",
    // 0x70-0x7F
    "BVS", "ADC", "ADC", "ADC", "STZ", "ADC", "ROR", "ADC",
    "SEI", "ADC", "PLY", "TDC", "JMP", "ADC", "ROR", "ADC",
    // 0x80-0x8F
    "BRA", "STA", "BRL", "STA", "STY", "STA", "STX", "STA",
    "DEY", "BIT", "TXA", "PHB", "STY", "STA", "STX", "STA",
    // 0x90-0x9F
    "BCC", "STA", "STA", "STA", "STY", "STA", "STX", "STA",
    "TYA", "STA", "TXS", "TXY", "STZ", "STA", "STZ", "STA",
    // 0xA0-0xAF
    "LDY", "LDA", "LDX", "LDA", "LDY", "LDA", "LDX", "LDA",
    "TAY", "LDA", "TAX", "PLB", "LDY", "LDA", "LDX", "LDA",
    // 0xB0-0xBF
    "BCS", "LDA", "LDA", "LDA", "LDY", "LDA", "LDX", "LDA",
    "CLV", "LDA", "TSX", "TYX", "LDY", "LDA", "LDX", "LDA",
    // 0xC0-0xCF
    "CPY", "CMP", "REP", "CMP", "CPY", "CMP", "DEC", "CMP",
    "INY", "CMP", "DEX", "WAI", "CPY", "CMP", "DEC", "CMP",
    // 0xD0-0xDF
    "BNE", "CMP", "CMP", "CMP", "PEI", "CMP", "DEC", "CMP",
    "CLD", "CMP", "PHX", "STP", "JML", "CMP", "DEC", "CMP",
    // 0xE0-0xEF
    "CPX", "SBC", "SEP", "SBC", "CPX", "SBC", "INC", "SBC",
    "INX", "SBC", "NOP", "XBA", "CPX", "SBC", "INC", "SBC",
    // 0xF0-0xFF
    "BEQ", "SBC", "SBC", "SBC", "PEA", "SBC", "INC", "SBC",
    "SED", "SBC", "PLX", "XCE", "JSR", "SBC", "INC", "SBC",
];

/// Base cycle counts per opcode (CPU cycles, not master cycles).
/// These are approximate — page-crossing penalties and 16-bit mode
/// adjustments are handled in the instruction implementations.
pub static OPCODE_CYCLES: [u8; 256] = [
    // 0x00-0x0F
    7, 6, 7, 4, 5, 3, 5, 6, 3, 2, 2, 4, 6, 4, 6, 5,
    // 0x10-0x1F
    2, 5, 5, 7, 5, 4, 6, 6, 2, 4, 2, 2, 6, 4, 6, 5,
    // 0x20-0x2F
    6, 6, 8, 4, 3, 3, 5, 6, 4, 2, 2, 5, 4, 4, 6, 5,
    // 0x30-0x3F
    2, 5, 5, 7, 4, 4, 6, 6, 2, 4, 2, 2, 4, 4, 6, 5,
    // 0x40-0x4F
    7, 6, 2, 7, 0, 3, 5, 6, 3, 2, 2, 3, 3, 4, 6, 5,
    // 0x50-0x5F
    2, 5, 5, 7, 0, 4, 6, 6, 2, 4, 3, 2, 4, 4, 6, 5,
    // 0x60-0x6F
    6, 6, 6, 4, 3, 3, 5, 6, 4, 2, 2, 6, 5, 4, 6, 5,
    // 0x70-0x7F
    2, 5, 5, 7, 4, 4, 6, 6, 2, 4, 4, 2, 6, 4, 6, 5,
    // 0x80-0x8F
    3, 6, 4, 4, 3, 3, 3, 6, 2, 2, 2, 3, 4, 4, 4, 5,
    // 0x90-0x9F
    2, 6, 5, 7, 4, 4, 4, 6, 2, 5, 2, 2, 4, 5, 5, 5,
    // 0xA0-0xAF
    2, 6, 2, 4, 3, 3, 3, 6, 2, 2, 2, 4, 4, 4, 4, 5,
    // 0xB0-0xBF
    2, 5, 5, 7, 4, 4, 4, 6, 2, 4, 2, 2, 4, 4, 4, 5,
    // 0xC0-0xCF
    2, 6, 3, 4, 3, 3, 5, 6, 2, 2, 2, 3, 4, 4, 6, 5,
    // 0xD0-0xDF
    2, 5, 5, 7, 6, 4, 6, 6, 2, 4, 3, 3, 6, 4, 6, 5,
    // 0xE0-0xEF
    2, 6, 3, 4, 3, 3, 5, 6, 2, 2, 2, 3, 4, 4, 6, 5,
    // 0xF0-0xFF
    2, 5, 5, 7, 6, 4, 6, 6, 2, 4, 4, 2, 8, 4, 6, 5,
];
