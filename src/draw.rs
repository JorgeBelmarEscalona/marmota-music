#![allow(unused_must_use)] // cairo returns Result for every path call
use gtk4 as gtk;
use cairo::{Context, Filter, ImageSurface, LineCap, LineJoin, SurfacePattern};
use gtk::pango;

use crate::model::{CollKind, Rect, Source, Track};
use crate::theme::{self, Rgb};
use crate::ui::{Account, App, Hit, List, RowItem, Tab};

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
    fd.set_family("Noto Sans, Segoe UI");
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
    fd.set_family("Noto Sans, Segoe UI");
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

/// Clock text: digits of equal width, so it doesn't wobble as it counts.
fn time_label(cr: &Context, text: &str, x: f64, y: f64, w: f64, right: bool, color: Rgb, alpha: f64) {
    let layout = pangocairo::functions::create_layout(cr);
    let mut fd = pango::FontDescription::new();
    fd.set_family("Noto Sans, Segoe UI");
    fd.set_absolute_size(10.5 * pango::SCALE as f64);
    layout.set_font_description(Some(&fd));
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFontFeatures::new("tnum"));
    layout.set_attributes(Some(&attrs));
    layout.set_text(text);
    layout.set_width((w * pango::SCALE as f64) as i32);
    if right {
        layout.set_alignment(pango::Alignment::Right);
    }
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
    fd.set_family("Noto Sans, Segoe UI");
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

fn rounded_pic(cr: &Context, r: Rect, radius: f64, surf: &ImageSurface, alpha: f64) {
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
    let _ = cr.paint_with_alpha(alpha.clamp(0.0, 1.0));
    let _ = cr.restore();
}

