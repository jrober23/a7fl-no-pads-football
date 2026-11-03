//! A small CPU rasteriser: colour framebuffer, z-buffer, triangles, lines and
//! alpha blending. Everything the broadcast draws goes through here.

use nopads_core::math::{clamp, Rgb};

use crate::font;

/// Fixed-point linear blend of two 0..255 samples with `t` in 0..256.
#[inline]
fn blend8(a: u32, b: u32, t: u32) -> u32 {
    (a * (256 - t) + b * t + 128) >> 8
}

/// `v * k + off`, clamped into a byte: the constant shade and fog of a triangle
/// folded into two operations per channel.
#[inline]
fn tint(v: u8, k: f32, off: f32) -> u8 {
    let x = v as f32 * k + off;
    if x <= 0.0 {
        0
    } else if x >= 255.0 {
        255
    } else {
        x as u8
    }
}

#[derive(Clone)]
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub color: Vec<Rgb>,
    pub depth: Vec<f32>,
}

impl Frame {
    pub fn new(w: usize, h: usize) -> Self {
        Frame {
            w,
            h,
            color: vec![Rgb::BLACK; w * h],
            // Depth holds 1/z, so "empty" is 0 and larger values are nearer.
            depth: vec![0.0; w * h],
        }
    }

    pub fn clear(&mut self, color: Rgb) {
        self.color.fill(color);
        self.depth.fill(0.0);
    }

