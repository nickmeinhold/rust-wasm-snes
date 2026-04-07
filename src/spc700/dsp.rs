/// S-DSP emulation — based on blargg's snes_spc reference implementation.
///
/// The DSP generates stereo audio at 32 kHz using 8 voices, each playing
/// BRR-compressed samples with pitch control and ADSR/GAIN envelopes.
/// Features 4-point Gaussian interpolation, 8-tap FIR echo filter.

// ─── Gaussian interpolation table (512 entries) ─────────────────────
// Left half of the Gaussian bell curve. The DSP indexes this with a
// forward and reverse pointer to get 4 interpolation coefficients.
#[rustfmt::skip]
const GAUSS: [i16; 512] = [
       0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
       1,   1,   1,   1,   1,   1,   1,   1,   1,   1,   1,   2,   2,   2,   2,   2,
       2,   2,   3,   3,   3,   3,   3,   4,   4,   4,   4,   4,   5,   5,   5,   5,
       6,   6,   6,   6,   7,   7,   7,   8,   8,   8,   9,   9,   9,  10,  10,  10,
      11,  11,  11,  12,  12,  13,  13,  14,  14,  15,  15,  15,  16,  16,  17,  17,
      18,  19,  19,  20,  20,  21,  21,  22,  23,  23,  24,  24,  25,  26,  27,  27,
      28,  29,  29,  30,  31,  32,  32,  33,  34,  35,  36,  36,  37,  38,  39,  40,
      41,  42,  43,  44,  45,  46,  47,  48,  49,  50,  51,  52,  53,  54,  55,  56,
      58,  59,  60,  61,  62,  64,  65,  66,  67,  69,  70,  71,  73,  74,  76,  77,
      78,  80,  81,  83,  84,  86,  87,  89,  90,  92,  94,  95,  97,  99, 100, 102,
     104, 106, 107, 109, 111, 113, 115, 117, 118, 120, 122, 124, 126, 128, 130, 132,
     134, 137, 139, 141, 143, 145, 147, 150, 152, 154, 156, 159, 161, 163, 166, 168,
     171, 173, 175, 178, 180, 183, 186, 188, 191, 193, 196, 199, 201, 204, 207, 210,
     212, 215, 218, 221, 224, 227, 230, 233, 236, 239, 242, 245, 248, 251, 254, 257,
     260, 263, 267, 270, 273, 276, 280, 283, 286, 290, 293, 297, 300, 304, 307, 311,
     314, 318, 321, 325, 328, 332, 336, 339, 343, 347, 351, 354, 358, 362, 366, 370,
     374, 378, 381, 385, 389, 393, 397, 401, 405, 410, 414, 418, 422, 426, 430, 434,
     439, 443, 447, 451, 456, 460, 464, 469, 473, 477, 482, 486, 491, 495, 499, 504,
     508, 513, 517, 522, 527, 531, 536, 540, 545, 550, 554, 559, 563, 568, 573, 577,
     582, 587, 592, 596, 601, 606, 611, 615, 620, 625, 630, 635, 640, 644, 649, 654,
     659, 664, 669, 674, 678, 683, 688, 693, 698, 703, 708, 713, 718, 723, 728, 732,
     737, 742, 747, 752, 757, 762, 767, 772, 777, 782, 787, 792, 797, 802, 806, 811,
     816, 821, 826, 831, 836, 841, 846, 851, 855, 860, 865, 870, 875, 880, 884, 889,
     894, 899, 904, 908, 913, 918, 923, 927, 932, 937, 941, 946, 951, 955, 960, 965,
     969, 974, 978, 983, 988, 992, 997,1001,1005,1010,1014,1019,1023,1027,1032,1036,
    1040,1045,1049,1053,1057,1061,1066,1070,1074,1078,1082,1086,1090,1094,1098,1102,
    1106,1109,1113,1117,1121,1125,1128,1132,1136,1139,1143,1146,1150,1153,1157,1160,
    1164,1167,1170,1174,1177,1180,1183,1186,1190,1193,1196,1199,1202,1205,1207,1210,
    1213,1216,1219,1221,1224,1227,1229,1232,1234,1237,1239,1241,1244,1246,1248,1251,
    1253,1255,1257,1259,1261,1263,1265,1267,1269,1270,1272,1274,1275,1277,1279,1280,
    1282,1283,1284,1286,1287,1288,1290,1291,1292,1293,1294,1295,1296,1297,1297,1298,
    1299,1300,1300,1301,1302,1302,1303,1303,1303,1304,1304,1304,1304,1304,1305,1305,
];

