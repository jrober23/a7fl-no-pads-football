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

    /// How far the fill at `row` is inset by the corner radius.
    fn corner_inset(row: i32, h: i32, r: i32) -> i32 {
        if r <= 0 || h <= 0 {
            return 0;
        }
        let from_edge = row.min(h - 1 - row);
        if from_edge >= r {
            return 0;
        }
        let dy = (r - from_edge) as f32 - 0.5;
        let inner = (r as f32 * r as f32 - dy * dy).max(0.0).sqrt();
        (r as f32 - inner).round() as i32
    }

    /// A panel rather than a slab: a soft shadow, the colour running from a lit
    /// top to a shaded bottom, a hairline edge and a highlight along the top.
    ///
    /// This is what an overlay is built from now. Flat rectangles of one colour
    /// were the other half of why the broadcast chrome read as an old console
    /// menu: nothing in it had depth, light or an edge.
    #[allow(clippy::too_many_arguments)]
    pub fn panel(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        top: Rgb,
        bottom: Rgb,
        edge: Rgb,
        shadow: f32,
    ) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = radius.max(0).min(w / 2).min(h / 2);
        if shadow > 0.0 {
            self.rounded_rect_alpha(x - 2, y - 1, w + 4, h + 5, r + 2, Rgb::BLACK, shadow * 0.45);
        }
        if r > 0 {
            self.rounded_rect(x - 1, y - 1, w + 2, h + 2, r + 1, edge);
        }
        for row in 0..h {
            let t = if h > 1 {
                row as f32 / (h - 1) as f32
            } else {
                0.0
            };
            let inset = Self::corner_inset(row, h, r);
            self.fill_rect(x + inset, y + row, w - inset * 2, 1, top.lerp(bottom, t));
        }
        if r > 0 {
            let lit = top.lerp(Rgb::new(255, 255, 255), 0.22);
            self.fill_rect(x + r, y, w - r * 2, 1, lit);
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
        let mut tile = self.whole();
        tile.textured_tri(v, tex, shade, fog, fog_amount);
    }

    /// A textured triangle whose corners carry their own lighting.
    ///
    /// The turf is drawn flat-shaded because the ground has one normal; cloth
    /// does not. This is `shade_tri` and `textured_tri` in one loop: the light
    /// runs across the triangle, the texture is sampled with a perspective
    /// correction, and both land on the pixel together.
    pub fn textured_shade_tri(
        &mut self,
        p: [(f32, f32, f32); 3],
        uv: [(f32, f32); 3],
        shades: [f32; 3],
        tex: &Texture,
        fog: Rgb,
        fog_amount: f32,
    ) {
        let mut tile = self.whole();
        tile.textured_shade_tri(p, uv, shades, tex, fog, fog_amount);
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
        let mut tile = self.whole();
        tile.shade_tri(p, color, shades, fog, fog_amount);
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
#[derive(Clone, Debug)]
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


/// A coverage buffer: what a body covers on the ground, before it becomes a
/// shadow.
///
/// Shadow shapes overlap themselves heavily -- a torso lies across its own
/// thighs, a swinging arm crosses the trunk -- so they cannot be blended
/// straight onto the picture: every overlap would darken the turf again and the
/// middle of a man would go black. Coverage is collected here instead and spent
/// once, at the end, after it has been softened.
pub struct Mask {
    pub w: usize,
    pub h: usize,
    /// 0 for clear ground, 255 for a pixel a body fully covers.
    pub cover: Vec<u8>,
}

impl Mask {
    pub fn new(w: usize, h: usize) -> Self {
        Mask { w, h, cover: vec![0; w * h] }
    }

    /// Cover a triangle's pixels, keeping the strongest coverage any shape gave
    /// a pixel, so a second shape over the first changes nothing.
    pub fn tri(&mut self, p: [(f32, f32); 3]) {
        if self.w == 0 || self.h == 0 {
            return;
        }
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        // Seen edge-on, a shape covers no ground: a body lying flat is a line.
        if area.abs() < 1e-6 {
            return;
        }
        let min_y = p.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor();
        let max_y = p.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil();
        let y0 = min_y.max(0.0) as i32;
        let y1 = max_y.min(self.h as f32 - 1.0) as i32;
        for yy in y0..=y1 {
            let yc = yy as f32 + 0.5;
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            for k in 0..3 {
                let (ax, ay) = p[k];
                let (bx, by) = p[(k + 1) % 3];
                if (ay - by).abs() < 1e-6 || yc < ay.min(by) || yc > ay.max(by) {
                    continue;
                }
                let x = ax + (bx - ax) * (yc - ay) / (by - ay);
                lo = lo.min(x);
                hi = hi.max(x);
            }
            if lo > hi {
                continue;
            }
            let xa = lo.ceil().max(0.0) as i32;
            let xb = hi.floor().min(self.w as f32 - 1.0) as i32;
            for xx in xa..=xb {
                self.cover[yy as usize * self.w + xx as usize] = 255;
            }
        }
    }

    /// Soften the coverage. Two box passes either way, which is close enough to a
    /// gaussian at this size and costs four running sums a pixel.
    pub fn blur(&mut self, radius: usize) {
        if radius == 0 || self.w == 0 || self.h == 0 {
            return;
        }
        let mut tmp = vec![0u8; self.cover.len()];
        for _ in 0..2 {
            box_pass(&self.cover, &mut tmp, self.w, self.h, radius as i32, true);
            box_pass(&tmp, &mut self.cover, self.w, self.h, radius as i32, false);
        }
    }

    /// Spend a half-size coverage buffer on the full picture.
    ///
    /// A shadow is a blurred silhouette, so it carries no detail a smaller mask
    /// would lose, and the mask, its blur and its composite are four times
    /// cheaper. Every pixel of the picture reads the mask pixel it covers and
    /// the bands still run in parallel.
    pub fn cast_scaled(&self, frame: &mut Frame, factor: usize, darkness: f32, tint: Rgb) {
        if factor == 0 || self.w == 0 || self.h == 0 || frame.w == 0 {
            return;
        }
        let w = frame.w;
        let rows = frame.h.div_ceil(8).max(1);
        std::thread::scope(|scope| {
            for (bi, band) in frame.color.chunks_mut(rows * w).enumerate() {
                scope.spawn(move || {
                    let y0 = bi * rows;
                    for local in 0..band.len() / w {
                        let my = (y0 + local) / factor;
                        if my >= self.h {
                            continue;
                        }
                        let cover = &self.cover[my * self.w..(my + 1) * self.w];
                        let row = &mut band[local * w..(local + 1) * w];
                        for (x, px) in row.iter_mut().enumerate() {
                            let c = cover[(x / factor).min(self.w - 1)];
                            if c == 0 {
                                continue;
                            }
                            let k = c as f32 / 255.0 * darkness;
                            *px = px.lerp(tint, k);
                        }
                    }
                });
            }
        });
    }

    /// Spend the coverage: darken the ground under it. Never touches depth, so a
    /// body drawn afterwards still stands in front of its own shadow.
    pub fn cast(&self, frame: &mut Frame, darkness: f32, tint: Rgb) {
        for (i, c) in self.cover.iter().enumerate() {
            if *c == 0 {
                continue;
            }
            let Some(px) = frame.color.get_mut(i) else { return };
            let k = *c as f32 / 255.0 * darkness;
            *px = px.lerp(tint, k);
        }
    }
}

fn box_pass(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: i32, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    if len == 0 || lines == 0 {
        return;
    }
    let span = (2 * r + 1) as u32;
    for line in 0..lines {
        let at = |i: i32, line: usize| -> u32 {
            let i = i.clamp(0, len as i32 - 1) as usize;
            let idx = if horizontal { line * w + i } else { i * w + line };
            src[idx] as u32
        };
        let mut sum: u32 = (-r..=r).map(|i| at(i, line)).sum();
        for i in 0..len as i32 {
            let o = if horizontal { line * w + i as usize } else { i as usize * w + line };
            dst[o] = (sum / span) as u8;
            sum -= at(i - r, line);
            sum += at(i + r + 1, line);
        }
    }
}

#[cfg(test)]
mod mask_tests {
    use super::*;

    const TURF: Rgb = Rgb::hex(0x3A7C41);
    const SHADE: Rgb = Rgb::hex(0x0B1A10);

    #[test]
    fn overlapping_shapes_darken_the_ground_once() {
        let mut one = Mask::new(32, 32);
        one.tri([(4.0, 4.0), (20.0, 4.0), (12.0, 20.0)]);
        let mut twice = Mask::new(32, 32);
        twice.tri([(4.0, 4.0), (20.0, 4.0), (12.0, 20.0)]);
        twice.tri([(6.0, 6.0), (18.0, 6.0), (12.0, 18.0)]);
        let mut a = Frame::new(32, 32);
        let mut b = Frame::new(32, 32);
        a.clear(TURF);
        b.clear(TURF);
        one.cast(&mut a, 0.42, SHADE);
        twice.cast(&mut b, 0.42, SHADE);
        assert_eq!(a.color, b.color, "a second shape over the first darkened it twice");
        assert_ne!(a.color[32 * 12 + 12], TURF, "the shadow never landed");
    }

    #[test]
    fn the_edge_of_a_shadow_is_softer_than_its_middle() {
        let mut mask = Mask::new(64, 64);
        mask.tri([(16.0, 16.0), (48.0, 16.0), (32.0, 48.0)]);
        let mut hard = Frame::new(64, 64);
        hard.clear(TURF);
        mask.cast(&mut hard, 0.42, SHADE);
        mask.blur(2);
        let mut soft = Frame::new(64, 64);
        soft.clear(TURF);
        mask.cast(&mut soft, 0.42, SHADE);
        assert_eq!(hard.color[64 * 26 + 32], soft.color[64 * 26 + 32], "softening dulled the middle");
        assert_ne!(hard.color[64 * 17 + 32], soft.color[64 * 17 + 32], "blur never reached the edge");
        assert_eq!(hard.color[64 * 14 + 32], TURF, "the hard shadow was already outside its shape");
        assert_ne!(soft.color[64 * 14 + 32], TURF, "the soft shadow has no fringe");
    }
}

#[cfg(test)]
mod texture_tests {
    use super::*;

    fn two_tone() -> Texture {
        let mut tex = Texture::new(2, 2, Rgb::hex(0xFF0000));
        tex.set(1, 0, Rgb::hex(0x0000FF));
        tex.set(0, 1, Rgb::hex(0xFF0000));
        tex.set(1, 1, Rgb::hex(0x0000FF));
        tex
    }

    /// A right triangle with `u` across it and `v` down it, at a constant depth.
    fn draw(frame: &mut Frame, tex: &Texture, shade: f32) {
        frame.textured_shade_tri(
            [(0.0, 0.0, 0.1), (16.0, 0.0, 0.1), (0.0, 16.0, 0.1)],
            [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)],
            [shade; 3],
            tex,
            Rgb::BLACK,
            0.0,
        );
    }

    #[test]
    fn a_textured_triangle_reads_across_its_texture() {
        let mut frame = Frame::new(16, 16);
        draw(&mut frame, &two_tone(), 1.0);
        let left = frame.get(2, 2);
        let right = frame.get(12, 2);
        assert!(left.r > right.r + 60, "the left of the triangle is not the red side: {left:?}");
        assert!(right.b > left.b + 60, "the right of the triangle is not the blue side");
    }

    #[test]
    fn the_light_still_runs_across_a_textured_triangle() {
        let mut lit = Frame::new(16, 16);
        let mut dim = Frame::new(16, 16);
        draw(&mut lit, &two_tone(), 1.0);
        draw(&mut dim, &two_tone(), 0.5);
        let a = lit.get(6, 6);
        let b = dim.get(6, 6);
        assert!(a.r > b.r + 20, "half the light did not darken the cloth");
    }
}

