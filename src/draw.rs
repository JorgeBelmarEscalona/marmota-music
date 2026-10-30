#![allow(unused_must_use)] // cairo returns Result for every path call
use gtk4 as gtk;
use cairo::{Context, Filter, ImageSurface, LineCap, LineJoin, SurfacePattern};
use gtk::pango;

use crate::model::{CollKind, Rect, Source, Track};
use crate::theme::{self, Rgb};
use crate::ui::{App, Hit, List, RowItem, Tab};

#[allow(dead_code)]
const PAD: f64 = 0.0;

fn rr(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w.abs() / 2.0).min(h.abs() / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.close_path();
}

fn set(cr: &Context, c: Rgb, a: f64) {
    cr.set_source_rgba(c.0, c.1, c.2, a.clamp(0.0, 1.0));
}

fn label(
    cr: &Context,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    size: f64,
    weight: pango::Weight,
    color: Rgb,
    alpha: f64,
) -> f64 {
    let layout = pangocairo::functions::create_layout(cr);
    let mut fd = pango::FontDescription::new();
    fd.set_family("Noto Sans");
    fd.set_absolute_size(size * pango::SCALE as f64);
    fd.set_weight(weight);
    layout.set_font_description(Some(&fd));
    layout.set_text(text);
    layout.set_width((w.max(1.0) * pango::SCALE as f64) as i32);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout.set_single_paragraph_mode(true);
    let (_, hh) = layout.pixel_size();
    cr.move_to(x, y);
    set(cr, color, alpha);
    pangocairo::functions::show_layout(cr, &layout);
    hh as f64
}

fn label_center(
    cr: &Context,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    size: f64,
    weight: pango::Weight,
    color: Rgb,
    alpha: f64,
) {
    let layout = pangocairo::functions::create_layout(cr);
    let mut fd = pango::FontDescription::new();
    fd.set_family("Noto Sans");
    fd.set_absolute_size(size * pango::SCALE as f64);
    fd.set_weight(weight);
    layout.set_font_description(Some(&fd));
    layout.set_text(text);
    layout.set_width((w.max(1.0) * pango::SCALE as f64) as i32);
    layout.set_alignment(pango::Alignment::Center);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout.set_single_paragraph_mode(true);
    cr.move_to(x, y);
    set(cr, color, alpha);
    pangocairo::functions::show_layout(cr, &layout);
}

fn para(
    cr: &Context,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    size: f64,
    weight: pango::Weight,
    color: Rgb,
    alpha: f64,
    center: bool,
) -> f64 {
    let layout = pangocairo::functions::create_layout(cr);
    let mut fd = pango::FontDescription::new();
    fd.set_family("Noto Sans");
    fd.set_absolute_size(size * pango::SCALE as f64);
    fd.set_weight(weight);
    layout.set_font_description(Some(&fd));
    layout.set_text(text);
    layout.set_width((w.max(1.0) * pango::SCALE as f64) as i32);
    layout.set_wrap(pango::WrapMode::WordChar);
    if center {
        layout.set_alignment(pango::Alignment::Center);
    }
    let (_, hh) = layout.pixel_size();
    cr.move_to(x, y);
    set(cr, color, alpha);
    pangocairo::functions::show_layout(cr, &layout);
    hh as f64
}

fn format_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn rounded_pic(cr: &Context, r: Rect, radius: f64, surf: &ImageSurface) {
    let sw = surf.width() as f64;
    let sh = surf.height() as f64;
    if sw <= 0.0 || sh <= 0.0 {
        return;
    }
    let s = (r.w / sw).max(r.h / sh);
    let dw = sw * s;
    let dh = sh * s;
    let _ = cr.save();
    rr(cr, r.x, r.y, r.w, r.h, radius);
    cr.clip();
    cr.translate(r.x + (r.w - dw) / 2.0, r.y + (r.h - dh) / 2.0);
    cr.scale(s, s);
    let _ = cr.set_source_surface(surf, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();
}

fn blurred_pic(cr: &Context, r: Rect, radius: f64, surf: &ImageSurface, alpha: f64) {
    let sw = surf.width() as f64;
    let sh = surf.height() as f64;
    if sw <= 0.0 || sh <= 0.0 {
        return;
    }
    let small = 12;
    let Ok(tmp) = ImageSurface::create(cairo::Format::ARgb32, small, small) else {
        return;
    };
    {
        let Ok(tcr) = Context::new(&tmp) else { return };
        tcr.scale(small as f64 / sw, small as f64 / sh);
        let _ = tcr.set_source_surface(surf, 0.0, 0.0);
        let _ = tcr.paint();
    }

    let _ = cr.save();
    rr(cr, r.x, r.y, r.w, r.h, radius);
    cr.clip();
    cr.translate(r.x, r.y);
    cr.scale(r.w / small as f64, r.h / small as f64);
    let pat = SurfacePattern::create(&tmp);
    pat.set_filter(Filter::Bilinear);
    cr.set_source(&pat).ok();
    cr.paint_with_alpha(alpha).ok();
    let _ = cr.restore();
}



fn placeholder_art(cr: &Context, r: Rect, radius: f64, accent: Rgb, alpha: f64) {
    let _ = cr.save();
    rr(cr, r.x, r.y, r.w, r.h, radius);
    cr.clip();
    let g = cairo::LinearGradient::new(r.x, r.y, r.x + r.w, r.y + r.h);
    g.add_color_stop_rgba(0.0, accent.0 * 0.55, accent.1 * 0.55, accent.2 * 0.55, alpha);
    g.add_color_stop_rgba(1.0, 0.05, 0.05, 0.07, alpha);
    cr.set_source(&g).ok();
    let _ = cr.paint();
    let _ = cr.restore();
    icon(cr, Icon::Music, r.cx(), r.cy(), r.w * 0.42, Rgb::new(1.0, 1.0, 1.0), 0.7 * alpha);
}

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Shuffle,
    Repeat,
    RepeatOne,
    Volume,
    ChevronUp,
    ChevronDown,
    ChevronLeft,
    PlayNext,
    Search,
    Plus,
    Close,
    Queue,
    Check,
    Music,
    Video,
    Spotify,
}

