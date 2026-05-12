/// Save state (snapshot) support for the emulator.
///
/// The WASM linear memory architecture makes save states nearly free: we
/// serialize the relevant emulator state into a `Vec<u8>` and can later
/// restore it byte-for-byte. The format is a simple length-prefixed binary
/// blob — no serde dependency, just hand-rolled little-endian writes.
///
/// **Format**:
/// ```text
/// 0..8    magic "SNES01\0\0"
/// 8       version byte (currently 1)
/// 9..     subsystem blobs in order: CPU, Bus, PPU, APU, SRAM
/// ```
///
/// ROM data is NOT included (it lives in the loaded cartridge already).
/// The PPU framebuffer is included so a snapshot taken mid-frame can
/// resume rendering correctly; a fresh frame would overwrite it anyway.
///
/// On restore, the magic header and version byte are checked; mismatches
/// return `Err`. Length-prefixed Vec/array fields prevent silent
/// truncation.
//
// Layout note: each `snapshot_*` method appends to the shared `Vec<u8>`
// and each `restore_*` method advances a shared `&mut &[u8]` cursor. This
// keeps the format strictly sequential and trivially auditable.

use crate::Emulator;
use crate::cpu::{Cpu, StatusRegister};
use crate::bus::Bus;
use crate::dma::{Dma, DmaChannel};
use crate::joypad::Joypad;
use crate::ppu::{Ppu, BgLayer};
use crate::spc700::Apu;

const MAGIC: &[u8; 8] = b"SNES01\0\0";
const VERSION: u8 = 1;

// ─── Writer helpers ─────────────────────────────────────────────────────

#[inline] pub(crate) fn w_u8(out: &mut Vec<u8>, v: u8)   { out.push(v); }
#[inline] pub(crate) fn w_u16(out: &mut Vec<u8>, v: u16) { out.extend_from_slice(&v.to_le_bytes()); }
#[inline] pub(crate) fn w_u32(out: &mut Vec<u8>, v: u32) { out.extend_from_slice(&v.to_le_bytes()); }
#[inline] pub(crate) fn w_u64(out: &mut Vec<u8>, v: u64) { out.extend_from_slice(&v.to_le_bytes()); }
#[inline] pub(crate) fn w_i16(out: &mut Vec<u8>, v: i16) { out.extend_from_slice(&v.to_le_bytes()); }
#[inline] pub(crate) fn w_i32(out: &mut Vec<u8>, v: i32) { out.extend_from_slice(&v.to_le_bytes()); }
#[inline] pub(crate) fn w_i64(out: &mut Vec<u8>, v: i64) { out.extend_from_slice(&v.to_le_bytes()); }
#[inline] pub(crate) fn w_bool(out: &mut Vec<u8>, v: bool) { out.push(if v { 1 } else { 0 }); }
pub(crate) fn w_bytes(out: &mut Vec<u8>, b: &[u8]) {
    w_u32(out, b.len() as u32);
    out.extend_from_slice(b);
}

// ─── Reader helpers ─────────────────────────────────────────────────────

