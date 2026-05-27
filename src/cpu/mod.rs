/// WDC 65C816 CPU emulation (as used in the Ricoh 5A22).
///
/// The 65C816 is a 16-bit extension of the 6502. It starts in "emulation mode"
/// (6502-compatible) and the game immediately switches to "native mode" via
/// CLC; XCE to unlock 16-bit registers and the full 24-bit address space.

pub mod addressing;
pub mod instructions;
pub mod tables;

use crate::bus::Bus;

/// Processor status flags.
///
/// In native mode all 8 bits are meaningful (NVMXDIZC).
/// In emulation mode, M is forced to 1 and X position becomes the Break flag.
#[derive(Clone, Copy, Debug)]
pub struct StatusRegister {
    pub n: bool, // Negative
    pub v: bool, // Overflow
    pub m: bool, // Accumulator/memory width: true = 8-bit
    pub x: bool, // Index register width: true = 8-bit (Break flag in emulation)
    pub d: bool, // Decimal mode
    pub i: bool, // IRQ disable
    pub z: bool, // Zero
    pub c: bool, // Carry
}

impl StatusRegister {
    fn new() -> Self {
        Self {
            n: false,
            v: false,
            m: true,
            x: true,
            d: false,
            i: true, // IRQs disabled on reset
            z: false,
            c: false,
        }
    }

    /// Pack into a byte. Bit 5 is always 1 in emulation mode (unused/B flag).
    pub fn to_byte(self, emulation: bool) -> u8 {
        let mut b = 0u8;
        if self.n { b |= 0x80; }
        if self.v { b |= 0x40; }
        if emulation {
            // Bit 5 = 1 (unused), bit 4 = break flag (we use x for this)
            b |= 0x20;
            if self.x { b |= 0x10; }
        } else {
            if self.m { b |= 0x20; }
            if self.x { b |= 0x10; }
        }
        if self.d { b |= 0x08; }
        if self.i { b |= 0x04; }
        if self.z { b |= 0x02; }
        if self.c { b |= 0x01; }
        b
    }

    /// Unpack from a byte.
    pub fn from_byte(&mut self, val: u8, emulation: bool) {
        self.n = val & 0x80 != 0;
        self.v = val & 0x40 != 0;
        if emulation {
            self.m = true;
            self.x = true;
        } else {
            self.m = val & 0x20 != 0;
            self.x = val & 0x10 != 0;
        }
        self.d = val & 0x08 != 0;
        self.i = val & 0x04 != 0;
        self.z = val & 0x02 != 0;
        self.c = val & 0x01 != 0;
    }
}

pub struct Cpu {
    // Registers
    pub a: u16,   // Accumulator (16-bit; high byte = "B" hidden accumulator)
    pub x: u16,   // Index X
    pub y: u16,   // Index Y
    pub sp: u16,  // Stack pointer
    pub dp: u16,  // Direct page
    pub pc: u16,  // Program counter
    pub pbr: u8,  // Program bank register
    pub dbr: u8,  // Data bank register
    pub p: StatusRegister,

    pub emulation: bool, // Emulation mode (starts true)
    pub cycles: u64,     // Master cycle counter

    pub nmi_pending: bool,
    pub irq_pending: bool,

    pub stopped: bool, // STP
    pub waiting: bool, // WAI

    /// Enable instruction tracing to stderr.
    pub trace: bool,

    /// Per-opcode execution count. Indexed by opcode byte. Box-allocated to
    /// avoid bloating the Cpu struct (256 × u64 = 2 KiB).
    pub opcode_counts: Box<[u64; 256]>,

    /// Number of times the idle-loop fast path fired this run. Diagnostic only.
    pub idle_skip_hits: u64,