fn icon(cr: &Context, kind: Icon, cx: f64, cy: f64, s: f64, color: Rgb, alpha: f64) {
    let _ = cr.save();
    cr.set_line_width((s * 0.085).max(1.4));
    cr.set_line_cap(LineCap::Round);
    cr.set_line_join(LineJoin::Round);
    set(cr, color, alpha);
    let h = s * 0.5;
    match kind {
        Icon::Play => {
            cr.move_to(cx - s * 0.26, cy - s * 0.36);
            cr.line_to(cx + s * 0.36, cy);
            cr.line_to(cx - s * 0.26, cy + s * 0.36);
            cr.close_path();
            cr.fill();
        }
        Icon::Pause => {
            let w = s * 0.16;
            let hh = s * 0.62;
            rr(cr, cx - s * 0.28, cy - hh / 2.0, w, hh, w / 2.0);
            cr.fill();
            rr(cr, cx + s * 0.12, cy - hh / 2.0, w, hh, w / 2.0);
            cr.fill();
        }
        Icon::Prev => {
            cr.rectangle(cx - s * 0.34, cy - s * 0.34, s * 0.13, s * 0.68);
            cr.fill();
            cr.move_to(cx + s * 0.34, cy - s * 0.34);
            cr.line_to(cx - s * 0.12, cy);
            cr.line_to(cx + s * 0.34, cy + s * 0.34);
            cr.close_path();
            cr.fill();
        }
        Icon::Next => {
            cr.rectangle(cx + s * 0.21, cy - s * 0.34, s * 0.13, s * 0.68);
            cr.fill();
            cr.move_to(cx - s * 0.34, cy - s * 0.34);
            cr.line_to(cx + s * 0.12, cy);
            cr.line_to(cx - s * 0.34, cy + s * 0.34);
            cr.close_path();
            cr.fill();
        }
        Icon::Shuffle => {
            cr.move_to(cx - h, cy - h * 0.55);
            cr.curve_to(cx - h * 0.2, cy - h * 0.55, cx + h * 0.2, cy + h * 0.55, cx + h * 0.72, cy + h * 0.55);
            cr.stroke();
            cr.move_to(cx - h, cy + h * 0.55);
            cr.curve_to(cx - h * 0.2, cy + h * 0.55, cx + h * 0.2, cy - h * 0.55, cx + h * 0.72, cy - h * 0.55);
            cr.stroke();
            cr.move_to(cx + h * 0.42, cy - h * 0.86);
            cr.line_to(cx + h * 0.78, cy - h * 0.55);
            cr.line_to(cx + h * 0.42, cy - h * 0.24);
            cr.stroke();
            cr.move_to(cx + h * 0.42, cy + h * 0.24);
            cr.line_to(cx + h * 0.78, cy + h * 0.55);
            cr.line_to(cx + h * 0.42, cy + h * 0.86);
            cr.stroke();
        }
        Icon::Repeat | Icon::RepeatOne => {
            cr.arc(cx, cy, h * 0.78, -1.15, 2.4);
            cr.stroke();
            cr.move_to(cx + h * 0.92, cy - h * 0.52);
            cr.line_to(cx + h * 0.78, cy - h * 0.02);
            cr.line_to(cx + h * 0.42, cy - h * 0.34);
            cr.stroke();
            if kind == Icon::RepeatOne {
                label_center(cr, "1", cx - 3.0, cy - 6.0, 8.0, 8.0, pango::Weight::Bold, color, alpha);
            }
        }
        Icon::Volume => {
            cr.move_to(cx - h * 0.95, cy - h * 0.28);
            cr.line_to(cx - h * 0.55, cy - h * 0.28);
            cr.line_to(cx - h * 0.05, cy - h * 0.72);
            cr.line_to(cx - h * 0.05, cy + h * 0.72);
            cr.line_to(cx - h * 0.55, cy + h * 0.28);
            cr.line_to(cx - h * 0.95, cy + h * 0.28);
            cr.close_path();
            cr.fill();
            cr.arc(cx - h * 0.05, cy, h * 0.42, -0.9, 0.9);
            cr.stroke();
            cr.arc(cx - h * 0.05, cy, h * 0.78, -0.9, 0.9);
            cr.stroke();
        }
        Icon::ChevronUp => {
            cr.move_to(cx - h * 0.55, cy + h * 0.22);
            cr.line_to(cx, cy - h * 0.32);
            cr.line_to(cx + h * 0.55, cy + h * 0.22);
            cr.stroke();
        }
        Icon::ChevronDown => {
            cr.move_to(cx - h * 0.55, cy - h * 0.22);
            cr.line_to(cx, cy + h * 0.32);
            cr.line_to(cx + h * 0.55, cy - h * 0.22);
            cr.stroke();
        }
        Icon::ChevronLeft => {
            cr.move_to(cx + h * 0.22, cy - h * 0.55);
            cr.line_to(cx - h * 0.32, cy);
            cr.line_to(cx + h * 0.22, cy + h * 0.55);
            cr.stroke();
        }
        Icon::PlayNext => {
            // Lines with a small play triangle: "play next".
            for i in 0..2 {
                let yy = cy - h * 0.5 + i as f64 * h * 0.5;
                cr.move_to(cx - h * 0.85, yy);
                cr.line_to(cx + h * 0.15, yy);
            }
            cr.move_to(cx - h * 0.85, cy + h * 0.5);
            cr.line_to(cx - h * 0.15, cy + h * 0.5);
            cr.stroke();
            cr.move_to(cx + h * 0.15, cy + h * 0.15);
            cr.line_to(cx + h * 0.85, cy + h * 0.5);
            cr.line_to(cx + h * 0.15, cy + h * 0.85);
            cr.close_path();
            cr.fill();
        }
        Icon::Search => {
            cr.arc(cx - h * 0.15, cy - h * 0.15, h * 0.55, 0.0, std::f64::consts::TAU);
            cr.stroke();
            cr.move_to(cx + h * 0.25, cy + h * 0.25);
            cr.line_to(cx + h * 0.78, cy + h * 0.78);
            cr.stroke();
        }
        Icon::Plus => {
            cr.move_to(cx - h * 0.6, cy);
            cr.line_to(cx + h * 0.6, cy);
            cr.move_to(cx, cy - h * 0.6);
            cr.line_to(cx, cy + h * 0.6);
            cr.stroke();
        }
        Icon::Close => {
            cr.move_to(cx - h * 0.5, cy - h * 0.5);
            cr.line_to(cx + h * 0.5, cy + h * 0.5);
            cr.move_to(cx + h * 0.5, cy - h * 0.5);
            cr.line_to(cx - h * 0.5, cy + h * 0.5);
            cr.stroke();
        }
        Icon::Queue => {
            for i in 0..3 {
                let yy = cy - h * 0.55 + i as f64 * h * 0.55;
                cr.move_to(cx - h * 0.8, yy);
                cr.line_to(cx + h * 0.3, yy);
            }
            cr.stroke();
            cr.move_to(cx + h * 0.45, cy + h * 0.05);
            cr.line_to(cx + h * 0.85, cy + h * 0.3);
            cr.line_to(cx + h * 0.45, cy + h * 0.55);
            cr.close_path();
            cr.fill();
        }
        Icon::Music => {
            cr.arc(cx - h * 0.34, cy + h * 0.52, h * 0.32, 0.0, std::f64::consts::TAU);
            cr.fill();
            cr.move_to(cx - h * 0.03, cy + h * 0.52);
            cr.line_to(cx - h * 0.03, cy - h * 0.72);
            cr.stroke();
            cr.move_to(cx - h * 0.03, cy - h * 0.72);
            cr.curve_to(cx + h * 0.55, cy - h * 0.68, cx + h * 0.72, cy - h * 0.30, cx + h * 0.52, cy - h * 0.02);
            cr.stroke();
        }
        Icon::Video => {
            rr(cr, cx - h * 0.78, cy - h * 0.5, h * 1.56, h, h * 0.26);
            cr.stroke();
            cr.move_to(cx - h * 0.12, cy - h * 0.24);
            cr.line_to(cx + h * 0.34, cy);
            cr.line_to(cx - h * 0.12, cy + h * 0.24);
            cr.close_path();
            cr.fill();
        }
        Icon::Spotify => {
            // Three stacked waves, like the Spotify mark: concentric arcs
            // whose centre sits below the icon so they bow upwards.
            cr.set_line_width((s * 0.10).max(1.5));
            for i in 0..3 {
                let half = s * (0.44 - 0.06 * i as f64);
                let y = cy - s * (0.20 - 0.20 * i as f64);
                let lift = s * 0.15;
                cr.move_to(cx - half, y);
                cr.curve_to(
                    cx - half * 0.35,
                    y - lift,
                    cx + half * 0.35,
                    y - lift,
                    cx + half,
                    y,
                );
                cr.stroke();
            }
        }
        Icon::Check => {
            cr.move_to(cx - h * 0.55, cy);
            cr.line_to(cx - h * 0.1, cy + h * 0.45);
            cr.line_to(cx + h * 0.65, cy - h * 0.45);
            cr.stroke();
        }
    }
    let _ = cr.restore();
}