#[inline]
pub(crate) fn r_u8(r: &mut &[u8]) -> Result<u8, String> {
    if r.is_empty() { return Err("snapshot: unexpected EOF (u8)".into()); }
    let v = r[0]; *r = &r[1..]; Ok(v)
}
pub(crate) fn r_u16(r: &mut &[u8]) -> Result<u16, String> {
    if r.len() < 2 { return Err("snapshot: unexpected EOF (u16)".into()); }
    let v = u16::from_le_bytes([r[0], r[1]]); *r = &r[2..]; Ok(v)
}
pub(crate) fn r_u32(r: &mut &[u8]) -> Result<u32, String> {
    if r.len() < 4 { return Err("snapshot: unexpected EOF (u32)".into()); }
    let v = u32::from_le_bytes([r[0], r[1], r[2], r[3]]); *r = &r[4..]; Ok(v)
}
pub(crate) fn r_u64(r: &mut &[u8]) -> Result<u64, String> {
    if r.len() < 8 { return Err("snapshot: unexpected EOF (u64)".into()); }
    let mut b = [0u8; 8]; b.copy_from_slice(&r[..8]); *r = &r[8..];
    Ok(u64::from_le_bytes(b))
}
pub(crate) fn r_i16(r: &mut &[u8]) -> Result<i16, String> { r_u16(r).map(|v| v as i16) }
pub(crate) fn r_i32(r: &mut &[u8]) -> Result<i32, String> { r_u32(r).map(|v| v as i32) }
pub(crate) fn r_i64(r: &mut &[u8]) -> Result<i64, String> { r_u64(r).map(|v| v as i64) }
pub(crate) fn r_bool(r: &mut &[u8]) -> Result<bool, String> { r_u8(r).map(|v| v != 0) }
pub(crate) fn r_bytes_into(r: &mut &[u8], dst: &mut [u8]) -> Result<(), String> {
    let n = r_u32(r)? as usize;
    if n != dst.len() {
        return Err(format!("snapshot: byte length mismatch (expected {}, got {})", dst.len(), n));
    }
    if r.len() < n { return Err("snapshot: unexpected EOF (bytes)".into()); }
    dst.copy_from_slice(&r[..n]);
    *r = &r[n..];
    Ok(())
}
pub(crate) fn r_bytes_vec(r: &mut &[u8]) -> Result<Vec<u8>, String> {
    let n = r_u32(r)? as usize;
    if r.len() < n { return Err("snapshot: unexpected EOF (bytes_vec)".into()); }
    let v = r[..n].to_vec();
    *r = &r[n..];
    Ok(v)
}

// ─── StatusRegister ─────────────────────────────────────────────────────

fn write_status(out: &mut Vec<u8>, p: &StatusRegister) {
    // Pack into a single byte (native mode encoding has all 8 bits).
    let mut b = 0u8;
    if p.n { b |= 0x80; }
    if p.v { b |= 0x40; }
    if p.m { b |= 0x20; }
    if p.x { b |= 0x10; }
    if p.d { b |= 0x08; }
    if p.i { b |= 0x04; }
    if p.z { b |= 0x02; }
    if p.c { b |= 0x01; }
    out.push(b);
}
fn read_status(r: &mut &[u8]) -> Result<StatusRegister, String> {
    let b = r_u8(r)?;
    Ok(StatusRegister {
        n: b & 0x80 != 0,
        v: b & 0x40 != 0,
        m: b & 0x20 != 0,
        x: b & 0x10 != 0,
        d: b & 0x08 != 0,
        i: b & 0x04 != 0,
        z: b & 0x02 != 0,
        c: b & 0x01 != 0,
    })
}

// ─── CPU ────────────────────────────────────────────────────────────────

fn write_cpu(out: &mut Vec<u8>, cpu: &Cpu) {
    w_u16(out, cpu.a);
    w_u16(out, cpu.x);
    w_u16(out, cpu.y);
    w_u16(out, cpu.sp);
    w_u16(out, cpu.dp);
    w_u16(out, cpu.pc);
    w_u8(out, cpu.pbr);
    w_u8(out, cpu.dbr);
    write_status(out, &cpu.p);
    w_bool(out, cpu.emulation);
    w_u64(out, cpu.cycles);
    w_bool(out, cpu.nmi_pending);
    w_bool(out, cpu.irq_pending);
    w_bool(out, cpu.stopped);
    w_bool(out, cpu.waiting);
}
fn read_cpu(r: &mut &[u8], cpu: &mut Cpu) -> Result<(), String> {
    cpu.a = r_u16(r)?;
    cpu.x = r_u16(r)?;
    cpu.y = r_u16(r)?;
    cpu.sp = r_u16(r)?;
    cpu.dp = r_u16(r)?;
    cpu.pc = r_u16(r)?;
    cpu.pbr = r_u8(r)?;
    cpu.dbr = r_u8(r)?;
    cpu.p = read_status(r)?;
    cpu.emulation = r_bool(r)?;
    cpu.cycles = r_u64(r)?;
    cpu.nmi_pending = r_bool(r)?;
    cpu.irq_pending = r_bool(r)?;
    cpu.stopped = r_bool(r)?;
    cpu.waiting = r_bool(r)?;
    Ok(())
}

