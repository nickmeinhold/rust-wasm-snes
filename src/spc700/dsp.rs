/// S-DSP (Sound Digital Signal Processor) emulation.
///
/// The DSP is a peripheral of the SPC700, accessed via registers $F2/$F3.
/// It has 128 registers controlling 8 voices, echo, noise, and mixing.
/// Each voice plays BRR-compressed samples with pitch control and ADSR/GAIN
/// envelopes. Output is stereo 16-bit at 32 kHz.

/// DSP register addresses (per-voice: add voice*0x10).
const V_VOL_L: u8 = 0x00;
const V_VOL_R: u8 = 0x01;
const V_PITCH_L: u8 = 0x02;
const V_PITCH_H: u8 = 0x03;
const V_SRCN: u8 = 0x04;
const V_ADSR1: u8 = 0x05;
const V_ADSR2: u8 = 0x06;
const V_GAIN: u8 = 0x07;
const V_ENVX: u8 = 0x08;
const V_OUTX: u8 = 0x09;

/// Global DSP registers.
const MVOL_L: u8 = 0x0C;
const MVOL_R: u8 = 0x1C;
const EVOL_L: u8 = 0x2C;
const EVOL_R: u8 = 0x3C;
const KON: u8 = 0x4C;
const KOFF: u8 = 0x5C;
const FLG: u8 = 0x6C;
const ENDX: u8 = 0x7C;
const EFB: u8 = 0x0D;
const NON: u8 = 0x3D;
const EON: u8 = 0x4D;
const DIR: u8 = 0x5D;
const ESA: u8 = 0x6D;
const EDL: u8 = 0x7D;
const FIR_BASE: u8 = 0x0F; // FIR coefficients at $xF (x=0..7)

/// ADSR envelope rates (in DSP ticks). Index by rate value 0-31.
/// Rate 0 = infinity (never advance). Higher values = faster rates.
const RATE_TABLE: [u16; 32] = [
    0, 2048, 1536, 1280, 1024, 768, 640, 512,
    384, 320, 256, 192, 160, 128, 96, 80,
    64, 48, 40, 32, 24, 20, 16, 12,
    10, 8, 6, 5, 4, 3, 2, 1,
];

#[derive(Clone, Copy, PartialEq)]
enum EnvPhase { Attack, Decay, Sustain, Release, Off }

#[derive(Clone)]
struct Voice {
    /// Volume (signed 8-bit, applied per-channel).
    vol_l: i8,
    vol_r: i8,
    /// 14-bit pitch (frequency control).
    pitch: u16,
    /// Source number (indexes the sample directory).
    srcn: u8,
    /// ADSR1/ADSR2 register values.
    adsr1: u8,
    adsr2: u8,
    /// GAIN register value.
    gain: u8,

    /// Current envelope level (0-0x7FF, 11-bit).
    env_level: i32,
    /// Envelope phase.
    env_phase: EnvPhase,
    /// Envelope tick counter (counts down from rate).
    env_counter: u16,

    /// BRR decoding state.
    brr_addr: u16,       // Current BRR block address in APU RAM
    brr_offset: u8,      // Sample index within current block (0-15)
    brr_buffer: [i16; 16], // Decoded samples for current block
    brr_old: [i16; 2],   // Previous two samples for BRR filter

    /// Pitch counter (fractional sample position, 12-bit fraction).
    pitch_counter: u16,

    /// Whether the voice is keyed on.
    key_on: bool,
    /// Set when BRR end flag is encountered.
    end_flag: bool,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            vol_l: 0, vol_r: 0, pitch: 0, srcn: 0,
            adsr1: 0, adsr2: 0, gain: 0,
            env_level: 0, env_phase: EnvPhase::Off, env_counter: 0,
            brr_addr: 0, brr_offset: 0, brr_buffer: [0; 16], brr_old: [0; 2],
            pitch_counter: 0, key_on: false, end_flag: false,
        }
    }
}

pub struct Dsp {
    /// Raw register file (128 bytes).
    pub regs: [u8; 128],
    /// DSP address latch ($F2).
    pub addr_reg: u8,
    /// 8 voices.
    voices: [Voice; 8],
    /// Echo buffer position.
    echo_pos: u16,
    /// Echo buffer length in bytes.
    echo_length: u16,
    /// Previous echo samples for FIR filter (stereo, 8 taps).
    echo_hist_l: [i16; 8],
    echo_hist_r: [i16; 8],
    echo_hist_pos: usize,
    /// Global tick counter for envelope timing.
    tick_counter: u32,
}

impl Dsp {
    pub fn new() -> Self {
        Self {
            regs: [0; 128],
            addr_reg: 0,
            voices: std::array::from_fn(|_| Voice::default()),
            echo_pos: 0,
            echo_length: 0,
            echo_hist_l: [0; 8],
            echo_hist_r: [0; 8],
            echo_hist_pos: 0,
            tick_counter: 0,
        }
    }

