/// Scanline-based PPU renderer for Mode 1.
///
/// Renders one scanline at a time: decodes BG tiles from VRAM, composites
/// layers by priority, looks up colors from CGRAM, and writes to the framebuffer.

use super::{BgLayer, Ppu};
use super::color::snes_to_argb;

/// Bits per pixel for each BG layer in each mode.
/// Index: [mode][bg_index]. 0 means layer is unused in that mode.
const MODE_BPP: [[u8; 4]; 8] = [
    [2, 2, 2, 2], // Mode 0
    [4, 4, 2, 0], // Mode 1 — LTTP uses this
    [4, 4, 0, 0], // Mode 2
    [8, 4, 0, 0], // Mode 3
    [8, 2, 0, 0], // Mode 4
    [4, 2, 0, 0], // Mode 5
    [4, 0, 0, 0], // Mode 6
    [8, 0, 0, 0], // Mode 7
];

/// Pixel from a BG layer: color index into CGRAM + priority bit.
#[derive(Clone, Copy, Default)]
struct BgPixel {
    /// CGRAM color index (0 = transparent).
    cgram_index: u16,
    /// Priority bit from tilemap entry.
    priority: bool,
}

/// Pixel from a sprite: color index + priority level (0-3).
#[derive(Clone, Copy, Default)]
struct ObjPixel {
    /// CGRAM color index (0 = transparent).
    cgram_index: u16,
    /// Priority (0-3).
    priority: u8,
}

/// Composited pixel with source-layer tracking for color math decisions.
///
/// After priority-based compositing picks the winning pixel, we need to know
/// WHERE it came from — color math is enabled per-layer via CGADSUB ($2131).
#[derive(Clone, Copy)]
struct CompositePixel {
    /// 15-bit SNES color (from CGRAM lookup).
    color: u16,
    /// Source layer: 0=BG1, 1=BG2, 2=BG3, 3=BG4, 4=OBJ, 5=backdrop.
    source: u8,
    /// True if this is an OBJ pixel from palette 4-7 (CGRAM 192-255).
    /// These sprites are exempt from color math even when OBJ math is enabled.
    obj_math_exempt: bool,
}

impl Default for CompositePixel {
    fn default() -> Self {
        Self { color: 0, source: 5, obj_math_exempt: false }
    }
}

/// Pre-computed window masks for one scanline.
///
/// Each entry is true when the pixel is "inside" the combined window area.
/// Computed once per scanline since window positions are constant within a line
/// (HDMA updates happen between scanlines).
struct WindowMasks {
    /// Index: 0=BG1, 1=BG2, 2=BG3, 3=BG4, 4=OBJ, 5=Color math window.
    layers: [[bool; 256]; 6],
}

/// Sprite sizes: (small_w, small_h, large_w, large_h) for each OBSEL size mode.
const OBJ_SIZES: [(u8, u8, u8, u8); 8] = [
    (8, 8, 16, 16),   // 0
    (8, 8, 32, 32),   // 1
    (8, 8, 64, 64),   // 2
    (16, 16, 32, 32), // 3
    (16, 16, 64, 64), // 4
    (32, 32, 64, 64), // 5
    (16, 32, 32, 64), // 6
    (16, 32, 32, 32), // 7
];