// ─── DMA ────────────────────────────────────────────────────────────────

fn write_dma(out: &mut Vec<u8>, dma: &Dma) {
    for ch in &dma.channels { write_dma_channel(out, ch); }
}
fn write_dma_channel(out: &mut Vec<u8>, c: &DmaChannel) {
    w_u8(out, c.control);
    w_u8(out, c.dest);
    w_u16(out, c.src_addr);
    w_u8(out, c.src_bank);
    w_u16(out, c.size);
    w_u8(out, c.hdma_indirect_bank);
    w_u16(out, c.hdma_addr);
    w_u8(out, c.hdma_line_counter);
    w_u8(out, c.unused);
    w_bool(out, c.hdma_terminated);
    w_bool(out, c.hdma_do_transfer);
}
fn read_dma(r: &mut &[u8], dma: &mut Dma) -> Result<(), String> {
    for ch in &mut dma.channels { read_dma_channel(r, ch)?; }
    Ok(())
}
fn read_dma_channel(r: &mut &[u8], c: &mut DmaChannel) -> Result<(), String> {
    c.control = r_u8(r)?;
    c.dest = r_u8(r)?;
    c.src_addr = r_u16(r)?;
    c.src_bank = r_u8(r)?;
    c.size = r_u16(r)?;
    c.hdma_indirect_bank = r_u8(r)?;
    c.hdma_addr = r_u16(r)?;
    c.hdma_line_counter = r_u8(r)?;
    c.unused = r_u8(r)?;
    c.hdma_terminated = r_bool(r)?;
    c.hdma_do_transfer = r_bool(r)?;
    Ok(())
}

// ─── Joypad ─────────────────────────────────────────────────────────────

fn write_joypad(out: &mut Vec<u8>, j: &Joypad) {
    // Joypad has private fields — use its public snapshot interface.
    // (Methods added below in `Joypad::snapshot_state`.)
    let blob = j.snapshot_state();
    w_bytes(out, &blob);
}
fn read_joypad(r: &mut &[u8], j: &mut Joypad) -> Result<(), String> {
    let blob = r_bytes_vec(r)?;
    j.restore_state(&blob)
}

// ─── PPU ────────────────────────────────────────────────────────────────

fn write_bg(out: &mut Vec<u8>, bg: &BgLayer) {
    w_u16(out, bg.tilemap_addr);
    w_u8(out, bg.tilemap_size);
    w_u16(out, bg.chr_addr);
    w_u16(out, bg.hscroll);
    w_u16(out, bg.vscroll);
    w_bool(out, bg.tile_size);
}
fn read_bg(r: &mut &[u8], bg: &mut BgLayer) -> Result<(), String> {
    bg.tilemap_addr = r_u16(r)?;
    bg.tilemap_size = r_u8(r)?;
    bg.chr_addr = r_u16(r)?;
    bg.hscroll = r_u16(r)?;
    bg.vscroll = r_u16(r)?;
    bg.tile_size = r_bool(r)?;
    Ok(())
}

