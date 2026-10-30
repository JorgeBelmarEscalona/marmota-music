use cairo::ImageSurface;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub f64, pub f64, pub f64);

impl Rgb {
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self(r, g, b)
    }
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        Rgb(
            self.0 + (other.0 - self.0) * t,
            self.1 + (other.1 - self.1) * t,
            self.2 + (other.2 - self.2) * t,
        )
    }
    pub fn lighten(self, t: f64) -> Rgb {
        self.mix(Rgb::new(1.0, 1.0, 1.0), t)
    }
    pub fn darken(self, t: f64) -> Rgb {
        self.mix(Rgb::new(0.0, 0.0, 0.0), t)
    }
    #[allow(dead_code)]
    pub fn alpha(self, a: f64) -> (f64, f64, f64, f64) {
        (self.0, self.1, self.2, a.clamp(0.0, 1.0))
    }
    pub fn to_hsv(self) -> (f64, f64, f64) {
        let (r, g, b) = (self.0, self.1, self.2);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / d) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        let h = if h < 0.0 { h + 360.0 } else { h };
        let s = if max == 0.0 { 0.0 } else { d / max };
        (h, s, max)
    }
    pub fn from_hsv(h: f64, s: f64, v: f64) -> Rgb {
        let c = v * s;
        let h = h.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as i32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        Rgb(r + m, g + m, b + m)
    }
}

pub const TEXT: Rgb = Rgb::new(0.965, 0.972, 0.985);
pub const MUTED: Rgb = Rgb::new(0.72, 0.74, 0.79);
pub const DIM: Rgb = Rgb::new(0.52, 0.54, 0.60);
pub const ACCENT_FALLBACK: Rgb = Rgb::new(0.878, 0.471, 0.310);

/// Pick a saturated accent from the cover, keeping it usable on dark glass.
pub fn accent_from_surface(surface: &mut ImageSurface) -> Rgb {
    let w = surface.width();
    let h = surface.height();
    if w < 4 || h < 4 {
        return ACCENT_FALLBACK;
    }
    let stride = surface.stride() as usize;
    let Ok(data) = surface.data() else {
        return ACCENT_FALLBACK;
    };
    let mut best: Vec<(f64, Rgb)> = Vec::new();
    let step_x = (w as usize / 48).max(1);
    let step_y = (h as usize / 48).max(1);
    for y in (0..h as usize).step_by(step_y) {
        for x in (0..w as usize).step_by(step_x) {
            let i = y * stride + x * 4;
            if i + 3 >= data.len() {
                continue;
            }
            let b = data[i] as f64 / 255.0;
            let g = data[i + 1] as f64 / 255.0;
            let r = data[i + 2] as f64 / 255.0;
            let c = Rgb(r, g, b);
            let (_, s, v) = c.to_hsv();
            if v < 0.18 || v > 0.97 {
                continue;
            }
            let weight = s * (0.4 + v * 0.6);
            if weight > 0.12 {
                best.push((weight, c));
            }
        }
    }
    if best.is_empty() {
        return ACCENT_FALLBACK;
    }
    best.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let take = (best.len() / 6).max(1);
    let mut r = 0.0;
    let mut g = 0.0;
    let mut b = 0.0;
    for (weight, c) in best.iter().take(take) {
        r += c.0 * weight;
        g += c.1 * weight;
        b += c.2 * weight;
    }
    let total: f64 = best.iter().take(take).map(|(w, _)| *w).sum();
    let avg = Rgb(r / total, g / total, b / total);
    let (hue, sat, val) = avg.to_hsv();
    let out = Rgb::from_hsv(hue, sat.max(0.52).min(0.92), val.max(0.62).min(0.94));
    // Blend a little towards the fallback so it never turns acid-green.
    out.mix(ACCENT_FALLBACK, 0.18)
}