impl Ppu {
    /// Render a single visible scanline (0-223) into the framebuffer.
    pub fn render_scanline(&mut self, y: u16) {
        let mode = (self.bgmode & 0x07) as usize;
        let brightness = self.inidisp & 0x0F;
        let forced_blank = self.inidisp & 0x80 != 0;

        let row_start = (y as usize) * 256;

        if forced_blank {
            for x in 0..256 {
                self.frame_buffer[row_start + x] = 0xFF000000; // Black
            }
            return;
        }

        // Pre-compute window masks (shared by Mode 7 and Mode 1 paths).
        let window_masks = self.compute_window_masks();

        // Mode 7 has a completely different rendering pipeline.
        if mode == 7 {
            self.render_mode7_scanline(y, brightness, row_start, &window_masks);
            return;
        }

        // Render each enabled BG layer into temporary buffers.
        // Use the union of main + sub screen enables — a layer might be on
        // the sub screen only (for blending) even if hidden on main.
        let mut bg_pixels: [[BgPixel; 256]; 4] = [[BgPixel::default(); 256]; 4];
        let layer_union = self.tm | self.ts;

        for bg_idx in 0..4 {
            let bpp = MODE_BPP[mode][bg_idx];
            if bpp == 0 { continue; }
            if layer_union & (1 << bg_idx) == 0 { continue; }

            self.render_bg_scanline(y, bg_idx, bpp, &mut bg_pixels[bg_idx]);
        }

        // Render sprites if enabled on either screen.
        let mut obj_pixels: [ObjPixel; 256] = [ObjPixel::default(); 256];
        if layer_union & 0x10 != 0 {
            self.render_obj_scanline(y, &mut obj_pixels);
        }

        let bg3_priority_bit = self.bgmode & 0x08 != 0;

        // Composite main screen (priority-based layer selection with window masking).
        let mut main_pixels = [CompositePixel::default(); 256];
        for x in 0..256 {
            main_pixels[x] = self.composite_layers(
                x, &bg_pixels, &obj_pixels,
                self.tm, self.tmw, &window_masks, bg3_priority_bit,
            );
        }

        // Composite sub screen only if color math actually needs it.
        // CGWSEL bit 1: 0 = use sub screen, 1 = use fixed color.
        let use_fixed_color = self.cgwsel & 0x02 != 0;
        let math_region = (self.cgwsel >> 4) & 3;
        let any_layer_math = self.cgadsub & 0x3F != 0;
        let need_sub = !use_fixed_color && math_region != 3 && any_layer_math;

        let mut sub_pixels = [CompositePixel::default(); 256];
        if need_sub {
            for x in 0..256 {
                sub_pixels[x] = self.composite_layers(
                    x, &bg_pixels, &obj_pixels,
                    self.ts, self.tsw, &window_masks, bg3_priority_bit,
                );
            }
        }

        // Build 15-bit fixed color from COLDATA components.
        let fixed_color = (self.fixed_color_r as u16)
            | ((self.fixed_color_g as u16) << 5)
            | ((self.fixed_color_b as u16) << 10);

        let clip_mode = (self.cgwsel >> 6) & 3;

        // Final output: clip-to-black → color math → brightness → framebuffer.
        for x in 0..256 {
            let main_px = &main_pixels[x];
            let in_color_window = window_masks.layers[5][x];

            // Force main screen black (CGWSEL bits 7-6).
            // 0=never, 1=outside color window, 2=inside, 3=always.
            let force_black = match clip_mode {
                1 => !in_color_window,
                2 => in_color_window,
                3 => true,
                _ => false,
            };
            let main_color = if force_black { 0 } else { main_px.color };

            // Color math enable region (CGWSEL bits 5-4).
            // 0=always, 1=inside color window, 2=outside, 3=never.
            let math_in_region = match math_region {
                0 => true,
                1 => in_color_window,
                2 => !in_color_window,
                _ => false,
            };

            // Per-layer color math enable (CGADSUB bits 0-5).
            let layer_math = match main_px.source {
                0..=3 => self.cgadsub & (1 << main_px.source) != 0,
                4 => self.cgadsub & 0x10 != 0 && !main_px.obj_math_exempt,
                5 => self.cgadsub & 0x20 != 0,
                _ => false,
            };

            let final_color = if math_in_region && layer_math {
                let sub_color = if use_fixed_color {
                    fixed_color
                } else {
                    sub_pixels[x].color
                };
                self.blend_colors(main_color, sub_color)
            } else {
                main_color
            };

            self.frame_buffer[row_start + x] = snes_to_argb(final_color, brightness);
        }
    }