    /// Read a DSP register.
    pub fn read(&self, addr: u8) -> u8 {
        let addr = addr & 0x7F;
        match addr {
            ENDX => self.regs[ENDX as usize],
            _ => {
                let voice = (addr >> 4) as usize;
                let reg = addr & 0x0F;
                if voice < 8 {
                    match reg {
                        0x08 => (self.voices[voice].env_level >> 4) as u8, // ENVX
                        0x09 => 0, // OUTX (stub — would need last sample)
                        _ => self.regs[addr as usize],
                    }
                } else {
                    self.regs[addr as usize]
                }
            }
        }
    }

    /// Write a DSP register.
    pub fn write(&mut self, addr: u8, val: u8) {
        let addr = addr & 0x7F;
        self.regs[addr as usize] = val;

        let voice_idx = (addr >> 4) as usize;
        let reg = addr & 0x0F;

        // Update voice state from register writes.
        if voice_idx < 8 {
            let v = &mut self.voices[voice_idx];
            match reg {
                0x00 => v.vol_l = val as i8,
                0x01 => v.vol_r = val as i8,
                0x02 => v.pitch = (v.pitch & 0x3F00) | val as u16,
                0x03 => v.pitch = (v.pitch & 0x00FF) | ((val as u16 & 0x3F) << 8),
                0x04 => v.srcn = val,
                0x05 => v.adsr1 = val,
                0x06 => v.adsr2 = val,
                0x07 => v.gain = val,
                _ => {}
            }
        }

        // Handle global register writes.
        match addr {
            KON => {
                for i in 0..8 {
                    if val & (1 << i) != 0 {
                        self.key_on_voice(i);
                    }
                }
            }
            KOFF => {
                for i in 0..8 {
                    if val & (1 << i) != 0 {
                        self.voices[i].env_phase = EnvPhase::Release;
                    }
                }
            }
            ENDX => {
                // Writing any value clears ENDX.
                self.regs[ENDX as usize] = 0;
            }
            EDL => {
                let delay = (val & 0x0F) as u16;
                self.echo_length = delay * 2048; // Each unit = 2KB
            }
            _ => {}
        }
    }

    /// Key on a voice: reset BRR decoding and start attack phase.
    fn key_on_voice(&mut self, idx: usize) {
        let v = &mut self.voices[idx];
        v.key_on = true;
        v.end_flag = false;
        v.env_level = 0;
        v.env_phase = EnvPhase::Attack;
        v.env_counter = 0;
        v.pitch_counter = 0;
        v.brr_offset = 0;
        v.brr_old = [0; 2];
        // BRR start address will be looked up from the sample directory.
    }

