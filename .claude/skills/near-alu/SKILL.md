---
name: near-alu
argument-hint: "[scope: 'adc', 'sbc', 'decimal', 'flags', '16bit', 'full']"
description: >
  Verify ALU flag computation against Near/byuu's ALU article. Near provides
  branchless ADC/SBC formulas with correct overflow and carry. For the 65816
  (WDC65C816), SBC is defined as ADC(target, ~source, carry) — NOT with
  inverted carry. Audits: SBC carry sense, overflow formula difference between
  ADC and SBC, decimal mode, 16-bit mode sign bit, and absence of spurious
  half-carry. Single-pass.
---

# Near ALU Compliance (`/near-alu`)

Single-pass audit of ALU operations against Near's article.

## Reference

From `cpu/alu/README.md`:

Near's ADC:
```cpp
auto adc(natural target, natural source, boolean carry) -> uint8 {
    natural result   = target + source + carry;
    natural carries  = target ^ source ^ result;
    natural overflow = (target ^ result) & (source ^ result);
    flag.overflow  = overflow & sign;
    flag.carry     = (carries ^ overflow) & sign;
}
```

Near's SBC (65816 carry-based):
```cpp
auto sbc(natural target, natural source, boolean carry) -> uint8 {
    return adc(target, ~source, carry);  // NOT !carry — carry, not borrow
}
```

Near: "The 6502 series of processors (MOS6502, Ricoh 6502, HuC6280, WDC65816)
use carry rather than borrow for subtraction."

Near on SBC via ADC: "The reason for this becomes clear when looking at an
alternate implementation: `return adc(target, ~source, carry);`"

## Audit Agent Prompt

```
First, read ~/.claude/AGENT.md for instructions.

Single-pass ALU audit. Report inline.

## Checks

### 1. Find the ALU Implementation

Search for ADC, SBC, CMP operations in the CPU code (likely `src/cpu/` or
`src/cpu/instructions.rs`). The 65816 has both 8-bit (M=1) and 16-bit (M=0)
modes for arithmetic.

### 2. SBC Carry Sense

THE MOST COMMON BUG. The 65816's SBC uses carry (not borrow):
  SBC(A, operand) = ADC(A, ~operand, C)

NOT:
  SBC(A, operand) = ADC(A, ~operand, !C)  // WRONG — this is borrow-based (Z80, x86)

Verify the carry flag is passed directly, not inverted.

### 3. Overflow Flag

ADC overflow: V = (target ^ result) & (source ^ result)
SBC overflow: V = (target ^ result) & (source ^ target)  // note: source ^ TARGET, not result

But if SBC is implemented via ADC(target, ~source, carry), the ADC overflow
formula automatically gives the correct result because ~source is used.
Verify the overflow flag is correct for both ADC and SBC.

### 4. Carry Flag

Near's formula: C = (carries ^ overflow) & sign

Where `carries = target ^ source ^ result` and `sign` is the MSB mask.

For 8-bit mode: sign = 0x80
For 16-bit mode: sign = 0x8000

Verify the sign mask changes correctly between 8-bit and 16-bit modes.

### 5. Decimal Mode

The 65816 supports BCD arithmetic when D flag is set. Decimal mode:
- ADC: adjust result for BCD (if nibble > 9, add 6)
- SBC: adjust result for BCD (if nibble < 0, subtract 6)
- N flag: set from MSB of result (even in decimal mode on 65816)
- Z flag: set if result is zero (even in decimal mode on 65816)
- V flag: undefined on 65816 in decimal mode (some sources say it's set
  from the binary overflow, others say undefined)
- C flag: set if BCD result > 99 (ADC) or >= 0 (SBC)

This is notoriously tricky. Many emulators get decimal mode wrong.
Verify at least the basic case works correctly.

### 6. 16-bit Arithmetic

When M=0, ADC/SBC operate on 16-bit values. Verify:
- The accumulator is treated as a full 16-bit value
- Carry out is from bit 15, not bit 7
- Overflow is computed from bit 15, not bit 7
- N flag is bit 15 of result
- Z flag is set if full 16-bit result is zero

### 7. CMP/CPX/CPY

Compare operations are subtraction without carry (always C=1 for SBC):
  CMP(A, operand) = A - operand (with carry forced to 1)

Verify:
- C=1 is forced (not read from the status register)
- N and Z flags are set from the result
- C flag is set if A >= operand (unsigned comparison)
- V flag is NOT affected by CMP on the 65816

### 8. No Half-Carry

The 65816 does NOT have a half-carry flag (that's Z80/GB). Verify no
spurious half-carry computation or flag exists.

## Report

For each check: PASS / FAIL / PARTIAL.
Flag the SBC carry sense prominently — it's the #1 ALU bug in emulators.
```