    /// Render a Mode 7 scanline using affine transformation.
    ///
    /// Mode 7 uses a single 128×128 tile BG layer with per-pixel affine
    /// transform. VRAM is interleaved: even bytes = tilemap, odd bytes = 8bpp
    /// character data. The 2×2 matrix (M7A-M7D) transforms screen coordinates
    /// to tilemap coordinates, enabling rotation, scaling, and skewing.
    fn render_mode7_scanline(&mut self, y: u16, brightness: u8, row_start: usize, window_masks: &WindowMasks) {
        // Matrix parameters (1.7.8 fixed-point for A-D).
        let a = self.m7a as i32;
        let b = self.m7b as i32;
        let c = self.m7c as i32;
        let d = self.m7d as i32;

        // Rotation/scroll center.
        let cx = self.m7x as i32;
        let cy = self.m7y as i32;

        // Scroll offsets (13-bit signed).
        let hofs = self.m7_hofs as i32;
        let vofs = self.m7_vofs as i32;

        // Pre-compute the row-constant terms of the affine transform.
        // For screen pixel (sx, sy), the VRAM coordinates are:
        //   vram_x = A*(hofs-cx) + B*(y+vofs-cy) + cx<<8
        //   vram_y = C*(hofs-cx) + D*(y+vofs-cy) + cy<<8
        // Then for each sx, add A*sx to vram_x and C*sx to vram_y.
        let y_term = y as i32 + vofs - cy;
        let mut vram_x = a.wrapping_mul(hofs - cx) + b.wrapping_mul(y_term) + (cx << 8);
        let mut vram_y = c.wrapping_mul(hofs - cx) + d.wrapping_mul(y_term) + (cy << 8);

        // Render sprites on top of Mode 7 BG.
        let mut obj_pixels: [ObjPixel; 256] = [ObjPixel::default(); 256];
        if self.tm & 0x10 != 0 {
            self.render_obj_scanline(y, &mut obj_pixels);
        }

        let bg1_enabled = self.tm & 0x01 != 0;
        let obj_enabled = self.tm & 0x10 != 0;
        let bg1_windowed = self.tmw & 0x01 != 0;
        let obj_windowed = self.tmw & 0x10 != 0;

        // Color math state for the output loop.
        let fixed_color = (self.fixed_color_r as u16)
            | ((self.fixed_color_g as u16) << 5)
            | ((self.fixed_color_b as u16) << 10);
        let use_fixed_color = self.cgwsel & 0x02 != 0;
        let clip_mode = (self.cgwsel >> 6) & 3;
        let math_region = (self.cgwsel >> 4) & 3;

        for x in 0..256usize {
            // Integer VRAM coordinates (drop the fractional .8 bits).
            let vx = vram_x >> 8;
            let vy = vram_y >> 8;

            // Advance for next pixel.
            vram_x = vram_x.wrapping_add(a);
            vram_y = vram_y.wrapping_add(c);

            // Window masking for layers at this pixel.
            let bg1_masked = bg1_windowed && window_masks.layers[0][x];
            let obj_masked = obj_windowed && window_masks.layers[4][x];

            // Mode 7 priority (front to back):
            //   OBJ pri 3 → OBJ pri 2 → OBJ pri 1 → BG1 → OBJ pri 0 → backdrop
            let obj = &obj_pixels[x];
            let has_sprite = obj_enabled && obj.cgram_index != 0 && !obj_masked;

            // Fetch Mode 7 BG pixel.
            let bg_color = if bg1_enabled && !bg1_masked {
                // Tile coordinates in the 128×128 tilemap.
                let tile_x = ((vx as u32) >> 3) & 0x7F;
                let tile_y = ((vy as u32) >> 3) & 0x7F;

                let tilemap_addr = ((tile_y * 128 + tile_x) as usize) * 2;
                let tile_index = if tilemap_addr < self.vram.len() {
                    self.vram[tilemap_addr] as usize
                } else {
                    0
                };

                let fine_x = (vx as u32) & 7;
                let fine_y = (vy as u32) & 7;

                let chr_addr = (tile_index * 64 + (fine_y as usize) * 8 + fine_x as usize) * 2 + 1;
                let pixel = if chr_addr < self.vram.len() {
                    self.vram[chr_addr]
                } else {
                    0
                };

                if pixel != 0 { pixel as u16 } else { 0 }
            } else {
                0
            };

            // Determine the winning pixel and its source layer.
            let main_px = if has_sprite && obj.priority >= 1 {
                CompositePixel {
                    color: self.read_cgram(obj.cgram_index),
                    source: 4,
                    obj_math_exempt: obj.cgram_index >= 192,
                }
            } else if bg_color != 0 {
                CompositePixel {
                    color: self.read_cgram(bg_color),
                    source: 0,
                    obj_math_exempt: false,
                }
            } else if has_sprite {
                CompositePixel {
                    color: self.read_cgram(obj.cgram_index),
                    source: 4,
                    obj_math_exempt: obj.cgram_index >= 192,
                }
            } else {
                CompositePixel {
                    color: self.read_cgram(0),
                    source: 5,
                    obj_math_exempt: false,
                }
            };

            // Apply clip-to-black and color math (same logic as Mode 1).
            let in_color_window = window_masks.layers[5][x];

            let force_black = match clip_mode {
                1 => !in_color_window,
                2 => in_color_window,
                3 => true,
                _ => false,
            };
            let main_color = if force_black { 0 } else { main_px.color };

            let math_in_region = match math_region {
                0 => true,
                1 => in_color_window,
                2 => !in_color_window,
                _ => false,
            };

            let layer_math = match main_px.source {
                0 => self.cgadsub & 0x01 != 0,
                4 => self.cgadsub & 0x10 != 0 && !main_px.obj_math_exempt,
                5 => self.cgadsub & 0x20 != 0,
                _ => false,
            };

            let final_color = if math_in_region && layer_math {
                let sub_color = if use_fixed_color { fixed_color } else { 0 };
                self.blend_colors(main_color, sub_color)
            } else {
                main_color
            };

            self.frame_buffer[row_start + x] = snes_to_argb(final_color, brightness);
        }
    }