    /// Generate one stereo sample pair (called at 32 kHz).
    pub fn generate_sample(&mut self, ram: &[u8; 65536]) -> (i16, i16) {
        let dir_base = (self.regs[DIR as usize] as u16) << 8;
        let mute = self.regs[FLG as usize] & 0x40 != 0;
        let noise_clock = self.regs[FLG as usize] & 0x1F;
        let _ = noise_clock; // TODO: noise generation

        let mut mix_l: i32 = 0;
        let mut mix_r: i32 = 0;
        let mut echo_in_l: i32 = 0;
        let mut echo_in_r: i32 = 0;

        for i in 0..8u8 {
            let v = &mut self.voices[i as usize];
            if v.env_phase == EnvPhase::Off { continue; }

            // Look up sample directory entry if starting a new sample.
            if v.key_on && v.brr_offset == 0 && v.pitch_counter == 0 {
                let dir_entry = dir_base.wrapping_add((v.srcn as u16) * 4);
                let start = ram[dir_entry as usize] as u16
                    | ((ram[dir_entry.wrapping_add(1) as usize] as u16) << 8);
                v.brr_addr = start;
                v.key_on = false;
                // Decode first BRR block.
                Self::decode_brr_block(ram, v);
            }

            // Get interpolated sample from BRR buffer.
            let sample_idx = (v.pitch_counter >> 12) as usize;
            let sample = if sample_idx < 16 {
                v.brr_buffer[sample_idx] as i32
            } else {
                0
            };

            // Apply envelope.
            let env = v.env_level;
            let output = (sample * env) >> 11;

            // Apply per-voice volume and accumulate.
            mix_l += (output * v.vol_l as i32) >> 7;
            mix_r += (output * v.vol_r as i32) >> 7;

            // Echo input.
            if self.regs[EON as usize] & (1 << i) != 0 {
                echo_in_l += output;
                echo_in_r += output;
            }

            // Advance pitch counter.
            v.pitch_counter = v.pitch_counter.wrapping_add(v.pitch);

            // Crossed a BRR sample boundary?
            while v.pitch_counter >= 0x4000 && v.env_phase != EnvPhase::Off {
                v.pitch_counter -= 0x4000;
                v.brr_offset += 1;
                if v.brr_offset >= 16 {
                    v.brr_offset = 0;
                    // Advance to next BRR block.
                    let header = ram[v.brr_addr as usize];
                    let is_end = header & 0x01 != 0;
                    let is_loop = header & 0x02 != 0;
                    if is_end {
                        self.regs[ENDX as usize] |= 1 << i;
                        if is_loop {
                            // Loop: jump to loop address from directory.
                            let dir_entry = dir_base.wrapping_add((v.srcn as u16) * 4);
                            let loop_addr = ram[dir_entry.wrapping_add(2) as usize] as u16
                                | ((ram[dir_entry.wrapping_add(3) as usize] as u16) << 8);
                            v.brr_addr = loop_addr;
                        } else {
                            v.env_phase = EnvPhase::Off;
                            v.env_level = 0;
                            break;
                        }
                    } else {
                        v.brr_addr = v.brr_addr.wrapping_add(9);
                    }
                    Self::decode_brr_block(ram, v);
                }
            }

            // Update envelope.
            Self::update_envelope(v);
        }

        self.tick_counter += 1;

        // Apply master volume.
        let mvol_l = self.regs[MVOL_L as usize] as i8 as i32;
        let mvol_r = self.regs[MVOL_R as usize] as i8 as i32;

        if mute {
            return (0, 0);
        }

        // Simple echo processing (FIR filter on echo buffer).
        let echo_out = self.process_echo(ram, echo_in_l as i16, echo_in_r as i16);

        let evol_l = self.regs[EVOL_L as usize] as i8 as i32;
        let evol_r = self.regs[EVOL_R as usize] as i8 as i32;

        let out_l = ((mix_l * mvol_l) >> 7) + ((echo_out.0 as i32 * evol_l) >> 7);
        let out_r = ((mix_r * mvol_r) >> 7) + ((echo_out.1 as i32 * evol_r) >> 7);

        (out_l.clamp(-32768, 32767) as i16, out_r.clamp(-32768, 32767) as i16)
    }

    /// Decode one 9-byte BRR block into 16 samples.
    fn decode_brr_block(ram: &[u8; 65536], v: &mut Voice) {
        let addr = v.brr_addr as usize;
        let header = ram[addr & 0xFFFF];
        let shift = (header >> 4) & 0x0F;
        let filter = (header >> 2) & 0x03;

        for i in 0..16 {
            let byte = ram[(addr + 1 + i / 2) & 0xFFFF];
            let nibble = if i & 1 == 0 { byte >> 4 } else { byte & 0x0F };

            // Sign-extend 4-bit to 16-bit.
            let mut s = ((nibble as i16) << 12) >> 12;

            // Apply shift.
            if shift <= 12 {
                s = (s << shift) >> 1;
            } else {
                s = if s < 0 { -(1 << 11) } else { 0 };
            }

            // Apply prediction filter using previous samples.
            let p1 = v.brr_old[0] as i32;
            let p2 = v.brr_old[1] as i32;

            let filtered = s as i32 + match filter {
                0 => 0,
                1 => p1 - (p1 >> 4),
                2 => p1 * 2 + ((-p1 * 3) >> 5) - p2 + (p2 >> 4),
                3 => p1 * 2 + ((-p1 * 13) >> 6) - p2 + ((p2 * 3) >> 4),
                _ => 0,
            };

            let clamped = filtered.clamp(-32768, 32767) as i16;
            v.brr_old[1] = v.brr_old[0];
            v.brr_old[0] = clamped;
            v.brr_buffer[i] = clamped;
        }
    }