    pub fn clear_depth(&mut self) {
        self.depth.fill(0.0);
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, color: Rgb) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let i = y as usize * self.w + x as usize;
        self.color[i] = color;
    }

    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, color: Rgb, alpha: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let a = clamp(alpha, 0.0, 1.0);
        let i = y as usize * self.w + x as usize;
        self.color[i] = self.color[i].lerp(color, a);
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Rgb {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return Rgb::BLACK;
        }
        self.color[y as usize * self.w + x as usize]
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w as i32);
        let y1 = (y + h).min(self.h as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                self.color[py as usize * self.w + px as usize] = color;
            }
        }
    }

    pub fn fill_rect_alpha(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb, alpha: f32) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w as i32);
        let y1 = (y + h).min(self.h as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend(px, py, color, alpha);
            }
        }
    }

    /// Vertical gradient rectangle.
    pub fn gradient_rect(&mut self, x: i32, y: i32, w: i32, h: i32, top: Rgb, bottom: Rgb) {
        for row in 0..h.max(1) {
            let t = row as f32 / h.max(1) as f32;
            let c = top.lerp(bottom, t);
            self.fill_rect(x, y + row, w, 1, c);
        }
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb, thickness: i32) {
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        let steps = dx.max(dy).max(1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let x = x0 as f32 + (x1 - x0) as f32 * t;
            let y = y0 as f32 + (y1 - y0) as f32 * t;
            for oy in -(thickness / 2)..=(thickness / 2) {
                self.put(x.round() as i32, y.round() as i32 + oy, color);
            }
        }
    }

    /// Round-ish rectangle, used for the broadcast score bug.
    pub fn rounded_rect(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, color: Rgb) {
        let r = radius.max(0).min(w / 2).min(h / 2);
        self.fill_rect(x + r, y, w - 2 * r, h, color);
        self.fill_rect(x, y + r, r, h - 2 * r, color);
        self.fill_rect(x + w - r, y + r, r, h - 2 * r, color);
        // Corners.
        for dy in 0..r {
            for dx in 0..r {
                let inside = (dx - r) * (dx - r) + (dy - r) * (dy - r) <= r * r;
                if inside {
                    self.put(x + dx, y + dy, color);
                    self.put(x + w - 1 - dx, y + dy, color);
                    self.put(x + dx, y + h - 1 - dy, color);
                    self.put(x + w - 1 - dx, y + h - 1 - dy, color);
                }
            }
        }
    }

    /// Alpha blended rounded rectangle.
    pub fn rounded_rect_alpha(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        color: Rgb,
        alpha: f32,
    ) {
        let mut tmp = Frame::new(w.max(1) as usize, h.max(1) as usize);
        tmp.rounded_rect(0, 0, w, h, radius, color);
        for py in 0..h {
            for px in 0..w {
                let c = tmp.color[py as usize * tmp.w + px as usize];
                if c != Rgb::BLACK || tmp.color[py as usize * tmp.w + px as usize] == color {
                    self.blend(x + px, y + py, c, alpha);
                }
            }
        }
    }

    pub fn text(&mut self, x: i32, y: i32, text: &str, scale: usize, color: Rgb) {
        font::draw(self, x, y, text, scale, color);
    }

    pub fn text_shadowed(&mut self, x: i32, y: i32, text: &str, scale: usize, color: Rgb, weight: usize) {
        font::draw_shadowed(self, x, y, text, scale, color, weight);
    }

    /// Draw a texture-mapped, perspective-projected triangle with a z test.
    ///
    /// Vertices are `(screen x, screen y, 1/z, u, v)`. Perspective-correct texture
    /// interpolation is done on `u/z` and `v/z`.
    pub fn textured_tri(
        &mut self,
        v: [(f32, f32, f32, f32, f32); 3],
        tex: &Texture,
        shade: f32,
        fog: Rgb,
        fog_amount: f32,
    ) {
        let (x0, y0, iz0, u0, v0) = v[0];
        let (x1, y1, iz1, u1, v1) = v[1];
        let (x2, y2, iz2, u2, v2) = v[2];

        let min_x = x0.min(x1).min(x2).floor().max(0.0) as i32;
        let max_x = x0.max(x1).max(x2).ceil().min(self.w as f32 - 1.0) as i32;
        let min_y = y0.min(y1).min(y2).floor().max(0.0) as i32;
        let max_y = y0.max(y1).max(y2).ceil().min(self.h as f32 - 1.0) as i32;
        if min_x > max_x || min_y > max_y {
            return;
        }

        let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if area.abs() < 1e-6 {
            return;
        }
        let inv_area = 1.0 / area;

        // Shade and fog are constant over a triangle, so fold them into one
        // multiply and one offset per channel instead of clamping and lerping
        // every channel of every pixel.
        let fog_t = clamp(fog_amount, 0.0, 1.0);
        let k = shade * (1.0 - fog_t);
        let fog_r = fog.r as f32 * fog_t;
        let fog_g = fog.g as f32 * fog_t;
        let fog_b = fog.b as f32 * fog_t;

        // Texture coordinates are interpolated as `u/z`, so premultiply once.
        let ku0 = u0 * iz0;
        let ku1 = u1 * iz1;
        let ku2 = u2 * iz2;
        let kv0 = v0 * iz0;
        let kv1 = v1 * iz1;
        let kv2 = v2 * iz2;

        // The edge functions are affine, so evaluate them once per scanline and
        // step along the row instead of re-deriving them for every pixel.
        let dw0_dx = (y1 - y2) * inv_area;
        let dw0_dy = (x2 - x1) * inv_area;
        let dw1_dx = (y2 - y0) * inv_area;
        let dw1_dy = (x0 - x2) * inv_area;
        let (sx, sy) = (min_x as f32 + 0.5, min_y as f32 + 0.5);
        let mut row0 = ((x1 - sx) * (y2 - sy) - (x2 - sx) * (y1 - sy)) * inv_area;
        let mut row1 = ((x2 - sx) * (y0 - sy) - (x0 - sx) * (y2 - sy)) * inv_area;

        for py in min_y..=max_y {
            let mut w0 = row0;
            let mut w1 = row1;
            let row = py as usize * self.w;
            for px in min_x..=max_x {
                let w2 = 1.0 - w0 - w1;
                if w0 >= -0.0005 && w1 >= -0.0005 && w2 >= -0.0005 {
                    let iz = w0 * iz0 + w1 * iz1 + w2 * iz2;
                    let idx = row + px as usize;
                    // 1/z grows as fragments get closer, so nearer fragments win.
                    if iz > 1e-6 && iz > self.depth[idx] {
                        let inv_z = 1.0 / iz;
                        let u = (w0 * ku0 + w1 * ku1 + w2 * ku2) * inv_z;
                        let vtex = (w0 * kv0 + w1 * kv1 + w2 * kv2) * inv_z;
                        self.depth[idx] = iz;
                        let c = tex.sample(u, vtex);
                        self.color[idx] = Rgb::new(
                            tint(c.r, k, fog_r),
                            tint(c.g, k, fog_g),
                            tint(c.b, k, fog_b),
                        );
                    }
                }
                w0 += dw0_dx;
                w1 += dw1_dx;
            }
            row0 += dw0_dy;
            row1 += dw1_dy;
        }
    }

    /// Flat shaded, alpha blended triangle (decals, shadows, yard lines). It is
    /// depth tested but does not write depth, so overlapping decals don't fight.
    pub fn flat_tri_alpha(&mut self, p: [(f32, f32, f32); 3], color: Rgb, alpha: f32) {
        let (x0, y0, iz0) = p[0];
        let (x1, y1, iz1) = p[1];
        let (x2, y2, iz2) = p[2];
        let min_x = x0.min(x1).min(x2).floor().max(0.0) as i32;
        let max_x = x0.max(x1).max(x2).ceil().min(self.w as f32 - 1.0) as i32;
        let min_y = y0.min(y1).min(y2).floor().max(0.0) as i32;
        let max_y = y0.max(y1).max(y2).ceil().min(self.h as f32 - 1.0) as i32;
        if min_x > max_x || min_y > max_y {
            return;
        }
        let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if area.abs() < 1e-6 {
            return;
        }
        let inv_area = 1.0 / area;
        let a = clamp(alpha, 0.0, 1.0);
        for py in min_y..=max_y {
            let fy = py as f32 + 0.5;
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let w0 = ((x1 - fx) * (y2 - fy) - (x2 - fx) * (y1 - fy)) * inv_area;
                let w1 = ((x2 - fx) * (y0 - fy) - (x0 - fx) * (y2 - fy)) * inv_area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let iz = w0 * iz0 + w1 * iz1 + w2 * iz2;
                let idx = py as usize * self.w + px as usize;
                if iz <= 1e-6 || iz <= self.depth[idx] {
                    continue;
                }
                self.color[idx] = self.color[idx].lerp(color, a);
            }
        }
    }

    /// Flat shaded triangle with a z test.
    pub fn flat_tri(
        &mut self,
        p: [(f32, f32, f32); 3],
        color: Rgb,
        shade: f32,
        fog: Rgb,
        fog_amount: f32,
    ) {
        let (x0, y0, iz0) = p[0];
        let (x1, y1, iz1) = p[1];
        let (x2, y2, iz2) = p[2];
        let min_x = x0.min(x1).min(x2).floor().max(0.0) as i32;
        let max_x = x0.max(x1).max(x2).ceil().min(self.w as f32 - 1.0) as i32;
        let min_y = y0.min(y1).min(y2).floor().max(0.0) as i32;
        let max_y = y0.max(y1).max(y2).ceil().min(self.h as f32 - 1.0) as i32;
        if min_x > max_x || min_y > max_y {
            return;
        }
        let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if area.abs() < 1e-6 {
            return;
        }
        let inv_area = 1.0 / area;
        let c = color.scale(shade).lerp(fog, fog_amount);
        let dw0_dx = (y1 - y2) * inv_area;
        let dw0_dy = (x2 - x1) * inv_area;
        let dw1_dx = (y2 - y0) * inv_area;
        let dw1_dy = (x0 - x2) * inv_area;
        let (sx, sy) = (min_x as f32 + 0.5, min_y as f32 + 0.5);
        let mut row0 = ((x1 - sx) * (y2 - sy) - (x2 - sx) * (y1 - sy)) * inv_area;
        let mut row1 = ((x2 - sx) * (y0 - sy) - (x0 - sx) * (y2 - sy)) * inv_area;
        for py in min_y..=max_y {
            let mut w0 = row0;
            let mut w1 = row1;
            let row = py as usize * self.w;
            for px in min_x..=max_x {
                let w2 = 1.0 - w0 - w1;
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    let iz = w0 * iz0 + w1 * iz1 + w2 * iz2;
                    let idx = row + px as usize;
                    if iz > 1e-6 && iz > self.depth[idx] {
                        self.depth[idx] = iz;
                        self.color[idx] = c;
                    }
                }
                w0 += dw0_dx;
                w1 += dw1_dx;
            }
            row0 += dw0_dy;
            row1 += dw1_dy;
        }
    }

    /// Gouraud shaded triangle with a z test: the shade at each corner is
    /// interpolated across the face.
    ///
    /// A facet is not what makes a limb look flat — a facet given a single shade
    /// *is*. Interpolated per-corner shading lets the same geometry read as a
    /// curved surface, which is why this replaced flat shading nearly everywhere
    /// the figures are drawn. The shade is affine in screen space, so it steps
    /// along a scanline like the edge functions do.
    pub fn shade_tri(
        &mut self,
        p: [(f32, f32, f32); 3],
        color: Rgb,
        shades: [f32; 3],
        fog: Rgb,
        fog_amount: f32,
    ) {
        let (x0, y0, iz0) = p[0];
        let (x1, y1, iz1) = p[1];
        let (x2, y2, iz2) = p[2];
        let min_x = x0.min(x1).min(x2).floor().max(0.0) as i32;
        let max_x = x0.max(x1).max(x2).ceil().min(self.w as f32 - 1.0) as i32;
        let min_y = y0.min(y1).min(y2).floor().max(0.0) as i32;
        let max_y = y0.max(y1).max(y2).ceil().min(self.h as f32 - 1.0) as i32;
        if min_x > max_x || min_y > max_y {
            return;
        }
        let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if area.abs() < 1e-6 {
            return;
        }
        let inv_area = 1.0 / area;

        // Fog is constant over the triangle, so fold it into one multiply and one
        // offset per channel rather than lerping every pixel.
        let fog_t = clamp(fog_amount, 0.0, 1.0);
        let k_fog = 1.0 - fog_t;
        let fog_r = fog.r as f32 * fog_t;
        let fog_g = fog.g as f32 * fog_t;
        let fog_b = fog.b as f32 * fog_t;

        // shade = s2 + (s0 - s2) * w0 + (s1 - s2) * w1
        let d0 = shades[0] - shades[2];
        let d1 = shades[1] - shades[2];

        let dw0_dx = (y1 - y2) * inv_area;
        let dw0_dy = (x2 - x1) * inv_area;
        let dw1_dx = (y2 - y0) * inv_area;
        let dw1_dy = (x0 - x2) * inv_area;
        let ds_dx = d0 * dw0_dx + d1 * dw1_dx;
        let ds_dy = d0 * dw0_dy + d1 * dw1_dy;

        let (sx, sy) = (min_x as f32 + 0.5, min_y as f32 + 0.5);
        let mut row0 = ((x1 - sx) * (y2 - sy) - (x2 - sx) * (y1 - sy)) * inv_area;
        let mut row1 = ((x2 - sx) * (y0 - sy) - (x0 - sx) * (y2 - sy)) * inv_area;
        let mut row_s = shades[2] + row0 * d0 + row1 * d1;
        for py in min_y..=max_y {
            let mut w0 = row0;
            let mut w1 = row1;
            let mut s = row_s;
            let row = py as usize * self.w;
            for px in min_x..=max_x {
                let w2 = 1.0 - w0 - w1;
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    let iz = w0 * iz0 + w1 * iz1 + w2 * iz2;
                    let idx = row + px as usize;
                    if iz > 1e-6 && iz > self.depth[idx] {
                        let k = s * k_fog;
                        self.depth[idx] = iz;
                        self.color[idx] = Rgb::new(
                            tint(color.r, k, fog_r),
                            tint(color.g, k, fog_g),
                            tint(color.b, k, fog_b),
                        );
                    }
                }
                w0 += dw0_dx;
                w1 += dw1_dx;
                s += ds_dx;
            }
            row0 += dw0_dy;
            row1 += dw1_dy;
            row_s += ds_dy;
        }
    }

    pub fn downsample(&self, factor: usize) -> Frame {
        if factor <= 1 {
            return self.clone();
        }
        let w = (self.w / factor).max(1);
        let h = (self.h / factor).max(1);
        let mut out = Frame::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0u32; 3];
                let mut n = 0u32;
                for oy in 0..factor {
                    for ox in 0..factor {
                        let sx = x * factor + ox;
                        let sy = y * factor + oy;
                        if sx < self.w && sy < self.h {
                            let c = self.color[sy * self.w + sx];
                            acc[0] += c.r as u32;
                            acc[1] += c.g as u32;
                            acc[2] += c.b as u32;
                            n += 1;
                        }
                    }
                }
                let n = n.max(1);
                out.color[y * w + x] = Rgb::new((acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8);
            }
        }
        out
    }

    pub fn to_rgb8(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.w * self.h * 3);
        for c in &self.color {
            buf.push(c.r);
            buf.push(c.g);
            buf.push(c.b);
        }
        buf
    }
}

