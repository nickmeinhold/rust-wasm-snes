/// APU stub — SPC700 communication port emulation.
///
/// The SNES APU has 4 bidirectional ports ($2140-$2143). In real hardware,
/// each port has two latches: one for CPU→APU writes and one for APU→CPU reads.
/// Since we don't emulate the SPC700, we simulate the boot handshake protocol
/// so the game can upload its music driver without hanging.

pub struct ApuStub {
    /// APU → CPU ports (what the CPU reads from $2140-$2143).
    ports_to_cpu: [u8; 4],
    /// CPU → APU ports (what the CPU writes to $2140-$2143).
    ports_from_cpu: [u8; 4],
    /// Cycle counter to simulate APU response timing.
    cycle_counter: u64,
}

impl ApuStub {
    pub fn new() -> Self {
        Self {
            ports_to_cpu: [0xAA, 0xBB, 0, 0], // IPL ROM ready signal
            ports_from_cpu: [0; 4],
            cycle_counter: 0,
        }
    }

    /// CPU reads from $2140-$2143.
    pub fn read(&mut self, addr: u16) -> u8 {
        let port = (addr & 0x03) as usize;
        self.cycle_counter += 1;
        self.ports_to_cpu[port]
    }

    /// CPU writes to $2140-$2143.
    pub fn write(&mut self, addr: u16, val: u8) {
        let port = (addr & 0x03) as usize;
        self.ports_from_cpu[port] = val;

        // Port 0 is the handshake/command port.
        // The protocol: CPU writes a value, APU echoes it to acknowledge.
        // We echo immediately since we have no real SPC700 processing to do.
        if port == 0 {
            self.ports_to_cpu[0] = val;
        }

        // After the first $CC handshake (and subsequent transfers), the game
        // will poll for $AA/$BB again. We need to re-signal readiness.
        // When port 0 gets $CC, that starts a new upload session.
        // When we see any port 3 write (transfer start commands), note it.
        // After port 0 is echoed, schedule a return to $AA/$BB.
        if port == 0 && val != 0xCC {
            // After echoing a non-$CC value, we'll eventually need to
            // re-signal $AA/$BB. We do this lazily: if the game polls
            // port 0 and gets the echoed value, it proceeds. When it
            // eventually polls for $AA/$BB again, we need to provide it.
            // Solution: after a brief period, reset to $AA/$BB.
            self.cycle_counter = 0;
        }

        // If port 0 written with $CC (handshake start), echo it.
        // The game will then begin sending data with incrementing counters
        // on port 0, and we echo each one.
    }

    /// Call periodically to let the APU "process" and re-signal readiness.
    /// Returns true if ports were reset to the ready signal.
    pub fn tick(&mut self) -> bool {
        // If we've been running for a while since last port 0 write,
        // and port 0 isn't already $AA, reset to ready state.
        if self.ports_to_cpu[0] != 0xAA && self.cycle_counter > 256 {
            self.ports_to_cpu[0] = 0xAA;
            self.ports_to_cpu[1] = 0xBB;
            return true;
        }
        false
    }
}