    /// Update ADSR/GAIN envelope for one voice.
    fn update_envelope(v: &mut Voice) {
        if v.env_phase == EnvPhase::Off { return; }

        let use_adsr = v.adsr1 & 0x80 != 0;

        if use_adsr {
            match v.env_phase {
                EnvPhase::Attack => {
                    let rate = ((v.adsr1 & 0x0F) as u16) * 2 + 1;
                    let step = if rate == 31 { 1024 } else { 32 };
                    if Self::env_tick(v, rate) {
                        v.env_level += step;
                        if v.env_level >= 0x7FF {
                            v.env_level = 0x7FF;
                            v.env_phase = EnvPhase::Decay;
                        }
                    }
                }
                EnvPhase::Decay => {
                    let rate = ((v.adsr1 >> 4) & 0x07) as u16 * 2 + 16;
                    if Self::env_tick(v, rate) {
                        v.env_level -= ((v.env_level - 1) >> 8) + 1;
                        let sustain_level = ((v.adsr2 >> 5) as i32 + 1) * 0x100;
                        if v.env_level <= sustain_level {
                            v.env_phase = EnvPhase::Sustain;
                        }
                    }
                }
                EnvPhase::Sustain => {
                    let rate = (v.adsr2 & 0x1F) as u16;
                    if rate > 0 && Self::env_tick(v, rate) {
                        v.env_level -= ((v.env_level - 1) >> 8) + 1;
                        if v.env_level <= 0 {
                            v.env_level = 0;
                            v.env_phase = EnvPhase::Off;
                        }
                    }
                }
                EnvPhase::Release => {
                    // Release always decrements by 8 every sample.
                    v.env_level -= 8;
                    if v.env_level <= 0 {
                        v.env_level = 0;
                        v.env_phase = EnvPhase::Off;
                    }
                }
                EnvPhase::Off => {}
            }
        } else {
            // GAIN mode.
            let mode = v.gain;
            if mode & 0x80 == 0 {
                // Direct: set envelope level immediately.
                v.env_level = ((mode & 0x7F) as i32) << 4;
            } else {
                let rate = (mode & 0x1F) as u16;
                if rate > 0 && Self::env_tick(v, rate) {
                    match (mode >> 5) & 0x03 {
                        0 => { // Linear decrease
                            v.env_level -= 32;
                        }
                        1 => { // Exponential decrease
                            v.env_level -= ((v.env_level - 1) >> 8) + 1;
                        }
                        2 => { // Linear increase
                            v.env_level += 32;
                        }
                        3 => { // Bent increase
                            v.env_level += if v.env_level < 0x600 { 32 } else { 8 };
                        }
                        _ => {}
                    }
                }
            }
            v.env_level = v.env_level.clamp(0, 0x7FF);
            if v.env_level == 0 && v.env_phase == EnvPhase::Release {
                v.env_phase = EnvPhase::Off;
            }
        }
    }

    /// Check if the envelope should tick based on the rate.
    fn env_tick(v: &mut Voice, rate: u16) -> bool {
        if rate == 0 || rate as usize >= RATE_TABLE.len() { return false; }
        let period = RATE_TABLE[rate as usize];
        if period == 0 { return true; }
        v.env_counter += 1;
        if v.env_counter >= period {
            v.env_counter = 0;
            true
        } else {
            false
        }
    }

    /// Process echo: read from echo buffer, apply FIR, write back.
    fn process_echo(&mut self, ram: &[u8; 65536], input_l: i16, input_r: i16) -> (i16, i16) {
        if self.echo_length == 0 {
            return (0, 0);
        }

        let esa = (self.regs[ESA as usize] as u16) << 8;
        let pos = esa.wrapping_add(self.echo_pos) as usize;

        // Read echo sample from buffer.
        let echo_l = ram[pos & 0xFFFF] as i16 | ((ram[(pos + 1) & 0xFFFF] as i16) << 8);
        let echo_r = ram[(pos + 2) & 0xFFFF] as i16 | ((ram[(pos + 3) & 0xFFFF] as i16) << 8);

        // Store in FIR history.
        self.echo_hist_l[self.echo_hist_pos] = echo_l;
        self.echo_hist_r[self.echo_hist_pos] = echo_r;

        // Apply 8-tap FIR filter.
        let mut fir_l: i32 = 0;
        let mut fir_r: i32 = 0;
        for tap in 0..8 {
            let coeff = self.regs[tap * 0x10 + FIR_BASE as usize] as i8 as i32;
            let hist_idx = (self.echo_hist_pos + 8 - tap) & 7;
            fir_l += (self.echo_hist_l[hist_idx] as i32 * coeff) >> 7;
            fir_r += (self.echo_hist_r[hist_idx] as i32 * coeff) >> 7;
        }

        self.echo_hist_pos = (self.echo_hist_pos + 1) & 7;

        // Write back to echo buffer (input + feedback).
        // Only write if echo write is not disabled (FLG bit 5).
        if self.regs[FLG as usize] & 0x20 == 0 {
            let efb = self.regs[EFB as usize] as i8 as i32;
            let write_l = (input_l as i32 + ((fir_l * efb) >> 7)).clamp(-32768, 32767) as i16;
            let write_r = (input_r as i32 + ((fir_r * efb) >> 7)).clamp(-32768, 32767) as i16;
            // Note: We'd write to RAM here, but since APU RAM is owned by Apu,
            // we skip echo writes for now. Full implementation needs &mut ram.
            let _ = (write_l, write_r);
        }

        // Advance echo position.
        self.echo_pos += 4;
        if self.echo_pos >= self.echo_length {
            self.echo_pos = 0;
        }

        (fir_l.clamp(-32768, 32767) as i16, fir_r.clamp(-32768, 32767) as i16)
    }
}