    /// Cumulative master cycles skipped by the idle-loop fast path. Diagnostic.
    pub idle_skip_cycles: u64,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0x01FF,
            dp: 0,
            pc: 0,
            pbr: 0,
            dbr: 0,
            p: StatusRegister::new(),
            emulation: true,
            cycles: 0,
            nmi_pending: false,
            irq_pending: false,
            stopped: false,
            waiting: false,
            trace: false,
            opcode_counts: Box::new([0u64; 256]),
            idle_skip_hits: 0,
            idle_skip_cycles: 0,
        }
    }

    /// Tier-1 idle-loop fast path. Detects the canonical 65816 polling shape
    ///
    /// ```text
    ///   loop:  LDA $xx    ; A5 xx
    ///          BEQ loop   ; F0 FC   (offset -4 lands on the LDA)
    /// ```
    ///
    /// and — if the polled address is in pure memory (WRAM / SRAM / ROM only)
    /// — advances `cpu.cycles` to just before the next scanline boundary,
    /// leaving PC at the LDA so the final one or two iterations execute
    /// normally and any pending interrupt fires at the correct cycle.
    ///
    /// Returns `Some(skip_cycles)` on success; the caller adds those cycles
    /// to `cpu.cycles` and gives them to `apu.catch_up`. Returns `None` if
    /// any precondition fails; the caller falls through to normal dispatch.
    ///
    /// **Determinism:** see `docs/T10_IDLE_LOOP_DETECTION.md` §3 for the
    /// full argument. CPU/framebuffer state IS preserved by this path
    /// (verified by one-skip cap experiment: fb hash bit-identical to
    /// reference). The audio hash exhibits a residual divergence even
    /// when simulating the unskipped catch_up chunk pattern — see PR for
    /// the empirical data and follow-up. Gated behind the `idle-skip`
    /// Cargo feature, default off, until audio determinism is resolved.
    #[cfg(feature = "idle-skip")]
    fn try_idle_skip(&mut self, bus: &mut Bus) -> Option<u64> {
        // Master-cycle headroom we leave before the scanline boundary. An
        // LDA dp + BEQ taken pair costs about 36 master cycles in 8-bit
        // mode (3 CPU cycles each × 6 master/CPU); we want enough headroom
        // that two full iterations can complete before the inner while
        // loop exits, so the per-scanline overshoot pattern stays close to
        // what the unskipped path produces.
        const SAFETY_MARGIN: u64 = 80;
        const MIN_SKIP: u64 = 150;
        // Master cycles per CPU step we simulate during the skip. Matches the
        // dominant per-step elapsed in the unskipped path: LDA dp and BEQ taken
        // are each 3 CPU cycles × 6 master = 18. Crucial for audio determinism:
        // the SPC700 runs in chunked cycle-debt mode and is sensitive to the
        // chunk size delivered to `apu.catch_up`. Empirically, calling
        // catch_up(18) N times during a skip does NOT exactly reproduce the
        // unskipped audio hash — there is a residual divergence the
        // design doc flagged as "the most likely failure mode" (§6 risk 1).
        // Resolving that is a multi-session task; this implementation lives
        // behind the `idle-skip` Cargo feature, default off, so the
        // determinism contract stays green on `main`.
        const SIMULATED_STEP_CYCLES: u32 = 18;

        // Tier 1 requires 8-bit accumulator mode (M=1). SMW polls in M=1.
        // 16-bit polls are a Tier 2 follow-up.
        if !self.p.m {
            return None;
        }

        // Peek the four-byte pattern at PBR:PC. bus.read takes &mut because
        // some addresses have read side effects; the instruction stream is
        // ROM/WRAM in practice so these reads are pure, but we don't rely
        // on that — we only commit to the skip after the pure-memory check
        // on the polled address.
        let pc = self.pc;
        if bus.read(self.pbr, pc) != 0xA5 {
            return None;
        }
        let dp_offset = bus.read(self.pbr, pc.wrapping_add(1));
        if bus.read(self.pbr, pc.wrapping_add(2)) != 0xF0 {
            return None;
        }
        if bus.read(self.pbr, pc.wrapping_add(3)) != 0xFC {
            return None;
        }

        // Direct-page LDA reads from bank $00 at (DP + dp_offset).
        let polled_addr = self.dp.wrapping_add(dp_offset as u16);
        if !bus.is_pure_memory(0x00, polled_addr) {
            return None;
        }

        // Compute the skip budget. Saturating arithmetic guards against
        // current_scanline_target ever being unset (== 0) or behind cycles.
        let target = bus.current_scanline_target;
        let budget = target.saturating_sub(self.cycles);
        if budget <= SAFETY_MARGIN {
            return None;
        }
        let skip = budget - SAFETY_MARGIN;
        if skip < MIN_SKIP {
            return None;
        }

        // Project the post-skip A and N/Z flags. After the skip, the next
        // real LDA iteration would read whatever the polled byte holds
        // right now (since pure-memory means nothing has mutated it during
        // the skipped span). Pre-applying the load keeps the visible CPU
        // state identical to what the unskipped path produces.
        let byte = bus.read(0x00, polled_addr);
        self.a = (self.a & 0xFF00) | byte as u16;
        self.p.z = byte == 0;
        self.p.n = (byte & 0x80) != 0;

        // Drive the APU for the skipped cycles. catch_up is chunk-insensitive
        // (absolute cycle_target, not relative debt) so a single call produces
        // identical results to many small calls (distributive law holds).
        //
        // Check if the APU wrote to its output ports during the skip — if so,
        // the CPU would normally read those ports between loop iterations
        // (handshake protocol), and skipping those reads may corrupt comms.
        bus.apu.bus.ports_written_during_run = false;
        bus.apu.catch_up(skip as u32);
        self.cycles += skip;
        // Update last_apu_sync to match the new master_clock that the frame
        // loop will set (= cpu.cycles after this return). Without this,
        // sync_apu() would see a delta of `skip` and double-credit the APU.
        bus.last_apu_sync = self.cycles;

        if bus.apu.bus.ports_written_during_run {
            // The APU wrote to a port during the bulk skip. This means it
            // may be in the middle of a handshake expecting the CPU to read.
            // We can't undo the catch_up, but we stop skipping immediately
            // so the CPU resumes normal execution and reads the port.
            // The skip up to this point is already committed.
            //
            // TODO(T10): This is the hook point for subdivided skip. A future
            // refinement could cap the skip to the SPC instruction boundary
            // where the port write occurred, reducing the window where the
            // CPU misses a handshake. For now, the skip is committed in full
            // and the CPU resumes normal execution on the next instruction.
        }

        // PC is intentionally NOT advanced — we resume at the LDA. The
        // remaining few iterations cost ~30-60 master cycles total and
        // ensure the interrupt-pending check sees the same boundary it
        // would have in the unskipped run.
        self.idle_skip_hits += 1;
        self.idle_skip_cycles += skip;
        // Return 0: cpu.cycles and apu.catch_up are both already accounted
        // for above. Outer loop will do `cpu.cycles += 0` and
        // `apu.catch_up(0)`, both no-ops.
        Some(0)
    }

    /// Load the reset vector and initialize CPU state.
    pub fn reset(&mut self, bus: &mut Bus) {
        self.emulation = true;
        self.p = StatusRegister::new();
        self.sp = 0x01FF;
        self.dp = 0;
        self.pbr = 0;
        self.dbr = 0;
        self.a = 0;
        self.x = 0;
        self.y = 0;

        // Reset vector is at $00:FFFC (emulation mode vector).
        let lo = bus.read(0x00, 0xFFFC) as u16;
        let hi = bus.read(0x00, 0xFFFD) as u16;
        self.pc = lo | (hi << 8);

        eprintln!("CPU reset → PC = ${:04X}", self.pc);
    }

    /// Execute one instruction. Returns the number of master cycles consumed.
    pub fn step(&mut self, bus: &mut Bus) -> u64 {
        if self.stopped {
            return 6;
        }

        // Handle WAI: wake on NMI or IRQ
        if self.waiting {
            if self.nmi_pending || (self.irq_pending && !self.p.i) {
                self.waiting = false;
            } else {
                return 6;
            }
        }

        // Handle NMI (non-maskable, highest priority after reset)
        if self.nmi_pending {
            self.nmi_pending = false;
            self.handle_nmi(bus);
            return 7 * 6; // ~7 cycles × 6 master cycles each
        }

        // Handle IRQ
        if self.irq_pending && !self.p.i {
            self.irq_pending = false;
            self.handle_irq(bus);
            return 7 * 6;
        }

        // Idle-loop fast path (T10). Gated behind the `idle-skip` Cargo
        // feature — off by default. CPU semantics are correct under this
        // path (framebuffer hash preserved). The APU's absolute cycle target
        // makes catch_up chunk-insensitive (distributive law holds), but
        // audio still diverges under idle-skip due to stale APU port state
        // during the bulk skip. See `docs/T10_IDLE_LOOP_DETECTION.md` §9.2.
        #[cfg(feature = "idle-skip")]
        if let Some(skip) = self.try_idle_skip(bus) {
            return skip;
        }

        // Feature-gated CPU execution trace (compile-time, before fetch)
        #[cfg(feature = "cpu-trace")]
        {
            let op = bus.read(self.pbr, self.pc);
            eprintln!(
                "PC:{:02X}:{:04X} OP:{:02X} A:{:04X} X:{:04X} Y:{:04X} SP:{:04X} P:{:02X} DP:{:04X} DB:{:02X} E:{}",
                self.pbr, self.pc, op,
                self.a, self.x, self.y, self.sp,
                self.p.to_byte(self.emulation),
                self.dp, self.dbr,
                if self.emulation { 1 } else { 0 },
            );
        }

        // Fetch opcode
        let opcode = self.fetch_byte(bus);
        self.opcode_counts[opcode as usize] += 1;

        if self.trace {
            let name = tables::OPCODE_NAMES[opcode as usize];
            eprintln!(
                "{:02X}:{:04X} {:02X} {:<4}  A:{:04X} X:{:04X} Y:{:04X} SP:{:04X} DP:{:04X} DBR:{:02X} P:{}{}{}{}{}{}{}{}{}",
                self.pbr, self.pc.wrapping_sub(1), opcode, name,
                self.a, self.x, self.y, self.sp, self.dp, self.dbr,
                if self.emulation { 'E' } else { 'e' },
                if self.p.n { 'N' } else { 'n' },
                if self.p.v { 'V' } else { 'v' },
                if self.p.m { 'M' } else { 'm' },
                if self.p.x { 'X' } else { 'x' },
                if self.p.d { 'D' } else { 'd' },
                if self.p.i { 'I' } else { 'i' },
                if self.p.z { 'Z' } else { 'z' },
                if self.p.c { 'C' } else { 'c' },
            );
        }

        // Execute and get cycle count
        let cycles = instructions::execute(self, bus, opcode);

        // Convert CPU cycles to master cycles (×6 for slow bus, simplified)
        cycles as u64 * 6
    }

    fn handle_nmi(&mut self, bus: &mut Bus) {
        if self.emulation {
            self.push_byte(bus, (self.pc >> 8) as u8);
            self.push_byte(bus, self.pc as u8);
            self.push_byte(bus, self.p.to_byte(true));
            self.p.i = true;
            self.p.d = false;
            let lo = bus.read(0x00, 0xFFFA) as u16;
            let hi = bus.read(0x00, 0xFFFB) as u16;
            self.pc = lo | (hi << 8);
        } else {
            self.push_byte(bus, self.pbr);
            self.push_byte(bus, (self.pc >> 8) as u8);
            self.push_byte(bus, self.pc as u8);
            self.push_byte(bus, self.p.to_byte(false));
            self.p.i = true;
            self.p.d = false;
            self.pbr = 0;
            let lo = bus.read(0x00, 0xFFEA) as u16;
            let hi = bus.read(0x00, 0xFFEB) as u16;
            self.pc = lo | (hi << 8);
        }
    }

    fn handle_irq(&mut self, bus: &mut Bus) {
        if self.emulation {
            self.push_byte(bus, (self.pc >> 8) as u8);
            self.push_byte(bus, self.pc as u8);
            self.push_byte(bus, self.p.to_byte(true) & !0x10); // Clear B flag
            self.p.i = true;
            self.p.d = false;
            let lo = bus.read(0x00, 0xFFFE) as u16;
            let hi = bus.read(0x00, 0xFFFF) as u16;
            self.pc = lo | (hi << 8);
        } else {
            self.push_byte(bus, self.pbr);
            self.push_byte(bus, (self.pc >> 8) as u8);
            self.push_byte(bus, self.pc as u8);
            self.push_byte(bus, self.p.to_byte(false));
            self.p.i = true;
            self.p.d = false;
            self.pbr = 0;
            let lo = bus.read(0x00, 0xFFEE) as u16;
            let hi = bus.read(0x00, 0xFFEF) as u16;
            self.pc = lo | (hi << 8);
        }
    }

    // ── Register width helpers ──────────────────────────────────────────

    /// Is the accumulator in 8-bit mode?
    pub fn is_m8(&self) -> bool {
        self.emulation || self.p.m
    }

    /// Are index registers in 8-bit mode?
    pub fn is_x8(&self) -> bool {
        self.emulation || self.p.x
    }

    /// Update N and Z flags for an 8-bit result.
    pub fn update_nz8(&mut self, val: u8) {
        self.p.z = val == 0;
        self.p.n = val & 0x80 != 0;
    }

    /// Update N and Z flags for a 16-bit result.
    pub fn update_nz16(&mut self, val: u16) {
        self.p.z = val == 0;
        self.p.n = val & 0x8000 != 0;
    }

    /// Update N and Z flags based on current accumulator width.
    pub fn update_nz_a(&mut self, val: u16) {
        if self.is_m8() {
            self.update_nz8(val as u8);
        } else {
            self.update_nz16(val);
        }
    }

    /// Update N and Z flags based on current index width.
    pub fn update_nz_x(&mut self, val: u16) {
        if self.is_x8() {
            self.update_nz8(val as u8);
        } else {
            self.update_nz16(val);
        }
    }

    // ── Memory access ───────────────────────────────────────────────────

    /// Fetch a byte from [PBR:PC] and increment PC.
    pub fn fetch_byte(&mut self, bus: &mut Bus) -> u8 {
        let val = bus.read(self.pbr, self.pc);
        self.pc = self.pc.wrapping_add(1);
        val
    }

    /// Fetch a 16-bit word (little-endian) from [PBR:PC] and increment PC by 2.
    pub fn fetch_word(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch_byte(bus) as u16;
        let hi = self.fetch_byte(bus) as u16;
        lo | (hi << 8)
    }

    /// Fetch a 24-bit long address from [PBR:PC].
    pub fn fetch_long(&mut self, bus: &mut Bus) -> (u8, u16) {
        let addr = self.fetch_word(bus);
        let bank = self.fetch_byte(bus);
        (bank, addr)
    }

    // ── Stack operations ────────────────────────────────────────────────

    pub fn push_byte(&mut self, bus: &mut Bus, val: u8) {
        bus.write(0x00, self.sp, val);
        self.sp = self.sp.wrapping_sub(1);
        if self.emulation {
            self.sp = 0x0100 | (self.sp & 0xFF);
        }
    }

    pub fn push_word(&mut self, bus: &mut Bus, val: u16) {
        self.push_byte(bus, (val >> 8) as u8);
        self.push_byte(bus, val as u8);
    }

    pub fn pull_byte(&mut self, bus: &mut Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        if self.emulation {
            self.sp = 0x0100 | (self.sp & 0xFF);
        }
        bus.read(0x00, self.sp)
    }

    pub fn pull_word(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.pull_byte(bus) as u16;
        let hi = self.pull_byte(bus) as u16;
        lo | (hi << 8)
    }
}