fn circle_button(cr: &Context, r: Rect, hovered: bool, alpha: f64) {
    if hovered {
        cr.arc(r.cx(), r.cy(), r.w / 2.0, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.09 * alpha);
        cr.fill();
    }
}

fn glass(cr: &Context, w: f64, h: f64, radius: f64, accent: Rgb, cover: Option<&ImageSurface>) {
    let base = Rgb::new(0.052, 0.055, 0.068).mix(accent.darken(0.52), 0.20);

    let _ = cr.save();
    rr(cr, 0.0, 0.0, w, h, radius);
    cr.clip();

    if let Some(surface) = cover {
        blurred_pic(cr, Rect::new(0.0, 0.0, w, h), 0.0, surface, 0.44);
    }

    let g = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
    g.add_color_stop_rgba(0.0, base.lighten(0.16).0, base.lighten(0.16).1, base.lighten(0.16).2, 0.90);
    g.add_color_stop_rgba(0.45, base.0, base.1, base.2, 0.95);
    g.add_color_stop_rgba(1.0, base.darken(0.48).0, base.darken(0.48).1, base.darken(0.48).2, 0.98);
    cr.set_source(&g).ok();
    let _ = cr.paint();

    // Accent blooms in the corners.
    let bloom = cairo::RadialGradient::new(w * 0.14, h * 0.05, 0.0, w * 0.14, h * 0.05, w * 0.55);
    bloom.add_color_stop_rgba(0.0, accent.0, accent.1, accent.2, 0.24);
    bloom.add_color_stop_rgba(1.0, accent.0, accent.1, accent.2, 0.0);
    cr.set_source(&bloom).ok();
    let _ = cr.paint();

    let bloom2 = cairo::RadialGradient::new(w * 0.92, h * 0.95, 0.0, w * 0.92, h * 0.95, w * 0.40);
    bloom2.add_color_stop_rgba(0.0, accent.0, accent.1, accent.2, 0.14);
    bloom2.add_color_stop_rgba(1.0, accent.0, accent.1, accent.2, 0.0);
    cr.set_source(&bloom2).ok();
    let _ = cr.paint();

    // Slow liquid sheen.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let phase = (now / 13.0).fract();
    let x = -w * 0.5 + phase * w * 2.0;
    let sheen = cairo::LinearGradient::new(x, -h * 0.2, x + w * 0.42, h * 1.2);
    sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.0);
    sheen.add_color_stop_rgba(0.45, 1.0, 1.0, 1.0, 0.045);
    sheen.add_color_stop_rgba(0.55, 1.0, 1.0, 1.0, 0.055);
    sheen.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
    cr.set_source(&sheen).ok();
    let _ = cr.paint();

    let _ = cr.restore();

    // Rim light.
    let rim = cairo::LinearGradient::new(0.0, 0.0, 0.0, h * 0.6);
    rim.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.30);
    rim.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.05);
    rr(cr, 0.75, 0.75, w - 1.5, h - 1.5, radius - 0.75);
    cr.set_source(&rim).ok();
    cr.set_line_width(1.1);
    cr.stroke();

    let inner = cairo::LinearGradient::new(0.0, h * 0.55, 0.0, h);
    inner.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
    inner.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.20);
    rr(cr, 1.5, 1.5, w - 3.0, h - 3.0, radius - 1.5);
    cr.set_source(&inner).ok();
    cr.set_line_width(1.0);
    cr.stroke();
}

impl App {
    pub fn draw(&self, cr: &Context, w: f64, h: f64) {
        let k = self.ui_scale;
        if k > 0.0 && (k - 1.0).abs() > 0.001 {
            cr.scale(k, k);
        }
        let radius = 37.0 + (30.0 - 37.0) * self.anim;
        glass(cr, w, h, radius, self.accent, self.cover.as_ref());

        if self.anim < 0.55 {
            let a = (1.0 - self.anim / 0.55).clamp(0.0, 1.0);
            self.draw_collapsed(cr, w, h, a);
        } else {
            let a = ((self.anim - 0.45) / 0.55).clamp(0.0, 1.0);
            self.draw_expanded(cr, w, h, a);
        }
    }

    fn draw_collapsed(&self, cr: &Context, w: f64, _h: f64, a: f64) {
        let l = self.collapsed_layout();

        // Progress hairline along the top edge.
        if self.dur > 0.0 {
            let prog = (self.pos / self.dur).clamp(0.0, 1.0);
            let pad = 18.0;
            rr(cr, pad, 2.0, w - pad * 2.0, 2.5, 1.25);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.10 * a);
            cr.fill();
            if prog > 0.0 {
                rr(cr, pad, 2.0, (w - pad * 2.0) * prog, 2.5, 1.25);
                set(cr, self.accent.lighten(0.15), 0.85 * a);
                cr.fill();
            }
        }