    /// Render one scanline of a BG layer.
    fn render_bg_scanline(
        &self,
        y: u16,
        bg_idx: usize,
        bpp: u8,
        out: &mut [BgPixel; 256],
    ) {
        let bg = &self.bg[bg_idx];

        for x in 0u16..256 {
            // Apply scroll.
            let sx = x.wrapping_add(bg.hscroll) & 0x3FF;
            let sy = y.wrapping_add(bg.vscroll) & 0x3FF;

            let pixel = self.fetch_bg_pixel(bg, bg_idx, bpp, sx, sy);
            out[x as usize] = pixel;
        }
    }

    /// Fetch a single BG pixel given the scrolled coordinates.
    fn fetch_bg_pixel(
        &self,
        bg: &BgLayer,
        bg_idx: usize,
        bpp: u8,
        sx: u16,
        sy: u16,
    ) -> BgPixel {
        let tile_size = if bg.tile_size { 16u16 } else { 8 };

        // Tile coordinates.
        let tile_x = sx / tile_size;
        let tile_y = sy / tile_size;

        // Pixel position within the tile.
        let mut fine_x = (sx % tile_size) as u8;
        let mut fine_y = (sy % tile_size) as u8;

        // For 16×16 tiles, determine which sub-tile we're in.
        let sub_tile_x = if tile_size == 16 { fine_x / 8 } else { 0 };
        let sub_tile_y = if tile_size == 16 { fine_y / 8 } else { 0 };
        if tile_size == 16 {
            fine_x %= 8;
            fine_y %= 8;
        }

        // Tilemap lookup.
        // Handle tilemap mirroring for sizes > 32×32.
        let map_x = tile_x & 0x1F; // Position within a 32×32 map
        let map_y = tile_y & 0x1F;

        // Which 32×32 screen are we in?
        let screen_x = (tile_x >> 5) & 1;
        let screen_y = (tile_y >> 5) & 1;

        let screen_offset: u16 = match bg.tilemap_size {
            0 => 0,                                  // 32×32: single screen
            1 => screen_x * 0x400,                   // 64×32: H mirror
            2 => screen_y * 0x400,                   // 32×64: V mirror
            3 => screen_x * 0x400 + screen_y * 0x800, // 64×64: both
            _ => 0,
        };

        let tilemap_entry_addr = bg.tilemap_addr
            .wrapping_add(screen_offset)
            .wrapping_add(map_y * 32 + map_x);

        // Read 2-byte tilemap entry from VRAM (word address → byte offset).
        let byte_addr = (tilemap_entry_addr as usize) * 2;
        if byte_addr + 1 >= self.vram.len() {
            return BgPixel::default();
        }
        let entry_lo = self.vram[byte_addr] as u16;
        let entry_hi = self.vram[byte_addr + 1] as u16;
        let entry = entry_lo | (entry_hi << 8);

        // Decode tilemap entry: vhopppcc cccccccc
        let tile_num = (entry & 0x03FF) as u16;
        let palette = ((entry >> 10) & 0x07) as u16;
        let priority = entry & 0x2000 != 0;
        let hflip = entry & 0x4000 != 0;
        let vflip = entry & 0x8000 != 0;

        // Adjust tile number for 16×16 sub-tile.
        let actual_tile = if tile_size == 16 {
            let sx = if hflip { 1 - sub_tile_x } else { sub_tile_x };
            let sy = if vflip { 1 - sub_tile_y } else { sub_tile_y };
            tile_num + sx as u16 + sy as u16 * 16
        } else {
            tile_num
        };

        // Apply flip to fine coordinates.
        let fx = if hflip { 7 - fine_x } else { fine_x };
        let fy = if vflip { 7 - fine_y } else { fine_y };

        // Fetch pixel color index from character data.
        let color_index = self.decode_tile_pixel(bg.chr_addr, actual_tile, bpp, fx, fy);

        if color_index == 0 {
            return BgPixel::default(); // Transparent
        }

        // Calculate CGRAM index.
        let colors_per_palette = 1u16 << bpp;
        // Mode 0 has separate palette regions per BG; modes 1+ share.
        let palette_base = if (self.bgmode & 0x07) == 0 {
            (bg_idx as u16) * 32
        } else {
            0
        };
        let cgram_index = palette_base + palette * colors_per_palette + color_index as u16;

        BgPixel { cgram_index, priority }
    }