// ─── Envelope rate periods (global counter system from blargg) ──────
const COUNTER_RATES: [u16; 32] = [
    30721, 2048, 1536, 1280, 1024, 768, 640, 512,
    384, 320, 256, 192, 160, 128, 96, 80,
    64, 48, 40, 32, 24, 20, 16, 12,
    10, 8, 6, 5, 4, 3, 2, 1,
];

const COUNTER_OFFSETS: [u16; 32] = [
    1, 0, 1040, 536, 0, 1040, 536, 0,
    1040, 536, 0, 1040, 536, 0, 1040, 536,
    0, 1040, 536, 0, 1040, 536, 0, 1040,
    536, 0, 1040, 536, 0, 1040, 0, 0,
];

/// DSP register addresses.
const KON: u8 = 0x4C;
const KOFF: u8 = 0x5C;
const FLG: u8 = 0x6C;
const ENDX: u8 = 0x7C;
const MVOL_L: u8 = 0x0C;
const MVOL_R: u8 = 0x1C;
const EVOL_L: u8 = 0x2C;
const EVOL_R: u8 = 0x3C;
const EFB: u8 = 0x0D;
const EON: u8 = 0x4D;
const DIR: u8 = 0x5D;
const ESA: u8 = 0x6D;
const EDL: u8 = 0x7D;
const FIR_BASE: u8 = 0x0F;

/// Clamp to signed 16-bit range (blargg's CLAMP16 macro).
fn clamp16(v: i32) -> i32 {
    if v as i16 as i32 != v {
        (v >> 31) ^ 0x7FFF
    } else {
        v
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EnvPhase { Attack, Decay, Sustain, Release, Off }

/// Per-voice state.
#[derive(Clone)]
struct Voice {
    vol_l: i8,
    vol_r: i8,
    pitch: u16,
    srcn: u8,
    adsr1: u8,
    adsr2: u8,
    gain: u8,

    env_level: i32,
    hidden_env: i32,
    env_phase: EnvPhase,

    /// BRR decode ring buffer (12 entries, wrapping).
    brr_buf: [i32; 12],
    brr_buf_pos: usize,
    brr_addr: u16,
    brr_header: u8,

    /// Pitch interpolation position (bits 15-12: sample index, 11-0: fraction).
    interp_pos: i32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            vol_l: 0, vol_r: 0, pitch: 0, srcn: 0,
            adsr1: 0, adsr2: 0, gain: 0,
            env_level: 0, hidden_env: 0, env_phase: EnvPhase::Off,
            brr_buf: [0; 12], brr_buf_pos: 0, brr_addr: 0, brr_header: 0,
            interp_pos: 0,
        }
    }
}

pub struct Dsp {
    pub regs: [u8; 128],
    pub addr_reg: u8,
    voices: [Voice; 8],
    global_counter: u32,
    echo_pos: u16,
    echo_length: u16,
    echo_hist_l: [i32; 8],
    echo_hist_r: [i32; 8],
    echo_hist_pos: usize,
    noise: i16,
    new_kon: u8,
    pub debug_log: Vec<String>,
}

impl Dsp {
    pub fn dump_voices(&self) -> String { String::new() }

    pub fn new() -> Self {
        Self {
            regs: [0; 128],
            addr_reg: 0,
            voices: std::array::from_fn(|_| Voice::default()),
            global_counter: 0,
            echo_pos: 0,
            echo_length: 0,
            echo_hist_l: [0; 8],
            echo_hist_r: [0; 8],
            echo_hist_pos: 0,
            noise: -(1 << 14),
            new_kon: 0,
            debug_log: Vec::new(),
        }
    }

    pub fn read(&self, addr: u8) -> u8 {
        let addr = addr & 0x7F;
        let voice = (addr >> 4) as usize;
        let reg = addr & 0x0F;
        if voice < 8 && reg == 0x08 {
            (self.voices[voice].env_level >> 4) as u8
        } else {
            self.regs[addr as usize]
        }
    }