fn blurred_pic(cr: &Context, r: Rect, radius: f64, surf: &ImageSurface, alpha: f64) {
    let sw = surf.width() as f64;
    let sh = surf.height() as f64;
    if sw <= 0.0 || sh <= 0.0 {
        return;
    }
    let small = 12;
    // Already tiny (the cached backdrop): use it as is.
    let tmp = if sw <= 16.0 && sh <= 16.0 {
        surf.clone()
    } else {
        let Ok(tmp) = ImageSurface::create(cairo::Format::ARgb32, small, small) else {
            return;
        };
        {
            let Ok(tcr) = Context::new(&tmp) else { return };
            tcr.scale(small as f64 / sw, small as f64 / sh);
            let _ = tcr.set_source_surface(surf, 0.0, 0.0);
            let _ = tcr.paint();
        }
        tmp
    };
    let small = tmp.width();

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

/// Tiny platform mark: YouTube Music (red disc, ring and play) or Spotify
/// (green disc with three waves).
fn platform_badge(cr: &Context, cx: f64, cy: f64, spotify: bool, video: bool, a: f64) {
    let r = 8.0;
    let _ = cr.save();
    cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    if spotify {
        cr.set_source_rgba(0.12, 0.84, 0.38, 0.92 * a);
        cr.fill();
        cr.set_source_rgba(0.04, 0.04, 0.04, 0.95 * a);
        cr.set_line_cap(LineCap::Round);
        for (i, w) in [(0, 1.6), (1, 1.35), (2, 1.1)] {
            let half = r * (0.58 - 0.09 * i as f64);
            let y = cy - r * 0.30 + i as f64 * r * 0.32;
            cr.set_line_width(w);
            cr.move_to(cx - half, y);
            cr.curve_to(cx - half * 0.3, y - r * 0.22, cx + half * 0.3, y - r * 0.22, cx + half, y + r * 0.06);
            cr.stroke();
        }
    } else {
        cr.set_source_rgba(1.0, 0.0, 0.2, 0.88 * a);
        cr.fill();
        cr.arc(cx, cy, r * 0.56, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.95 * a);
        cr.set_line_width(1.0);
        cr.stroke();
        cr.move_to(cx - r * 0.17, cy - r * 0.27);
        cr.line_to(cx + r * 0.29, cy);
        cr.line_to(cx - r * 0.17, cy + r * 0.27);
        cr.close_path();
        cr.fill();
    }
    let _ = video;
    let _ = cr.restore();
}

fn circle_button(cr: &Context, r: Rect, hover: f64, alpha: f64) {
    if hover > 0.0 {
        cr.arc(r.cx(), r.cy(), r.w / 2.0, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10 * hover * alpha);
        cr.fill();
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// 0 below `from`, 1 above `to`, smooth in between.
fn smooth(from: f64, to: f64, x: f64) -> f64 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Play triangle (m = 0) morphing into the pause bars (m = 1).
fn play_pause(cr: &Context, cx: f64, cy: f64, s: f64, m: f64, color: Rgb, alpha: f64) {
    const PLAY: [[(f64, f64); 4]; 2] = [
        [(-0.26, -0.36), (0.05, -0.18), (0.05, 0.18), (-0.26, 0.36)],
        [(0.05, -0.18), (0.36, 0.0), (0.36, 0.0), (0.05, 0.18)],
    ];
    const PAUSE: [[(f64, f64); 4]; 2] = [
        [(-0.28, -0.31), (-0.10, -0.31), (-0.10, 0.31), (-0.28, 0.31)],
        [(0.10, -0.31), (0.28, -0.31), (0.28, 0.31), (0.10, 0.31)],
    ];
    for (a, b) in PLAY.iter().zip(PAUSE.iter()) {
        for i in 0..4 {
            let x = cx + lerp(a[i].0, b[i].0, m) * s;
            let y = cy + lerp(a[i].1, b[i].1, m) * s;
            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        cr.close_path();
    }
    set(cr, color, alpha);
    cr.fill();
}

/// Indeterminate ring: an arc that turns and breathes.
fn spinner(cr: &Context, cx: f64, cy: f64, r: f64, width: f64, color: Rgb, alpha: f64, t: f64) {
    let _ = cr.save();
    cr.set_line_width(width);
    cr.set_line_cap(LineCap::Round);
    cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.10 * alpha);
    cr.stroke();
    let start = t * std::f64::consts::TAU * 0.95;
    let len = std::f64::consts::PI * (0.75 + 0.45 * (t * 2.4).sin());
    cr.arc(cx, cy, r, start, start + len);
    set(cr, color, alpha);
    cr.stroke();
    let _ = cr.restore();
}

/// Where a segment that keeps sliding across a bar is right now (0..1 span).
fn sweep(t: f64, period: f64, len: f64) -> (f64, f64) {
    let p = (t / period).fract();
    let p = p * p * (3.0 - 2.0 * p);
    let x = p * (1.0 + len) - len;
    (x.max(0.0), (x + len).min(1.0))
}

/// The upper half of the pill's outline, inset by `inset`: from its left
/// tip over the top to its right tip. `f0..f1` picks a stretch of it (0..1).
fn top_edge(cr: &Context, w: f64, r: f64, inset: f64, f0: f64, f1: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let ri = (r - inset).max(1.0);
    let arc = ri * FRAC_PI_2;
    let straight = (w - 2.0 * r).max(0.0);
    let total = 2.0 * arc + straight;
    let (d0, d1) = (f0.clamp(0.0, 1.0) * total, f1.clamp(0.0, 1.0) * total);
    cr.new_path();
    if d0 < arc {
        cr.arc(r, r, ri, PI + d0 / ri, PI + d1.min(arc) / ri);
    }
    if d1 > arc && d0 < arc + straight {
        let x = r + (d0 - arc).max(0.0);
        if d0 >= arc {
            cr.move_to(x, inset);
        }
        cr.line_to(r + (d1 - arc).min(straight), inset);
    }
    if d1 > arc + straight {
        let from = (d0 - arc - straight).max(0.0);
        cr.arc(w - r, r, ri, 1.5 * PI + from / ri, 1.5 * PI + (d1 - arc - straight) / ri);
    }
}

fn busy_track(cr: &Context, x: f64, y: f64, w: f64, h: f64, color: Rgb, alpha: f64, t: f64) {
    let (a, b) = sweep(t, 1.25, 0.35);
    if b > a {
        rr(cr, x + w * a, y, w * (b - a), h, h / 2.0);
        set(cr, color, alpha);
        cr.fill();
    }
}

#[allow(clippy::too_many_arguments)]
fn glass(
    cr: &Context,
    w: f64,
    h: f64,
    radius: f64,
    accent: Rgb,
    cover: Option<&ImageSurface>,
    prev: Option<&ImageSurface>,
    mix: f64,
) {
    let base = Rgb::new(0.052, 0.055, 0.068).mix(accent.darken(0.52), 0.20);

    let _ = cr.save();
    rr(cr, 0.0, 0.0, w, h, radius);
    cr.clip();

    if let Some(surface) = prev {
        blurred_pic(cr, Rect::new(0.0, 0.0, w, h), 0.0, surface, 0.44 * (1.0 - mix));
    }
    if let Some(surface) = cover {
        blurred_pic(cr, Rect::new(0.0, 0.0, w, h), 0.0, surface, 0.44 * mix);
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

    // A soft diagonal sheen.
    let x = w * 0.18;
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
        let (ox, oy) = self.origin();
        cr.translate(ox, oy);
        let fading = self.presence < 0.999;
        if fading {
            cr.translate(0.0, (1.0 - self.presence) * 16.0);
            let _ = cr.push_group();
        }
        let radius = 37.0 + (30.0 - 37.0) * self.anim;
        self.paint_glass(cr, w, h, radius);

        let _ = cr.save();
        rr(cr, 0.0, 0.0, w, h, radius);
        cr.clip();
        // The two faces overlap briefly so the pill never looks empty.
        let collapsed = 1.0 - smooth(0.0, 0.30, self.anim);
        let expanded = smooth(0.28, 0.85, self.anim);
        if expanded > 0.005 {
            let _ = cr.save();
            cr.translate(0.0, (1.0 - expanded) * 10.0);
            self.draw_expanded(cr, w, h, expanded);
            let _ = cr.restore();
        }
        self.draw_art(cr);
        if collapsed > 0.005 {
            self.draw_collapsed(cr, w, radius, collapsed);
        }
        let _ = cr.restore();

        if fading {
            let _ = cr.pop_group_to_source();
            let _ = cr.paint_with_alpha(self.presence.clamp(0.0, 1.0));
        }
    }

    /// The backdrop costs more than everything on top of it, so it is kept
    /// in a surface and repainted only when its size, colour or cover change.
    fn paint_glass(&self, cr: &Context, w: f64, h: f64, radius: f64) {
        let settled = !self.animating && self.cover_mix >= 1.0 && self.accent == self.accent_to;
        let cover = self.cover.as_ref().and(self.cover_blur.as_ref());
        let prev = self.prev_blur.as_ref();
        let mix = if cover.is_some() { self.cover_mix } else { 0.0 };
        if !settled {
            glass(cr, w, h, radius, self.accent, cover, prev, mix);
            return;
        }
        let scale = cr.matrix().xx();
        let key = (
            w.to_bits(),
            h.to_bits(),
            scale.to_bits(),
            self.cover_gen,
            self.accent.0.to_bits() ^ self.accent.1.to_bits().rotate_left(21) ^ self.accent.2.to_bits().rotate_left(42),
        );
        let mut cache = self.glass_cache.borrow_mut();
        if cache.as_ref().map(|(k, _)| *k) != Some(key) {
            *cache = None;
            let (pw, ph) = ((w * scale).ceil() as i32, (h * scale).ceil() as i32);
            if let Ok(surface) = ImageSurface::create(cairo::Format::ARgb32, pw, ph) {
                surface.set_device_scale(scale, scale);
                if let Ok(c) = Context::new(&surface) {
                    glass(&c, w, h, radius, self.accent, cover, prev, mix);
                    drop(c);
                    surface.flush();
                    *cache = Some((key, surface));
                }
            }
        }
        match cache.as_ref() {
            Some((_, surface)) => {
                let pat = SurfacePattern::create(surface);
                pat.set_filter(Filter::Nearest);
                let _ = cr.set_source(&pat);
                let _ = cr.paint();
            }
            None => glass(cr, w, h, radius, self.accent, cover, prev, mix),
        }
    }

    /// The artwork is one element in both faces: it grows from the small
    /// thumbnail into the big cover as the pill opens.
    fn draw_art(&self, cr: &Context) {
        let (small, big) = (self.collapsed_layout().art, self.expanded_layout().cover);
        let t = self.anim;
        let r = Rect::new(
            lerp(small.x, big.x, t),
            lerp(small.y, big.y, t),
            lerp(small.w, big.w, t),
            lerp(small.h, big.h, t),
        );
        let radius = lerp(13.0, 20.0, t);
        let cover = self.cover.as_ref();
        let mix = if cover.is_some() { self.cover_mix } else { 0.0 };
        match (self.prev_cover.as_ref(), cover) {
            (None, None) => placeholder_art(cr, r, radius, self.accent, 1.0),
            (prev, cur) => {
                if let Some(s) = prev {
                    rounded_pic(cr, r, radius, s, 1.0);
                }
                if let Some(s) = cur {
                    rounded_pic(cr, r, radius, s, if prev.is_some() { mix } else { mix.max(0.0) });
                }
            }
        }
        // The video itself, in miniature, while the pill is closed.
        if let Some(frame) = self.video_surface.as_ref().filter(|_| self.showing_video()) {
            let v = 1.0 - smooth(0.0, 0.5, t);
            if v > 0.0 {
                rounded_pic(cr, r, radius, frame, v);
            }
        }
        let _ = cr.save();
        rr(cr, r.x, r.y, r.w, r.h, radius);
        cr.clip();
        if t > 0.01 && self.cover.is_some() {
            let gloss = cairo::LinearGradient::new(0.0, r.y, 0.0, r.y + r.h);
            gloss.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.10 * t);
            gloss.add_color_stop_rgba(0.35, 1.0, 1.0, 1.0, 0.0);
            gloss.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.16 * t);
            let _ = cr.set_source(&gloss);
            let _ = cr.paint();
        }
        // Dimmed while the song it belongs to is on its way.
        if self.loading_mix > 0.0 {
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.30 * self.loading_mix);
            let _ = cr.paint();
        }
        let _ = cr.restore();
        rr(cr, r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, radius - 0.5);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.13);
        cr.set_line_width(1.0);
        cr.stroke();
    }

    /// Title and subtitle block, crossfading to the next song. The subtitle
    /// gives way to what the player is doing whenever there is something to say.
    #[allow(clippy::too_many_arguments)]
    fn now_playing_text(
        &self,
        cr: &Context,
        title: Rect,
        artist: Rect,
        title_size: f64,
        artist_size: f64,
        shift: f64,
        a: f64,
    ) {
        let m = self.track_mix;
        if m < 1.0 {
            let y = -shift * m;
            label(cr, &self.prev_title, title.x, title.y + y, title.w, title_size, pango::Weight::Semibold, theme::TEXT, a * (1.0 - m));
            label(cr, &self.prev_artist, artist.x, artist.y + y, artist.w, artist_size, pango::Weight::Normal, theme::MUTED, 0.75 * a * (1.0 - m));
        }
        let y = shift * (1.0 - m);
        label(cr, &self.shown_title, title.x, title.y + y, title.w, title_size, pango::Weight::Semibold, theme::TEXT, a * m);
        let on = self.activity_on;
        if on < 1.0 {
            label(cr, &self.shown_artist, artist.x, artist.y + y, artist.w, artist_size, pango::Weight::Normal, theme::MUTED, 0.75 * a * m * (1.0 - on));
        }
        if on > 0.0 {
            const ERR: Rgb = Rgb::new(1.0, 0.52, 0.46);
            let color = if self.activity_err { ERR } else { self.accent.lighten(0.45) };
            let s = self.activity_swap;
            if s < 1.0 && !self.activity_prev.is_empty() {
                label(cr, &self.activity_prev, artist.x, artist.y, artist.w, artist_size, pango::Weight::Medium, color, 0.95 * a * on * (1.0 - s));
            }
            label(cr, &self.activity, artist.x, artist.y, artist.w, artist_size, pango::Weight::Medium, color, 0.95 * a * on * s);
        }
    }

    /// Hairline that follows the pill's own top edge: the song's progress,
    /// or a segment travelling along it while the song loads.
    fn draw_edge_progress(&self, cr: &Context, w: f64, radius: f64, a: f64) {
        const INSET: f64 = 3.0;
        let _ = cr.save();
        cr.set_line_cap(LineCap::Round);
        cr.set_line_width(2.0);
        top_edge(cr, w, radius, INSET, 0.0, 1.0);
        if self.failed {
            cr.set_source_rgba(1.0, 0.52, 0.46, 0.45 * a);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.09 * a);
        }
        cr.stroke();
        let load = self.loading_mix;
        if self.dur > 0.0 && load < 1.0 {
            let prog = (self.shown_pos / self.dur).clamp(0.0, 1.0);
            if prog > 0.001 {
                top_edge(cr, w, radius, INSET, 0.0, prog);
                set(cr, self.accent.lighten(0.25), 0.95 * a * (1.0 - load));
                cr.stroke();
            }
        }
        if load > 0.0 {
            let (f0, f1) = sweep(self.clock(), 1.5, 0.28);
            if f1 > f0 {
                top_edge(cr, w, radius, INSET, f0, f1);
                set(cr, self.accent.lighten(0.45), 0.95 * a * load);
                cr.stroke();
            }
        }
        let _ = cr.restore();
    }

    /// Ring around the play button while waiting for sound.
    fn play_ring(&self, cr: &Context, r: Rect, gap: f64, width: f64, a: f64) {
        let wait = if self.buffer_t0.is_some_and(|t| t.elapsed().as_millis() > 350) {
            1.0
        } else {
            self.loading_mix
        };
        if wait > 0.0 {
            spinner(cr, r.cx(), r.cy(), r.w / 2.0 + gap, width, self.accent.lighten(0.45), wait * a, self.clock());
        }
    }

    fn draw_collapsed(&self, cr: &Context, w: f64, radius: f64, a: f64) {
        let l = self.collapsed_layout();
        self.draw_edge_progress(cr, w, radius, a);
        self.now_playing_text(cr, l.title, l.artist, 14.0, 11.0, 7.0, a);

        // Mini lyrics: the lines slide up as the song moves on.
        if !self.lyrics.is_empty() {
            let _ = cr.save();
            cr.rectangle(l.lyric.x, l.lyric.y + 2.0, l.lyric.w, l.lyric.h - 4.0);
            cr.clip();
            const STEP: f64 = 23.0;
            let idx = self.lyric_idx.min(self.lyrics.len() - 1) as i64;
            for i in (idx - 1).max(0)..=(idx + 2).min(self.lyrics.len() as i64 - 1) {
                let d = (i - idx) as f64 + self.lyric_shift;
                let alpha = if d < 0.0 {
                    0.88 * (1.0 + d).max(0.0)
                } else if d <= 1.0 {
                    lerp(0.88, 0.40, d)
                } else {
                    lerp(0.40, 0.0, (d - 1.0).min(1.0))
                };
                if alpha > 0.01 {
                    let line = &self.lyrics[i as usize].text;
                    label(cr, line, l.lyric.x, l.lyric.y + 7.0 + d * STEP, l.lyric.w, 11.5, pango::Weight::Medium, if d < 0.5 { theme::TEXT } else { theme::MUTED }, alpha * a);
                }
            }
            let _ = cr.restore();
        } else if self.lyrics_state == 1 {
            label(cr, "Buscando letra…", l.lyric.x, l.lyric.y + 20.0, l.lyric.w, 10.5, pango::Weight::Normal, theme::DIM, 0.7 * a);
        }

        // Controls.
        circle_button(cr, l.prev, self.hv(Hit::Prev), a);
        icon(cr, Icon::Prev, l.prev.cx(), l.prev.cy(), 17.0 * (1.0 - 0.14 * self.pressed(Hit::Prev)), theme::TEXT, lerp(0.82, 1.0, self.hv(Hit::Prev)) * a);
        circle_button(cr, l.next, self.hv(Hit::Next), a);
        icon(cr, Icon::Next, l.next.cx(), l.next.cy(), 17.0 * (1.0 - 0.14 * self.pressed(Hit::Next)), theme::TEXT, lerp(0.82, 1.0, self.hv(Hit::Next)) * a);

        let hover = self.hv(Hit::Play);
        let grow = 1.0 + 0.05 * hover - 0.09 * self.pressed(Hit::Play);
        let _ = cr.save();
        cr.translate(l.play.cx(), l.play.cy());
        cr.scale(grow, grow);
        cr.translate(-l.play.cx(), -l.play.cy());
        cr.arc(l.play.cx(), l.play.cy(), l.play.w / 2.0, 0.0, std::f64::consts::TAU);
        set(cr, self.accent.lighten(0.05), lerp(0.22, 0.36, hover) * a);
        cr.fill();
        if self.loading_mix < 1.0 {
            cr.arc(l.play.cx(), l.play.cy(), l.play.w / 2.0 - 0.5, 0.0, std::f64::consts::TAU);
            set(cr, self.accent.lighten(0.35), 0.55 * a * (1.0 - self.loading_mix));
            cr.set_line_width(1.1);
            cr.stroke();
        }
        play_pause(cr, l.play.cx(), l.play.cy(), 17.0, self.play_mix, theme::TEXT, 0.95 * a);
        let _ = cr.restore();
        self.play_ring(cr, l.play, -1.0, 2.0, a);

        circle_button(cr, l.chevron, self.hv(Hit::Chevron), a);
        icon(cr, Icon::ChevronUp, l.chevron.cx(), l.chevron.cy() - 1.5 * self.hv(Hit::Chevron), 15.0, theme::MUTED, lerp(0.75, 1.0, self.hv(Hit::Chevron)) * a);
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

        // Album sits under the artist unless a status line needs the room.
        let album = self.track.as_ref().map(|t| t.album.as_str()).unwrap_or("");
        let (title, artist) = (l.title, l.artist);
        self.now_playing_text(cr, title, artist, 23.0, 13.5, 9.0, a);
        if !album.is_empty() {
            label(cr, album, l.artist.x, l.artist.y + 24.0, l.artist.w, 10.5, pango::Weight::Normal, theme::DIM, 0.75 * a * self.track_mix);
        }

        // Seek bar.
        let seek_on = if self.dragging.is_some() && self.hover != Some(Hit::Volume) { 1.0 } else { self.hv(Hit::Seek) };
        let load = self.loading_mix;
        let prog = if self.dur > 0.0 {
            (self.shown_pos / self.dur).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bar_h = lerp(4.0, 6.0, seek_on);
        let bar_cy = l.seek.y + 8.5;
        let bar_y = bar_cy - bar_h / 2.0;
        rr(cr, l.seek.x, bar_y, l.seek.w, bar_h, bar_h / 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12 * a);
        cr.fill();
        // What is already downloaded ahead of the playhead.
        if self.dur > 0.0 && self.cache_ahead > 0.5 && load < 1.0 {
            let to = ((self.shown_pos + self.cache_ahead) / self.dur).clamp(0.0, 1.0);
            rr(cr, l.seek.x, bar_y, l.seek.w * to, bar_h, bar_h / 2.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.12 * a * (1.0 - load));
            cr.fill();
        }
        if prog > 0.0 {
            rr(cr, l.seek.x, bar_y, (l.seek.w * prog).max(bar_h), bar_h, bar_h / 2.0);
            set(cr, self.accent.lighten(0.22), 0.95 * a * (1.0 - load));
            cr.fill();
        }
        if load > 0.0 {
            let _ = cr.save();
            rr(cr, l.seek.x, bar_y, l.seek.w, bar_h, bar_h / 2.0);
            cr.clip();
            busy_track(cr, l.seek.x, bar_y, l.seek.w, bar_h, self.accent.lighten(0.40), a * load, self.clock());
            let _ = cr.restore();
        }
        let thumb = (1.0 - load) * a;
        if thumb > 0.01 {
            let dot_x = l.seek.x + l.seek.w * prog;
            let glow_r = lerp(10.0, 15.0, seek_on);
            let glow = cairo::RadialGradient::new(dot_x, bar_cy, 0.0, dot_x, bar_cy, glow_r);
            glow.add_color_stop_rgba(0.0, self.accent.0, self.accent.1, self.accent.2, 0.50 * thumb);
            glow.add_color_stop_rgba(1.0, self.accent.0, self.accent.1, self.accent.2, 0.0);
            cr.set_source(&glow).ok();
            cr.arc(dot_x, bar_cy, glow_r, 0.0, std::f64::consts::TAU);
            cr.fill();
            cr.arc(dot_x, bar_cy, lerp(4.2, 6.0, seek_on), 0.0, std::f64::consts::TAU);
            set(cr, Rgb::new(1.0, 1.0, 1.0), 0.97 * thumb);
            cr.fill();
        }

        time_label(cr, &format_time(self.pos), l.seek.x, l.seek.y - 13.0, 80.0, false, theme::MUTED, 0.72 * a);
        time_label(cr, &format_time(self.dur), l.seek.x + l.seek.w - 80.0, l.seek.y - 13.0, 80.0, true, theme::MUTED, 0.72 * a);

        // Time under the pointer.
        if seek_on > 0.01 && self.dur > 0.0 && load < 0.5 {
            let f = ((self.mouse.0 - l.seek.x) / l.seek.w).clamp(0.0, 1.0);
            let (tw, th) = (46.0, 20.0);
            let tx = (l.seek.x + l.seek.w * f - tw / 2.0).clamp(l.seek.x, l.seek.x + l.seek.w - tw);
            let ty = bar_y - th - 7.0 + (1.0 - seek_on) * 3.0;
            rr(cr, tx, ty, tw, th, th / 2.0);
            cr.set_source_rgba(0.02, 0.02, 0.03, 0.92 * seek_on * a);
            cr.fill();
            time_label(cr, &format_time(f * self.dur), tx, ty + 3.0, tw - 11.0, true, theme::TEXT, seek_on * a);
        }

        // Transport.
        circle_button(cr, l.shuffle, self.hv(Hit::Shuffle), a);
        icon(
            cr,
            Icon::Shuffle,
            l.shuffle.cx(),
            l.shuffle.cy(),
            17.0,
            if self.shuffle { self.accent.lighten(0.35) } else { theme::MUTED },
            if self.shuffle { 0.95 * a } else { 0.7 * a },
        );
        circle_button(cr, l.repeat, self.hv(Hit::Repeat), a);
        icon(
            cr,
            if self.repeat == crate::ui::Repeat::One { Icon::RepeatOne } else { Icon::Repeat },
            l.repeat.cx(),
            l.repeat.cy(),
            18.0,
            if self.repeat != crate::ui::Repeat::Off { self.accent.lighten(0.35) } else { theme::MUTED },
            if self.repeat != crate::ui::Repeat::Off { 0.95 * a } else { 0.7 * a },
        );

        circle_button(cr, l.prev, self.hv(Hit::Prev), a);
        icon(cr, Icon::Prev, l.prev.cx(), l.prev.cy(), 21.0 * (1.0 - 0.14 * self.pressed(Hit::Prev)), theme::TEXT, lerp(0.88, 1.0, self.hv(Hit::Prev)) * a);
        circle_button(cr, l.next, self.hv(Hit::Next), a);
        icon(cr, Icon::Next, l.next.cx(), l.next.cy(), 21.0 * (1.0 - 0.14 * self.pressed(Hit::Next)), theme::TEXT, lerp(0.88, 1.0, self.hv(Hit::Next)) * a);

        // Play button.
        let hover = self.hv(Hit::Play);
        let grow = 1.0 + 0.045 * hover - 0.08 * self.pressed(Hit::Play);
        let pr = l.play.w / 2.0;
        let _ = cr.save();
        cr.translate(l.play.cx(), l.play.cy());
        cr.scale(grow, grow);
        cr.translate(-l.play.cx(), -l.play.cy());
        let halo = cairo::RadialGradient::new(l.play.cx(), l.play.cy(), pr * 0.7, l.play.cx(), l.play.cy(), pr * 1.6);
        halo.add_color_stop_rgba(0.0, self.accent.0, self.accent.1, self.accent.2, lerp(0.18, 0.32, hover) * a);
        halo.add_color_stop_rgba(1.0, self.accent.0, self.accent.1, self.accent.2, 0.0);
        cr.set_source(&halo).ok();
        cr.arc(l.play.cx(), l.play.cy(), pr * 1.6, 0.0, std::f64::consts::TAU);
        cr.fill();

        let (c0, c1) = (self.accent.lighten(0.30), self.accent.darken(0.12));
        let pg = cairo::LinearGradient::new(l.play.x, l.play.y, l.play.x + l.play.w, l.play.y + l.play.h);
        pg.add_color_stop_rgba(0.0, c0.0, c0.1, c0.2, 0.95 * a);
        pg.add_color_stop_rgba(1.0, c1.0, c1.1, c1.2, 0.92 * a);
        cr.set_source(&pg).ok();
        cr.arc(l.play.cx(), l.play.cy(), pr, 0.0, std::f64::consts::TAU);
        cr.fill();
        cr.arc(l.play.cx(), l.play.cy(), pr - 0.75, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.30 * a);
        cr.set_line_width(1.0);
        cr.stroke();
        play_pause(cr, l.play.cx(), l.play.cy(), 25.0, self.play_mix, Rgb::new(0.06, 0.05, 0.06), 0.92 * a);
        let _ = cr.restore();
        self.play_ring(cr, l.play, 5.0, 2.4, a);

        // Volume.
        let vy = l.volume.y + 11.0;
        icon(cr, Icon::Volume, l.volume.x + 11.0, l.volume.cy(), 16.0, theme::MUTED, 0.75 * a);
        let vx = l.volume.x + 30.0;
        let vw = l.volume.w - 76.0;
        let von = self.hv(Hit::Volume);
        let vh = lerp(4.0, 5.5, von);
        let vcy = vy + 0.5;
        rr(cr, vx, vcy - vh / 2.0, vw, vh, vh / 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12 * a);
        cr.fill();
        let vf = self.volume / 100.0;
        rr(cr, vx, vcy - vh / 2.0, vw * vf, vh, vh / 2.0);
        set(cr, theme::MUTED.lighten(0.25).mix(self.accent.lighten(0.3), von), lerp(0.75, 0.95, von) * a);
        cr.fill();
        cr.arc(vx + vw * vf, vcy, lerp(4.0, 5.5, von), 0.0, std::f64::consts::TAU);
        set(cr, Rgb::new(1.0, 1.0, 1.0), 0.92 * a);
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
        let save_hover = self.hv(Hit::SaveCurrent);
        if save_hover > 0.0 {
            rr(cr, l.save.x, l.save.y, l.save.w, l.save.h, l.save.h / 2.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.06 * save_hover * a);
            cr.fill();
        }

        // Video toggle.
        let vhover = self.hv(Hit::Video);
        let vactive = self.video;
        rr(cr, l.video.x, l.video.y, l.video.w, l.video.h, l.video.h / 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, lerp(0.055, 0.12, vhover) * a);
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
        let shover = self.hv(Hit::Spotify);
        let sactive = self.source == Source::Spotify;
        let spotify_on = crate::theme::Rgb::new(0.11, 0.84, 0.38);
        rr(cr, l.spotify.x, l.spotify.y, l.spotify.w, l.spotify.h, l.spotify.h / 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, lerp(0.055, 0.12, shover) * a);
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

        // Tabs: one indicator that slides between them.
        let tabs = ["Letra", "Biblioteca", "Buscar", "Cola"];
        let order = [Tab::Lyrics, Tab::Library, Tab::Search, Tab::Queue];
        let (ty, th) = (l.tabs[0].y, l.tabs[0].h);
        if self.tab_w > 0.0 {
            rr(cr, self.tab_x, ty, self.tab_w, th, th / 2.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.075 * a);
            cr.fill();
            rr(cr, self.tab_x + 0.5, ty + 0.5, self.tab_w - 1.0, th - 1.0, th / 2.0);
            set(cr, self.accent.lighten(0.25), 0.28 * a);
            cr.set_line_width(1.0);
            cr.stroke();
            cr.arc(self.tab_x + 14.0, ty + th / 2.0, 2.2, 0.0, std::f64::consts::TAU);
            set(cr, self.accent.lighten(0.35), 0.95 * a);
            cr.fill();
        }
        for (i, r) in l.tabs.iter().enumerate() {
            let on = self.tab_mix[i];
            let hover = self.hv(Hit::Tab(order[i]));
            let alpha = lerp(lerp(0.55, 0.85, hover), 0.95, on);
            let shown = if i == 0 && self.showing_video() { "Vídeo" } else { tabs[i] };
            label(
                cr,
                shown,
                r.x + lerp(16.0, 24.0, on),
                r.cy() - 8.0,
                r.w - 30.0,
                12.0,
                if on > 0.5 { pango::Weight::Semibold } else { pango::Weight::Normal },
                theme::TEXT,
                alpha * a,
            );
        }

        circle_button(cr, l.chevron, self.hv(Hit::Chevron), a);
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
        let shown = a * self.content_mix;
        cr.translate(0.0, (1.0 - self.content_mix) * 8.0);
        if self.tab == Tab::Lyrics {
            self.draw_lyrics(cr, &l, shown);
        } else {
            self.draw_rows(cr, l.content, shown);
        }
        let _ = cr.restore();
        if self.tab != Tab::Lyrics {
            self.draw_scrollbar(cr, l.content, shown);
        }
        // Lyrics melt into the pane at its top and bottom edges.
        if self.tab == Tab::Lyrics && l.screen.w <= 0.0 {
            let _ = cr.save();
            rr(cr, px, py, pw, ph, 24.0);
            cr.clip();
            let top = cairo::LinearGradient::new(0.0, py, 0.0, py + 54.0);
            top.add_color_stop_rgba(0.0, 0.016, 0.018, 0.024, 0.55 * a);
            top.add_color_stop_rgba(1.0, 0.016, 0.018, 0.024, 0.0);
            cr.set_source(&top).ok();
            cr.rectangle(px, py, pw, 54.0);
            cr.fill();
            let bottom = cairo::LinearGradient::new(0.0, py + ph - 62.0, 0.0, py + ph);
            bottom.add_color_stop_rgba(0.0, 0.012, 0.013, 0.018, 0.0);
            bottom.add_color_stop_rgba(1.0, 0.012, 0.013, 0.018, 0.55 * a);
            cr.set_source(&bottom).ok();
            cr.rectangle(px, py + ph - 62.0, pw, 62.0);
            cr.fill();
            let _ = cr.restore();
        }
        // Results or an album on their way.
        if self.list_busy {
            rr(cr, px + 24.0, py + 5.0, pw - 48.0, 2.0, 1.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.06 * a);
            cr.fill();
            busy_track(cr, px + 24.0, py + 5.0, pw - 48.0, 2.0, self.accent.lighten(0.40), 0.9 * a, self.clock());
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
                rounded_pic(cr, l.screen, 16.0, frame, a);
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
            if self.lyrics_state == 1 {
                spinner(cr, content.cx(), content.y + 150.0, 15.0, 2.2, self.accent.lighten(0.4), 0.85 * a, self.clock());
            } else {
                icon(cr, Icon::Music, content.cx(), content.y + 150.0, 42.0, theme::DIM, 0.35 * a);
            }
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
                // (size, alpha, brightness) by distance from the sung line;
                // eased between the previous line's emphasis and this one's.
                let look = |dist: i64| -> (f64, f64, f64) {
                    match dist {
                        0 => (20.0, 0.96, 1.0),
                        _ if !self.lyrics_synced => (15.0, 0.62, 0.45),
                        1 => (15.0, 0.55, 0.45),
                        2 => (14.0, 0.32, 0.45),
                        _ => (13.5, 0.20, 0.25),
                    }
                };
                let now = look((i as i64 - self.lyric_idx as i64).abs());
                let was = look((i as i64 - self.lyric_prev_idx as i64).abs());
                let m = self.lyric_shift;
                let size = lerp(now.0, was.0, m);
                let alpha = lerp(now.1, was.1, m);
                let bright = lerp(now.2, was.2, m);
                let color = if bright > 0.45 {
                    theme::MUTED.mix(theme::TEXT.lighten(0.05), (bright - 0.45) / 0.55)
                } else {
                    theme::DIM.mix(theme::MUTED, (bright - 0.25) / 0.20)
                };
                let weight = if bright > 0.72 { pango::Weight::Semibold } else { pango::Weight::Normal };
                let hovered = if self.lyrics_synced { self.hv(Hit::Row(item)) } else { 0.0 };
                if hovered > 0.0 {
                    rr(cr, r.x - 6.0, r.y + 2.0, r.w + 12.0, r.h - 4.0, 10.0);
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.05 * hovered * a);
                    cr.fill();
                }
                para(cr, &line.text, r.x + 10.0, r.y + 2.0, r.w - 20.0, size, weight, color, alpha * a, true);
            }
        }
        let _ = cr.restore();

        if !self.lyrics_synced && l.screen.w <= 0.0 {
            label(cr, "Letra sin sincronizar", content.x + content.w - 150.0, content.y + content.h - 16.0, 150.0, 9.5, pango::Weight::Normal, theme::DIM, 0.6 * a);
        } else if self.lyrics_synced {
            // Timed against a different recording (video intro, live take…)
            // or already corrected: say how to fix it.
            let mismatch = self.lyric_dur > 0.0 && self.dur > 0.0 && (self.dur - self.lyric_dur).abs() > 3.0;
            let hint = if self.lyric_offset.abs() >= 0.05 {
                Some(format!("Ajuste {:+.1} s · [ ] para corregir", self.lyric_offset).replace('.', ","))
            } else if mismatch {
                Some("¿Desfasada? Clic derecho en la línea que suena, o [ ]".to_string())
            } else {
                None
            };
            if let Some(h) = hint {
                label_center(cr, &h, content.x, content.y + content.h - 16.0, content.w, 9.5, pango::Weight::Normal, theme::DIM, 0.7 * a);
            }
        }
    }

    fn row_background(&self, cr: &Context, r: Rect, _hovered: bool, a: f64) {
        let hover = self.row_fade.get();
        if hover > 0.0 {
            rr(cr, r.x, r.y + 2.0, r.w, r.h - 4.0, 12.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.06 * hover * a);
            cr.fill();
        }
    }

    /// Accounts view, YouTube Music signed in or on its way: photo, name and
    /// handle, then the import's progress, the error, or what is in the library.
    fn youtube_card(&self, cr: &Context, r: Rect, a: f64) {
        const OK: Rgb = Rgb::new(0.45, 0.86, 0.56);
        const ERR: Rgb = Rgb::new(1.0, 0.50, 0.46);
        rr(cr, r.x, r.y, r.w, r.h, 14.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.05 * a);
        cr.fill();

        let (name, handle, photo) = &self.yt_account;
        let avatar = Rect::new(r.x + 14.0, r.y + 14.0, 46.0, 46.0);
        self.thumb_or_placeholder(cr, avatar, 23.0, photo, Icon::Music, a);
        let tx = avatar.x + avatar.w + 14.0;
        let name = if !name.is_empty() {
            name.as_str()
        } else if self.yt_google {
            "Tu cuenta de Google"
        } else {
            "Tu cuenta de YouTube"
        };
        label(cr, name, tx, r.y + 15.0, r.w - (tx - r.x) - 120.0, 14.0, pango::Weight::Semibold, theme::TEXT, a);
        let sub = if !handle.is_empty() {
            handle.clone()
        } else if self.importing {
            "Leyendo tu cuenta…".into()
        } else {
            self.yt_browser.map(|b| format!("Sesión de {b}")).unwrap_or_default()
        };
        label(cr, &sub, tx, r.y + 37.0, r.w - (tx - r.x) - 120.0, 11.0, pango::Weight::Normal, theme::MUTED, 0.75 * a);
        if self.accounts_ok && !self.importing && self.account_error.is_none() {
            let badge = Rect::new(r.x + r.w - 116.0, r.y + 18.0, 102.0, 26.0);
            rr(cr, badge.x, badge.y, badge.w, badge.h, 13.0);
            set(cr, OK, 0.16 * a);
            cr.fill();
            icon(cr, Icon::Check, badge.x + 16.0, badge.cy(), 12.0, OK, a);
            label(cr, "Conectada", badge.x + 28.0, badge.cy() - 7.5, badge.w - 30.0, 11.0, pango::Weight::Semibold, OK, a);
        }

        let y = r.y + 72.0;
        let w = r.w - 28.0;
        if let Some((done, total, list)) = &self.import_progress {
            let text = if *total == 0 {
                list.clone()
            } else {
                format!("Importando tus listas · {} de {} · {list}", (done + 1).min(*total), total)
            };
            label(cr, &text, r.x + 14.0, y, w, 11.0, pango::Weight::Medium, theme::TEXT, 0.85 * a);
            let bar = Rect::new(r.x + 14.0, y + 20.0, w, 6.0);
            rr(cr, bar.x, bar.y, bar.w, bar.h, 3.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.10 * a);
            cr.fill();
            let (from, to) = if *total == 0 {
                // Nothing to count yet: a segment sweeping across.
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| (d.as_millis() % 1400) as f64 / 1400.0)
                    .unwrap_or(0.0);
                let x = t * 1.3 - 0.3;
                (x.max(0.0), (x + 0.3).min(1.0))
            } else {
                (0.0, (*done as f64 / *total as f64).max(0.03))
            };
            rr(cr, bar.x + bar.w * from, bar.y, bar.w * (to - from), bar.h, 3.0);
            set(cr, self.accent.lighten(0.15), a);
            cr.fill();
        } else if let Some(err) = &self.account_error {
            para(cr, err, r.x + 14.0, y, w, 10.5, pango::Weight::Medium, ERR, 0.95 * a, false);
        } else if self.accounts_ok {
            let n = self.playlists.iter().filter(|p| !p.remote.is_empty()).count();
            label(
                cr,
                &format!("Sesión iniciada · {n} listas de YouTube Music en tu Biblioteca"),
                r.x + 14.0,
                y + 6.0,
                w,
                11.0,
                pango::Weight::Medium,
                OK,
                0.9 * a,
            );
        }
    }

    /// Accounts view: (state line, explanation) for one service.
    fn account_text(&self, acc: Account) -> (String, String) {
        match acc {
            Account::YouTube if self.accounts_ok && self.yt_google => (
                "Conectada con tu cuenta de Google".into(),
                "Tus listas y tus «Me gusta» están en la Biblioteca.".into(),
            ),
            Account::YouTube if !self.accounts_ok && self.login_browser.is_some() => (
                "Sin conectar".into(),
                format!(
                    "Sin cuenta igual puedes buscar y escuchar. Para traer tus listas y tus «Me gusta», \
                     inicia sesión: se abre una ventana de {} solo para esto.",
                    self.login_browser.unwrap_or("tu navegador")
                ),
            ),
            Account::YouTube if self.accounts_ok => (
                match self.yt_browser {
                    Some(name) => format!("Conectada con tu sesión de {name}"),
                    None => "Conectada".to_string(),
                },
                "Tus listas y tus «Me gusta» están en la Biblioteca. Elige otro navegador para cambiar de cuenta.".into(),
            ),
            Account::YouTube => (
                "Sin conectar".into(),
                "Sin cuenta igual puedes buscar y escuchar. Para traer tus listas, inicia sesión en YouTube \
                 Music en tu navegador y elígelo aquí."
                    .into(),
            ),
            Account::Spotify => {
                let playback = if cfg!(windows) {
                    "Sus canciones suenan desde YouTube Music."
                } else if self.spotify.snapshot().available || self.spotify.snapshot().standby {
                    "Reproducción con spotifyd lista."
                } else {
                    "Sin spotifyd, sus canciones suenan desde YouTube Music."
                };
                if self.spotify_search {
                    ("Búsqueda conectada".into(), playback.into())
                } else {
                    (
                        "Sin conectar".into(),
                        "Spotify pide una app gratuita para buscar: créala (Redirect URI \
                         http://127.0.0.1:8888/callback, marca Web API) y pega aquí sus claves."
                            .into(),
                    )
                }
            }
        }
    }

    fn pill(&self, cr: &Context, r: Rect, ic: Icon, text: &str, _hovered: bool, accent: bool, a: f64) {
        let hover = self.row_fade.get();
        rr(cr, r.x, r.y, r.w, r.h, r.h / 2.0);
        if accent {
            set(cr, self.accent.lighten(lerp(0.18, 0.30, hover)), 0.9 * a);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, lerp(0.05, 0.11, hover) * a);
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
            Some(s) => rounded_pic(cr, r, radius, &s, a),
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
            self.row_fade.set(self.hv(Hit::Row(*item)));
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
                RowItem::Connect if self.tab == Tab::Library && !self.accounts_ok => {
                    self.pill(cr, *r, Icon::Plus, "Conectar cuentas", hovered, false, a);
                }
                RowItem::Connect if self.tab == Tab::Library => {
                    let sp = if self.spotify_search { " · Spotify ✓" } else { "" };
                    self.pill(cr, *r, Icon::Check, &format!("Cuentas · YouTube Music ✓{sp}"), hovered, false, a);
                }
                RowItem::Connect => {
                    self.pill(cr, *r, Icon::Spotify, "Conectar Spotify para buscar también allí", hovered, false, a);
                }
                RowItem::AccountsBack => self.pill(cr, *r, Icon::ChevronLeft, "Biblioteca", hovered, false, a),
                RowItem::AccountInfo(Account::YouTube) if r.h > 60.0 => self.youtube_card(cr, *r, a),
                RowItem::AccountInfo(acc) => {
                    let (state, help) = self.account_text(acc);
                    label(cr, &state, r.x + 4.0, r.y, r.w - 8.0, 13.0, pango::Weight::Semibold, theme::TEXT, 0.92 * a);
                    para(cr, &help, r.x + 4.0, r.y + 21.0, r.w - 8.0, 10.5, pango::Weight::Normal, theme::MUTED, 0.7 * a, false);
                }
                RowItem::Browser(i) => {
                    if let Some((name, _)) = self.browsers.get(i) {
                        let current = self.accounts_ok && self.yt_browser == Some(*name);
                        let text = if self.importing && !self.accounts_ok {
                            format!("{name} · leyendo…")
                        } else if current {
                            format!("{name} · conectada")
                        } else {
                            format!("Usar mi sesión de {name}")
                        };
                        let ic = if current { Icon::Check } else { Icon::Music };
                        self.pill(cr, *r, ic, &text, hovered, current, a);
                    }
                }
                RowItem::GoogleLogin => {
                    let name = self.login_browser.unwrap_or("el navegador");
                    let text = if self.logging_in {
                        format!("Inicia sesión en {name} y cierra esa ventana al terminar")
                    } else if self.importing {
                        "Importando tus listas…".to_string()
                    } else if self.accounts_ok {
                        "Volver a iniciar sesión".to_string()
                    } else {
                        "Iniciar sesión con Google".to_string()
                    };
                    self.pill(cr, *r, Icon::Play, &text, hovered, !self.accounts_ok, a);
                }
                RowItem::YouTubeLogin => {
                    self.pill(cr, *r, Icon::Play, "Iniciar sesión en YouTube Music", hovered, false, a)
                }
                RowItem::YouTubeDisconnect => self.pill(cr, *r, Icon::Close, "Desconectar", hovered, false, a),
                RowItem::SpotifyApp => {
                    self.pill(cr, *r, Icon::Spotify, "1 · Crear app en Spotify", hovered, false, a)
                }
                RowItem::SpotifyKeys => {
                    let text = if self.checking_spotify {
                        "Comprobando…"
                    } else if self.spotify_search {
                        "Cambiar claves"
                    } else {
                        "2 · Pegar Client ID y secret"
                    };
                    self.pill(cr, *r, Icon::Plus, text, hovered, false, a);
                }
                RowItem::SpotifyForget => self.pill(cr, *r, Icon::Close, "Quitar claves", hovered, false, a),
                RowItem::SpotifyPlayback => self.pill(
                    cr,
                    *r,
                    Icon::Play,
                    "Reproducir con Premium (spotifyd · se abre una terminal)",
                    hovered,
                    false,
                    a,
                ),
                RowItem::Import => {
                    let text = match &self.import_progress {
                        Some((done, total, _)) if *total > 0 => format!("Importando… {} de {total}", (done + 1).min(*total)),
                        _ if self.importing => "Importando…".to_string(),
                        _ if !self.accounts_ok => "Conectar cuentas".to_string(),
                        _ => "Importar de YouTube Music".to_string(),
                    };
                    self.pill(cr, *r, Icon::Queue, &text, hovered, false, a);
                }
                RowItem::NewPlaylist => {
                    self.pill(cr, *r, Icon::Plus, "Nueva lista", hovered, false, a);
                }
                RowItem::Back => {
                    rr(cr, r.x, r.y, r.w, r.h, r.h / 2.0);
                    cr.set_source_rgba(1.0, 1.0, 1.0, lerp(0.05, 0.11, self.row_fade.get()) * a);
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
            if self.loading_mix > 0.0 {
                spinner(cr, thumb.cx(), thumb.cy(), 7.5, 1.8, Rgb::new(1.0, 1.0, 1.0), 0.95 * a * self.loading_mix, self.clock());
            }
            if self.loading_mix < 1.0 {
                play_pause(cr, thumb.cx(), thumb.cy(), 12.0, self.play_mix, Rgb::new(1.0, 1.0, 1.0), 0.95 * a * (1.0 - self.loading_mix));
            }
        }

        let text_x = thumb.x + thumb.w + 12.0;
        let text_w = r.x + r.w - text_x - if hovered { 124.0 } else { 90.0 };
        let title_color = if current { self.accent.lighten(0.45) } else { theme::TEXT };
        label(cr, &t.title, text_x, r.y + 7.0, text_w, 12.5, pango::Weight::Medium, title_color, 0.92 * a);
        let sub = if t.album.is_empty() || t.album == t.title {
            t.subtitle()
        } else {
            format!("{} · {}", t.subtitle(), t.album)
        };
        label(cr, &sub, text_x, r.y + 25.0, text_w, 10.5, pango::Weight::Normal, theme::MUTED, 0.62 * a);
        platform_badge(cr, r.x + r.w - 20.0, r.cy(), t.is_spotify(), t.video, a);
        if t.duration > 0.0 && !hovered {
            label(cr, &format_time(t.duration), r.x + r.w - 80.0, r.y + 15.0, 44.0, 10.0, pango::Weight::Normal, theme::DIM, 0.7 * a);
        }
    }
}
