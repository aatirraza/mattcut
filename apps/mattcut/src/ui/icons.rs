//! Line icons drawn with the painter (crisp at any scale, no icon font needed).

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2, pos2, vec2};

use super::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Media,
    Audio,
    Text,
    Sticker,
    Effects,
    Transitions,
    Filters,
    Captions,
    Undo,
    Redo,
    Split,
    TrimLeft,
    TrimRight,
    Delete,
    Duplicate,
    Marker,
    Magnet,
    Snap,
    ZoomIn,
    ZoomOut,
    Fit,
    Play,
    Pause,
    Prev,
    Next,
    Home,
    Export,
    Plus,
    Search,
    Lock,
    Eye,
    Speaker,
    Close,
    Cloud,
    Computer,
    Folder,
    Keyboard,
}

/// Draw `icon` centred in `r` with stroke colour `c`.
pub fn draw(p: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let s = r.width().min(r.height());
    let k = s / 20.0; // design grid: 20×20
    let o = r.center() - vec2(10.0 * k, 10.0 * k);
    let pt = |x: f32, y: f32| -> Pos2 { o + vec2(x * k, y * k) };
    let st = Stroke::new((1.6 * k).max(1.2), c);
    let line = |a: (f32, f32), b: (f32, f32)| p.line_segment([pt(a.0, a.1), pt(b.0, b.1)], st);
    let poly = |pts: &[(f32, f32)], closed: bool| {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| pt(*x, *y)).collect();
        if closed {
            p.add(Shape::closed_line(v, st));
        } else {
            p.add(Shape::line(v, st));
        }
    };
    let fill = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| pt(*x, *y)).collect();
        p.add(Shape::convex_polygon(v, c, Stroke::NONE));
    };
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32, rad: f32| {
        p.rect_stroke(Rect::from_min_max(pt(x0, y0), pt(x1, y1)), CornerRadius::same((rad * k) as u8), st, StrokeKind::Middle);
    };
    let circle = |x: f32, y: f32, rad: f32| p.circle_stroke(pt(x, y), rad * k, st);
    let arc = |cx: f32, cy: f32, rad: f32, a0: f32, a1: f32| {
        let n = 14;
        let v: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                pt(cx + rad * a.cos(), cy + rad * a.sin())
            })
            .collect();
        p.add(Shape::line(v, st));
    };
    use std::f32::consts::PI;
    match icon {
        Icon::Media => {
            rect(3.0, 4.0, 17.0, 16.0, 2.5);
            fill(&[(8.5, 7.5), (13.0, 10.0), (8.5, 12.5)]);
        }
        Icon::Audio => {
            line((8.0, 14.5), (8.0, 4.0));
            line((8.0, 4.0), (15.0, 3.0));
            line((15.0, 3.0), (15.0, 13.0));
            circle(6.0, 14.5, 2.2);
            circle(13.0, 13.0, 2.2);
        }
        Icon::Text => {
            line((4.0, 4.5), (16.0, 4.5));
            line((10.0, 4.5), (10.0, 16.0));
            line((7.5, 16.0), (12.5, 16.0));
        }
        Icon::Sticker => {
            circle(10.0, 10.0, 7.0);
            arc(10.0, 10.5, 3.6, 0.15 * PI, 0.85 * PI);
            p.circle_filled(pt(7.5, 8.0), 1.0 * k, c);
            p.circle_filled(pt(12.5, 8.0), 1.0 * k, c);
        }
        Icon::Effects => {
            poly(&[(10.0, 2.5), (11.8, 8.2), (17.5, 10.0), (11.8, 11.8), (10.0, 17.5), (8.2, 11.8), (2.5, 10.0), (8.2, 8.2)], true);
            p.circle_filled(pt(16.0, 4.0), 1.1 * k, c);
        }
        Icon::Transitions => {
            rect(2.5, 5.0, 11.0, 15.0, 2.0);
            rect(9.0, 5.0, 17.5, 15.0, 2.0);
        }
        Icon::Filters => {
            circle(10.0, 7.5, 4.3);
            circle(7.0, 12.5, 4.3);
            circle(13.0, 12.5, 4.3);
        }
        Icon::Captions => {
            rect(2.5, 4.5, 17.5, 15.5, 2.5);
            arc(7.6, 10.0, 2.2, 0.25 * PI, 1.75 * PI);
            arc(13.2, 10.0, 2.2, 0.25 * PI, 1.75 * PI);
        }
        Icon::Undo | Icon::Redo => {
            let flip = icon == Icon::Redo;
            let fx = |x: f32| if flip { 20.0 - x } else { x };
            let v: Vec<Pos2> = (0..=14)
                .map(|i| {
                    let a = -0.5 * PI + PI * i as f32 / 14.0;
                    pt(fx(10.0 + 5.0 * a.cos()), 10.5 + 5.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(v, st));
            line((fx(10.0), 5.5), (fx(5.0), 5.5));
            line((fx(10.0), 15.5), (fx(6.0), 15.5));
            poly(&[(fx(7.5), 3.0), (fx(5.0), 5.5), (fx(7.5), 8.0)], false);
        }
        Icon::Split => {
            line((10.0, 2.5), (10.0, 17.5));
            rect(3.0, 6.0, 8.0, 14.0, 1.5);
            rect(12.0, 6.0, 17.0, 14.0, 1.5);
        }
        Icon::TrimLeft | Icon::TrimRight => {
            let fx = |x: f32| if icon == Icon::TrimRight { 20.0 - x } else { x };
            line((fx(6.0), 3.0), (fx(6.0), 17.0));
            rect(fx(9.0).min(fx(16.0)), 6.0, fx(9.0).max(fx(16.0)), 14.0, 1.5);
            line((fx(2.5), 8.0), (fx(4.5), 10.0));
            line((fx(4.5), 10.0), (fx(2.5), 12.0));
        }
        Icon::Delete => {
            line((4.0, 5.5), (16.0, 5.5));
            line((8.0, 5.5), (8.5, 3.5));
            line((8.5, 3.5), (11.5, 3.5));
            line((11.5, 3.5), (12.0, 5.5));
            poly(&[(5.5, 5.5), (6.5, 16.5), (13.5, 16.5), (14.5, 5.5)], false);
            line((8.6, 8.5), (8.9, 13.5));
            line((11.4, 8.5), (11.1, 13.5));
        }
        Icon::Duplicate => {
            rect(3.0, 6.5, 13.0, 16.5, 2.0);
            poly(&[(7.0, 6.5), (7.0, 3.5), (17.0, 3.5), (17.0, 13.0), (13.0, 13.0)], false);
        }
        Icon::Marker => {
            poly(&[(5.0, 3.0), (15.0, 3.0), (15.0, 12.5), (10.0, 17.0), (5.0, 12.5)], true);
        }
        Icon::Magnet => {
            arc(10.0, 9.0, 5.5, 0.0, PI);
            line((4.5, 9.0), (4.5, 4.0));
            line((15.5, 9.0), (15.5, 4.0));
            line((3.0, 4.0), (6.5, 4.0));
            line((13.5, 4.0), (17.0, 4.0));
        }
        Icon::Snap => {
            line((4.0, 3.0), (4.0, 17.0));
            line((16.0, 3.0), (16.0, 17.0));
            line((7.0, 10.0), (13.0, 10.0));
            poly(&[(9.0, 8.0), (7.0, 10.0), (9.0, 12.0)], false);
            poly(&[(11.0, 8.0), (13.0, 10.0), (11.0, 12.0)], false);
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            circle(8.5, 8.5, 5.0);
            line((12.2, 12.2), (16.5, 16.5));
            line((6.3, 8.5), (10.7, 8.5));
            if icon == Icon::ZoomIn {
                line((8.5, 6.3), (8.5, 10.7));
            }
        }
        Icon::Fit => {
            poly(&[(3.0, 7.0), (3.0, 3.0), (7.0, 3.0)], false);
            poly(&[(13.0, 3.0), (17.0, 3.0), (17.0, 7.0)], false);
            poly(&[(17.0, 13.0), (17.0, 17.0), (13.0, 17.0)], false);
            poly(&[(7.0, 17.0), (3.0, 17.0), (3.0, 13.0)], false);
        }
        Icon::Play => fill(&[(6.0, 3.5), (16.5, 10.0), (6.0, 16.5)]),
        Icon::Pause => {
            p.rect_filled(Rect::from_min_max(pt(5.0, 4.0), pt(8.5, 16.0)), CornerRadius::same((1.0 * k) as u8), c);
            p.rect_filled(Rect::from_min_max(pt(11.5, 4.0), pt(15.0, 16.0)), CornerRadius::same((1.0 * k) as u8), c);
        }
        Icon::Prev => {
            fill(&[(15.0, 4.5), (7.0, 10.0), (15.0, 15.5)]);
            p.rect_filled(Rect::from_min_max(pt(4.5, 4.5), pt(6.5, 15.5)), CornerRadius::ZERO, c);
        }
        Icon::Next => {
            fill(&[(5.0, 4.5), (13.0, 10.0), (5.0, 15.5)]);
            p.rect_filled(Rect::from_min_max(pt(13.5, 4.5), pt(15.5, 15.5)), CornerRadius::ZERO, c);
        }
        Icon::Home => {
            poly(&[(3.0, 9.5), (10.0, 3.5), (17.0, 9.5)], false);
            poly(&[(5.0, 8.0), (5.0, 16.5), (15.0, 16.5), (15.0, 8.0)], false);
        }
        Icon::Export => {
            line((10.0, 3.0), (10.0, 12.5));
            poly(&[(6.5, 6.5), (10.0, 3.0), (13.5, 6.5)], false);
            poly(&[(4.0, 11.0), (4.0, 16.5), (16.0, 16.5), (16.0, 11.0)], false);
        }
        Icon::Plus => {
            line((10.0, 4.0), (10.0, 16.0));
            line((4.0, 10.0), (16.0, 10.0));
        }
        Icon::Search => {
            circle(8.5, 8.5, 5.0);
            line((12.2, 12.2), (16.5, 16.5));
        }
        Icon::Lock => {
            rect(4.5, 9.0, 15.5, 16.5, 1.5);
            arc(10.0, 9.0, 3.5, PI, 2.0 * PI);
        }
        Icon::Eye => {
            arc(10.0, 15.0, 8.0, 1.2 * PI, 1.8 * PI);
            arc(10.0, 5.0, 8.0, 0.2 * PI, 0.8 * PI);
            circle(10.0, 10.0, 2.2);
        }
        Icon::Speaker => {
            poly(&[(3.0, 8.0), (6.0, 8.0), (10.0, 4.5), (10.0, 15.5), (6.0, 12.0), (3.0, 12.0)], true);
            arc(10.5, 10.0, 3.5, -0.3 * PI, 0.3 * PI);
            arc(10.5, 10.0, 6.5, -0.3 * PI, 0.3 * PI);
        }
        Icon::Close => {
            line((5.0, 5.0), (15.0, 15.0));
            line((15.0, 5.0), (5.0, 15.0));
        }
        Icon::Cloud => {
            arc(7.0, 11.5, 3.5, 0.5 * PI, 1.5 * PI);
            arc(10.5, 8.5, 4.5, PI, 2.0 * PI);
            arc(14.0, 12.0, 3.5, 1.5 * PI, 2.5 * PI);
            line((7.0, 15.0), (14.0, 15.5));
        }
        Icon::Computer => {
            rect(2.5, 3.5, 17.5, 13.0, 1.5);
            line((7.0, 16.5), (13.0, 16.5));
            line((10.0, 13.0), (10.0, 16.5));
        }
        Icon::Folder => {
            poly(&[(2.5, 5.0), (8.0, 5.0), (9.5, 6.8), (17.5, 6.8), (17.5, 16.0), (2.5, 16.0)], true);
        }
        Icon::Keyboard => {
            rect(2.0, 5.0, 18.0, 15.0, 2.0);
            for x in [5.0, 8.0, 11.0, 14.0] {
                p.circle_filled(pt(x, 8.5), 0.8 * k, c);
            }
            line((6.0, 12.0), (14.0, 12.0));
        }
    }
}

/// A square icon button with a hover / active background. Returns the response.
pub fn button(ui: &mut egui::Ui, icon: Icon, tip: &str, active: bool) -> egui::Response {
    button_sized(ui, icon, tip, active, 28.0)
}

pub fn button_sized(ui: &mut egui::Ui, icon: Icon, tip: &str, active: bool, size: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let p = ui.painter();
    let enabled = ui.is_enabled();
    if active {
        p.rect_filled(r, CornerRadius::same(theme::R_CTRL), theme::ACCENT_SOFT);
    } else if resp.hovered() && enabled {
        p.rect_filled(r, CornerRadius::same(theme::R_CTRL), theme::HOVER);
    }
    let c = if !enabled {
        theme::TEXT_FAINT
    } else if active {
        theme::ACCENT_HOVER
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        theme::TEXT_DIM
    };
    draw(p, r.shrink(size * 0.2), icon, c);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// A rounded pill button with an icon and a label.
pub fn pill(ui: &mut egui::Ui, icon: Icon, label: &str, fill: Color32, text: Color32) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), text);
    let w = 14.0 + 16.0 + 6.0 + galley.size().x + 14.0;
    let (r, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
    let f = if resp.hovered() { fill.gamma_multiply(1.15) } else { fill };
    ui.painter().rect_filled(r, CornerRadius::same(15), f);
    draw(ui.painter(), Rect::from_center_size(pos2(r.min.x + 22.0, r.center().y), Vec2::splat(16.0)), icon, text);
    ui.painter().galley(pos2(r.min.x + 36.0, r.center().y - galley.size().y / 2.0), galley, text);
    resp
}
