/// SPC700 timer emulation.
///
/// Three timers: T0 and T1 tick at 8 kHz (every 128 SPC cycles),
/// T2 ticks at 64 kHz (every 16 SPC cycles). Each has an 8-bit target
/// register and a 4-bit output counter that increments when the internal
/// divider reaches the target. Reading the counter clears it.

pub struct Timer {
    /// Target value ($FA-$FC). 0 means 256.
    pub target: u16,
    /// Internal divider (counts up to target).
    divider: u16,
    /// 4-bit output counter ($FD-$FF). Wraps at 0xF.
    pub counter: u8,
    /// Whether the timer is enabled (CONTROL register bits 0-2).
    pub enabled: bool,
    /// Debug: total number of times the counter incremented.
    pub fire_count: u32,
    /// Debug: total number of counter reads.
    pub read_count: u32,
}

impl Timer {
    pub fn new(target: u16) -> Self {
        Self { target, divider: 0, counter: 0, enabled: false, fire_count: 0, read_count: 0 }
    }

    /// Advance the timer by one tick at its native rate.
    /// Called every 128 SPC cycles for T0/T1, every 16 for T2.
    pub fn tick(&mut self) {
        if !self.enabled { return; }
        self.divider += 1;
        if self.divider >= self.target {
            self.divider = 0;
            self.counter = (self.counter + 1) & 0x0F;
            self.fire_count += 1;
        }
    }

    /// Read and clear the output counter.
    pub fn read_counter(&mut self) -> u8 {
        let val = self.counter;
        self.counter = 0;
        self.read_count += 1;
        val
    }
}