/// Simple RGB texture with bilinear sampling.
#[derive(Clone)]
pub struct Texture {
    pub w: usize,
    pub h: usize,
    pub px: Vec<Rgb>,
    pub wrap: bool,
}

impl Texture {
    pub fn new(w: usize, h: usize, fill: Rgb) -> Self {
        Texture { w, h, px: vec![fill; w * h], wrap: false }
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        self.px[y as usize * self.w + x as usize] = c;
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Rgb {
        let xx = clamp(x as f32, 0.0, self.w as f32 - 1.0) as i32;
        let yy = clamp(y as f32, 0.0, self.h as f32 - 1.0) as i32;
        self.px[yy as usize * self.w + xx as usize]
    }

    /// `u`, `v` are normalised 0..1. Bilinear in fixed point: the ground pass
    /// calls this once per pixel, so it stays off the clamped-float path.
    #[inline]
    pub fn sample(&self, u: f32, v: f32) -> Rgb {
        let w = self.w;
        let h = self.h;
        let fx = clamp(u, 0.0, 0.999_9) * (w as f32 - 1.0);
        let fy = clamp(v, 0.0, 0.999_9) * (h as f32 - 1.0);
        let x0 = fx as usize;
        let y0 = fy as usize;
        let x1 = (x0 + 1).min(w - 1);
        let y1 = (y0 + 1).min(h - 1);
        let tx = ((fx - x0 as f32) * 256.0) as u32;
        let ty = ((fy - y0 as f32) * 256.0) as u32;
        let row0 = y0 * w;
        let row1 = y1 * w;
        let p00 = self.px[row0 + x0];
        let p10 = self.px[row0 + x1];
        let p01 = self.px[row1 + x0];
        let p11 = self.px[row1 + x1];
        let top_r = blend8(p00.r as u32, p10.r as u32, tx);
        let top_g = blend8(p00.g as u32, p10.g as u32, tx);
        let top_b = blend8(p00.b as u32, p10.b as u32, tx);
        let bot_r = blend8(p01.r as u32, p11.r as u32, tx);
        let bot_g = blend8(p01.g as u32, p11.g as u32, tx);
        let bot_b = blend8(p01.b as u32, p11.b as u32, tx);
        Rgb::new(
            blend8(top_r, bot_r, ty) as u8,
            blend8(top_g, bot_g, ty) as u8,
            blend8(top_b, bot_b, ty) as u8,
        )
    }
}