fn write_ppu(out: &mut Vec<u8>, ppu: &Ppu) {
    // Memory blocks
    w_bytes(out, &*ppu.vram);
    w_bytes(out, &*ppu.oam);
    w_bytes(out, &*ppu.cgram);

    // Display control
    w_u8(out, ppu.inidisp);
    w_u8(out, ppu.bgmode);
    w_u8(out, ppu.mosaic);
    for bg in &ppu.bg { write_bg(out, bg); }

    // VRAM access
    w_u16(out, ppu.vram_addr);
    w_u8(out, ppu.vram_increment);
    w_u16(out, ppu.vram_prefetch);
    w_u8(out, ppu.vram_remap);

    // CGRAM access
    w_u8(out, ppu.cgram_addr);
    w_u8(out, ppu.cgram_latch);
    w_bool(out, ppu.cgram_flipflop);

    // OAM access
    w_u16(out, ppu.oam_addr);
    w_u16(out, ppu.oam_internal_addr);
    w_u8(out, ppu.oam_latch);
    w_bool(out, ppu.oam_flipflop);
    w_u8(out, ppu.obj_size);
    w_u16(out, ppu.obj_base);
    w_u16(out, ppu.obj_name_select);

    // Scroll latches
    w_u8(out, ppu.scroll_latch);
    w_u8(out, ppu.bghofs_latch);

    // Mode 7
    w_i16(out, ppu.m7a);
    w_i16(out, ppu.m7b);
    w_i16(out, ppu.m7c);
    w_i16(out, ppu.m7d);
    w_i16(out, ppu.m7x);
    w_i16(out, ppu.m7y);
    w_u8(out, ppu.m7_latch);
    w_i16(out, ppu.m7_hofs);
    w_i16(out, ppu.m7_vofs);

    // Screen designation
    w_u8(out, ppu.tm);
    w_u8(out, ppu.ts);
    w_u8(out, ppu.tmw);
    w_u8(out, ppu.tsw);

    // Color math
    w_u8(out, ppu.cgwsel);
    w_u8(out, ppu.cgadsub);
    w_u8(out, ppu.fixed_color_r);
    w_u8(out, ppu.fixed_color_g);
    w_u8(out, ppu.fixed_color_b);

    // Window
    w_u8(out, ppu.w1_left);
    w_u8(out, ppu.w1_right);
    w_u8(out, ppu.w2_left);
    w_u8(out, ppu.w2_right);
    w_u8(out, ppu.wbglog);
    w_u8(out, ppu.wobjlog);
    w_u8(out, ppu.w12sel);
    w_u8(out, ppu.w34sel);
    w_u8(out, ppu.wobjsel);

    // Rendering state
    w_u16(out, ppu.scanline);
    // Framebuffer is recomputable but we include it so a mid-frame
    // snapshot/restore is bit-exact.
    let fb_bytes: Vec<u8> = ppu.frame_buffer.iter()
        .flat_map(|p| p.to_le_bytes())
        .collect();
    w_bytes(out, &fb_bytes);

    // Status
    w_bool(out, ppu.latch_hv);
    w_u16(out, ppu.ophct);
    w_u16(out, ppu.opvct);
    w_bool(out, ppu.ophct_flipflop);
    w_bool(out, ppu.opvct_flipflop);
}
fn read_ppu(r: &mut &[u8], ppu: &mut Ppu) -> Result<(), String> {
    r_bytes_into(r, &mut *ppu.vram)?;
    r_bytes_into(r, &mut *ppu.oam)?;
    r_bytes_into(r, &mut *ppu.cgram)?;

    ppu.inidisp = r_u8(r)?;
    ppu.bgmode = r_u8(r)?;
    ppu.mosaic = r_u8(r)?;
    for bg in &mut ppu.bg { read_bg(r, bg)?; }

    ppu.vram_addr = r_u16(r)?;
    ppu.vram_increment = r_u8(r)?;
    ppu.vram_prefetch = r_u16(r)?;
    ppu.vram_remap = r_u8(r)?;

    ppu.cgram_addr = r_u8(r)?;
    ppu.cgram_latch = r_u8(r)?;
    ppu.cgram_flipflop = r_bool(r)?;

    ppu.oam_addr = r_u16(r)?;
    ppu.oam_internal_addr = r_u16(r)?;
    ppu.oam_latch = r_u8(r)?;
    ppu.oam_flipflop = r_bool(r)?;
    ppu.obj_size = r_u8(r)?;
    ppu.obj_base = r_u16(r)?;
    ppu.obj_name_select = r_u16(r)?;

    ppu.scroll_latch = r_u8(r)?;
    ppu.bghofs_latch = r_u8(r)?;

    ppu.m7a = r_i16(r)?;
    ppu.m7b = r_i16(r)?;
    ppu.m7c = r_i16(r)?;
    ppu.m7d = r_i16(r)?;
    ppu.m7x = r_i16(r)?;
    ppu.m7y = r_i16(r)?;
    ppu.m7_latch = r_u8(r)?;
    ppu.m7_hofs = r_i16(r)?;
    ppu.m7_vofs = r_i16(r)?;

    ppu.tm = r_u8(r)?;
    ppu.ts = r_u8(r)?;
    ppu.tmw = r_u8(r)?;
    ppu.tsw = r_u8(r)?;

    ppu.cgwsel = r_u8(r)?;
    ppu.cgadsub = r_u8(r)?;
    ppu.fixed_color_r = r_u8(r)?;
    ppu.fixed_color_g = r_u8(r)?;
    ppu.fixed_color_b = r_u8(r)?;

    ppu.w1_left = r_u8(r)?;
    ppu.w1_right = r_u8(r)?;
    ppu.w2_left = r_u8(r)?;
    ppu.w2_right = r_u8(r)?;
    ppu.wbglog = r_u8(r)?;
    ppu.wobjlog = r_u8(r)?;
    ppu.w12sel = r_u8(r)?;
    ppu.w34sel = r_u8(r)?;
    ppu.wobjsel = r_u8(r)?;

    ppu.scanline = r_u16(r)?;
    let fb_bytes = r_bytes_vec(r)?;
    if fb_bytes.len() != ppu.frame_buffer.len() * 4 {
        return Err("snapshot: framebuffer size mismatch".into());
    }
    for (i, chunk) in fb_bytes.chunks_exact(4).enumerate() {
        ppu.frame_buffer[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }

    ppu.latch_hv = r_bool(r)?;
    ppu.ophct = r_u16(r)?;
    ppu.opvct = r_u16(r)?;
    ppu.ophct_flipflop = r_bool(r)?;
    ppu.opvct_flipflop = r_bool(r)?;
    Ok(())
}

// ─── Bus ────────────────────────────────────────────────────────────────

fn write_bus(out: &mut Vec<u8>, bus: &Bus) {
    // 128KB WRAM
    w_bytes(out, &*bus.wram);

    // SRAM (cartridge — only mutable cart state we snapshot; ROM excluded).
    w_bytes(out, &bus.cart.sram);

    // CPU internal registers
    w_u8(out, bus.nmitimen);
    w_u16(out, bus.htime);
    w_u16(out, bus.vtime);
    w_u8(out, bus.hdmaen);
    w_u8(out, bus.memsel);

    // Math hardware
    w_u8(out, bus.wrmpya);
    w_u8(out, bus.wrmpyb);
    w_u16(out, bus.wrdiv);
    w_u8(out, bus.wrdivb);
    w_u16(out, bus.rddiv);
    w_u16(out, bus.rdmpy);

    // WRAM data port
    w_u32(out, bus.wram_addr);

    // Timing/status
    w_bool(out, bus.vblank);
    w_bool(out, bus.hblank);
    w_bool(out, bus.nmi_flag);
    w_bool(out, bus.irq_flag);
    w_bool(out, bus.auto_joypad_busy);
    w_u8(out, bus.open_bus);
    w_u64(out, bus.pending_dma_cycles);
    w_u8(out, bus.last_write_bank);
    w_u16(out, bus.last_write_pc);

    // Sub-components
    write_ppu(out, &bus.ppu);
    write_dma(out, &bus.dma);
    write_joypad(out, &bus.joypad);

    // APU is large — delegate to its own method.
    let apu_blob = bus.apu.snapshot();
    w_bytes(out, &apu_blob);
}
fn read_bus(r: &mut &[u8], bus: &mut Bus) -> Result<(), String> {
    r_bytes_into(r, &mut *bus.wram)?;

    let sram = r_bytes_vec(r)?;
    if sram.len() != bus.cart.sram.len() {
        return Err(format!(
            "snapshot: SRAM size mismatch (expected {}, got {})",
            bus.cart.sram.len(), sram.len()
        ));
    }
    bus.cart.sram.copy_from_slice(&sram);

    bus.nmitimen = r_u8(r)?;
    bus.htime = r_u16(r)?;
    bus.vtime = r_u16(r)?;
    bus.hdmaen = r_u8(r)?;
    bus.memsel = r_u8(r)?;

    bus.wrmpya = r_u8(r)?;
    bus.wrmpyb = r_u8(r)?;
    bus.wrdiv = r_u16(r)?;
    bus.wrdivb = r_u8(r)?;
    bus.rddiv = r_u16(r)?;
    bus.rdmpy = r_u16(r)?;

    bus.wram_addr = r_u32(r)?;

    bus.vblank = r_bool(r)?;
    bus.hblank = r_bool(r)?;
    bus.nmi_flag = r_bool(r)?;
    bus.irq_flag = r_bool(r)?;
    bus.auto_joypad_busy = r_bool(r)?;
    bus.open_bus = r_u8(r)?;
    bus.pending_dma_cycles = r_u64(r)?;
    bus.last_write_bank = r_u8(r)?;
    bus.last_write_pc = r_u16(r)?;

    read_ppu(r, &mut bus.ppu)?;
    read_dma(r, &mut bus.dma)?;
    read_joypad(r, &mut bus.joypad)?;

    let apu_blob = r_bytes_vec(r)?;
    bus.apu.restore(&apu_blob)?;
    Ok(())
}

// ─── Free-function entry points ─────────────────────────────────────────
//
// Exposed publicly so native test harnesses (which can't go through the
// wasm-bindgen `Emulator` constructor) can drive snapshot/restore against
// raw CPU + Bus instances.

/// Serialize CPU + Bus + frame_count into a self-contained blob.
pub fn snapshot_state(cpu: &Cpu, bus: &Bus, frame_count: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(256 * 1024);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    write_cpu(&mut out, cpu);
    write_bus(&mut out, bus);
    w_u64(&mut out, frame_count);
    out
}

/// Restore CPU + Bus + frame_count from a blob produced by `snapshot_state`.
pub fn restore_state(
    cpu: &mut Cpu,
    bus: &mut Bus,
    frame_count: &mut u64,
    bytes: &[u8],
) -> Result<(), String> {
    if bytes.len() < 9 {
        return Err("snapshot: too short".into());
    }
    if &bytes[..8] != MAGIC {
        return Err("snapshot: bad magic header".into());
    }
    if bytes[8] != VERSION {
        return Err(format!("snapshot: unsupported version {}", bytes[8]));
    }
    let mut r: &[u8] = &bytes[9..];
    read_cpu(&mut r, cpu)?;
    read_bus(&mut r, bus)?;
    *frame_count = r_u64(&mut r)?;
    Ok(())
}

// ─── Top-level Emulator API ─────────────────────────────────────────────

impl Emulator {
    /// Serialize emulator state into a binary blob.
    ///
    /// Excludes ROM (immutable, already in memory) but includes mutable
    /// cartridge SRAM. The resulting `Vec<u8>` can later be fed back into
    /// [`Emulator::restore_snapshot`] to resume from the exact same state.
    pub fn snapshot(&self) -> Vec<u8> {
        snapshot_state(&self.cpu, &self.bus, self.frame_count)
    }

    /// Restore emulator state from a blob produced by [`Emulator::snapshot`].
    pub fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut fc = self.frame_count;
        restore_state(&mut self.cpu, &mut self.bus, &mut fc, bytes)?;
        self.frame_count = fc;
        Ok(())
    }
}