    /// Decode one pixel from a tile in VRAM.
    /// `chr_base` is the character data base address (byte offset = word_addr * 2).
    /// Returns color index (0-255 depending on bpp).
    fn decode_tile_pixel(
        &self,
        chr_base: u16,
        tile_num: u16,
        bpp: u8,
        fine_x: u8,
        fine_y: u8,
    ) -> u8 {
        let bytes_per_tile = (bpp as u16) * 8; // 8 rows × bpp bits / 8 bits = bpp bytes per row × 8 rows... actually: 2bpp=16, 4bpp=32, 8bpp=64
        let tile_byte_addr = (chr_base as usize) * 2 + (tile_num as usize) * (bytes_per_tile as usize);

        let bit_index = 7 - fine_x;
        let mut color: u8 = 0;

        // Bitplanes are interleaved in pairs.
        // 2bpp: row = BP0, BP1 (2 bytes per row, 16 bytes per tile)
        // 4bpp: rows 0-7 = BP0,BP1; rows 0-7 again at +16 = BP2,BP3
        // 8bpp: +0=BP0,BP1; +16=BP2,BP3; +32=BP4,BP5; +48=BP6,BP7

        for plane_pair in 0..(bpp / 2) {
            let pair_offset = plane_pair as usize * 16;
            let row_offset = fine_y as usize * 2;
            let addr = tile_byte_addr + pair_offset + row_offset;

            if addr + 1 < self.vram.len() {
                let bp0 = self.vram[addr];
                let bp1 = self.vram[addr + 1];
                if bp0 & (1 << bit_index) != 0 { color |= 1 << (plane_pair * 2); }
                if bp1 & (1 << bit_index) != 0 { color |= 1 << (plane_pair * 2 + 1); }
            }
        }

        color
    }