        // Artwork (or the video itself, in miniature).
        if let Some(frame) = self.video_surface.as_ref().filter(|_| self.showing_video()) {
            rounded_pic(cr, l.art, 13.0, frame);
        } else if let Some(surface) = &self.cover {
            rounded_pic(cr, l.art, 13.0, surface);
        } else {
            placeholder_art(cr, l.art, 13.0, self.accent, a);
        }
        rr(cr, l.art.x + 0.5, l.art.y + 0.5, l.art.w - 1.0, l.art.h - 1.0, 12.5);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12 * a);
        cr.set_line_width(1.0);
        cr.stroke();

        let title = self
            .track
            .as_ref()
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "Nada sonando".into());
        let artist = self
            .track
            .as_ref()
            .map(|t| t.subtitle())
            .unwrap_or_else(|| "Super + Shift + M".into());
        label(cr, &title, l.title.x, l.title.y, l.title.w, 14.0, pango::Weight::Semibold, theme::TEXT, a);
        label(cr, &artist, l.artist.x, l.artist.y, l.artist.w, 11.0, pango::Weight::Normal, theme::MUTED, 0.72 * a);

        // Mini lyrics.
        if !self.lyrics.is_empty() {
            let idx = self.lyric_idx.min(self.lyrics.len() - 1);
            let cur = &self.lyrics[idx];
            label(cr, &cur.text, l.lyric.x, l.lyric.y + 5.0, l.lyric.w, 12.0, pango::Weight::Medium, theme::TEXT, 0.88 * a);
            if let Some(next) = self.lyrics.get(idx + 1) {
                label(cr, &next.text, l.lyric.x, l.lyric.y + 28.0, l.lyric.w, 10.0, pango::Weight::Normal, theme::MUTED, 0.40 * a);
            }
            let _ = cr.save();
            let fade = cairo::LinearGradient::new(l.lyric.x, 0.0, l.lyric.x + l.lyric.w, 0.0);
            fade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            fade.add_color_stop_rgba(0.12, 0.0, 0.0, 0.0, 0.0);
            fade.add_color_stop_rgba(0.8, 1.0, 1.0, 1.0, 0.0);
            let _ = cr.restore();
        } else if self.lyrics_state == 1 {
            label(cr, "buscando letra…", l.lyric.x, l.lyric.y + 12.0, l.lyric.w, 10.5, pango::Weight::Normal, theme::DIM, 0.7 * a);
        }

        // Controls.
        circle_button(cr, l.prev, self.hover == Some(Hit::Prev), a);
        icon(cr, Icon::Prev, l.prev.cx(), l.prev.cy(), 17.0, theme::TEXT, 0.82 * a);
        circle_button(cr, l.next, self.hover == Some(Hit::Next), a);
        icon(cr, Icon::Next, l.next.cx(), l.next.cy(), 17.0, theme::TEXT, 0.82 * a);

        let playing = self.playing;
        cr.arc(l.play.cx(), l.play.cy(), l.play.w / 2.0, 0.0, std::f64::consts::TAU);
        set(cr, self.accent.lighten(0.05), 0.22 * a);
        cr.fill();
        cr.arc(l.play.cx(), l.play.cy(), l.play.w / 2.0 - 0.5, 0.0, std::f64::consts::TAU);
        set(cr, self.accent.lighten(0.35), 0.55 * a);
        cr.set_line_width(1.1);
        cr.stroke();
        icon(
            cr,
            if playing { Icon::Pause } else { Icon::Play },
            l.play.cx(),
            l.play.cy(),
            17.0,
            theme::TEXT,
            0.95 * a,
        );

        circle_button(cr, l.chevron, self.hover == Some(Hit::Chevron), a);
        icon(cr, Icon::ChevronUp, l.chevron.cx(), l.chevron.cy(), 15.0, theme::MUTED, 0.75 * a);
    }

    fn draw_expanded(&self, cr: &Context, _w: f64, h: f64, a: f64) {
        let l = self.expanded_layout();

        // Divider.
        let dg = cairo::LinearGradient::new(0.0, 40.0, 0.0, h - 40.0);
        dg.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.0);
        dg.add_color_stop_rgba(0.15, 1.0, 1.0, 1.0, 0.10);
        dg.add_color_stop_rgba(0.85, 1.0, 1.0, 1.0, 0.10);
        dg.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
        cr.set_source(&dg).ok();
        cr.rectangle(406.0, 30.0, 1.0, h - 60.0);
        cr.fill();

        // Cover.
        if let Some(surface) = &self.cover {
            rounded_pic(cr, l.cover, 20.0, surface);
            let _ = cr.save();
            rr(cr, l.cover.x, l.cover.y, l.cover.w, l.cover.h, 20.0);
            cr.clip();
            let gloss = cairo::LinearGradient::new(0.0, l.cover.y, 0.0, l.cover.y + l.cover.h);
            gloss.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.10);
            gloss.add_color_stop_rgba(0.35, 1.0, 1.0, 1.0, 0.0);
            gloss.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.16);
            cr.set_source(&gloss).ok();
            let _ = cr.paint();
            let _ = cr.restore();
        } else {
            placeholder_art(cr, l.cover, 20.0, self.accent, a);
        }
        rr(cr, l.cover.x + 0.5, l.cover.y + 0.5, l.cover.w - 1.0, l.cover.h - 1.0, 19.5);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.14 * a);
        cr.set_line_width(1.0);
        cr.stroke();

        let (title, artist, album) = match &self.track {
            Some(t) => (t.title.clone(), t.subtitle(), t.album.clone()),
            None => ("Nada sonando".into(), "Elige una canción".into(), String::new()),
        };
        label(cr, &title, l.title.x, l.title.y, l.title.w, 23.0, pango::Weight::Semibold, theme::TEXT, a);
        label(cr, &artist, l.artist.x, l.artist.y, l.artist.w, 13.5, pango::Weight::Normal, theme::MUTED, 0.8 * a);
        if !album.is_empty() {
            label(cr, &album, l.artist.x, l.artist.y + 24.0, l.artist.w, 10.5, pango::Weight::Normal, theme::DIM, 0.75 * a);
        }

        // Seek bar.
        let prog = if self.dur > 0.0 {
            (self.pos / self.dur).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bar_y = l.seek.y + 6.0;
        rr(cr, l.seek.x, bar_y, l.seek.w, 5.0, 2.5);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.13 * a);
        cr.fill();
        if prog > 0.0 {
            rr(cr, l.seek.x, bar_y, l.seek.w * prog, 5.0, 2.5);
            set(cr, self.accent.lighten(0.22), 0.95 * a);
            cr.fill();
        }
        let dot_x = l.seek.x + l.seek.w * prog;
        let glow = cairo::RadialGradient::new(dot_x, bar_y + 2.5, 0.0, dot_x, bar_y + 2.5, 13.0);
        glow.add_color_stop_rgba(0.0, self.accent.0, self.accent.1, self.accent.2, 0.55 * a);
        glow.add_color_stop_rgba(1.0, self.accent.0, self.accent.1, self.accent.2, 0.0);
        cr.set_source(&glow).ok();
        cr.arc(dot_x, bar_y + 2.5, 13.0, 0.0, std::f64::consts::TAU);
        cr.fill();
        cr.arc(dot_x, bar_y + 2.5, 4.6, 0.0, std::f64::consts::TAU);
        set(cr, Rgb::new(1.0, 1.0, 1.0), 0.96 * a);
        cr.fill();

        label(cr, &format_time(self.pos), l.seek.x, l.seek.y - 12.0, 80.0, 10.0, pango::Weight::Normal, theme::MUTED, 0.65 * a);
        let total = format_time(self.dur);
        let layout_w = 80.0;
        label(cr, &total, l.seek.x + l.seek.w - layout_w, l.seek.y - 12.0, layout_w, 10.0, pango::Weight::Normal, theme::MUTED, 0.65 * a);

        // Transport.
        circle_button(cr, l.shuffle, self.hover == Some(Hit::Shuffle), a);
        icon(
            cr,
            Icon::Shuffle,
            l.shuffle.cx(),
            l.shuffle.cy(),
            17.0,
            if self.shuffle { self.accent.lighten(0.35) } else { theme::MUTED },
            if self.shuffle { 0.95 * a } else { 0.7 * a },
        );
        circle_button(cr, l.repeat, self.hover == Some(Hit::Repeat), a);
        icon(
            cr,
            if self.repeat == crate::ui::Repeat::One { Icon::RepeatOne } else { Icon::Repeat },
            l.repeat.cx(),
            l.repeat.cy(),
            18.0,
            if self.repeat != crate::ui::Repeat::Off { self.accent.lighten(0.35) } else { theme::MUTED },
            if self.repeat != crate::ui::Repeat::Off { 0.95 * a } else { 0.7 * a },
        );

        circle_button(cr, l.prev, self.hover == Some(Hit::Prev), a);
        icon(cr, Icon::Prev, l.prev.cx(), l.prev.cy(), 21.0, theme::TEXT, 0.88 * a);
        circle_button(cr, l.next, self.hover == Some(Hit::Next), a);
        icon(cr, Icon::Next, l.next.cx(), l.next.cy(), 21.0, theme::TEXT, 0.88 * a);

        // Play button.
        let pr = l.play.w / 2.0;
        let pulse = 0.5 + 0.5 * ((std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) * 1.4).sin());
        let halo = cairo::RadialGradient::new(l.play.cx(), l.play.cy(), pr * 0.7, l.play.cx(), l.play.cy(), pr * 1.6);
        halo.add_color_stop_rgba(0.0, self.accent.0, self.accent.1, self.accent.2, (0.16 + 0.05 * pulse) * a);
        halo.add_color_stop_rgba(1.0, self.accent.0, self.accent.1, self.accent.2, 0.0);
        cr.set_source(&halo).ok();
        cr.arc(l.play.cx(), l.play.cy(), pr * 1.6, 0.0, std::f64::consts::TAU);
        cr.fill();

        let pg = cairo::LinearGradient::new(l.play.x, l.play.y, l.play.x + l.play.w, l.play.y + l.play.h);
        pg.add_color_stop_rgba(0.0, self.accent.lighten(0.30).0, self.accent.lighten(0.30).1, self.accent.lighten(0.30).2, 0.95 * a);
        pg.add_color_stop_rgba(1.0, self.accent.darken(0.12).0, self.accent.darken(0.12).1, self.accent.darken(0.12).2, 0.92 * a);
        cr.set_source(&pg).ok();
        cr.arc(l.play.cx(), l.play.cy(), pr, 0.0, std::f64::consts::TAU);
        cr.fill();
        cr.arc(l.play.cx(), l.play.cy(), pr - 0.75, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.30 * a);
        cr.set_line_width(1.0);
        cr.stroke();
        icon(
            cr,
            if self.playing { Icon::Pause } else { Icon::Play },
            l.play.cx(),
            l.play.cy(),
            25.0,
            Rgb::new(0.06, 0.05, 0.06),
            0.92 * a,
        );

        // Volume.
        let vy = l.volume.y + 11.0;
        icon(cr, Icon::Volume, l.volume.x + 11.0, l.volume.cy(), 16.0, theme::MUTED, 0.75 * a);
        let vx = l.volume.x + 30.0;
        let vw = l.volume.w - 76.0;
        rr(cr, vx, vy - 1.5, vw, 4.0, 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12 * a);
        cr.fill();
        let vf = self.volume / 100.0;
        rr(cr, vx, vy - 1.5, vw * vf, 4.0, 2.0);
        set(cr, theme::MUTED.lighten(0.25), 0.75 * a);
        cr.fill();
        cr.arc(vx + vw * vf, vy + 0.5, 4.0, 0.0, std::f64::consts::TAU);
        set(cr, Rgb::new(1.0, 1.0, 1.0), 0.9 * a);
        cr.fill();
        label(
            cr,
            &format!("{}%", self.volume.round() as i32),
            l.volume.x + l.volume.w - 42.0,
            l.volume.y + 3.0,
            42.0,
            10.0,
            pango::Weight::Normal,
            theme::DIM,
            0.8 * a,
        );

        // Save button.
        rr(cr, l.save.x, l.save.y, l.save.w, l.save.h, l.save.h / 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.055 * a);
        cr.fill();
        rr(cr, l.save.x + 0.5, l.save.y + 0.5, l.save.w - 1.0, l.save.h - 1.0, l.save.h / 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12 * a);
        cr.set_line_width(1.0);
        cr.stroke();
        if self.current_saved {
            icon(cr, Icon::Check, l.save.x + 20.0, l.save.cy(), 14.0, self.accent.lighten(0.4), 0.95 * a);
            label(cr, "Guardada", l.save.x + 34.0, l.save.y + 9.0, l.save.w - 40.0, 11.5, pango::Weight::Medium, self.accent.lighten(0.4), 0.95 * a);
        } else {
            icon(cr, Icon::Plus, l.save.x + 20.0, l.save.cy(), 14.0, theme::TEXT, 0.85 * a);
            label(cr, "Guardar", l.save.x + 34.0, l.save.y + 9.0, l.save.w - 40.0, 11.5, pango::Weight::Medium, theme::TEXT, 0.85 * a);
        }
        if self.hover == Some(Hit::SaveCurrent) {
            rr(cr, l.save.x, l.save.y, l.save.w, l.save.h, l.save.h / 2.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.05 * a);
            cr.fill();
        }

        // Video toggle.
        let vhover = self.hover == Some(Hit::Video);
        let vactive = self.video;
        rr(cr, l.video.x, l.video.y, l.video.w, l.video.h, l.video.h / 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, (if vhover { 0.10 } else { 0.055 }) * a);
        cr.fill();
        rr(cr, l.video.x + 0.5, l.video.y + 0.5, l.video.w - 1.0, l.video.h - 1.0, l.video.h / 2.0);
        if vactive {
            set(cr, self.accent.lighten(0.3), 0.55 * a);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.12 * a);
        }
        cr.set_line_width(1.0);
        cr.stroke();
        icon(
            cr,
            Icon::Video,
            l.video.cx(),
            l.video.cy(),
            15.0,
            if vactive { self.accent.lighten(0.4) } else { theme::TEXT },
            if vactive { 0.95 } else { 0.8 } * a,
        );

        // Spotify source toggle.
        let shover = self.hover == Some(Hit::Spotify);
        let sactive = self.source == Source::Spotify;
        let spotify_on = crate::theme::Rgb::new(0.11, 0.84, 0.38);
        rr(cr, l.spotify.x, l.spotify.y, l.spotify.w, l.spotify.h, l.spotify.h / 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, (if shover { 0.10 } else { 0.055 }) * a);
        cr.fill();
        rr(cr, l.spotify.x + 0.5, l.spotify.y + 0.5, l.spotify.w - 1.0, l.spotify.h - 1.0, l.spotify.h / 2.0);
        if sactive {
            set(cr, spotify_on, 0.60 * a);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.12 * a);
        }
        cr.set_line_width(1.0);
        cr.stroke();
        icon(
            cr,
            Icon::Spotify,
            l.spotify.cx(),
            l.spotify.cy() - 1.0,
            18.0,
            if sactive { spotify_on.lighten(0.25) } else { theme::TEXT },
            if sactive { 0.95 } else { 0.75 } * a,
        );

        // Source + status.
        let source = match self.source {
            Source::YouTube => "YouTube Music",
            Source::Spotify => "Spotify",
        };
        label(cr, source, l.spotify.x + l.spotify.w + 12.0, l.save.y + 10.0, 110.0, 10.0, pango::Weight::Normal, theme::DIM, 0.7 * a);
        if !self.status.is_empty() && self.status_at.elapsed().as_secs() < 4 {
            label(cr, &self.status, 30.0, h - 22.0, 350.0, 10.0, pango::Weight::Normal, self.accent.lighten(0.3), 0.85 * a);
        }

        // Tabs.
        let tabs = ["Letra", "Biblioteca", "Buscar", "Cola"];
        for (i, r) in l.tabs.iter().enumerate() {
            let active = match i {
                0 => self.tab == Tab::Lyrics,
                1 => self.tab == Tab::Library,
                2 => self.tab == Tab::Search,
                _ => self.tab == Tab::Queue,
            };
            if active {
                rr(cr, r.x, r.y, r.w, r.h, r.h / 2.0);
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.075 * a);
                cr.fill();
                rr(cr, r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, r.h / 2.0);
                set(cr, self.accent.lighten(0.25), 0.28 * a);
                cr.set_line_width(1.0);
                cr.stroke();
                cr.arc(r.x + 14.0, r.cy(), 2.2, 0.0, std::f64::consts::TAU);
                set(cr, self.accent.lighten(0.35), 0.95 * a);
                cr.fill();
            }
            let alpha = if active { 0.95 } else { 0.55 };
            let shown = if i == 0 && self.showing_video() { "Vídeo" } else { tabs[i] };
            label(
                cr,
                shown,
                r.x + if active { 24.0 } else { 16.0 },
                r.cy() - 8.0,
                r.w - 30.0,
                12.0,
                if active { pango::Weight::Semibold } else { pango::Weight::Normal },
                theme::TEXT,
                alpha * a,
            );
        }

        circle_button(cr, l.chevron, self.hover == Some(Hit::Chevron), a);
        icon(cr, Icon::ChevronDown, l.chevron.cx(), l.chevron.cy(), 15.0, theme::MUTED, 0.75 * a);

        // Content pane: slightly darker frosted glass so text stays readable
        // over busy desktops (Hyprland offers no layer blur on this build).
        let px = l.content.x - 10.0;
        let py = l.content.y - 14.0;
        let pw = l.content.w + 20.0;
        let ph = l.content.h + 26.0;
        rr(cr, px, py, pw, ph, 24.0);
        let pane = cairo::LinearGradient::new(0.0, py, 0.0, py + ph);
        pane.add_color_stop_rgba(0.0, 0.018, 0.020, 0.028, 0.34 * a);
        pane.add_color_stop_rgba(0.55, 0.018, 0.020, 0.028, 0.26 * a);
        pane.add_color_stop_rgba(1.0, 0.012, 0.013, 0.018, 0.16 * a);
        cr.set_source(&pane).ok();
        cr.fill_preserve();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.055 * a);
        cr.set_line_width(1.0);
        cr.stroke();

        // Content.
        let _ = cr.save();
        cr.rectangle(l.content.x - 4.0, l.content.y - 4.0, l.content.w + 8.0, l.content.h + 8.0);
        cr.clip();
        if self.tab == Tab::Lyrics {
            self.draw_lyrics(cr, &l, a);
        } else {
            self.draw_rows(cr, l.content, a);
        }
        let _ = cr.restore();
        if self.tab != Tab::Lyrics {
            self.draw_scrollbar(cr, l.content, a);
        }
    }

    fn draw_scrollbar(&self, cr: &Context, content: Rect, a: f64) {
        let rows = self.rows(content);
        let bottom = rows.iter().map(|(r, _)| r.y + r.h).fold(content.y, f64::max) + self.list_scroll;
        let total = bottom - content.y + 12.0;
        if total <= content.h + 1.0 {
            return;
        }
        let h = (content.h * content.h / total).max(28.0);
        let y = content.y + (content.h - h) * (self.list_scroll / (total - content.h)).clamp(0.0, 1.0);
        rr(cr, content.x + content.w + 4.0, y, 3.0, h, 1.5);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.16 * a);
        cr.fill();
    }

    fn draw_lyrics(&self, cr: &Context, l: &crate::ui::Layout, a: f64) {
        let content = l.lyrics;
        if l.screen.w > 0.0 {
            if let Some(frame) = &self.video_surface {
                rr(cr, l.screen.x, l.screen.y, l.screen.w, l.screen.h, 16.0);
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.9 * a);
                cr.fill();
                rounded_pic(cr, l.screen, 16.0, frame);
                rr(cr, l.screen.x + 0.5, l.screen.y + 0.5, l.screen.w - 1.0, l.screen.h - 1.0, 15.5);
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.10 * a);
                cr.set_line_width(1.0);
                cr.stroke();
            }
        }
        if self.lyrics.is_empty() {
            if l.screen.w > 0.0 {
                return;
            }
            let msg = match self.lyrics_state {
                1 => "Buscando letra…",
                3 => "Sin letra para esta canción",
                _ => "Sin letra",
            };
            icon(cr, Icon::Music, content.cx(), content.y + 150.0, 42.0, theme::DIM, 0.35 * a);
            label_center(cr, msg, content.x, content.y + 190.0, content.w, 13.0, pango::Weight::Normal, theme::MUTED, 0.65 * a);
            if self.lyrics_state == 3 {
                label_center(cr, "No está en LRCLIB ni en NetEase", content.x, content.y + 214.0, content.w, 10.5, pango::Weight::Normal, theme::DIM, 0.55 * a);
            }
            return;
        }

        let _ = cr.save();
        cr.rectangle(content.x - 4.0, content.y, content.w + 8.0, content.h);
        cr.clip();
        for (r, item) in self.rows(content) {
            if let RowItem::Lyric(i) = item {
                let Some(line) = self.lyrics.get(i) else { continue };
                let dist = (i as i64 - self.lyric_idx as i64).abs();
                let current = i == self.lyric_idx && (self.lyrics_synced || self.dur > 0.0);
                let (size, weight, color, alpha) = if current {
                    (20.0, pango::Weight::Semibold, theme::TEXT.lighten(0.05), 0.96)
                } else if !self.lyrics_synced {
                    (15.0, pango::Weight::Normal, theme::MUTED, 0.62)
                } else if dist == 1 {
                    (15.0, pango::Weight::Normal, theme::MUTED, 0.55)
                } else if dist == 2 {
                    (14.0, pango::Weight::Normal, theme::MUTED, 0.32)
                } else {
                    (13.5, pango::Weight::Normal, theme::DIM, 0.20)
                };
                let hovered = self.lyrics_synced && self.hover == Some(Hit::Row(item));
                if hovered {
                    rr(cr, r.x - 6.0, r.y + 2.0, r.w + 12.0, r.h - 4.0, 10.0);
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.045 * a);
                    cr.fill();
                }
                para(cr, &line.text, r.x + 10.0, r.y + 2.0, r.w - 20.0, size, weight, color, alpha * a, true);
            }
        }
        let _ = cr.restore();

        // Fade the list into the glass at the top and bottom.
        let top = cairo::LinearGradient::new(0.0, content.y, 0.0, content.y + 26.0);
        top.add_color_stop_rgba(0.0, 0.02, 0.02, 0.03, 0.40 * a);
        top.add_color_stop_rgba(1.0, 0.03, 0.03, 0.04, 0.0);
        cr.set_source(&top).ok();
        cr.rectangle(content.x, content.y, content.w, 26.0);
        cr.fill();
        let bottom = cairo::LinearGradient::new(0.0, content.y + content.h - 34.0, 0.0, content.y + content.h);
        bottom.add_color_stop_rgba(0.0, 0.03, 0.03, 0.04, 0.0);
        bottom.add_color_stop_rgba(1.0, 0.02, 0.02, 0.03, 0.45 * a);
        cr.set_source(&bottom).ok();
        cr.rectangle(content.x, content.y + content.h - 34.0, content.w, 34.0);
        cr.fill();
        if !self.lyrics_synced && l.screen.w <= 0.0 {
            label(cr, "Letra sin sincronizar", content.x + content.w - 150.0, content.y + content.h - 16.0, 150.0, 9.5, pango::Weight::Normal, theme::DIM, 0.6 * a);
        }
    }

    fn row_background(&self, cr: &Context, r: Rect, hovered: bool, a: f64) {
        if hovered {
            rr(cr, r.x, r.y + 2.0, r.w, r.h - 4.0, 12.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.055 * a);
            cr.fill();
        }
    }

    fn pill(&self, cr: &Context, r: Rect, ic: Icon, text: &str, hovered: bool, accent: bool, a: f64) {
        rr(cr, r.x, r.y, r.w, r.h, r.h / 2.0);
        if accent {
            set(cr, self.accent.lighten(if hovered { 0.30 } else { 0.18 }), 0.9 * a);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, if hovered { 0.10 } else { 0.05 } * a);
        }
        cr.fill();
        if !accent {
            rr(cr, r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, r.h / 2.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.10 * a);
            cr.set_line_width(1.0);
            cr.stroke();
        }
        let fg = if accent { Rgb::new(0.06, 0.05, 0.06) } else { theme::TEXT };
        icon(cr, ic, r.x + 18.0, r.cy(), 13.0, fg, 0.9 * a);
        label(cr, text, r.x + 32.0, r.cy() - 8.0, r.w - 38.0, 11.5, pango::Weight::Medium, fg, 0.92 * a);
    }

    fn thumb_or_placeholder(&self, cr: &Context, r: Rect, radius: f64, art: &str, fallback: Icon, a: f64) {
        match self.thumb(art) {
            Some(s) => rounded_pic(cr, r, radius, &s),
            None => {
                let _ = cr.save();
                rr(cr, r.x, r.y, r.w, r.h, radius);
                cr.clip();
                let g = cairo::LinearGradient::new(r.x, r.y, r.x + r.w, r.y + r.h);
                g.add_color_stop_rgba(0.0, self.accent.0 * 0.5, self.accent.1 * 0.5, self.accent.2 * 0.5, a);
                g.add_color_stop_rgba(1.0, 0.05, 0.05, 0.07, a);
                cr.set_source(&g).ok();
                let _ = cr.paint();
                let _ = cr.restore();
                icon(cr, fallback, r.cx(), r.cy(), r.w * 0.42, Rgb::new(1.0, 1.0, 1.0), 0.6 * a);
            }
        }
        rr(cr, r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, radius - 0.5);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10 * a);
        cr.set_line_width(1.0);
        cr.stroke();
    }

    fn coll_row(&self, cr: &Context, r: Rect, title: &str, subtitle: &str, art: &str, kind: CollKind, hovered: bool, a: f64) {
        self.row_background(cr, r, hovered, a);
        let thumb = Rect::new(r.x + 8.0, r.y + (r.h - 42.0) / 2.0, 42.0, 42.0);
        let fallback = if kind == CollKind::Mix { Icon::Music } else { Icon::Queue };
        self.thumb_or_placeholder(cr, thumb, 10.0, art, fallback, a);
        if kind == CollKind::Mix {
            // A small radio badge so mixes read differently from playlists.
            cr.arc(thumb.x + thumb.w - 6.0, thumb.y + thumb.h - 6.0, 7.0, 0.0, std::f64::consts::TAU);
            set(cr, self.accent.lighten(0.25), 0.95 * a);
            cr.fill();
            icon(cr, Icon::Play, thumb.x + thumb.w - 5.5, thumb.y + thumb.h - 6.0, 7.0, Rgb::new(0.06, 0.05, 0.06), a);
        }
        let tx = r.x + 62.0;
        let tw = r.w - 62.0 - if hovered { 84.0 } else { 16.0 };
        label(cr, title, tx, r.y + 9.0, tw, 13.0, pango::Weight::Medium, theme::TEXT, 0.92 * a);
        let kind_label = match kind {
            CollKind::Album => "Álbum",
            CollKind::Playlist => "Lista",
            CollKind::Mix => "Mix",
        };
        let sub = if subtitle.is_empty() { kind_label.to_string() } else { format!("{kind_label} · {subtitle}") };
        label(cr, &sub, tx, r.y + 28.0, tw, 10.5, pango::Weight::Normal, theme::MUTED, 0.62 * a);
    }

    fn draw_rows(&self, cr: &Context, content: Rect, a: f64) {
        let rows = self.rows(content);
        let visible = |r: &Rect| r.y + r.h >= content.y - 4.0 && r.y <= content.y + content.h + 4.0;
        let (mx, my) = self.mouse;
        let mouse_in = content.contains(mx, my);
        let row_hovered = |r: &Rect| mouse_in && r.contains(mx, my);

        // Pass 1: rows.
        for (r, item) in &rows {
            if !visible(r) {
                continue;
            }
            let hovered = row_hovered(r);
            match *item {
                RowItem::Header(text) => {
                    label(cr, text, r.x + 4.0, r.y + 10.0, r.w, 9.5, pango::Weight::Bold, theme::DIM, 0.9 * a);
                }
                RowItem::Message(text) => {
                    label_center(cr, text, r.x, r.y + 13.0, r.w, 12.0, pango::Weight::Normal, theme::MUTED, 0.6 * a);
                }
                RowItem::Track(list, i) => {
                    if let Some(t) = self.list_track(list, i) {
                        let current = match list {
                            List::Queue => i == self.index,
                            _ => self.track.as_ref().is_some_and(|c| c.id == t.id),
                        };
                        let number = match list {
                            List::Detail | List::Saved => Some(i + 1),
                            _ => None,
                        };
                        self.track_row(cr, *r, t, hovered, current, number, a);
                    }
                }
                RowItem::SearchColl(i) => {
                    if let Some(c) = self.colls.get(i) {
                        self.coll_row(cr, *r, &c.title, &c.subtitle, &c.art, c.kind, hovered, a);
                    }
                }
                RowItem::HomeColl(i) => {
                    if let Some(c) = self.home_colls.get(i) {
                        self.coll_row(cr, *r, &c.title, &c.subtitle, &c.art, c.kind, hovered, a);
                    }
                }
                RowItem::Mix(i) => {
                    if let Some(c) = self.mixes.get(i) {
                        self.coll_row(cr, *r, &c.title, &c.subtitle, &c.art, c.kind, hovered, a);
                    }
                }
                RowItem::Playlist(i) => {
                    if let Some(p) = self.playlists.get(i) {
                        let sub = if p.remote.is_empty() {
                            format!("{} canciones", p.count)
                        } else if p.count == 0 {
                            "YouTube Music · ábrela para cargarla".to_string()
                        } else {
                            format!("{} canciones · YouTube Music", p.count)
                        };
                        let kind = if p.remote.starts_with("MPRE") { CollKind::Album } else { CollKind::Playlist };
                        self.coll_row(cr, *r, &p.name, &sub, &p.art, kind, hovered, a);
                    }
                }
                RowItem::Import => {
                    let text = if self.importing { "Importando…" } else { "Importar de YouTube Music" };
                    self.pill(cr, *r, Icon::Queue, text, hovered, false, a);
                }
                RowItem::NewPlaylist => {
                    self.pill(cr, *r, Icon::Plus, "Nueva lista", hovered, false, a);
                }
                RowItem::Back => {
                    rr(cr, r.x, r.y, r.w, r.h, r.h / 2.0);
                    cr.set_source_rgba(1.0, 1.0, 1.0, if hovered { 0.10 } else { 0.05 } * a);
                    cr.fill();
                    icon(cr, Icon::ChevronLeft, r.x + 16.0, r.cy(), 13.0, theme::TEXT, 0.85 * a);
                    label(cr, "Volver", r.x + 30.0, r.cy() - 8.0, r.w - 34.0, 11.5, pango::Weight::Medium, theme::TEXT, 0.85 * a);
                    if let Some(d) = &self.detail {
                        let art = Rect::new(r.x, r.y + 38.0, 92.0, 92.0 - 26.0);
                        let art = Rect::new(art.x, art.y, art.h, art.h);
                        let fallback = if d.kind == CollKind::Mix { Icon::Music } else { Icon::Queue };
                        self.thumb_or_placeholder(cr, art, 12.0, &d.art, fallback, a);
                        label(cr, &d.title, r.x + 108.0, r.y + 36.0, content.w - 116.0, 17.0, pango::Weight::Semibold, theme::TEXT, a);
                        let mut sub = d.subtitle.clone();
                        if !d.tracks.is_empty() && !sub.contains("canciones") {
                            sub = format!("{sub} · {} canciones", d.tracks.len());
                        }
                        label(cr, sub.trim_start_matches(" · "), r.x + 108.0, r.y + 58.0, content.w - 116.0, 10.5, pango::Weight::Normal, theme::MUTED, 0.7 * a);
                    }
                }
                RowItem::DetailPlay => self.pill(cr, *r, Icon::Play, "Reproducir", hovered, true, a),
                RowItem::DetailShuffle => self.pill(cr, *r, Icon::Shuffle, "Aleatorio", hovered, false, a),
                RowItem::DetailSave => self.pill(cr, *r, Icon::Plus, "Guardar", hovered, false, a),
                _ => {}
            }
        }

        // Pass 2: small action buttons, shown while their row is hovered.
        for (r, item) in &rows {
            if !visible(r) {
                continue;
            }
            let (ic, danger) = match *item {
                RowItem::Enqueue(..) => (Icon::PlayNext, false),
                RowItem::Unsave(_) | RowItem::Dequeue(_) | RowItem::RemoveFromPlaylist(_) => (Icon::Close, false),
                RowItem::AddTo(_) => (Icon::Plus, false),
                RowItem::DeletePlaylist(pid) => (
                    Icon::Close,
                    self.confirm_delete.is_some_and(|(id, at)| id == pid && at.elapsed().as_secs() < 4),
                ),
                _ => continue,
            };
            let on_row = mouse_in && my >= r.y - 10.0 && my <= r.y + r.h + 10.0 && mx >= content.x;
            if !on_row && !danger {
                continue;
            }
            let hovered = row_hovered(r);
            cr.arc(r.cx(), r.cy(), 15.0, 0.0, std::f64::consts::TAU);
            if danger {
                cr.set_source_rgba(0.85, 0.25, 0.22, 0.55 * a);
            } else {
                cr.set_source_rgba(1.0, 1.0, 1.0, if hovered { 0.16 } else { 0.07 } * a);
            }
            cr.fill();
            icon(cr, ic, r.cx(), r.cy(), 13.0, theme::TEXT, if hovered { 0.95 } else { 0.7 } * a);
        }
    }

    fn track_row(&self, cr: &Context, r: Rect, t: &Track, hovered: bool, current: bool, number: Option<usize>, a: f64) {
        self.row_background(cr, r, hovered, a);
        if current {
            rr(cr, r.x, r.y + 4.0, r.w, r.h - 8.0, 12.0);
            set(cr, self.accent, 0.10 * a);
            cr.fill();
            rr(cr, r.x, r.y + 8.0, 2.5, r.h - 16.0, 1.25);
            set(cr, self.accent.lighten(0.2), 0.9 * a);
            cr.fill();
        }
        let mut x = r.x + 8.0;
        if let Some(n) = number {
            label_center(cr, &format!("{n}"), x - 4.0, r.cy() - 7.0, 26.0, 9.5, pango::Weight::Normal, theme::DIM, 0.6 * a);
            x += 24.0;
        }
        let thumb = Rect::new(x, r.y + (r.h - 36.0) / 2.0, 36.0, 36.0);
        self.thumb_or_placeholder(cr, thumb, 8.0, &t.art, Icon::Music, a);
        if current {
            rr(cr, thumb.x, thumb.y, thumb.w, thumb.h, 8.0);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.45 * a);
            cr.fill();
            icon(cr, if self.playing { Icon::Pause } else { Icon::Play }, thumb.cx(), thumb.cy(), 12.0, Rgb::new(1.0, 1.0, 1.0), 0.95 * a);
        }

        let text_x = thumb.x + thumb.w + 12.0;
        let text_w = r.x + r.w - text_x - if hovered { 84.0 } else { 58.0 };
        let title_color = if current { self.accent.lighten(0.45) } else { theme::TEXT };
        label(cr, &t.title, text_x, r.y + 7.0, text_w, 12.5, pango::Weight::Medium, title_color, 0.92 * a);
        let sub = if t.album.is_empty() || t.album == t.title {
            t.subtitle()
        } else {
            format!("{} · {}", t.subtitle(), t.album)
        };
        label(cr, &sub, text_x, r.y + 25.0, text_w, 10.5, pango::Weight::Normal, theme::MUTED, 0.62 * a);
        if t.duration > 0.0 && !hovered {
            label(cr, &format_time(t.duration), r.x + r.w - 50.0, r.y + 15.0, 44.0, 10.0, pango::Weight::Normal, theme::DIM, 0.7 * a);
        }
    }
}