    pub fn write(&mut self, addr: u8, val: u8) {
        let addr = addr & 0x7F;
        self.regs[addr as usize] = val;

        let vi = (addr >> 4) as usize;
        let reg = addr & 0x0F;

        if vi < 8 {
            let v = &mut self.voices[vi];
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

        match addr {
            KON => { self.new_kon = val; }
            KOFF => {
                for i in 0..8 { if val & (1 << i) != 0 { self.voices[i].env_phase = EnvPhase::Release; } }
            }
            ENDX => { self.regs[ENDX as usize] = 0; }
            EDL => { self.echo_length = (val & 0x0F) as u16 * 2048; }
            _ => {}
        }
    }

    /// Check if a rate fires this tick (global counter system).
    fn rate_fires(&self, rate: usize) -> bool {
        if rate == 0 || rate >= 32 { return false; }
        let period = COUNTER_RATES[rate] as u32;
        if period == 0 { return true; }
        (self.global_counter.wrapping_add(COUNTER_OFFSETS[rate] as u32)) % period == 0
    }

    /// Decode one BRR block (16 samples) into the voice's ring buffer.
    /// Uses blargg's half-precision scheme with clamp-then-double.
    fn decode_brr(ram: &[u8; 65536], v: &mut Voice) {
        let addr = v.brr_addr as usize;
        let header = ram[addr & 0xFFFF];
        v.brr_header = header;
        let shift = (header >> 4) & 0x0F;
        let filter = (header >> 2) & 0x03;

        for i in 0..16usize {
            let byte = ram[(addr + 1 + i / 2) & 0xFFFF];
            let nibble = if i & 1 == 0 { byte >> 4 } else { byte & 0x0F };

            let mut s = (((nibble as i8) << 4) >> 4) as i32;

            s = if shift <= 12 {
                (s << shift) >> 1
            } else if s < 0 {
                -2048
            } else {
                0
            };

            let p1_idx = (v.brr_buf_pos + 12 - 1) % 12;
            let p2_idx = (v.brr_buf_pos + 12 - 2) % 12;
            let p1 = v.brr_buf[p1_idx];
            let p2 = v.brr_buf[p2_idx] >> 1;

            match filter {
                1 => {
                    s += p1 >> 1;
                    s += (-p1) >> 5;
                }
                2 => {
                    s += p1;
                    s -= p2;
                    s += p2 >> 4;
                    s += (p1 * -3) >> 6;
                }
                3 => {
                    s += p1;
                    s -= p2;
                    s += (p1 * -13) >> 7;
                    s += (p2 * 3) >> 4;
                }
                _ => {}
            }

            s = clamp16(s);
            s = (s as i16).wrapping_mul(2) as i32;

            v.brr_buf[v.brr_buf_pos] = s;
            v.brr_buf_pos = (v.brr_buf_pos + 1) % 12;
        }
    }

    /// Generate one stereo sample pair (called at 32 kHz).
    pub fn generate_sample(&mut self, ram: &[u8; 65536]) -> (i16, i16) {
        let dir_base = (self.regs[DIR as usize] as u16) << 8;
        let mute = self.regs[FLG as usize] & 0x40 != 0;

        // Process KON.
        let kon = self.new_kon;
        self.new_kon = 0;
        for i in 0..8u8 {
            if kon & (1 << i) != 0 {
                let v = &mut self.voices[i as usize];
                v.env_phase = EnvPhase::Attack;
                v.env_level = 0;
                v.hidden_env = 0;
                v.interp_pos = 0;
                v.brr_buf = [0; 12];
                v.brr_buf_pos = 0;
                let dir_entry = dir_base.wrapping_add((v.srcn as u16) * 4);
                v.brr_addr = ram[dir_entry as usize] as u16
                    | ((ram[dir_entry.wrapping_add(1) as usize] as u16) << 8);
                Self::decode_brr(ram, v);
                self.regs[ENDX as usize] &= !(1 << i);
            }
        }

        // Noise LFSR.
        let noise_rate = (self.regs[FLG as usize] & 0x1F) as usize;
        if noise_rate > 0 && self.rate_fires(noise_rate) {
            let bit = (self.noise as i32 >> 13) ^ (self.noise as i32 >> 14);
            self.noise = (((self.noise as i32) << 1) | (bit & 1)) as i16;
        }

        let noise_enabled = self.regs[0x3D];
        let echo_on = self.regs[EON as usize];

        let mut main_l: i32 = 0;
        let mut main_r: i32 = 0;
        let mut echo_l: i32 = 0;
        let mut echo_r: i32 = 0;

        for i in 0..8u8 {
            let v = &mut self.voices[i as usize];
            if v.env_phase == EnvPhase::Off { continue; }

            // ── Interpolated sample ───────────────────────────
            let output = if noise_enabled & (1 << i) != 0 {
                (self.noise as i32) * 2
            } else {
                let offset = ((v.interp_pos >> 4) & 0xFF) as usize;
                let base = ((v.interp_pos >> 12) & 0x03) as usize;

                // Read 4 samples from ring buffer for Gaussian interpolation.
                let s = |n: usize| -> i32 { v.brr_buf[(v.brr_buf_pos + 12 - 4 + base + n) % 12] };

                let mut out = (GAUSS[255 - offset] as i32 * s(0)) >> 11;
                out += (GAUSS[511 - offset] as i32 * s(1)) >> 11;
                out += (GAUSS[256 + offset] as i32 * s(2)) >> 11;
                out = out as i16 as i32; // 16-bit wrap after 3 terms (matches blargg)
                out += (GAUSS[offset] as i32 * s(3)) >> 11;
                clamp16(out) & !1
            };

            // ── Envelope × sample ─────────────────────────────
            let amp = ((output * v.env_level) >> 11) & !1;

            // ── Volume and accumulate ─────────────────────────
            let left = (amp * v.vol_l as i32) >> 7;
            let right = (amp * v.vol_r as i32) >> 7;
            main_l = clamp16(main_l + left);
            main_r = clamp16(main_r + right);

            if echo_on & (1 << i) != 0 {
                echo_l = clamp16(echo_l + left);
                echo_r = clamp16(echo_r + right);
            }

            // ── Advance pitch ─────────────────────────────────
            v.interp_pos = (v.interp_pos & 0x3FFF) + v.pitch as i32;
            if v.interp_pos > 0x7FFF { v.interp_pos = 0x7FFF; }

            while v.interp_pos >= 0x4000 {
                v.interp_pos -= 0x4000;
                let is_end = v.brr_header & 0x01 != 0;
                let is_loop = v.brr_header & 0x02 != 0;
                if is_end {
                    self.regs[ENDX as usize] |= 1 << i;
                    if is_loop {
                        let dir_entry = dir_base.wrapping_add((v.srcn as u16) * 4);
                        v.brr_addr = ram[(dir_entry + 2) as usize] as u16
                            | ((ram[(dir_entry + 3) as usize] as u16) << 8);
                    } else {
                        v.env_phase = EnvPhase::Off;
                        v.env_level = 0;
                        break;
                    }
                } else {
                    v.brr_addr = v.brr_addr.wrapping_add(9);
                }
                Self::decode_brr(ram, v);
            }

            // ── Envelope update ───────────────────────────────
            Self::update_envelope_step(v, self.global_counter);
        }

        self.global_counter = self.global_counter.wrapping_add(1);

        if mute { return (0, 0); }

        // ── Echo ──────────────────────────────────────────────
        let echo_out = self.process_echo(ram, echo_l as i16, echo_r as i16);

        // ── Final mix ─────────────────────────────────────────
        let mvol_l = self.regs[MVOL_L as usize] as i8 as i32;
        let mvol_r = self.regs[MVOL_R as usize] as i8 as i32;
        let evol_l = self.regs[EVOL_L as usize] as i8 as i32;
        let evol_r = self.regs[EVOL_R as usize] as i8 as i32;

        let out_l = clamp16(((main_l * mvol_l) >> 7) + ((echo_out.0 as i32 * evol_l) >> 7));
        let out_r = clamp16(((main_r * mvol_r) >> 7) + ((echo_out.1 as i32 * evol_r) >> 7));

        (out_l as i16, out_r as i16)
    }

    /// Update envelope for one voice (static to avoid borrow issues).
    fn update_envelope_step(v: &mut Voice, counter: u32) {
        // Helper: check rate against global counter.
        let fires = |rate: usize| -> bool {
            if rate == 0 || rate >= 32 { return false; }
            let period = COUNTER_RATES[rate] as u32;
            (counter.wrapping_add(COUNTER_OFFSETS[rate] as u32)) % period == 0
        };

        match v.env_phase {
            EnvPhase::Off => return,
            EnvPhase::Release => {
                // Release fires every sample, no counter gating.
                v.env_level -= 8;
                if v.env_level <= 0 { v.env_level = 0; v.env_phase = EnvPhase::Off; }
                return;
            }
            _ => {}
        }

        if v.adsr1 & 0x80 != 0 {
            // ADSR mode.
            match v.env_phase {
                EnvPhase::Attack => {
                    let rate = ((v.adsr1 & 0x0F) as usize) * 2 + 1;
                    if fires(rate) {
                        v.env_level += if rate == 31 { 1024 } else { 32 };
                        if v.env_level >= 0x7FF { v.env_level = 0x7FF; v.env_phase = EnvPhase::Decay; }
                    }
                }
                EnvPhase::Decay => {
                    let rate = (((v.adsr1 >> 3) & 0x0E) + 0x10) as usize;
                    if fires(rate) {
                        v.env_level -= 1;
                        v.env_level -= v.env_level >> 8;
                        let sustain = ((v.adsr2 >> 5) as i32 + 1) * 0x100;
                        if v.env_level <= sustain { v.env_phase = EnvPhase::Sustain; }
                    }
                }
                EnvPhase::Sustain => {
                    let rate = (v.adsr2 & 0x1F) as usize;
                    if fires(rate) {
                        v.env_level -= 1;
                        v.env_level -= v.env_level >> 8;
                    }
                }
                _ => {}
            }
        } else {
            // GAIN mode.
            if v.gain & 0x80 == 0 {
                v.env_level = (v.gain as i32 & 0x7F) * 0x10;
                v.hidden_env = v.env_level;
            } else {
                let rate = (v.gain & 0x1F) as usize;
                let mode = (v.gain >> 5) & 0x03;
                if fires(rate) {
                    match mode {
                        0 => v.env_level -= 32,
                        1 => { v.env_level -= 1; v.env_level -= v.env_level >> 8; }
                        2 => v.env_level += 32,
                        3 => {
                            v.env_level += if v.hidden_env < 0x600 { 32 } else { 8 };
                            v.hidden_env = v.env_level;
                        }
                        _ => {}
                    }
                }
            }
        }
        v.env_level = v.env_level.clamp(0, 0x7FF);
    }

    fn process_echo(&mut self, ram: &[u8; 65536], input_l: i16, input_r: i16) -> (i16, i16) {
        if self.echo_length == 0 { return (0, 0); }

        let esa = (self.regs[ESA as usize] as u16) << 8;
        let pos = esa.wrapping_add(self.echo_pos) as usize;

        let echo_l = ram[pos & 0xFFFF] as i16 | ((ram[(pos + 1) & 0xFFFF] as i16) << 8);
        let echo_r = ram[(pos + 2) & 0xFFFF] as i16 | ((ram[(pos + 3) & 0xFFFF] as i16) << 8);

        let hp = self.echo_hist_pos;
        self.echo_hist_l[hp] = echo_l as i32;
        self.echo_hist_r[hp] = echo_r as i32;

        let mut fir_l: i32 = 0;
        let mut fir_r: i32 = 0;
        for tap in 0..8 {
            let coeff = self.regs[tap * 0x10 + FIR_BASE as usize] as i8 as i32;
            let idx = (hp + 8 - tap) & 7;
            fir_l += (self.echo_hist_l[idx] * coeff) >> 6;
            fir_r += (self.echo_hist_r[idx] * coeff) >> 6;
        }
        fir_l = clamp16(fir_l) & !1;
        fir_r = clamp16(fir_r) & !1;

        self.echo_hist_pos = (hp + 1) & 7;

        // Echo write-back (disabled when FLG bit 5 set).
        if self.regs[FLG as usize] & 0x20 == 0 {
            let efb = self.regs[EFB as usize] as i8 as i32;
            let _write_l = clamp16(input_l as i32 + ((fir_l * efb) >> 7)) as i16;
            let _write_r = clamp16(input_r as i32 + ((fir_r * efb) >> 7)) as i16;
            // TODO: write to APU RAM (needs &mut ram).
        }

        self.echo_pos += 4;
        if self.echo_pos >= self.echo_length { self.echo_pos = 0; }

        (fir_l as i16, fir_r as i16)
    }
}