    /// Render sprites for one scanline.
    fn render_obj_scanline(&self, y: u16, out: &mut [ObjPixel; 256]) {
        let size_mode = self.obj_size as usize;
        let (small_w, small_h, large_w, large_h) = OBJ_SIZES[size_mode];

        // Scan all 128 sprites (lower index = higher priority for ties).
        for sprite_idx in 0..128 {
            let oam_offset = sprite_idx * 4;

            // Read low table (4 bytes per sprite).
            let x_lo = self.oam[oam_offset] as u16;
            let y_pos = self.oam[oam_offset + 1];
            let tile = self.oam[oam_offset + 2] as u16;
            let attr = self.oam[oam_offset + 3];

            // Read high table (2 bits per sprite).
            let hi_byte = self.oam[512 + sprite_idx / 4];
            let hi_shift = (sprite_idx % 4) * 2;
            let x_hi = (hi_byte >> hi_shift) & 0x01;
            let size_bit = (hi_byte >> (hi_shift + 1)) & 0x01;

            // Full X position (9-bit signed).
            let x_pos = (x_hi as u16) << 8 | x_lo;
            let x_signed = if x_pos >= 256 { x_pos as i16 - 512 } else { x_pos as i16 };

            // Sprite size.
            let (w, h) = if size_bit != 0 {
                (large_w as u16, large_h as u16)
            } else {
                (small_w as u16, small_h as u16)
            };

            // Check if sprite is on this scanline (Y wraps at 256).
            let dy = y.wrapping_sub(y_pos as u16) & 0xFF;
            if dy >= h { continue; }

            // Decode attributes.
            let vflip = attr & 0x80 != 0;
            let hflip = attr & 0x40 != 0;
            let priority = (attr >> 4) & 0x03;
            let palette = ((attr >> 1) & 0x07) as u16;
            let name_hi = (attr & 0x01) as u16;

            // Tile row within sprite (handle vflip).
            let row = if vflip { (h - 1 - dy) as u8 } else { dy as u8 };

            // Render each pixel of this sprite on this scanline.
            for dx in 0..w {
                let screen_x = x_signed + dx as i16;
                if screen_x < 0 || screen_x >= 256 { continue; }
                let sx = screen_x as usize;

                // Don't overwrite higher-priority sprites (lower index).
                if out[sx].cgram_index != 0 { continue; }

                // Column within sprite (handle hflip).
                let col = if hflip { (w - 1 - dx) as u8 } else { dx as u8 };

                // Calculate tile number for this position within the sprite.
                let tile_col = col / 8;
                let tile_row = row / 8;
                let fine_x = col % 8;
                let fine_y = row % 8;

                // Sprite tiles are arranged in a 16-tile-wide grid in VRAM.
                // name_hi selects the name table (handled by chr_base), not the tile number.
                let tile_num = tile
                    .wrapping_add(tile_col as u16)
                    .wrapping_add((tile_row as u16) * 16);

                // OBJ character base address (word address).
                // name_hi bit selects between base table and base+gap table.
                let chr_base = if name_hi != 0 {
                    self.obj_base.wrapping_add(self.obj_name_select)
                } else {
                    self.obj_base
                };

                // Decode pixel (sprites are always 4bpp).
                let color_idx = self.decode_tile_pixel(
                    chr_base, // Already a word address matching decode_tile_pixel's expectation
                    tile_num,
                    4,
                    fine_x,
                    fine_y,
                );

                if color_idx == 0 { continue; } // Transparent

                // Sprite palette starts at CGRAM index 128 (palette 0-7, 16 colors each).
                let cgram_index = 128 + palette * 16 + color_idx as u16;

                out[sx] = ObjPixel { cgram_index, priority };
            }
        }
    }

    /// Composite layers at pixel position `x` using Mode 1 priority ordering.
    ///
    /// This is used for both main and sub screen compositing — the difference
    /// is which layers are enabled (`enables` from TM or TS) and which respect
    /// their window masks (`win_enables` from TMW or TSW).
    fn composite_layers(
        &self,
        x: usize,
        bg_pixels: &[[BgPixel; 256]; 4],
        obj_pixels: &[ObjPixel; 256],
        enables: u8,
        win_enables: u8,
        window_masks: &WindowMasks,
        bg3_high_priority: bool,
    ) -> CompositePixel {
        // A BG pixel is visible if: layer enabled, pixel opaque, not window-masked.
        // Window masking makes a pixel transparent (reveals the layer behind it).
        let bg = |idx: usize| -> Option<&BgPixel> {
            if enables & (1 << idx) == 0 { return None; }
            let px = &bg_pixels[idx][x];
            if px.cgram_index == 0 { return None; }
            if win_enables & (1 << idx) != 0 && window_masks.layers[idx][x] { return None; }
            Some(px)
        };

        let obj = &obj_pixels[x];
        let obj_vis = enables & 0x10 != 0
            && obj.cgram_index != 0
            && !(win_enables & 0x10 != 0 && window_masks.layers[4][x]);
        let obj_exempt = obj.cgram_index >= 192; // Palette 4-7

        // Mode 1 priority (front to back):
        //   BG3 pri1 (if bg3_high_priority)
        //   OBJ pri3 → BG1 pri1 → BG2 pri1 → OBJ pri2
        //   BG1 pri0 → BG2 pri0 → OBJ pri1
        //   BG3 pri1 (normal) → OBJ pri0 → BG3 pri0 → backdrop

        if bg3_high_priority {
            if let Some(px) = bg(2) { if px.priority {
                return CompositePixel { color: self.read_cgram(px.cgram_index), source: 2, obj_math_exempt: false };
            }}
        }

        if obj_vis && obj.priority == 3 {
            return CompositePixel { color: self.read_cgram(obj.cgram_index), source: 4, obj_math_exempt: obj_exempt };
        }

        if let Some(px) = bg(0) { if px.priority {
            return CompositePixel { color: self.read_cgram(px.cgram_index), source: 0, obj_math_exempt: false };
        }}
        if let Some(px) = bg(1) { if px.priority {
            return CompositePixel { color: self.read_cgram(px.cgram_index), source: 1, obj_math_exempt: false };
        }}

        if obj_vis && obj.priority == 2 {
            return CompositePixel { color: self.read_cgram(obj.cgram_index), source: 4, obj_math_exempt: obj_exempt };
        }

        if let Some(px) = bg(0) { if !px.priority {
            return CompositePixel { color: self.read_cgram(px.cgram_index), source: 0, obj_math_exempt: false };
        }}
        if let Some(px) = bg(1) { if !px.priority {
            return CompositePixel { color: self.read_cgram(px.cgram_index), source: 1, obj_math_exempt: false };
        }}

        if obj_vis && obj.priority == 1 {
            return CompositePixel { color: self.read_cgram(obj.cgram_index), source: 4, obj_math_exempt: obj_exempt };
        }

        if !bg3_high_priority {
            if let Some(px) = bg(2) { if px.priority {
                return CompositePixel { color: self.read_cgram(px.cgram_index), source: 2, obj_math_exempt: false };
            }}
        }

        if obj_vis && obj.priority == 0 {
            return CompositePixel { color: self.read_cgram(obj.cgram_index), source: 4, obj_math_exempt: obj_exempt };
        }

        if let Some(px) = bg(2) { if !px.priority {
            return CompositePixel { color: self.read_cgram(px.cgram_index), source: 2, obj_math_exempt: false };
        }}

        // Backdrop
        CompositePixel { color: self.read_cgram(0), source: 5, obj_math_exempt: false }
    }