/// A horizontal band of the frame.
///
/// Every pass that fills a lot of pixels runs on a band rather than on the whole
/// picture, so bands can be filled in parallel: each owns a disjoint slice of
/// colour and depth, so two threads can never touch the same pixel and nothing
/// needs a lock. `Frame`'s own methods are the same code with one band covering
/// everything, for the small passes that are not worth splitting.
pub struct Tile<'a> {
    pub w: usize,
    pub y0: usize,
    pub rows: usize,
    pub color: &'a mut [Rgb],
    pub depth: &'a mut [f32],
}

impl Tile<'_> {
    /// The rows of a triangle that fall inside this band.
    #[inline]
    fn rows_of(&self, ys: [f32; 3]) -> (i32, i32) {
        let min = ys.iter().copied().fold(f32::MAX, f32::min).floor();
        let max = ys.iter().copied().fold(f32::MIN, f32::max).ceil();
        let top = self.y0 as f32;
        let bottom = (self.y0 + self.rows) as f32 - 1.0;
        (min.max(top) as i32, max.min(bottom) as i32)
    }

    #[inline]
    fn row(&self, py: i32) -> usize {
        (py as usize - self.y0) * self.w
    }

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
        let (min_y, max_y) = self.rows_of([y0, y1, y2]);
        if min_x > max_x || min_y > max_y {
            return;
        }
        let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if area.abs() < 1e-6 {
            return;
        }
        let inv_area = 1.0 / area;

        let fog_t = clamp(fog_amount, 0.0, 1.0);
        let k_fog = 1.0 - fog_t;
        let fog_r = fog.r as f32 * fog_t;
        let fog_g = fog.g as f32 * fog_t;
        let fog_b = fog.b as f32 * fog_t;

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
            let row = self.row(py);
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
        let (min_y, max_y) = self.rows_of([y0, y1, y2]);
        if min_x > max_x || min_y > max_y {
            return;
        }
        let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if area.abs() < 1e-6 {
            return;
        }
        let inv_area = 1.0 / area;

        let fog_t = clamp(fog_amount, 0.0, 1.0);
        let k = shade * (1.0 - fog_t);
        let fog_r = fog.r as f32 * fog_t;
        let fog_g = fog.g as f32 * fog_t;
        let fog_b = fog.b as f32 * fog_t;

        let ku0 = u0 * iz0;
        let ku1 = u1 * iz1;
        let ku2 = u2 * iz2;
        let kv0 = v0 * iz0;
        let kv1 = v1 * iz1;
        let kv2 = v2 * iz2;

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
            let row = self.row(py);
            for px in min_x..=max_x {
                let w2 = 1.0 - w0 - w1;
                if w0 >= -0.0005 && w1 >= -0.0005 && w2 >= -0.0005 {
                    let iz = w0 * iz0 + w1 * iz1 + w2 * iz2;
                    let idx = row + px as usize;
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

    #[allow(clippy::too_many_arguments)]
    pub fn textured_shade_tri(
        &mut self,
        p: [(f32, f32, f32); 3],
        uv: [(f32, f32); 3],
        shades: [f32; 3],
        tex: &Texture,
        fog: Rgb,
        fog_amount: f32,
    ) {
        let (x0, y0, iz0) = p[0];
        let (x1, y1, iz1) = p[1];
        let (x2, y2, iz2) = p[2];
        let min_x = x0.min(x1).min(x2).floor().max(0.0) as i32;
        let max_x = x0.max(x1).max(x2).ceil().min(self.w as f32 - 1.0) as i32;
        let (min_y, max_y) = self.rows_of([y0, y1, y2]);
        if min_x > max_x || min_y > max_y {
            return;
        }
        let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if area.abs() < 1e-6 {
            return;
        }
        let inv_area = 1.0 / area;

        let fog_t = clamp(fog_amount, 0.0, 1.0);
        let k_fog = 1.0 - fog_t;
        let fog_r = fog.r as f32 * fog_t;
        let fog_g = fog.g as f32 * fog_t;
        let fog_b = fog.b as f32 * fog_t;

        let ((u0, w0t), (u1, w1t), (u2, w2t)) = (uv[0], uv[1], uv[2]);
        let ku0 = u0 * iz0;
        let ku1 = u1 * iz1;
        let ku2 = u2 * iz2;
        let kv0 = w0t * iz0;
        let kv1 = w1t * iz1;
        let kv2 = w2t * iz2;

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
            let row = self.row(py);
            for px in min_x..=max_x {
                let w2 = 1.0 - w0 - w1;
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    let iz = w0 * iz0 + w1 * iz1 + w2 * iz2;
                    let idx = row + px as usize;
                    if iz > 1e-6 && iz > self.depth[idx] {
                        let inv_z = 1.0 / iz;
                        let u = (w0 * ku0 + w1 * ku1 + w2 * ku2) * inv_z;
                        let vtex = (w0 * kv0 + w1 * kv1 + w2 * kv2) * inv_z;
                        let k = s * k_fog;
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
                s += ds_dx;
            }
            row0 += dw0_dy;
            row1 += dw1_dy;
            row_s += ds_dy;
        }
    }
}

impl Frame {
    /// The whole picture as one band, for passes not worth splitting.
    pub fn whole(&mut self) -> Tile<'_> {
        Tile { w: self.w, y0: 0, rows: self.h, color: &mut self.color, depth: &mut self.depth }
    }

    /// Split the frame into horizontal bands that can be filled in parallel.
    pub fn tiles(&mut self, count: usize) -> Vec<Tile<'_>> {
        if self.w == 0 || self.h == 0 {
            return Vec::new();
        }
        let count = count.clamp(1, self.h);
        let rows = self.h.div_ceil(count);
        let w = self.w;
        let mut out = Vec::with_capacity(count);
        let mut colors = self.color.chunks_mut(rows * w);
        let mut depths = self.depth.chunks_mut(rows * w);
        let mut y0 = 0;
        while let (Some(color), Some(depth)) = (colors.next(), depths.next()) {
            let r = color.len() / w;
            out.push(Tile { w, y0, rows: r, color, depth });
            y0 += r;
        }
        out
    }
}