    /// Pre-compute window masks for all 256 pixels of the current scanline.
    ///
    /// Each of the 6 mask targets (BG1-4, OBJ, Color) has two configurable
    /// rectangular windows (W1, W2) that combine with boolean logic. The result
    /// is a per-pixel boolean: true = "inside the combined window area".
    fn compute_window_masks(&self) -> WindowMasks {
        let mut masks = WindowMasks { layers: [[false; 256]; 6] };

        // 4-bit selection fields per layer from the three select registers.
        // Each field: bit0=W1 invert, bit1=W1 enable, bit2=W2 invert, bit3=W2 enable.
        let sel: [u8; 6] = [
            self.w12sel & 0x0F,          // BG1
            (self.w12sel >> 4) & 0x0F,   // BG2
            self.w34sel & 0x0F,          // BG3
            (self.w34sel >> 4) & 0x0F,   // BG4
            self.wobjsel & 0x0F,         // OBJ
            (self.wobjsel >> 4) & 0x0F,  // Color math
        ];

        // 2-bit logic operator per layer (OR/AND/XOR/XNOR).
        let logic: [u8; 6] = [
            self.wbglog & 0x03,
            (self.wbglog >> 2) & 0x03,
            (self.wbglog >> 4) & 0x03,
            (self.wbglog >> 6) & 0x03,
            self.wobjlog & 0x03,
            (self.wobjlog >> 2) & 0x03,
        ];

        for layer in 0..6 {
            let s = sel[layer];
            let w1_en = s & 0x02 != 0;
            let w2_en = s & 0x08 != 0;
            if !w1_en && !w2_en { continue; }

            let w1_inv = s & 0x01 != 0;
            let w2_inv = s & 0x04 != 0;
            let log = logic[layer];

            for x in 0u16..256 {
                let xb = x as u8;

                let w1 = if w1_en {
                    (self.w1_left <= xb && xb <= self.w1_right) ^ w1_inv
                } else {
                    false
                };
                let w2 = if w2_en {
                    (self.w2_left <= xb && xb <= self.w2_right) ^ w2_inv
                } else {
                    false
                };

                masks.layers[layer][x as usize] = match (w1_en, w2_en) {
                    (true, false) => w1,
                    (false, true) => w2,
                    (true, true) => match log {
                        0 => w1 | w2,
                        1 => w1 & w2,
                        2 => w1 ^ w2,
                        _ => !(w1 ^ w2), // 3 = XNOR
                    },
                    _ => false,
                };
            }
        }

        masks
    }

    /// Blend two 15-bit SNES colors using the color math operation.
    ///
    /// CGADSUB ($2131) controls the operation:
    ///   bit 7: 0 = add, 1 = subtract
    ///   bit 6: half-math (divide result by 2)
    ///
    /// Order: add/subtract → halve → clamp to [0, 31] per channel.
    /// The halve-then-clamp order matters for subtraction: (-3)/2 = -1 → clamp 0.
    fn blend_colors(&self, main: u16, sub: u16) -> u16 {
        let subtract = self.cgadsub & 0x80 != 0;
        let half = self.cgadsub & 0x40 != 0;

        let mr = (main & 0x1F) as i16;
        let mg = ((main >> 5) & 0x1F) as i16;
        let mb = ((main >> 10) & 0x1F) as i16;

        let sr = (sub & 0x1F) as i16;
        let sg = ((sub >> 5) & 0x1F) as i16;
        let sb = ((sub >> 10) & 0x1F) as i16;

        let (mut r, mut g, mut b) = if subtract {
            (mr - sr, mg - sg, mb - sb)
        } else {
            (mr + sr, mg + sg, mb + sb)
        };

        if half {
            r >>= 1;
            g >>= 1;
            b >>= 1;
        }

        r = r.clamp(0, 31);
        g = g.clamp(0, 31);
        b = b.clamp(0, 31);

        (r as u16) | ((g as u16) << 5) | ((b as u16) << 10)
    }

    /// Read a 15-bit color from CGRAM.
    fn read_cgram(&self, index: u16) -> u16 {
        let byte_idx = (index as usize) * 2;
        if byte_idx + 1 < self.cgram.len() {
            self.cgram[byte_idx] as u16 | ((self.cgram[byte_idx + 1] as u16) << 8)
        } else {
            0
        }
    }

    /// Probe a specific screen pixel on BG1 — returns full decode chain.
    pub fn probe_bg_pixel(&self, screen_x: u16, screen_y: u16) -> String {
        let mode = self.bgmode & 0x07;
        let bg = &self.bg[0]; // BG1
        let bpp = if mode == 1 { 4u8 } else { 2 };

        // Scrolled position
        let sx = screen_x.wrapping_add(bg.hscroll) & 0x3FF;
        let sy = screen_y.wrapping_add(bg.vscroll) & 0x3FF;

        let map_x = (sx / 8) as u16;
        let map_y = (sy / 8) as u16;
        let fine_x = (sx % 8) as u8;
        let fine_y = (sy % 8) as u8;

        // Screen offset for >32-tile maps
        let mut screen_offset: u16 = 0;
        let size_x = bg.tilemap_size & 1 != 0; // bit 0 = wide
        let size_y = bg.tilemap_size & 2 != 0; // bit 1 = tall
        if map_x >= 32 && size_x { screen_offset += 0x400; }
        if map_y >= 32 && size_y { screen_offset += if size_x { 0x800 } else { 0x400 }; }
        let map_x = map_x & 31;
        let map_y = map_y & 31;

        let tilemap_entry_addr = bg.tilemap_addr
            .wrapping_add(screen_offset)
            .wrapping_add(map_y * 32 + map_x);

        let byte_addr = (tilemap_entry_addr as usize) * 2;
        let entry = if byte_addr + 1 < self.vram.len() {
            self.vram[byte_addr] as u16 | ((self.vram[byte_addr + 1] as u16) << 8)
        } else { 0 };

        let tile_num = entry & 0x03FF;
        let palette = (entry >> 10) & 0x07;
        let priority = entry & 0x2000 != 0;
        let hflip = entry & 0x4000 != 0;
        let vflip = entry & 0x8000 != 0;

        let fx = if hflip { 7 - fine_x } else { fine_x };
        let fy = if vflip { 7 - fine_y } else { fine_y };
        let color_idx = self.decode_tile_pixel(bg.chr_addr, tile_num, bpp, fx, fy);

        let colors_per_pal = 1u16 << bpp;
        let cgram_idx = palette * colors_per_pal + color_idx as u16;
        let color = self.read_cgram(cgram_idx);
        let r = color & 0x1F;
        let g = (color >> 5) & 0x1F;
        let b = (color >> 10) & 0x1F;

        format!(
            "screen({},{}) scroll({},{}) map({},{}) fine({},{}) tmap_addr={:04X} entry={:04X} tile={} pal={} hf={} vf={} pri={} pixel={} cgram_idx={} color={:04X} R={} G={} B={}",
            screen_x, screen_y, bg.hscroll, bg.vscroll,
            map_x, map_y, fine_x, fine_y,
            tilemap_entry_addr, entry, tile_num, palette,
            hflip as u8, vflip as u8, priority as u8,
            color_idx, cgram_idx, color, r, g, b
        )
    }
}
