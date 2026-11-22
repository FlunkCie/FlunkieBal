//! Every screen and sheet, drawn with egui on top of the 3D scene.

use crate::net::{self, Link};
use crate::app::{anton, fade, CtlKind, FlunkiApp, MatchMenu, PlayStep, Screen, Sheet, View};
use crate::arenas::{ArenaKey, PITCHES};
use crate::icons::*;
use crate::game::{self, Ctrl, Drill, Foot, Mode, State, Tut};
use crate::save::{self, Unlock, XpResult};
use crate::voice;
use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use std::sync::Arc;

pub const GOLD: Color32 = Color32::from_rgb(0xf2, 0xb6, 0x32);
pub const INK: Color32 = Color32::from_rgb(0x12, 0x20, 0x1a);
pub const PANEL: Color32 = Color32::from_rgba_premultiplied(12, 22, 17, 236);
pub const CARD: Color32 = Color32::from_rgba_premultiplied(30, 44, 36, 230);
pub const MUTED: Color32 = Color32::from_rgb(0xb8, 0xc8, 0xbc);
pub const GOOD: Color32 = Color32::from_rgb(0x6f, 0xdc, 0x6f);
pub const WARN: Color32 = Color32::from_rgb(0xff, 0xd2, 0x3f);
pub const BAD: Color32 = Color32::from_rgb(0xff, 0x5a, 0x4a);
pub const BLUE: Color32 = Color32::from_rgb(0x55, 0xb9, 0xd9);
pub const PURPLE: Color32 = Color32::from_rgb(0x8a, 0x5a, 0xd0);

pub fn setup_style(ctx: &egui::Context) {
    // Barlow Condensed for text (as in the JavaScript version), Anton for headlines, then our icons
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("anton".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/anton.ttf"))));
    fonts.font_data.insert("barlow".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/BarlowCondensed-Medium.ttf"))));
    fonts.font_data.insert("barlow-b".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/BarlowCondensed-Bold.ttf"))));
    fonts.font_data.insert("icons".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/Phosphor-Fill.ttf"))));
    let mut fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fallback.insert(0, "icons".to_string());
    let with = |first: &str| {
        let mut v = vec![first.to_string()];
        v.extend(fallback.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, with("barlow"));
    fonts.families.insert(FontFamily::Name("anton".into()), with("anton"));
    fonts.families.insert(FontFamily::Name("bold".into()), with("barlow-b"));
    ctx.set_fonts(fonts);

    let mut vis = egui::Visuals::dark();
    vis.panel_fill = Color32::TRANSPARENT;
    vis.window_fill = PANEL;
    vis.override_text_color = Some(Color32::from_rgb(0xf2, 0xee, 0xe2));
    vis.extreme_bg_color = Color32::from_rgb(0x0a, 0x13, 0x0f);
    vis.text_edit_bg_color = Some(Color32::from_rgb(0x0a, 0x13, 0x0f));
    vis.selection.bg_fill = Color32::from_rgba_unmultiplied(0xf2, 0xb6, 0x32, 90);
    vis.selection.stroke = Stroke::new(1.5, GOLD);
    vis.text_cursor.stroke = Stroke::new(2.0, GOLD);
    let r = CornerRadius::same(12);
    let w = &mut vis.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_white_alpha(22));
    w.noninteractive.corner_radius = r;
    w.inactive.weak_bg_fill = Color32::from_white_alpha(14);
    w.inactive.bg_fill = Color32::from_white_alpha(22);
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_white_alpha(26));
    w.inactive.fg_stroke = Stroke::new(1.5, Color32::from_rgb(0xf2, 0xee, 0xe2));
    w.inactive.corner_radius = r;
    w.hovered.weak_bg_fill = Color32::from_white_alpha(26);
    w.hovered.bg_fill = Color32::from_white_alpha(34);
    w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xf2, 0xb6, 0x32, 140));
    w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    w.active.weak_bg_fill = Color32::from_rgba_unmultiplied(0xf2, 0xb6, 0x32, 70);
    w.active.bg_fill = GOLD;
    w.active.bg_stroke = Stroke::new(1.5, GOLD);
    w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    w.open = w.active;
    vis.window_shadow = egui::Shadow::NONE;
    vis.popup_shadow = egui::Shadow::NONE;
    ctx.set_visuals(vis);
    ctx.global_style_mut(|s| {
        s.spacing.button_padding = vec2(14.0, 9.0);
        s.spacing.item_spacing = vec2(10.0, 10.0);
        s.spacing.interact_size = vec2(44.0, 40.0);
        s.spacing.slider_width = 220.0;
        s.spacing.slider_rail_height = 6.0;
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        s.spacing.scroll.bar_width = 5.0;
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(18.0));
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(17.0));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::proportional(16.0));
        s.interaction.selectable_labels = false;
    });
}

/// The size of the big banner title on a screen this wide.
pub(crate) fn banner_size(w: f32) -> f32 {
    crate::app::anton_size((w * 0.12).clamp(34.0, 64.0))
}

/// Lay out every font and size the game uses, once, while it starts. Each size seen for the first
/// time puts its glyphs in egui's atlas, and a full atlas grows by doubling and goes to the GPU whole:
/// that was the hitch on opening a page, on the first throw and on the first HIT!.
pub fn warm_fonts(ctx: &egui::Context, w: f32) {
    let mut text: String = (32u8..127).map(|c| c as char).collect();
    text.push_str("…·×–—’éëüöï€");
    let icons: String = crate::icons::ALL.concat();
    let prop = [11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 20.0, 22.0, 26.0, 29.0];
    let bolds = [11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0];
    let antons = crate::app::ANTON_SIZES;
    let _ = w;
    ctx.fonts_mut(|f| {
        let mut go = |id: FontId, icons_too: bool| {
            // a few offsets, so the glyphs land on more than one sub-pixel position
            for pre in [""] {
                let _ = f.layout_no_wrap(format!("{pre}{text}"), id.clone(), Color32::WHITE);
            }
            if icons_too {
                let _ = f.layout_no_wrap(icons.clone(), id, Color32::WHITE);
            }
        };
        for s in prop {
            go(FontId::proportional(s), true);
        }
        for s in bolds {
            go(bold(s), false);
        }
        for s in antons {
            go(anton(s), false);
        }
        go(FontId::monospace(10.0), false);
    });
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// A sheet's title row with a close button. Returns true when closed.
fn sheet_head(ui: &mut Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(anton(28.0)).color(GOLD));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
            let p = ui.painter();
            p.circle_filled(r.center(), 18.0, if resp.hovered() { Color32::from_white_alpha(40) } else { Color32::from_white_alpha(20) });
            let c = r.center();
            for (a, b) in [(vec2(-6.0, -6.0), vec2(6.0, 6.0)), (vec2(-6.0, 6.0), vec2(6.0, -6.0))] {
                p.line_segment([c + a, c + b], Stroke::new(2.0, Color32::WHITE));
            }
            close = resp.clicked();
        });
    });
    close
}
/// A labelled on/off switch.
fn toggle(ui: &mut Ui, on: &mut bool, label: &str, w: f32) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(w, 44.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let p = ui.painter();
    p.text(r.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(17.0), Color32::WHITE);
    let sw = Rect::from_center_size(r.right_center() - vec2(26.0, 0.0), vec2(48.0, 28.0));
    let t = ui.ctx().animate_bool(resp.id, *on);
    let col = Color32::from_white_alpha(30).lerp_to_gamma(GOLD, t);
    p.rect_filled(sw, 14.0, col);
    p.circle_filled(pos2(sw.left() + 14.0 + t * 20.0, sw.center().y), 11.0, Color32::WHITE);
    resp.clicked()
}
/// A labelled slider with its value on the right.
fn slider_row(ui: &mut Ui, label: &str, v: &mut f32, lo: f32, hi: f32, fmt: impl Fn(f32) -> String, w: f32) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(17.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(fmt(*v)).font(bold(17.0)).color(GOLD));
        });
    });
    ui.spacing_mut().slider_width = w;
    ui.add(egui::Slider::new(v, lo..=hi).show_value(false)).changed()
}

// ---------- small building blocks ----------
fn shadow_text(p: &egui::Painter, pos: Pos2, al: Align2, s: &str, f: FontId, c: Color32) {
    p.text(pos + vec2(0.0, 2.5), al, s, f.clone(), Color32::from_black_alpha((150.0 * (c.a() as f32 / 255.0)) as u8));
    p.text(pos, al, s, f, c);
}
fn vgrad(p: &egui::Painter, r: Rect, top: u8, bottom: u8) {
    let mut m = egui::Mesh::default();
    let (a, b) = (Color32::from_black_alpha(top), Color32::from_black_alpha(bottom));
    m.colored_vertex(r.left_top(), a);
    m.colored_vertex(r.right_top(), a);
    m.colored_vertex(r.right_bottom(), b);
    m.colored_vertex(r.left_bottom(), b);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}
fn panel() -> egui::Frame {
    egui::Frame::NONE.fill(PANEL).corner_radius(CornerRadius::same(20)).inner_margin(egui::Margin::same(16)).stroke(Stroke::new(1.0, Color32::from_white_alpha(26)))
}
fn big(ui: &mut Ui, text: &str, primary: bool, width: f32) -> egui::Response {
    let rt = RichText::new(text).font(anton(21.0)).color(if primary { INK } else { Color32::WHITE });
    ui.add(egui::Button::new(rt).fill(if primary { GOLD } else { Color32::from_white_alpha(22) }).corner_radius(CornerRadius::same(14)).min_size(vec2(width, 52.0)))
}
fn small_btn(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).size(15.0)).fill(Color32::from_white_alpha(18)).corner_radius(CornerRadius::same(10)))
}
fn seg(ui: &mut Ui, text: &str, sel: bool, locked: bool, w: f32) -> egui::Response {
    let label = if locked && !sel { format!("{LOCK} {text}") } else { text.to_string() };
    let rt = RichText::new(label).size(15.0).color(if sel { INK } else if locked { Color32::from_white_alpha(110) } else { Color32::WHITE });
    ui.add(egui::Button::new(rt).fill(if sel { Color32::from_rgb(0xff, 0xe0, 0x8a) } else { Color32::from_white_alpha(16) }).corner_radius(CornerRadius::same(10)).min_size(vec2(w, 36.0)))
}
/// A card-style button with a title and a line under it.
fn card_btn(ui: &mut Ui, title: &str, sub: &str, w: f32, accent: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(w, 64.0), Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { Color32::from_rgba_premultiplied(44, 62, 50, 240) } else { CARD };
    p.rect_filled(r, 14.0, fill);
    p.rect_filled(Rect::from_min_size(r.min + vec2(0.0, 12.0), vec2(4.0, r.height() - 24.0)), 2.0, accent);
    p.text(r.left_top() + vec2(16.0, 12.0), Align2::LEFT_TOP, title, anton(20.0), Color32::WHITE);
    p.text(r.left_top() + vec2(16.0, 40.0), Align2::LEFT_TOP, sub, FontId::proportional(13.0), MUTED);
    p.text(r.right_center() - vec2(16.0, 0.0), Align2::RIGHT_CENTER, "›", FontId::proportional(26.0), MUTED);
    resp
}
fn heading(ui: &mut Ui, t: &str) {
    ui.label(RichText::new(t).font(anton(28.0)).color(GOLD));
}
fn label_caps(ui: &mut Ui, t: &str) {
    ui.label(RichText::new(t.to_uppercase()).size(12.0).color(MUTED).font(bold(16.0)));
}
fn swatch(ui: &mut Ui, c: u32, sel: bool, locked: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
    let p = ui.painter();
    let col = Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
    p.circle_filled(r.center(), 15.0, if locked && !sel { col.gamma_multiply(0.35) } else { col });
    if sel {
        p.circle_stroke(r.center(), 18.0, Stroke::new(2.5, Color32::WHITE));
    }
    if locked && !sel {
        p.text(r.center(), Align2::CENTER_CENTER, LOCK, FontId::proportional(13.0), Color32::WHITE);
    }
    resp
}

/// The XP card on the results screens.
fn xp_box(ui: &mut Ui, x: &XpResult, w: f32) {
    egui::Frame::NONE.fill(Color32::from_white_alpha(10)).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(w - 24.0);
        let up = x.after.lv > x.before.lv;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("+{} XP", x.gain)).font(anton(22.0)).color(GOLD));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(if up { format!("Level up! Level {}", x.after.lv) } else { format!("Level {}", x.after.lv) }).color(if up { GOOD } else { Color32::WHITE }).font(bold(16.0)));
            });
        });
        let (r, _) = ui.allocate_exact_size(vec2(w - 24.0, 8.0), Sense::hover());
        ui.painter().rect_filled(r, 4.0, Color32::from_white_alpha(25));
        let f = if x.after.need > 0 { x.after.into as f32 / x.after.need as f32 } else { 1.0 };
        ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width() * f, r.height())), 4.0, GOLD);
        if !x.parts.is_empty() {
            ui.label(RichText::new(x.parts.join(" · ")).size(13.0).color(MUTED));
        }
        if !x.unlocked.is_empty() {
            label_caps(ui, "Unlocked");
            ui.horizontal_wrapped(|ui| {
                for u in &x.unlocked {
                    egui::Frame::NONE.fill(Color32::from_rgba_premultiplied(60, 50, 10, 220)).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                        // whole chips go to the next line: a chip that wraps itself ends up a letter wide
                        ui.add(egui::Label::new(RichText::new(format!("{LOCK_OPEN} {u}")).size(13.0).color(Color32::WHITE)).wrap_mode(egui::TextWrapMode::Extend));
                    });
                }
            });
        }
    });
}

/// Who you are, as the chip and the clubhouse card show it: name, a line under it, and whether there's anyone.
fn who_am_i(app: &FlunkiApp) -> (String, bool) {
    if app.save.locked() {
        ("Guest".into(), false)
    } else {
        (app.save.profile.name.clone(), true)
    }
}
/// A ring round the avatar: the track, and how far into this level you are, clockwise from the top.
fn xp_ring(p: &egui::Painter, c: Pos2, r: f32, width: f32, f: f32) {
    p.circle_stroke(c, r, Stroke::new(width, Color32::from_white_alpha(28)));
    if f > 0.005 {
        let n = (64.0 * f).ceil().max(2.0) as usize;
        let pts: Vec<Pos2> = (0..=n).map(|k| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * f * k as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        }).collect();
        p.add(egui::Shape::line(pts, Stroke::new(width, GOLD)));
    }
}
/// Your shirt's colour with your initial, or an empty seat for a guest.
fn avatar(p: &egui::Painter, c: Pos2, r: f32, app: &FlunkiApp, someone: bool) {
    if someone {
        let col = save::SWATCHES[app.save.profile.color.min(save::SWATCHES.len() - 1)];
        p.circle_filled(c, r, hex_col(col));
        p.circle_filled(c + vec2(0.0, -r * 0.35), r * 0.9, Color32::from_white_alpha(18));
        let initial: String = app.save.profile.name.chars().next().map_or("?".into(), |c| c.to_uppercase().collect());
        p.text(c + vec2(0.0, 1.0), Align2::CENTER_CENTER, initial, anton(r * 1.05), INK);
    } else {
        p.circle_filled(c, r, Color32::from_white_alpha(22));
        p.text(c, Align2::CENTER_CENTER, USER, FontId::proportional((r * 1.05).round()), MUTED);
    }
}
/// Soft gold light behind something: rings fading out.
fn glow(p: &egui::Painter, c: Pos2, r: f32, a: u8) {
    for k in 0..8 {
        let t = k as f32 / 8.0;
        p.circle_filled(c, r * (1.0 - t * 0.6), Color32::from_rgba_unmultiplied(0xf2, 0xb6, 0x32, (a as f32 * (0.12 + t * 0.1)) as u8));
    }
}
fn hex_col(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Top right on the main menu: your avatar in its XP ring, your name and level. Takes you to the clubhouse, where all of that lives.
fn level_chip(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let lv = save::level_of(app.save.xp);
    let (name, someone) = who_am_i(app);
    let f = if lv.need > 0 { lv.into as f32 / lv.need as f32 } else { 1.0 };
    let name_f = bold(16.0);
    let name_g = ctx.fonts_mut(|fo| fo.layout_no_wrap(name.clone(), name_f.clone(), Color32::WHITE));
    let sub = if someone || !save::needs_login() { format!("LEVEL {}", lv.lv) } else { format!("{SIGN_IN}  LOG IN") };
    let sub_g = ctx.fonts_mut(|fo| fo.layout_no_wrap(sub.clone(), bold(12.0), GOLD));
    let w = (52.0 + name_g.size().x.max(sub_g.size().x) + 18.0).clamp(120.0, 230.0);
    egui::Area::new(egui::Id::new("lvchip")).fixed_pos(pos2(sr.right() - 12.0 - w, sr.top() + 12.0)).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(w, 46.0), Sense::click());
        let p = ui.painter();
        let hot = resp.hovered();
        p.rect_filled(r, 23.0, PANEL);
        p.rect_stroke(r, 23.0, Stroke::new(1.0, if someone { Color32::from_rgba_unmultiplied(0xf2, 0xb6, 0x32, if hot { 150 } else { 90 }) } else { Color32::from_white_alpha(if hot { 70 } else { 40 }) }), StrokeKind::Inside);
        let c = r.left_center() + vec2(23.0, 0.0);
        if someone {
            glow(p, c, 26.0, if hot { 46 } else { 30 });
        }
        avatar(p, c, 14.5, app, someone);
        xp_ring(p, c, 18.5, 2.5, if someone || !save::needs_login() { f } else { 0.0 });
        let tx = r.left() + 50.0;
        // clipped to the chip, should a name be long
        let pc = p.with_clip_rect(r.shrink2(vec2(10.0, 0.0)));
        pc.galley(pos2(tx, r.top() + 7.0), name_g, Color32::WHITE);
        pc.galley(pos2(tx, r.top() + 27.0), sub_g, GOLD);
        if resp.clicked() {
            app.audio.unlock();
            app.view = View::Club;
        }
    });
}

/// Your card in the clubhouse: avatar in its ring, name, account, level and XP. Tap to dress up.
fn profile_card(app: &FlunkiApp, ui: &mut Ui, w: f32) -> egui::Response {
    let lv = save::level_of(app.save.xp);
    let (name, someone) = who_am_i(app);
    let locked = app.save.locked();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 104.0), Sense::click());
    let hot = resp.hovered();
    let p = ui.painter();
    p.rect_filled(r, 16.0, if hot { Color32::from_white_alpha(20) } else { CARD });
    p.rect_stroke(r, 16.0, Stroke::new(1.0, if someone { Color32::from_rgba_unmultiplied(0xf2, 0xb6, 0x32, if hot { 120 } else { 60 }) } else { Color32::from_white_alpha(30) }), StrokeKind::Inside);
    let c = r.left_center() + vec2(52.0, -6.0);
    if someone {
        glow(p, c, 50.0, if hot { 44 } else { 32 });
    }
    avatar(p, c, 28.0, app, someone);
    let f = if lv.need > 0 { lv.into as f32 / lv.need as f32 } else { 1.0 };
    xp_ring(p, c, 35.0, 4.0, if someone || !save::needs_login() { f } else { 0.0 });
    // the level, on a badge under the avatar
    let g = p.layout_no_wrap(format!("LV {}", lv.lv), anton(13.5), INK);
    let badge = Rect::from_center_size(c + vec2(0.0, 37.0), g.size() + vec2(16.0, 4.0));
    p.rect_filled(badge, badge.height() / 2.0, GOLD);
    p.rect_stroke(badge, badge.height() / 2.0, Stroke::new(2.0, CARD), StrokeKind::Outside);
    p.galley(badge.center() - g.size() / 2.0, g, INK);
    let tx = r.left() + 106.0;
    let hint = if locked { format!("{SIGN_IN} LOG IN") } else { format!("{TSHIRT} CUSTOMIZE") };
    let hg = p.layout_no_wrap(hint, bold(12.0), GOLD);
    let hx = r.right() - 14.0 - hg.size().x;
    p.galley(pos2(hx, r.top() + 14.0), hg, GOLD);
    // clipped short of the hint, should a name be long
    let pc = p.with_clip_rect(Rect::from_min_max(r.min, pos2(hx - 8.0, r.bottom())));
    pc.text(pos2(tx, r.top() + 11.0), Align2::LEFT_TOP, &name, anton(24.0), Color32::WHITE);
    let (status, col) = if app.save.logged_in() && !locked {
        (format!("{CHECK}  Logged in as {}", app.save.user), GOOD)
    } else if locked {
        (format!("{LOCK}  Guest: nothing is kept"), WARN)
    } else {
        ("This device's player".to_string(), MUTED)
    };
    let pc = p.with_clip_rect(r.shrink(8.0));
    pc.text(pos2(tx, r.top() + 45.0), Align2::LEFT_TOP, status, FontId::proportional(14.0), col);
    let bar = Rect::from_min_size(pos2(tx, r.top() + 70.0), vec2(r.right() - tx - 16.0, 8.0));
    p.rect_filled(bar, 4.0, Color32::from_white_alpha(25));
    p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f, 8.0)), 4.0, GOLD);
    pc.text(pos2(tx, bar.bottom() + 5.0), Align2::LEFT_TOP, format!("{} / {} XP to level {}", lv.into, lv.need, lv.lv + 1), FontId::proportional(12.0), MUTED);
    resp
}

// ---------- the whole UI ----------
thread_local! {
    /// the safe rect and the full screen, for placing windows
    static UR: std::cell::Cell<(Rect, Rect)> = const { std::cell::Cell::new((Rect::NOTHING, Rect::NOTHING)) };
    /// where text fields are, so a tap can open the phone's keyboard straight away
    pub static TEXT_RECTS: std::cell::RefCell<Vec<Rect>> = const { std::cell::RefCell::new(Vec::new()) };
}
/// The point of the safe rect a window is pinned to.
fn anc(al: Align2, off: egui::Vec2) -> Pos2 {
    let r = UR.with(|u| u.get().0);
    let x = match al.x() {
        Align::Min => r.left(),
        Align::Center => r.center().x,
        Align::Max => r.right(),
    };
    let y = match al.y() {
        Align::Min => r.top(),
        Align::Center => r.center().y,
        Align::Max => r.bottom(),
    };
    pos2(x, y) + off
}
fn full_rect() -> Rect {
    UR.with(|u| u.get().1)
}
/// `name` keeps the field's identity, which the phone's text prompt comes back to.
fn text_field(ui: &mut Ui, name: &str, te: egui::TextEdit) -> egui::Response {
    let id = egui::Id::new(("textfield", name));
    #[cfg(target_os = "android")]
    let typed = crate::android::text_answer(name);
    #[cfg(target_os = "android")]
    if let Some(t) = typed.as_ref() {
        // as if selected all and typed it, so limits and `changed()` work as ever
        ui.memory_mut(|m| m.request_focus(id));
        let key = |k| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND };
        ui.input_mut(|i| {
            i.events.push(key(egui::Key::A));
            if t.is_empty() {
                i.events.push(egui::Event::Key { key: egui::Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
            } else {
                i.events.push(egui::Event::Text(t.clone()));
            }
        });
    }
    let out = te.id(id).show(ui);
    let r = out.response.response;
    TEXT_RECTS.with(|t| t.borrow_mut().push(r.rect));
    #[cfg(target_os = "android")]
    {
        // a phone's keyboard belongs to the system: ask in its own text prompt
        if typed.is_some() {
            r.surrender_focus();
        } else if r.clicked() {
            crate::android::ask_text(name, out.galley.text());
            r.surrender_focus();
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = &out.galley;
    r
}

pub fn draw(app: &mut FlunkiApp, ctx: &egui::Context, p: &egui::Painter, sr: Rect, full: Rect) {
    UR.with(|u| u.set((sr, full)));
    TEXT_RECTS.with(|t| t.borrow_mut().clear());
    invite_prompt(app, ctx);
    match app.screen {
        Screen::Home => {
            if app.view == View::Menu {
                main_menu(app, ctx, p, sr)
            } else {
                clubhouse(app, ctx, p, sr)
            }
        }
        Screen::Play => play_panel(app, ctx, p, sr),
        Screen::Playing => hud(app, ctx, p, sr),
        Screen::Paused => {
            hud(app, ctx, p, sr);
            pause(app, ctx, p, sr);
        }
        Screen::Over => {
            hud(app, ctx, p, sr);
            over(app, ctx, p, sr);
        }
        Screen::TrainOver => train_over(app, ctx, p, sr),
    }
    if app.sheet != Sheet::None {
        p.rect_filled(full, 0.0, Color32::from_black_alpha(if app.sheet == Sheet::Profile { 30 } else { 110 }));
        match app.sheet {
            Sheet::Profile => profile_sheet(app, ctx, sr),
            Sheet::Stats => stats_sheet(app, ctx, sr),
            Sheet::Progress => progress_sheet(app, ctx, sr),
            Sheet::Train => train_sheet(app, ctx, sr),
            Sheet::Settings => settings_sheet(app, ctx, sr),
            Sheet::Rules => rules_sheet(app, ctx, sr),
            Sheet::Friends => friends_sheet(app, ctx, sr),
            Sheet::Account => account_sheet(app, ctx, sr),
            Sheet::Bug => bug_sheet(app, ctx, sr),
            Sheet::None => {}
        }
    }
    if let Some((t, life)) = &app.lock_note {
        let gal = p.layout_no_wrap(t.clone(), FontId::proportional(15.0), Color32::WHITE);
        let r = Rect::from_center_size(pos2(sr.center().x, sr.top() + 70.0), gal.size() + vec2(24.0, 14.0));
        let a = (life / 0.3).min(1.0);
        ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("locknote"))).rect_filled(r, 12.0, fade(Color32::from_rgb(0x3a, 0x2a, 0x0a), a));
        ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("locknote"))).galley(r.min + vec2(12.0, 7.0), gal, Color32::WHITE);
    }
}

/// The brand pill in the top-left corner, as in the JavaScript version.
fn brand_pill(ctx: &egui::Context, sr: Rect, text: &str, back: bool) -> bool {
    let mut clicked = false;
    egui::Area::new(egui::Id::new("brand")).fixed_pos(sr.min + vec2(12.0, 12.0)).show(ctx, |ui| {
        let gal = ui.painter().layout_no_wrap(text.into(), anton(19.0), GOLD);
        let w = gal.size().x + 62.0;
        let (r, resp) = ui.allocate_exact_size(vec2(w, 42.0), if back { Sense::click() } else { Sense::hover() });
        let p = ui.painter();
        p.rect_filled(r, 21.0, PANEL);
        p.rect_stroke(r, 21.0, Stroke::new(1.0, Color32::from_white_alpha(30)), StrokeKind::Inside);
        let c = r.left_center() + vec2(21.0, 0.0);
        if back {
            p.text(c, Align2::CENTER_CENTER, ARROW_LEFT, FontId::proportional(20.0), Color32::WHITE);
        } else {
            p.circle_stroke(c, 13.0, Stroke::new(1.5, GOLD));
            p.text(c, Align2::CENTER_CENTER, "FB", anton(12.0), GOLD);
        }
        p.galley(pos2(r.left() + 44.0, r.center().y - gal.size().y / 2.0), gal, GOLD);
        clicked = resp.clicked();
    });
    clicked
}
/// A wide row inside the menu panel: icon, title, a line under it.
fn row_btn(ui: &mut Ui, icon: &str, title: &str, sub: &str, w: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(w, 58.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 12.0, if resp.hovered() { Color32::from_white_alpha(18) } else { Color32::from_white_alpha(9) });
    p.rect_stroke(r, 12.0, Stroke::new(1.0, Color32::from_white_alpha(22)), StrokeKind::Inside);
    p.text(r.left_center() + vec2(22.0, 0.0), Align2::CENTER_CENTER, icon, FontId::proportional(20.0), Color32::WHITE);
    if sub.is_empty() {
        p.text(r.left_center() + vec2(44.0, 0.0), Align2::LEFT_CENTER, title, anton(17.0), Color32::WHITE);
    } else {
        p.text(r.left_top() + vec2(44.0, 11.0), Align2::LEFT_TOP, title, anton(17.0), Color32::WHITE);
        p.text(r.left_top() + vec2(44.0, 34.0), Align2::LEFT_TOP, sub, FontId::proportional(12.0), MUTED);
    }
    resp
}

fn main_menu(app: &mut FlunkiApp, ctx: &egui::Context, p: &egui::Painter, sr: Rect) {
    let w = sr.width();
    vgrad(p, Rect::from_min_max(pos2(full_rect().left(), sr.bottom() - sr.height() * 0.42), full_rect().max), 0, 150);
    brand_pill(ctx, sr, "FLUNKIEBAL", false);
    level_chip(app, ctx, sr);
    if app.sheet != Sheet::None {
        return;
    }
    let bw = (w - 24.0).min(440.0);
    let name = app.save.profile.name.clone();
    egui::Area::new(egui::Id::new("menu")).pivot(Align2::CENTER_BOTTOM).fixed_pos(anc(Align2::CENTER_BOTTOM, vec2(0.0, -14.0))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            let iw = bw - 32.0;
            ui.set_width(iw);
            update_row(app, ui, iw);
            let rt = RichText::new(format!("PLAY A MATCH  {ARROW_RIGHT}")).font(anton(26.0)).color(INK);
            if ui.add(egui::Button::new(rt).fill(GOLD).corner_radius(CornerRadius::same(14)).min_size(vec2(iw, 64.0))).clicked() {
                app.audio.unlock();
                app.screen = Screen::Play;
                app.step = PlayStep::Choose;
            }
            if row_btn(ui, HOUSE, "CLUBHOUSE", &format!("{name} · training · stats · account"), iw).clicked() {
                app.audio.unlock();
                app.view = View::Club;
            }
            let sub = friends_line(app);
            if row_btn(ui, FRIENDS, "FRIENDS", &sub, iw).clicked() {
                app.sheet = Sheet::Friends;
            }
            ui.horizontal(|ui| {
                let hw = (iw - 10.0) / 2.0;
                if row_btn(ui, BOOK, "RULEBOOK", "", hw).clicked() {
                    app.sheet = Sheet::Rules;
                }
                if row_btn(ui, GEAR, "SETTINGS", "", hw).clicked() {
                    app.sheet = Sheet::Settings;
                }
            });
        });
    });
}

/// The clubhouse is you: your card, training, your numbers, friends and your account.
fn clubhouse(app: &mut FlunkiApp, ctx: &egui::Context, p: &egui::Painter, sr: Rect) {
    vgrad(p, Rect::from_min_max(pos2(full_rect().left(), sr.bottom() - sr.height() * 0.45), full_rect().max), 0, 170);
    if brand_pill(ctx, sr, "THE CLUBHOUSE", true) {
        app.view = View::Menu;
    }
    if app.sheet != Sheet::None {
        return;
    }
    let bw = (sr.width() - 24.0).min(440.0);
    let locked = app.save.locked();
    let n = app.save.matches.len();
    let recs = app.save.train_pb.iter().filter(|v| v.is_some()).count();
    let mine: Vec<&save::MatchRec> = app.save.matches.iter().filter(|m| m.win.is_some()).collect();
    let wins = mine.iter().filter(|m| m.win == Some(true)).count();
    let lv = save::level_of(app.save.xp).lv;
    let mut go: Option<Sheet> = None;
    let mut log_out = false;
    egui::Area::new(egui::Id::new("clubdock")).pivot(Align2::CENTER_BOTTOM).fixed_pos(anc(Align2::CENTER_BOTTOM, vec2(0.0, -14.0))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            let iw = bw - 32.0;
            ui.set_width(iw);
            if profile_card(app, ui, iw).clicked() {
                go = Some(if locked { Sheet::Account } else { Sheet::Profile });
            }
            let hw = (iw - 8.0) / 2.0;
            let train = if !app.save.tut_done { "Start here".to_string() } else if recs > 0 { format!("{recs} of 3 records set") } else { "Throw · drink · run".into() };
            let progress = match (lv + 1..=save::TOP_UNLOCK).find(|l| save::UNLOCKS.iter().any(|u| u.0 == *l)) {
                _ if locked => format!("{LOCK} Log in to level up"),
                Some(next) => format!("Next unlock: level {next}"),
                None => format!("{CROWN} Everything unlocked"),
            };
            let stats = if n == 0 { "Your player file".to_string() } else { format!("{n} played · {wins} won") };
            let (acc_icon, acc, acc_sub) = if app.save.logged_in() && !locked {
                (SIGN_OUT, "LOG OUT", app.save.user.clone())
            } else if locked {
                (SIGN_IN, "LOG IN", "Or sign up".to_string())
            } else {
                (SIGN_IN, "LOG IN", "Keep your player safe".to_string())
            };
            let rows: [[(&str, &str, String, Option<Sheet>); 2]; 3] = [
                [(TRAINING, "TRAINING", train, Some(Sheet::Train)), (STAR, "PROGRESS", progress, Some(Sheet::Progress))],
                [(STATS, "STATS", stats, Some(Sheet::Stats)), (FRIENDS, "FRIENDS", friends_line(app), Some(Sheet::Friends))],
                [(GEAR, "SETTINGS", "Sound, controls".into(), Some(Sheet::Settings)), (acc_icon, acc, acc_sub, if acc == "LOG OUT" { None } else { Some(Sheet::Account) })],
            ];
            for row in rows {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for (icon, title, sub, to) in row {
                        let ui_w = hw;
                        let r = ui.allocate_ui(vec2(ui_w, 58.0), |ui| {
                            // a long line stays inside its button
                            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                            row_btn(ui, icon, title, &sub, ui_w)
                        }).inner;
                        if r.clicked() {
                            match to {
                                Some(s) => go = Some(s),
                                None => log_out = true,
                            }
                        }
                    }
                });
            }
        });
    });
    if log_out {
        app.log_out();
    } else if let Some(s) = go {
        if s == Sheet::Profile {
            app.name_edit = app.save.profile.name.clone();
        }
        app.sheet = s;
    }
}

fn arena_picker(app: &mut FlunkiApp, ui: &mut Ui, w: f32) {
    label_caps(ui, "Arena");
    let cw = (w - 10.0) / 2.0;
    for row in PITCHES.chunks(2) {
        ui.horizontal(|ui| {
            for &k in row {
                let need = save::unlock_lv(Unlock::Arena(k.id()));
                let locked = app.save.level() < need;
                let sel = app.arena == k;
                let (r, resp) = ui.allocate_exact_size(vec2(cw, 56.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(r, 12.0, if sel { Color32::from_rgba_premultiplied(90, 70, 20, 235) } else { CARD });
                if sel {
                    p.rect_stroke(r, 12.0, Stroke::new(2.0, GOLD), StrokeKind::Inside);
                }
                let tc = if locked { Color32::from_white_alpha(110) } else { Color32::WHITE };
                p.text(r.left_top() + vec2(12.0, 9.0), Align2::LEFT_TOP, k.name(), FontId::proportional(16.0), tc);
                p.text(r.left_top() + vec2(12.0, 31.0), Align2::LEFT_TOP, if locked { format!("{LOCK} Level {need}") } else { k.blurb().chars().take(30).collect::<String>() }, FontId::proportional(12.0), if locked { WARN } else { MUTED });
                if resp.clicked() {
                    if locked {
                        app.lock_note = Some((format!("{LOCK} {} unlocks at level {need} · you are level {}", k.name(), app.save.level()), 2.6));
                    } else {
                        app.arena = k;
                        app.save.arena = k.id().into();
                        app.save.store();
                    }
                }
            }
        });
    }
}

fn play_panel(app: &mut FlunkiApp, ctx: &egui::Context, p: &egui::Painter, sr: Rect) {
    vgrad(p, Rect::from_min_max(pos2(sr.left(), sr.bottom() - sr.height() * 0.6), sr.max), 0, 200);
    let bw = (sr.width() - 24.0).min(440.0);
    let title = match app.step {
        PlayStep::Choose => "Play a match",
        PlayStep::Cpu => "Vs the computer",
        PlayStep::Hot => "Pass and play",
        PlayStep::Online => "Online",
    };
    egui::Area::new(egui::Id::new("play")).pivot(Align2::CENTER_BOTTOM).fixed_pos(anc(Align2::CENTER_BOTTOM, vec2(0.0, -16.0))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.set_width(bw - 32.0);
            let iw = bw - 32.0;
            ui.horizontal(|ui| {
                if small_btn(ui, BACK).clicked() {
                    app.back();
                }
                ui.label(RichText::new(title).font(anton(24.0)).color(Color32::WHITE));
            });
            match app.step {
                PlayStep::Choose => {
                    if card_btn(ui, "Vs the computer", "Your three against three of theirs", iw, GOLD).clicked() {
                        app.step = PlayStep::Cpu;
                    }
                    if card_btn(ui, "Pass and play", "Two teams on one phone", iw, BLUE).clicked() {
                        app.step = PlayStep::Hot;
                    }
                    if card_btn(ui, "Online", "Friends with a code, or anyone", iw, GOOD).clicked() {
                        app.step = PlayStep::Online;
                    }
                    if !app.save.tut_done && small_btn(ui, "New here? Play the tutorial").clicked() {
                        app.start_tutorial();
                    }
                }
                PlayStep::Cpu => {
                    label_caps(ui, "Opponent");
                    ui.horizontal(|ui| {
                        let sw = (iw - 20.0) / 3.0;
                        for (i, n) in game::DIFF_NAMES.iter().enumerate() {
                            if seg(ui, n, app.save.diff == i, false, sw).clicked() {
                                app.save.diff = i;
                                app.save.store();
                            }
                        }
                    });
                    arena_picker(app, ui, iw);
                    if big(ui, "START", true, iw).clicked() {
                        app.start(Mode::Cpu);
                    }
                }
                PlayStep::Hot => {
                    ui.label(RichText::new("Left side of the screen is one team, right side the other.").color(MUTED).size(14.0));
                    arena_picker(app, ui, iw);
                    if big(ui, "START", true, iw).clicked() {
                        app.start(Mode::Hot);
                    }
                }
                PlayStep::Online => online_panel(app, ui, iw),
            }
        });
    });
}

fn online_panel(app: &mut FlunkiApp, ui: &mut Ui, iw: f32) {
    update_row(app, ui, iw);
    if app.save.locked() {
        login_box(app, ui, iw, "Log in to play online.");
        return;
    }
    let in_room = app.net.as_ref().map_or(false, |n| n.in_room);
    if !app.net_err.is_empty() {
        ui.label(RichText::new(app.net_err.clone()).color(BAD).size(14.0));
    }
    if !in_room {
        // the connection is kept in the background; say so only while it isn't there
        match app.net.as_ref().map(|n| n.sock.state()) {
            Some(1) => {}
            Some(0) => {
                ui.label(RichText::new("Connecting…").color(MUTED));
            }
            _ => {
                ui.label(RichText::new("Offline · trying again in the background").color(WARN));
            }
        }
        if big(ui, "HOST A MATCH", true, iw).clicked() {
            app.net_host();
        }
        ui.horizontal(|ui| {
            text_field(ui, "code", egui::TextEdit::singleline(&mut app.code_in).hint_text("CODE").char_limit(4).desired_width(90.0).margin(vec2(12.0, 11.0)).font(anton(20.0)));
            app.code_in = app.code_in.to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            if big(ui, "JOIN", false, 90.0).clicked() && app.code_in.len() == 4 {
                let c = app.code_in.clone();
                app.net_join(&c);
            }
            if big(ui, "FIND", false, iw - 210.0).clicked() {
                app.net_list();
            }
        });
        let rooms = app.net.as_ref().map(|n| n.rooms.clone()).unwrap_or_default();
        if !rooms.is_empty() {
            label_caps(ui, "Open matches");
            for r in rooms {
                if card_btn(ui, &format!("{}'s match", r.name), &format!("{} of 6 · code {}", r.n, r.code), iw, GOOD).clicked() {
                    app.net_join(&r.code);
                }
            }
        } else if app.net.as_ref().map_or(false, |n| n.sock.state() == 1) {
            ui.label(RichText::new("No open matches right now. Host one, or ask a friend for their code.").color(MUTED).size(13.0));
        }
        return;
    }
    // in a room: the lobby
    let (code, host, members, my_id, open) = {
        let n = app.net.as_ref().unwrap();
        (n.code.clone(), n.host(), n.members.clone(), n.id, n.open)
    };
    if app.net.as_ref().map_or(false, |n| n.reconnect.is_some()) {
        ui.label(RichText::new("Connection lost. Getting you back in…").color(WARN).size(14.0));
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("CODE").size(12.0).color(MUTED));
        ui.label(RichText::new(code.clone()).font(anton(30.0)).color(GOLD));
        copy_btn(app, ui, &code);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if small_btn(ui, "Leave").clicked() {
                app.leave_online();
            }
        });
    });
    let status = app.net.as_ref().map(|n| n.status.clone()).unwrap_or_default();
    let my_team = members.iter().find(|m| m.id == my_id).map_or(0, |m| m.team);
    let full = |t: u8| members.iter().filter(|m| m.team == t).count() >= net::TEAM_MAX;
    // two cards side by side, always the same size: three rows each, a team has three players
    let cw = ((iw - 8.0) / 2.0).floor();
    let mut switch = false;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for t in 0..2u8 {
            let other = t != my_team;
            let joinable = other && !full(t);
            let (r, resp) = ui.allocate_exact_size(vec2(cw, LOBBY_HEAD + 3.0 * LOBBY_ROW + 8.0), if joinable { Sense::click() } else { Sense::hover() });
            if resp.clicked() {
                switch = true;
            }
            team_card(ui.painter(), r, t, app.game.teams[t as usize].name, &members, &status, my_id, host_id(app), joinable, resp.hovered());
        }
    });
    if switch {
        app.net_set_team(1 - my_team);
    }
    ui.horizontal(|ui| {
        if !full(1 - my_team) && small_btn(ui, "Switch team").clicked() {
            app.net_set_team(1 - my_team);
        }
        if small_btn(ui, &format!("{FRIENDS} Invite friends")).clicked() {
            app.sheet = Sheet::Friends;
        }
    });
    if host {
        arena_picker(app, ui, iw);
        let mut o = open;
        if toggle(ui, &mut o, "Open to anyone (shows in Find)", iw) {
            app.net_open(o);
        }
        if big(ui, "START MATCH", true, iw).clicked() {
            app.net_start();
        }
    } else {
        let arena = ArenaKey::from_id(&app.net.as_ref().unwrap().arena);
        ui.label(RichText::new(format!("{} · waiting for the host to start", arena.name())).color(MUTED));
    }
}

const LOBBY_HEAD: f32 = 34.0;
const LOBBY_ROW: f32 = 40.0;
fn host_id(app: &FlunkiApp) -> u32 {
    app.net.as_ref().map_or(0, |n| n.host_id)
}
/// `s` cut short with an ellipsis so it fits in `w`.
fn fit(p: &egui::Painter, s: &str, f: FontId, w: f32) -> String {
    if p.layout_no_wrap(s.to_string(), f.clone(), Color32::WHITE).size().x <= w {
        return s.to_string();
    }
    let cs: Vec<char> = s.chars().collect();
    for n in (1..cs.len()).rev() {
        let t: String = cs[..n].iter().collect::<String>().trim_end().to_string() + "…";
        if p.layout_no_wrap(t.clone(), f.clone(), Color32::WHITE).size().x <= w {
            return t;
        }
    }
    "…".into()
}
/// One team in the lobby: its name and how full it is, then a row per person (shirt, name, level,
/// host, connection) and what fills the empty spots. A long name is cut short, never widens the card.
#[allow(clippy::too_many_arguments)]
fn team_card(p: &egui::Painter, r: Rect, t: u8, name: &str, members: &[net::Member], status: &[(u32, u16, u8)], me: u32, host: u32, joinable: bool, hot: bool) {
    let accent = if t == 0 { GOLD } else { BLUE };
    let hs: Vec<&net::Member> = members.iter().filter(|m| m.team == t).collect();
    let mine = hs.iter().any(|m| m.id == me);
    p.rect_filled(r, 14.0, if hot { Color32::from_white_alpha(22) } else { CARD });
    p.rect_stroke(r, 14.0, Stroke::new(if mine { 1.5 } else { 1.0 }, if mine { accent } else { Color32::from_white_alpha(24) }), StrokeKind::Inside);
    // the head: team colour, name, 2/3
    let count = format!("{}/{}", hs.len(), net::TEAM_MAX);
    let cg = p.layout_no_wrap(count, bold(12.0), MUTED);
    let hx = r.left() + 12.0;
    let hy = r.top() + LOBBY_HEAD / 2.0 + 2.0;
    p.circle_filled(pos2(hx + 4.0, hy), 4.0, accent);
    let nw = r.width() - 34.0 - cg.size().x;
    p.text(pos2(hx + 14.0, hy), Align2::LEFT_CENTER, fit(p, &name.to_uppercase(), bold(13.0), nw), bold(13.0), accent);
    p.galley(pos2(r.right() - 10.0 - cg.size().x, hy - cg.size().y / 2.0), cg, MUTED);
    // people with players: a short team shares the spares, see `owner_of`
    let here: Vec<&&net::Member> = hs.iter().filter(|m| !m.away).collect();
    for k in 0..3 {
        let row = Rect::from_min_size(pos2(r.left() + 6.0, r.top() + LOBBY_HEAD + k as f32 * LOBBY_ROW), vec2(r.width() - 12.0, LOBBY_ROW - 4.0));
        let c = pos2(row.left() + 17.0, row.center().y);
        let tx = row.left() + 34.0;
        if let Some(m) = hs.get(k) {
            let you = m.id == me;
            if you {
                p.rect_filled(row, 10.0, Color32::from_rgba_unmultiplied(0xf2, 0xb6, 0x32, 22));
            }
            // their shirt, with their initial on it
            p.circle_filled(c, 12.0, hex_col(save::SWATCHES[m.look[0] as usize % 8]));
            p.circle_stroke(c, 12.0, Stroke::new(1.5, Color32::from_black_alpha(80)));
            let ini: String = m.name.chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
            p.text(c, Align2::CENTER_CENTER, ini, bold(12.0), Color32::WHITE);
            // the connection, a dot on the shirt
            let conn = status.iter().find(|s| s.0 == m.id).map_or(net::CONN_GOOD, |s| s.2);
            let dot = if m.away || conn == net::CONN_AWAY { BAD } else if conn == net::CONN_SLOW { WARN } else { GOOD };
            p.circle_filled(c + vec2(9.0, 9.0), 4.0, CARD);
            p.circle_filled(c + vec2(9.0, 9.0), 2.8, dot);
            let tw = row.right() - tx - 4.0;
            let crown = if m.id == host { format!("{CROWN} ") } else { String::new() };
            let nm = fit(p, &format!("{crown}{}", m.name), bold(14.0), tw);
            p.text(pos2(tx, row.top() + 3.0), Align2::LEFT_TOP, nm, bold(14.0), if m.away { MUTED } else { Color32::WHITE });
            let plays = if m.away { 0 } else { (0..3).filter(|i| here.get(i % here.len().max(1)).map_or(false, |x| x.id == m.id)).count() };
            let mut sub = vec![format!("LV {}", m.look[6].max(1))];
            if you {
                sub.insert(0, "You".into());
            }
            if m.away {
                sub = vec!["reconnecting…".into()];
            } else if plays > 1 {
                sub.push(format!("plays {plays}"));
            }
            let sc = if m.away { WARN } else if you { GOLD } else { MUTED };
            p.text(pos2(tx, row.top() + 20.0), Align2::LEFT_TOP, fit(p, &sub.join(" · "), FontId::proportional(11.0), tw), FontId::proportional(11.0), sc);
        } else {
            // an empty spot
            let stroke = Stroke::new(1.0, Color32::from_white_alpha(if joinable && hot { 70 } else { 28 }));
            let pts = [row.left_top(), row.right_top(), row.right_bottom(), row.left_bottom(), row.left_top()];
            for w in pts.windows(2) {
                p.extend(egui::Shape::dashed_line(&[w[0], w[1]], stroke, 4.0, 4.0));
            }
            let (txt, col) = if hs.is_empty() && k == 0 {
                ("Computer plays".to_string(), MUTED)
            } else if joinable && k == hs.len() {
                ("+ Tap to join".to_string(), if hot { Color32::WHITE } else { accent })
            } else {
                ("Open spot".to_string(), Color32::from_white_alpha(90))
            };
            p.text(row.center(), Align2::CENTER_CENTER, fit(p, &txt, FontId::proportional(12.0), row.width() - 8.0), FontId::proportional(12.0), col);
        }
    }
}

// ---------- in a match ----------
fn hud(app: &mut FlunkiApp, ctx: &egui::Context, p: &egui::Painter, sr: Rect) {
    let g = &app.game;
    let (w, h) = (sr.width(), sr.height());
    let drill = g.drill_hud();
    if let Some((k, v)) = &drill {
        let gal = p.layout_no_wrap(v.clone(), anton(22.0), Color32::WHITE);
        let r = Rect::from_center_size(pos2(sr.center().x, sr.top() + 42.0), vec2(gal.size().x.max(120.0) + 36.0, 58.0));
        p.rect_filled(r, 16.0, PANEL);
        p.text(r.center_top() + vec2(0.0, 8.0), Align2::CENTER_TOP, k.to_uppercase(), FontId::proportional(11.0), GOOD);
        p.galley(pos2(r.center().x - gal.size().x / 2.0, r.top() + 22.0), gal, Color32::WHITE);
    } else {
        // scoreboard
        let pw = ((w - 24.0 - 64.0) / 2.0).min(230.0);
        for t in 0..2 {
            let tm = &g.teams[t];
            let x0 = if t == 0 { sr.left() + 12.0 } else { sr.right() - 12.0 - pw };
            let r = Rect::from_min_size(pos2(x0, sr.top() + 10.0), vec2(pw, 62.0));
            p.rect_filled(r, 14.0, PANEL);
            let accent = if t == 0 { GOLD } else { BLUE };
            if g.turn == t && !matches!(g.state, State::Over | State::Menu) {
                p.rect_stroke(r, 14.0, Stroke::new(2.0, accent), StrokeKind::Inside);
            }
            let you = g.my_team == Some(t) && g.mode != Mode::Hot;
            let name = if you { format!("{} · you", tm.name) } else { tm.name.to_string() };
            p.text(r.left_top() + vec2(12.0, 8.0), Align2::LEFT_TOP, name, FontId::proportional(13.0), accent);
            let cw = ((pw - 24.0) / 3.0 - 8.0).min(52.0);
            for i in 0..3 {
                let b = tm.beers[i];
                let cr = Rect::from_min_size(pos2(r.left() + 12.0 + i as f32 * (cw + 8.0), r.top() + 30.0), vec2(cw, 20.0));
                p.rect_filled(cr, 6.0, Color32::from_white_alpha(20));
                let frac = if b <= 0.0 {
                    0.0
                } else {
                    let f = b - (b - 1e-6).floor();
                    if f <= 1e-6 {
                        1.0
                    } else {
                        f
                    }
                };
                if frac > 0.0 {
                    p.rect_filled(Rect::from_min_size(cr.min, vec2(cr.width() * frac.min(1.0), cr.height())), 6.0, if b > 1.0 { BAD } else { Color32::from_rgb(0xe8, 0xc0, 0x4a) });
                }
                if tm.out[i] {
                    p.text(cr.center(), Align2::CENTER_CENTER, CHECK, FontId::proportional(13.0), GOOD);
                } else if b > 1.0 {
                    p.text(cr.center(), Align2::CENTER_CENTER, format!("+{}", (b - 1e-6).floor() as i32), FontId::proportional(12.0), Color32::WHITE);
                }
            }
            for k in 0..tm.straf {
                p.circle_filled(pos2(r.right() - 12.0 - k as f32 * 10.0, r.top() + 14.0), 3.5, BAD);
            }
        }
        let st = g.status_text();
        if !st.is_empty() {
            let gal = p.layout_no_wrap(st, FontId::proportional(15.0), Color32::WHITE);
            let r = Rect::from_center_size(pos2(sr.center().x, sr.top() + 98.0), gal.size() + vec2(24.0, 10.0));
            p.rect_filled(r, 12.0, PANEL);
            p.galley(r.min + vec2(12.0, 5.0), gal, Color32::WHITE);
        }
    }
    nametags(app, p, sr);
    // speech bubbles over heads
    for b in &app.bubbles {
        let head = app.heads[b.t][b.i];
        let behind = app.cam.pos.distance(head) < 1.2;
        let fr = full_rect();
        let on_screen = app.cam.world_to_screen(head).map(|(x, y)| pos2(fr.left() + x * fr.width(), fr.top() + y * fr.height())).filter(|p| sr.contains(*p));
        let Some(pos) = on_screen.filter(|_| !behind) else {
            // your own player is behind the camera while you aim: show it as a toast instead
            if g.my_players[b.t][b.i] {
                let a = (b.life / 0.35).min(1.0);
                let gal = p.layout_no_wrap(format!("You: {}", voice::QM[b.msg % 9]), FontId::proportional(15.0), fade(INK, a));
                let r = Rect::from_center_size(pos2(sr.center().x, sr.bottom() + HINT_Y - 34.0), gal.size() + vec2(22.0, 12.0));
                p.rect_filled(r, 14.0, fade(Color32::from_rgb(0xfd, 0xfa, 0xf0), a));
                p.galley(r.min + vec2(11.0, 6.0), gal, INK);
            }
            continue;
        };
        let a = (b.life / 0.35).min(1.0);
        let k = ((2.6 - b.life) / 0.15).min(1.0);
        let txt = voice::QM[b.msg % 9];
        let gal = p.layout_no_wrap(txt.into(), FontId::proportional(15.0), fade(INK, a));
        let r = Rect::from_center_size(pos - vec2(0.0, 22.0 + gal.size().y / 2.0), gal.size() + vec2(20.0, 12.0));
        p.rect_filled(r, 12.0, fade(Color32::from_rgb(0xfd, 0xfa, 0xf0), a));
        p.add(egui::Shape::convex_polygon(vec![pos2(pos.x - 7.0, r.bottom() - 1.0), pos2(pos.x + 7.0, r.bottom() - 1.0), pos2(pos.x, r.bottom() + 9.0)], fade(Color32::from_rgb(0xfd, 0xfa, 0xf0), a), Stroke::NONE));
        p.galley(r.min + vec2(10.0, 6.0), gal, INK);
    }
    // banner
    if let Some((b, age)) = &app.banner {
        let a = (age / 0.12).min(1.0) * ((b.dur + 0.35 - age) / 0.35).min(1.0);
        // the banner drops in from slightly above; the font size itself never animates, because every
        // new size makes egui rasterise the whole glyph set again (that was the stutter on every HIT!)
        let k = (age / 0.22).min(1.0);
        let drop = (1.0 - k) * (1.0 - k) * 18.0;
        let y = sr.top() + h * 0.27 - drop;
        let col = if b.straf { BAD } else { Color32::WHITE };
        let ts = banner_size(w);
        shadow_text(p, pos2(sr.center().x, y), Align2::CENTER_CENTER, &b.title, anton(ts), fade(col, a));
        if !b.sub.is_empty() {
            let gal = p.layout_no_wrap(b.sub.clone(), FontId::proportional(16.0), fade(Color32::WHITE, a));
            let r = Rect::from_center_size(pos2(sr.center().x, y + ts * 0.62 + 10.0), gal.size() + vec2(22.0, 10.0));
            p.rect_filled(r, 12.0, fade(PANEL, a));
            p.galley(r.min + vec2(11.0, 5.0), gal, Color32::WHITE);
        }
    }
    net_hud(app, p, sr);
    let g = &app.game;
    // aim hint
    if g.local_aiming() && app.screen == Screen::Playing {
        let armed = app.flick.as_ref().map_or(0, |f| f.armed);
        let txt = match armed {
            1 => "Curveball: bends left, aim right",
            -1 => "Curveball: bends right, aim left",
            _ => "Flick up to throw · spin in circles for a curveball",
        };
        hint_pill(p, pos2(sr.center().x, sr.bottom() + HINT_Y), txt.to_string(), armed != 0);
    } else if g.state == State::Aim && g.mode == Mode::Online && !g.my_players[g.turn][g.teams[g.turn].idx] {
        let who = app.net.as_ref().and_then(|n| n.owner(g.turn, g.teams[g.turn].idx)).map(|m| m.name.clone()).unwrap_or_default();
        if !who.is_empty() {
            hint_pill(p, pos2(sr.center().x, sr.bottom() + HINT_Y), format!("{who} is aiming…"), false);
        }
    } else if let Some(txt) = team_hint(app) {
        hint_pill(p, pos2(sr.center().x, sr.bottom() + HINT_Y), txt, false);
    }
    if let Some((txt, _)) = &app.hint {
        let gal = p.layout_no_wrap(txt.clone(), FontId::proportional(15.0), Color32::WHITE);
        let r = Rect::from_center_size(pos2(sr.center().x, sr.top() + h * 0.42), gal.size() + vec2(22.0, 12.0));
        p.rect_filled(r, 12.0, PANEL);
        p.galley(r.min + vec2(11.0, 6.0), gal, Color32::WHITE);
    }
    controls(app, p, sr);
    if let Some(t) = app.game.tut {
        coach(app, ctx, sr, t);
    }
    match app.menu {
        MatchMenu::Chat => qm_panel(app, ctx, sr),
        MatchMenu::Emotes => emote_panel(app, ctx, sr),
        MatchMenu::None => {}
    }
    if app.screen == Screen::Playing {
        egui::Area::new(egui::Id::new("pausebtn")).pivot(Align2::CENTER_TOP).fixed_pos(anc(Align2::CENTER_TOP, vec2(0.0, if drill.is_some() { 80.0 } else { 21.0 }))).show(ctx, |ui| {
            let b = egui::Button::new("").fill(PANEL).corner_radius(CornerRadius::same(20)).min_size(vec2(44.0, 40.0));
            let r = ui.add(b);
            // two bars, drawn: not every phone's fonts have a pause sign
            let c = r.rect.center();
            for dx in [-4.5, 4.5] {
                ui.painter().rect_filled(Rect::from_center_size(c + vec2(dx, 0.0), vec2(5.0, 15.0)), 1.5, ui.visuals().text_color());
            }
            if r.clicked() {
                app.screen = Screen::Paused;
            }
        });
    }
}

/// Online, with teammates: when it's their runner or their beer and not yours, say so instead of nothing.
fn team_hint(app: &FlunkiApp) -> Option<String> {
    let g = &app.game;
    let n = app.net.as_ref().filter(|_| g.mode == Mode::Online && app.screen == Screen::Playing)?;
    let t = g.my_team?;
    let names = |is: &[usize]| {
        let mut v: Vec<String> = Vec::new();
        for &i in is {
            if let Some(m) = n.owner(t, i).filter(|m| m.id != n.id) {
                if !v.contains(&m.name) {
                    v.push(m.name.clone());
                }
            }
        }
        v.join(" & ")
    };
    if g.state == State::Run && g.hitter == Some(1 - t) && g.run_phase_active() && !app.show_pads(t) {
        let (_, a, b) = g.runners()?;
        let who = names(&[Some(a), b].into_iter().flatten().collect::<Vec<_>>());
        return (!who.is_empty()).then(|| format!("{who} {} · cheer them on!", if who.contains('&') { "run" } else { "runs" }));
    }
    if g.false_start_window(t) && !app.show_pads(t) {
        let tm = &g.teams[t];
        let who = names(&[Some(tm.runner_idx()), tm.runner2_idx()].into_iter().flatten().collect::<Vec<_>>());
        return (!who.is_empty()).then(|| format!("If they hit, {who} {}", if who.contains('&') { "run" } else { "runs" }));
    }
    if g.can_drink(t) && !app.show_drink(t) {
        return Some("Your beer is empty · your teammates drink on".into());
    }
    None
}

/// Online: a name over every player, so you can see who plays whom and which ones are yours. The name
/// goes with its person: someone who drops out is covered by a teammate (their name) or the computer.
fn nametags(app: &FlunkiApp, p: &egui::Painter, sr: Rect) {
    let g = &app.game;
    let Some(n) = app.net.as_ref().filter(|_| g.mode == Mode::Online && g.mirror && app.screen == Screen::Playing) else { return };
    let fr = full_rect();
    // where each head is on screen, first: a name is never wider than the room to its neighbours
    let mut at: Vec<(usize, usize, Pos2)> = Vec::new();
    for t in 0..2 {
        for i in 0..3 {
            let head = app.heads[t][i];
            // your own thrower is right behind the camera while you aim
            if !g.teams[t].players[i].visible || app.bubbles.iter().any(|b| b.t == t && b.i == i) || app.cam.pos.distance(head) < 1.6 {
                continue;
            }
            if let Some(q) = app.cam.world_to_screen(head).map(|(x, y)| pos2(fr.left() + x * fr.width(), fr.top() + y * fr.height())).filter(|q| sr.contains(*q)) {
                at.push((t, i, q));
            }
        }
    }
    for &(t, i, pos) in &at {
        let room = at.iter().filter(|o| (o.0, o.1) != (t, i) && (o.2.y - pos.y).abs() < 22.0).map(|o| (o.2.x - pos.x).abs() - 6.0).fold(150.0f32, f32::min).max(30.0);
        let accent = if t == 0 { GOLD } else { BLUE };
        let mine = g.my_players[t][i];
        let (txt, fill, col) = match n.owner(t, i) {
            _ if mine => ("YOU".to_string(), GOLD, INK),
            Some(m) => (m.name.clone(), PANEL, Color32::WHITE),
            None => ("CPU".to_string(), Color32::from_black_alpha(150), MUTED),
        };
        // done: off the pitch, the name stays but steps back
        let a = if g.teams[t].out[i] { 0.45 } else { 1.0 };
        let pad = if mine { 14.0 } else { 20.0 };
        let gal = p.layout_no_wrap(fit(p, &txt, bold(13.0), room - pad), bold(13.0), fade(col, a));
        let r = Rect::from_center_size(pos - vec2(0.0, 14.0 + gal.size().y / 2.0), gal.size() + vec2(pad, 6.0));
        p.rect_filled(r, r.height() / 2.0, fade(fill, a));
        if !mine {
            // the team's colour, as a dot in front of the name
            p.circle_filled(r.left_center() + vec2(7.0, 0.0), 3.0, fade(accent, a));
        }
        p.add(egui::Shape::convex_polygon(vec![pos2(pos.x - 5.0, r.bottom() - 1.0), pos2(pos.x + 5.0, r.bottom() - 1.0), pos2(pos.x, r.bottom() + 5.0)], fade(fill, a), Stroke::NONE));
        p.galley(r.min + vec2(if mine { 7.0 } else { 13.0 }, 3.0), gal, fade(col, a));
    }
}

/// Every line of advice sits in the same place above the controls.
const HINT_Y: f32 = -222.0;
fn hint_pill(p: &egui::Painter, c: Pos2, txt: String, warn: bool) {
    let gal = p.layout_no_wrap(txt, FontId::proportional(14.0), if warn { INK } else { Color32::WHITE });
    let r = Rect::from_center_size(c, gal.size() + vec2(22.0, 10.0));
    p.rect_filled(r, 12.0, if warn { WARN } else { PANEL });
    p.galley(r.min + vec2(11.0, 5.0), gal, Color32::WHITE);
}

/// The connection, always in view during an online match: a small ping chip, and in words whenever
/// something is wrong with it, whether that's your connection or someone else's.
fn net_hud(app: &FlunkiApp, p: &egui::Painter, sr: Rect) {
    let g = &app.game;
    let Some(n) = app.net.as_ref().filter(|_| g.mirror) else { return };
    let link = n.link(g.pred.snap_age, true);
    let (dot, ms) = match link {
        Link::Good => (GOOD, format!("{:.0} ms", n.rtt * 1000.0)),
        Link::Slow => (WARN, format!("{:.0} ms", n.rtt * 1000.0)),
        Link::Stalled => (WARN, "…".to_string()),
        Link::Reconnecting => (BAD, "offline".to_string()),
    };
    let gal = p.layout_no_wrap(ms, FontId::proportional(11.0), Color32::WHITE);
    let r = Rect::from_center_size(pos2(sr.center().x, sr.top() + 72.0), gal.size() + vec2(26.0, 6.0));
    p.rect_filled(r, 8.0, PANEL);
    p.circle_filled(r.left_center() + vec2(9.0, 0.0), 3.5, dot);
    p.galley(r.min + vec2(18.0, 3.0), gal, Color32::WHITE);
    // what's the matter, in words
    let pause = g.net_pause.map(|(who, left)| (n.name_of(who), left));
    let (title, sub, col): (String, String, Color32) = match (&link, pause) {
        (Link::Reconnecting, _) => {
            let tries = n.reconnect.as_ref().map_or(0, |r| r.tries);
            ("Connection lost".into(), format!("Reconnecting… try {tries}. The match waits a little for you."), BAD)
        }
        (_, Some((who, left))) if left > 2.0 || n.members.iter().any(|m| m.name == who && m.away) => (format!("Waiting for {who}"), format!("Their connection dropped · going on in {left:.0} s"), WARN),
        (_, Some(_)) => ("Get ready".into(), "The match goes on…".into(), GOOD),
        (Link::Stalled, None) => ("Connection trouble".into(), format!("Nothing from the server for {:.0} s", g.pred.snap_age), WARN),
        (Link::Slow, None) => (String::new(), "Slow connection: things may lag a little".into(), WARN),
        (Link::Good, None) => {
            let slow: Vec<String> = n.status.iter().filter(|(id, _, c)| *c == net::CONN_SLOW && *id != n.id).map(|(id, _, _)| n.name_of(*id)).collect();
            let away: Vec<String> = n.members.iter().filter(|m| m.away).map(|m| m.name.clone()).collect();
            if !away.is_empty() {
                (String::new(), format!("{} reconnecting…", away.join(", ")), WARN)
            } else if !slow.is_empty() {
                (String::new(), format!("{} has a slow connection", slow.join(", ")), MUTED)
            } else {
                (String::new(), String::new(), MUTED)
            }
        }
    };
    if !title.is_empty() {
        let tg = p.layout_no_wrap(title, anton(28.0), col);
        let sg = p.layout_no_wrap(sub, FontId::proportional(14.0), Color32::WHITE);
        let w = tg.size().x.max(sg.size().x) + 32.0;
        let box_r = Rect::from_center_size(pos2(sr.center().x, sr.top() + sr.height() * 0.40), vec2(w, 72.0));
        p.rect_filled(box_r, 14.0, PANEL);
        p.galley(pos2(box_r.center().x - tg.size().x / 2.0, box_r.top() + 8.0), tg, col);
        p.galley(pos2(box_r.center().x - sg.size().x / 2.0, box_r.top() + 46.0), sg, Color32::WHITE);
    } else if !sub.is_empty() {
        let gal = p.layout_no_wrap(sub, FontId::proportional(13.0), col);
        let r = Rect::from_center_size(pos2(sr.center().x, sr.top() + 126.0), gal.size() + vec2(18.0, 6.0));
        p.rect_filled(r, 8.0, PANEL);
        p.galley(r.min + vec2(9.0, 3.0), gal, col);
    }
    // an idle thrower's clock
    if g.state == State::Aim && g.net_aim_left >= 0.0 && g.net_aim_left < 10.0 {
        let gal = p.layout_no_wrap(format!("{:.0}", g.net_aim_left.ceil()), anton(26.0), BAD);
        let c = pos2(sr.right() - 34.0, sr.top() + 100.0);
        p.circle_filled(c, 22.0, PANEL);
        p.galley(c - gal.size() / 2.0, gal, BAD);
    }
}

fn controls(app: &FlunkiApp, p: &egui::Painter, sr: Rect) {
    let g = &app.game;
    for c in app.controls(sr) {
        match c.kind {
            CtlKind::Drink(t) => {
                let Some(k) = app.lead(t) else { continue };
                let d = &g.teams[t].dc[k];
                let cen = c.rect.center();
                let rad = c.rect.width() * 0.5;
                let stop_now = !g.can_drink(t) && d.stop_show > 0.0;
                let fill = if stop_now { BAD } else if d.cough > 0.0 { Color32::from_rgb(0x8a, 0x5a, 0x3a) } else if d.hold { Color32::from_rgb(0xd9, 0x9a, 0x1e) } else { GOLD };
                p.circle_filled(cen + vec2(0.0, 4.0), rad, Color32::from_black_alpha(90));
                p.circle_filled(cen, rad * if d.hold { 0.96 } else { 1.0 }, fill);
                let gc = if d.p < 0.6 { GOOD } else if d.p < 0.85 { WARN } else { BAD };
                p.circle_stroke(cen, rad + 9.0, Stroke::new(7.0, Color32::from_black_alpha(110)));
                if d.p > 0.005 {
                    let n = 40;
                    let pts: Vec<Pos2> = (0..=n)
                        .map(|i| {
                            let a = -std::f32::consts::FRAC_PI_2 + d.p.min(1.0) * std::f32::consts::TAU * i as f32 / n as f32;
                            cen + vec2(a.cos(), a.sin()) * (rad + 9.0)
                        })
                        .collect();
                    p.add(egui::Shape::line(pts, Stroke::new(7.0, gc)));
                }
                let (m1, m2) = if stop_now {
                    ("STOP", "let go!")
                } else if d.cough > 0.0 {
                    ("COUGH", "spilled")
                } else if d.hold {
                    (if d.p > 0.8 { "LET GO" } else { "GLUG" }, "breathe soon")
                } else {
                    ("HOLD", if g.mode == Mode::Hot { g.teams[t].name } else { "to drink" })
                };
                p.text(cen + vec2(0.0, -8.0), Align2::CENTER_CENTER, m1, anton(30.0), if stop_now { Color32::WHITE } else { INK });
                p.text(cen + vec2(0.0, 20.0), Align2::CENTER_CENTER, m2, FontId::proportional(12.0), if stop_now { Color32::WHITE } else { INK });
            }
            CtlKind::Pad(t, f) => {
                let Some(k) = app.lead(t) else { continue };
                let rc = &g.teams[t].rc[k];
                let wait = g.false_start_window(t);
                let next = !wait && ((f == Foot::L && rc.last != Some(Foot::L)) || (f == Foot::R && rc.last == Some(Foot::L)));
                let down = rc.down[f as usize] > 0.0;
                let r = if down { c.rect.shrink(4.0) } else { c.rect };
                p.rect_filled(r.translate(vec2(0.0, 4.0)), 18.0, Color32::from_black_alpha(90));
                let base = if rc.lock > 0.0 { Color32::from_gray(70) } else if wait { Color32::from_rgb(0x3a, 0x44, 0x40) } else if next { Color32::from_rgb(0x2f, 0x86, 0xb8) } else { Color32::from_rgb(0x24, 0x4a, 0x60) };
                p.rect_filled(r, 18.0, base);
                if next {
                    p.rect_stroke(r, 18.0, Stroke::new(3.0, Color32::from_rgb(0xbf, 0xe8, 0xff)), StrokeKind::Inside);
                }
                p.text(r.center(), Align2::CENTER_CENTER, if f == Foot::L { "L" } else { "R" }, anton(44.0), Color32::WHITE);
                if f == Foot::R {
                    let hint = if rc.lock > 0.0 {
                        "Autoclicker blocked".to_string()
                    } else if wait {
                        "Wait for the hit…".to_string()
                    } else if rc.msg_t > 0.0 {
                        rc.msg.clone()
                    } else if g.mode == Mode::Hot {
                        format!("{}: run!", g.teams[t].name)
                    } else {
                        "Tap L and R in turn".to_string()
                    };
                    hint_pill(p, pos2(c.rect.left() - 5.0, sr.bottom() + HINT_Y), hint, false);
                }
            }
            CtlKind::Sneak => {
                let prog = g.sneak_progress();
                let (cen, rad) = (c.rect.center(), c.rect.width() * 0.5);
                p.circle_filled(cen, rad, if prog.is_some() { Color32::from_rgb(0x3c, 0x2a, 0x5c) } else { PANEL });
                p.circle_stroke(cen, rad - 2.0, Stroke::new(3.0, Color32::from_rgba_unmultiplied(0xc8, 0xa8, 0xff, 70)));
                if let Some(k) = prog {
                    let n = 32;
                    let pts: Vec<Pos2> = (0..=n)
                        .map(|i| {
                            let a = -std::f32::consts::FRAC_PI_2 + k.min(1.0) * std::f32::consts::TAU * i as f32 / n as f32;
                            cen + vec2(a.cos(), a.sin()) * (rad - 2.0)
                        })
                        .collect();
                    p.add(egui::Shape::line(pts, Stroke::new(3.5, Color32::from_rgb(0xc8, 0xa8, 0xff))));
                }
                let (l1, l2) = if prog.is_some() { ("Sipping", "stay low") } else { ("Sneak", "sip") };
                p.text(cen + vec2(0.0, -7.0), Align2::CENTER_CENTER, l1, FontId::proportional(15.0), Color32::WHITE);
                p.text(cen + vec2(0.0, 10.0), Align2::CENTER_CENTER, l2, FontId::proportional(12.0), Color32::from_white_alpha(190));
            }
            CtlKind::Qm | CtlKind::Emote => {
                p.circle_filled(c.rect.center(), 26.0, PANEL);
                p.circle_stroke(c.rect.center(), 26.0, Stroke::new(1.0, Color32::from_white_alpha(60)));
                p.text(c.rect.center(), Align2::CENTER_CENTER, if matches!(c.kind, CtlKind::Qm) { CHAT } else { EMOTE }, FontId::proportional(22.0), Color32::WHITE);
            }
        }
    }
}

/// A button in a quick menu; a locked one says at what level it comes, and only tells you that when tapped.
fn quick_btn(ui: &mut Ui, text: String, size: f32, need: Option<u32>, fill: Color32, col: Color32, min: egui::Vec2) -> egui::Response {
    let (text, fill, col, size) = match need {
        Some(lv) => (format!("{}\n{LOCK} Level {lv}", text.rsplit('\n').next().unwrap_or("")), Color32::from_white_alpha(14), MUTED, 12.0),
        None => (text, fill, col, size),
    };
    ui.add(egui::Button::new(RichText::new(text).size(size).color(col)).fill(fill).corner_radius(CornerRadius::same(10)).min_size(min))
}
fn quick_need(app: &FlunkiApp, u: Unlock) -> Option<u32> {
    (!app.save.unlocked(u)).then(|| save::unlock_lv(u))
}
const MENU_ABOVE: f32 = -200.0;

fn emote_panel(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let w = (sr.width() - 24.0).min(380.0);
    egui::Area::new(egui::Id::new("emotepanel")).order(egui::Order::Foreground).pivot(Align2::LEFT_BOTTOM).fixed_pos(anc(Align2::LEFT_BOTTOM, vec2(12.0, MENU_ABOVE))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.set_width(w - 32.0);
            label_caps(ui, "Emotes");
            let ew = (w - 32.0 - 30.0) / 4.0;
            for (r, row) in crate::anim::EMOTES.chunks(4).enumerate() {
                ui.horizontal(|ui| {
                    for (k, (name, icon, _)) in row.iter().enumerate() {
                        let i = r * 4 + k;
                        let need = quick_need(app, Unlock::Emote(i));
                        if quick_btn(ui, format!("{icon}\n{name}"), 13.0, need, CARD, Color32::WHITE, vec2(ew, 56.0)).clicked() {
                            match need {
                                Some(lv) => app.hint = Some((format!("{name} unlocks at level {lv}"), 1.6)),
                                None => app.send_emote(i),
                            }
                        }
                    }
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_btn(ui, "Close").clicked() {
                    app.menu = MatchMenu::None;
                }
            });
        });
    });
}

fn qm_panel(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let w = (sr.width() - 24.0).min(380.0);
    egui::Area::new(egui::Id::new("qmpanel")).order(egui::Order::Foreground).pivot(Align2::LEFT_BOTTOM).fixed_pos(anc(Align2::LEFT_BOTTOM, vec2(12.0, MENU_ABOVE))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.set_width(w - 32.0);
            label_caps(ui, "Quick chat");
            let cw = (w - 32.0 - 20.0) / 3.0;
            for row in 0..3 {
                ui.horizontal(|ui| {
                    for col in 0..3 {
                        let i = row * 3 + col;
                        let need = quick_need(app, Unlock::Quick(i));
                        if quick_btn(ui, voice::QM[i].to_string(), if (6..8).contains(&i) { 22.0 } else { 15.0 }, need, Color32::from_rgb(0xfd, 0xfa, 0xf0), INK, vec2(cw, 44.0)).clicked() {
                            match need {
                                Some(lv) => app.hint = Some((format!("{} unlocks at level {lv}", voice::QM[i]), 1.6)),
                                None => app.send_qm(i),
                            }
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                if app.game.mode == Mode::Online && small_btn(ui, &if app.qm_muted { format!("{SOUND} Unmute others") } else { format!("{MUTE} Mute others") }).clicked() {
                    app.qm_muted = !app.qm_muted;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if small_btn(ui, "Close").clicked() {
                        app.menu = MatchMenu::None;
                    }
                });
            });
        });
    });
}

fn coach(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect, t: Tut) {
    let (title, text) = t.text();
    let w = (sr.width() - 24.0).min(420.0);
    let y = if t.hold() { sr.height() * 0.38 } else { 84.0 };
    egui::Area::new(egui::Id::new("coach")).order(egui::Order::Foreground).pivot(Align2::CENTER_TOP).fixed_pos(anc(Align2::CENTER_TOP, vec2(0.0, y))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.set_width(w - 32.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("TUTORIAL · STEP {} OF 5", t.step())).size(12.0).color(GOOD).font(bold(16.0)));
                if t != Tut::Done {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("Skip").size(13.0)).fill(Color32::TRANSPARENT)).clicked() {
                            app.save.tut_done = true;
                            app.save.store();
                            app.game.tut = None;
                        }
                    });
                }
            });
            ui.label(RichText::new(title).font(anton(24.0)).color(Color32::WHITE));
            ui.label(RichText::new(text).color(MUTED).size(15.0));
            match t {
                Tut::Intro => {
                    if big(ui, "SHOW ME", true, w - 32.0).clicked() {
                        app.game.set_tut(Tut::Throw);
                    }
                }
                Tut::Ref => {
                    if big(ui, "GOT IT", true, w - 32.0).clicked() {
                        app.game.set_tut(Tut::Done);
                        app.finish_tutorial();
                    }
                }
                Tut::Done => {
                    ui.horizontal(|ui| {
                        let hw = (w - 42.0) / 2.0;
                        if big(ui, "REAL MATCH", true, hw).clicked() {
                            app.start(Mode::Cpu);
                        }
                        if big(ui, "KEEP PLAYING", false, hw).clicked() {
                            app.game.tut = None;
                        }
                    });
                }
                _ => {}
            }
        });
    });
}

fn pause(app: &mut FlunkiApp, ctx: &egui::Context, p: &egui::Painter, sr: Rect) {
    let bw = (sr.width() - 48.0).min(320.0);
    p.rect_filled(full_rect(), 0.0, Color32::from_black_alpha(120));
    egui::Area::new(egui::Id::new("pause")).order(egui::Order::Foreground).pivot(Align2::CENTER_CENTER).fixed_pos(anc(Align2::CENTER_CENTER, vec2(0.0, 0.0))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.set_width(bw);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("PAUSED").font(anton(34.0)).color(GOLD));
                if app.game.mode == Mode::Online {
                    ui.label(RichText::new("The match goes on for everyone else").color(MUTED).size(13.0));
                }
                if big(ui, "RESUME", true, bw).clicked() {
                    app.screen = Screen::Playing;
                }
                if big(ui, "SETTINGS", false, bw).clicked() {
                    app.sheet = Sheet::Settings;
                }
                let quit = if app.game.train.is_some() { "BACK TO THE CLUBHOUSE" } else if app.game.mode == Mode::Online { "LEAVE THE MATCH" } else { "QUIT TO MENU" };
                if big(ui, quit, false, bw).clicked() {
                    let v = if app.game.train.is_some() { View::Club } else { app.view };
                    app.go_home(v);
                }
            });
        });
    });
}

fn over(app: &mut FlunkiApp, ctx: &egui::Context, p: &egui::Painter, sr: Rect) {
    p.rect_filled(full_rect(), 0.0, Color32::from_black_alpha(100));
    let g = &app.game;
    let Some(wn) = g.win else { return };
    let hot = g.mode == Mode::Hot;
    let mine = !hot && g.my_team == Some(wn);
    let title = if hot { format!("{} win{}", g.teams[wn].name, if wn == 0 { "!" } else { "s!" }) } else if mine { "You win!".into() } else { format!("{} win", g.teams[wn].name) };
    let rows = [
        ("Hits / throws", format!("{} / {}", g.teams[0].hits, g.teams[0].throws), format!("{} / {}", g.teams[1].hits, g.teams[1].throws)),
        ("Strafbier", g.teams[0].straf.to_string(), g.teams[1].straf.to_string()),
    ];
    let names = (g.teams[0].name, g.teams[1].name);
    let guest = g.mirror && !app.net.as_ref().map_or(false, |n| n.host());
    let online = g.mode == Mode::Online;
    let bw = (sr.width() - 40.0).min(380.0);
    let xp = app.last_xp.clone();
    let mut act = 0;
    egui::Area::new(egui::Id::new("over")).order(egui::Order::Foreground).pivot(Align2::CENTER_CENTER).fixed_pos(anc(Align2::CENTER_CENTER, vec2(0.0, 20.0))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.set_width(bw);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(title).font(anton(38.0)).color(if mine || hot { GOLD } else { Color32::WHITE }));
            });
            egui::Grid::new("stats").num_columns(3).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
                ui.label("");
                ui.label(RichText::new(names.0).color(GOLD).font(bold(16.0)));
                ui.label(RichText::new(names.1).color(BLUE).font(bold(16.0)));
                ui.end_row();
                for (k, a, b) in rows.iter() {
                    ui.label(RichText::new(*k).color(MUTED));
                    ui.label(a);
                    ui.label(b);
                    ui.end_row();
                }
            });
            if let Some(x) = &xp {
                xp_box(ui, x, bw);
            } else if app.save.locked() {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(format!("{LOCK} Logged out: no XP, nothing kept")).color(WARN).size(14.0));
                    if small_btn(ui, "Log in").clicked() {
                        act = 9;
                    }
                });
            }
            ui.vertical_centered(|ui| {
                if guest {
                    ui.label(RichText::new("The host can start the next match").color(MUTED));
                } else if big(ui, "PLAY AGAIN", true, bw).clicked() {
                    act = 1;
                }
                ui.horizontal(|ui| {
                    let hw = (bw - 10.0) / 2.0;
                    if big(ui, "STATS", false, hw).clicked() {
                        act = 2;
                    }
                    if big(ui, if online { "LEAVE" } else { "MENU" }, false, hw).clicked() {
                        act = 3;
                    }
                });
            });
        });
    });
    match act {
        1 => {
            if online {
                app.net_start();
            } else {
                let m = app.game.mode;
                app.start(m);
            }
        }
        2 => app.sheet = Sheet::Stats,
        9 => app.sheet = Sheet::Account,
        3 => {
            let v = app.view;
            app.go_home(v);
        }
        _ => {}
    }
}

fn train_over(app: &mut FlunkiApp, ctx: &egui::Context, p: &egui::Painter, sr: Rect) {
    p.rect_filled(full_rect(), 0.0, Color32::from_black_alpha(110));
    let Some((k, v, note, rec, prev)) = app.train_res.clone() else { return };
    let bw = (sr.width() - 40.0).min(380.0);
    let xp = app.last_xp.clone();
    let mut act = 0;
    egui::Area::new(egui::Id::new("trainover")).order(egui::Order::Foreground).pivot(Align2::CENTER_CENTER).fixed_pos(anc(Align2::CENTER_CENTER, vec2(0.0, 0.0))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.set_width(bw);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(format!("TRAINING · {}", k.name().to_uppercase())).size(12.0).color(GOOD).font(bold(16.0)));
                ui.label(RichText::new(if rec && prev.is_some() { "New record!" } else if rec { "First record set" } else { "Drill done" }).font(anton(34.0)).color(if rec { GOLD } else { Color32::WHITE }));
                ui.label(RichText::new(k.fmt(Some(v))).font(anton(56.0)).color(Color32::WHITE));
                let more = if rec { String::new() } else { format!(" Your record: {}.", k.fmt(prev)) };
                ui.label(RichText::new(format!("{note}{more}")).color(MUTED));
            });
            if let Some(x) = &xp {
                xp_box(ui, x, bw);
            } else if app.save.locked() {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(format!("{LOCK} Logged out: no XP, nothing kept")).color(WARN).size(14.0));
                    if small_btn(ui, "Log in").clicked() {
                        act = 9;
                    }
                });
            }
            ui.horizontal(|ui| {
                let hw = (bw - 10.0) / 2.0;
                if big(ui, "AGAIN", true, hw).clicked() {
                    act = 1;
                }
                if big(ui, "CLUBHOUSE", false, hw).clicked() {
                    act = 2;
                }
            });
        });
    });
    match act {
        1 => app.start_drill(k),
        2 => app.go_home(View::Club),
        9 => app.sheet = Sheet::Account,
        _ => {}
    }
}

// ---------- sheets ----------
fn sheet<R>(ctx: &egui::Context, sr: Rect, id: &str, max_h: f32, add: impl FnOnce(&mut Ui, f32) -> R) {
    let w = (sr.width() - 16.0).min(520.0);
    egui::Area::new(egui::Id::new(id)).order(egui::Order::Foreground).pivot(Align2::CENTER_BOTTOM).fixed_pos(anc(Align2::CENTER_BOTTOM, vec2(0.0, -8.0))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.set_width(w - 32.0);
            egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                add(ui, w - 44.0);
            });
        });
    });
}
// ---------- friends ----------
/// A sheet of a fixed height, for what grows: gives `add` its width and the height to fill.
fn tall_sheet(ctx: &egui::Context, sr: Rect, id: &str, frac: f32, add: impl FnOnce(&mut Ui, f32, f32)) {
    let w = (sr.width() - 16.0).min(520.0);
    let h = sr.height() * frac;
    egui::Area::new(egui::Id::new(id)).order(egui::Order::Foreground).pivot(Align2::CENTER_TOP).fixed_pos(anc(Align2::CENTER_BOTTOM, vec2(0.0, -8.0 - h))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.set_width(w - 32.0);
            ui.set_height(h - 32.0);
            add(ui, w - 44.0, h - 32.0);
        });
    });
}
fn friends_line(app: &FlunkiApp) -> String {
    let so = &app.social;
    let on = so.friends.iter().filter(|f| f.online).count();
    let mut parts = Vec::new();
    if so.unread() > 0 {
        parts.push(format!("{} new", so.unread()));
    }
    if !so.requests.is_empty() {
        parts.push(format!("{} asking", so.requests.len()));
    }
    if parts.is_empty() {
        parts.push(if so.friends.is_empty() { "Add friends with a code".to_string() } else { format!("{on} of {} online", so.friends.len()) });
    }
    parts.join(" · ")
}
fn status_text(f: &crate::net::Friend) -> (&'static str, Color32) {
    if !f.online {
        ("Offline", MUTED)
    } else if f.status == crate::net::STATUS_MATCH {
        ("In a match", WARN)
    } else if f.status == crate::net::STATUS_LOBBY {
        ("In a lobby", GOOD)
    } else {
        ("Online", GOOD)
    }
}
/// Someone asks you to play. Not during your own match: it waits (a while) until that's over.
fn invite_prompt(app: &mut FlunkiApp, ctx: &egui::Context) {
    if matches!(app.screen, Screen::Playing | Screen::Paused) {
        return;
    }
    let Some(i) = app.social.invite.as_ref() else { return };
    let (name, room) = (i.name.clone(), i.room.clone());
    let mut act = 0;
    egui::Area::new(egui::Id::new("invite")).order(egui::Order::Tooltip).pivot(Align2::CENTER_TOP).fixed_pos(anc(Align2::CENTER_TOP, vec2(0.0, 84.0))).show(ctx, |ui| {
        panel().show(ui, |ui| {
            ui.label(RichText::new(format!("{name} invites you")).font(anton(22.0)).color(GOLD));
            ui.label(RichText::new(format!("Match {room}")).color(MUTED).size(14.0));
            ui.horizontal(|ui| {
                if big(ui, "JOIN", true, 120.0).clicked() {
                    act = 1;
                }
                if big(ui, "NOT NOW", false, 120.0).clicked() {
                    act = 2;
                }
            });
        });
    });
    match act {
        1 => app.accept_invite(),
        2 => app.social.invite = None,
        _ => {}
    }
}
fn friends_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    if let Some(code) = app.social.open_chat.clone() {
        chat_sheet(app, ctx, sr, &code);
        return;
    }
    if let Some(code) = app.social.open_stats.clone() {
        pair_stats_sheet(app, ctx, sr, &code);
        return;
    }
    let mut close = false;
    let room = app.net.as_ref().filter(|n| n.in_room && n.phase != crate::net::PHASE_PLAYING).map(|n| n.code.clone());
    enum Act {
        Add(String),
        Remove(String),
        Chat(String),
        Stats(String),
        Invite(String),
        Copy(String),
    }
    let mut act = None;
    tall_sheet(ctx, sr, "friends", 0.72, |ui, w, h| {
        if sheet_head(ui, "Friends") {
            close = true;
        }
        if app.save.locked() {
            login_box(app, ui, w, "Log in to add friends, chat and invite them to a match.");
            return;
        }
        let so = &mut app.social;
        egui::ScrollArea::vertical().id_salt("friendlist").max_height(h - 52.0).auto_shrink([false, false]).show(ui, |ui| {
        if !so.err.is_empty() {
            ui.label(RichText::new(so.err.clone()).color(BAD).size(14.0));
        }
        if !so.ready {
            ui.label(RichText::new(if so.stopped { "Friends aren't available: update the game" } else { "Connecting to the server…" }).color(MUTED));
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("YOUR CODE").size(12.0).color(MUTED));
                ui.label(RichText::new(so.code.clone()).font(anton(26.0)).color(GOLD));
                if small_btn(ui, &format!("{COPY} Copy")).clicked() {
                    act = Some(Act::Copy(so.code.clone()));
                }
            });
            ui.label(RichText::new("Give this to a friend, or add theirs:").color(MUTED).size(13.0));
            ui.horizontal(|ui| {
                text_field(ui, "friendcode", egui::TextEdit::singleline(&mut so.add_in).hint_text("FRIEND CODE").char_limit(crate::net::CODE_LEN).desired_width(w - 110.0).margin(vec2(12.0, 11.0)).font(anton(18.0)));
                so.add_in = so.add_in.to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
                if big(ui, "ADD", false, 90.0).clicked() && !so.add_in.is_empty() {
                    act = Some(Act::Add(std::mem::take(&mut so.add_in)));
                }
            });
        }
        if !so.requests.is_empty() {
            label_caps(ui, "Want to be friends");
            for f in &so.requests {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(f.name.clone()).color(Color32::WHITE).size(16.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if small_btn(ui, "Decline").clicked() {
                            act = Some(Act::Remove(f.code.clone()));
                        }
                        if small_btn(ui, "Accept").clicked() {
                            act = Some(Act::Add(f.code.clone()));
                        }
                    });
                });
            }
        }
        if !so.friends.is_empty() {
            label_caps(ui, "Your friends");
        } else if so.ready && so.requests.is_empty() {
            ui.label(RichText::new("No friends yet. Trade codes and add each other.").color(MUTED).size(14.0));
        }
        let mut list: Vec<_> = so.friends.iter().collect();
        list.sort_by_key(|f| (!f.online, f.name.to_lowercase()));
        for f in list {
            egui::Frame::NONE.fill(CARD).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(w - 20.0);
                ui.horizontal(|ui| {
                    let (txt, col) = status_text(f);
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 5.0, col);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(f.name.clone()).color(Color32::WHITE).size(16.0));
                        ui.label(RichText::new(txt).color(MUTED).size(13.0));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let chat = if f.unread > 0 { format!("Chat ({})", f.unread) } else { "Chat".to_string() };
                        if small_btn(ui, &chat).clicked() {
                            act = Some(Act::Chat(f.code.clone()));
                        }
                        if small_btn(ui, "Stats").clicked() {
                            act = Some(Act::Stats(f.code.clone()));
                        }
                        // offline friends get a notification on their phone
                        if room.is_some() && f.status != crate::net::STATUS_MATCH && small_btn(ui, "Invite").clicked() {
                            act = Some(Act::Invite(f.code.clone()));
                        }
                    });
                });
            });
        }
        if !so.sent.is_empty() {
            label_caps(ui, "Waiting for an answer");
            for f in &so.sent {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(f.name.clone()).color(MUTED).size(15.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if small_btn(ui, "Cancel").clicked() {
                            act = Some(Act::Remove(f.code.clone()));
                        }
                    });
                });
            }
        }
        });
    });
    use crate::net::ToServer as T;
    app.social.err = if act.is_some() { String::new() } else { std::mem::take(&mut app.social.err) };
    match act {
        Some(Act::Add(code)) => app.social.send(T::FriendAdd { code }),
        Some(Act::Remove(code)) => app.social.send(T::FriendRemove { code }),
        Some(Act::Chat(code)) => app.social.open(&code),
        Some(Act::Stats(code)) => app.social.open_stats_for(&code),
        Some(Act::Invite(to)) => {
            let name = app.social.name_of(&to);
            let online = app.social.friends.iter().any(|f| f.code == to && f.online);
            app.social.send(T::Invite { to, room: room.clone().unwrap_or_default() });
            app.hint = Some((if online { format!("Invited {name}") } else { format!("Invited {name}: they'll get a notification") }, 2.0));
        }
        Some(Act::Copy(code)) => {
            copy_text(ctx, &code);
            app.lock_note = Some((format!("{CHECK} Copied {code}"), 1.6));
        }
        None => {}
    }
    if close {
        app.sheet = Sheet::None;
    }
}
/// What you've played with this friend. The server keeps count, so it's the same on any screen.
fn pair_stats_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect, code: &str) {
    let mut back = false;
    let name = app.social.name_of(code);
    let st = app.social.stats.get(code).cloned();
    tall_sheet(ctx, sr, "pairstats", 0.72, |ui, w, h| {
        ui.horizontal(|ui| {
            if small_btn(ui, BACK).clicked() {
                back = true;
            }
            ui.label(RichText::new(format!("You & {name}")).font(anton(24.0)).color(GOLD));
        });
        let Some(st) = st.as_ref() else {
            ui.label(RichText::new(if app.social.err.is_empty() { "Getting the numbers…".to_string() } else { app.social.err.clone() }).color(MUTED));
            return;
        };
        let played = st.together + st.against;
        if played == 0 {
            ui.label(RichText::new(format!("You haven't finished an online match with {name} yet. Matches you play together or against each other from now on show up here.")).color(MUTED).size(14.0));
            return;
        }
        egui::ScrollArea::vertical().id_salt("pairstats").max_height(h - 52.0).auto_shrink([false, false]).show(ui, |ui| {
            let row = |ui: &mut Ui, k: &str, v: String| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(k).color(MUTED).size(15.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(v).color(Color32::WHITE).font(bold(16.0)));
                    });
                });
            };
            let rate = |h: u32, t: u32| if t == 0 { "–".to_string() } else { format!("{h} of {t} ({:.0}%)", h as f32 * 100.0 / t as f32) };
            label_caps(ui, "Against each other");
            if st.against == 0 {
                ui.label(RichText::new("No matches yet").color(MUTED).size(14.0));
            } else {
                row(ui, "You – them", format!("{} – {}", st.my_wins, st.their_wins));
            }
            label_caps(ui, "On the same team");
            if st.together == 0 {
                ui.label(RichText::new("No matches yet").color(MUTED).size(14.0));
            } else {
                row(ui, "Won", format!("{} of {}", st.together_wins, st.together));
            }
            label_caps(ui, "In those matches");
            row(ui, "Your hits", rate(st.my_hits, st.my_throws));
            row(ui, "Their hits", rate(st.their_hits, st.their_throws));
            let m = st.seconds / 60;
            row(ui, "Played", if m >= 60 { format!("{} h {} min", m / 60, m % 60) } else { format!("{m} min") });
            label_caps(ui, "Latest matches");
            for r in &st.recent {
                let (txt, col) = if r.won { ("Won", GOOD) } else { ("Lost", BAD) };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(txt).color(col).font(bold(15.0)));
                    ui.label(RichText::new(format!("{} · {}", if r.together { "same team" } else { "against" }, ArenaKey::from_id(&r.arena).name())).color(Color32::WHITE).size(14.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("hits {} – {}", r.my_hits, r.their_hits)).color(MUTED).size(13.0));
                    });
                });
            }
        });
    });
    if back {
        app.social.open_stats = None;
    }
}
fn chat_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect, code: &str) {
    let mut back = false;
    let mut unfriend = false;
    let name = app.social.name_of(code);
    let me = app.social.code.clone();
    let online = app.social.friends.iter().find(|f| f.code == code).map_or(("", MUTED), status_text);
    let msgs = app.social.chats.get(code).cloned().unwrap_or_default();
    tall_sheet(ctx, sr, "chat", 0.72, |ui, w, h| {
        ui.horizontal(|ui| {
            if small_btn(ui, BACK).clicked() {
                back = true;
            }
            ui.label(RichText::new(name.clone()).font(anton(24.0)).color(GOLD));
            ui.label(RichText::new(online.0).color(online.1).size(13.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_btn(ui, "Remove").clicked() {
                    unfriend = true;
                }
            });
        });
        let err = app.social.err.clone();
        if !err.is_empty() {
            ui.label(RichText::new(err).color(BAD).size(13.0));
        }
        // the message box stays put under this, however long the chat gets
        let log_h = h - 118.0;
        egui::ScrollArea::vertical().id_salt("chatlog").max_height(log_h).min_scrolled_height(log_h).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            ui.set_width(w);
            if msgs.is_empty() {
                ui.label(RichText::new(format!("Nothing yet. Say hi to {name}. They'll get it even if they're offline.")).color(MUTED).size(14.0));
            }
            for m in &msgs {
                let mine = m.from == me;
                let layout = if mine { Layout::right_to_left(Align::TOP) } else { Layout::left_to_right(Align::TOP) };
                ui.with_layout(layout, |ui| {
                    egui::Frame::NONE.fill(if mine { Color32::from_rgb(0x8a, 0x6a, 0x1a) } else { CARD }).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::symmetric(11, 7)).show(ui, |ui| {
                        ui.set_max_width(w * 0.75);
                        ui.add(egui::Label::new(RichText::new(m.text.clone()).color(Color32::WHITE).size(16.0)).wrap());
                    });
                });
                ui.add_space(3.0);
            }
        });
        ui.horizontal(|ui| {
            let so = &mut app.social;
            let r = text_field(ui, "chat", egui::TextEdit::singleline(&mut so.say_in).hint_text("Message…").char_limit(crate::net::MAX_CHAT).desired_width(w - 100.0).margin(vec2(12.0, 11.0)).font(FontId::proportional(16.0)));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (big(ui, "SEND", true, 84.0).clicked() || enter) && !so.say_in.trim().is_empty() {
                so.err.clear();
                so.say_open_chat();
            }
        });
    });
    if back {
        app.social.open_chat = None;
    }
    if unfriend {
        app.social.send(crate::net::ToServer::FriendRemove { code: code.to_string() });
        app.social.open_chat = None;
    }
}
/// Put a code on the clipboard, each platform its own way.
fn copy_text(ctx: &egui::Context, t: &str) {
    #[cfg(target_os = "android")]
    {
        let _ = ctx;
        crate::android::copy_text(t);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        web_copy(t);
    }
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    ctx.copy_text(t.to_string());
}
/// `navigator.clipboard` only exists on https (and localhost): otherwise the old way, a selected
/// textarea and `execCommand`. Everything through `Reflect`, as a missing API mustn't throw.
#[cfg(target_arch = "wasm32")]
fn web_copy(t: &str) {
    use js_sys::{Function, Reflect};
    use wasm_bindgen::{JsCast, JsValue};
    let Some(w) = web_sys::window() else { return };
    let get = |o: &JsValue, k: &str| Reflect::get(o, &k.into()).ok().filter(|v| !v.is_undefined() && !v.is_null());
    let call = |o: &JsValue, k: &str, args: &[JsValue]| get(o, k).and_then(|f| f.dyn_into::<Function>().ok()).and_then(|f| f.apply(o, &args.iter().cloned().collect()).ok());
    let w: JsValue = w.into();
    if let Some(clip) = get(&w, "navigator").and_then(|n| get(&n, "clipboard")) {
        if let Some(p) = call(&clip, "writeText", &[t.into()]) {
            // a refusal arrives later, as a rejected promise: nobody needs to hear of it
            if let Ok(p) = p.dyn_into::<js_sys::Promise>() {
                let ignore = wasm_bindgen::closure::Closure::<dyn FnMut(JsValue)>::new(|_: JsValue| {});
                let _ = p.catch(&ignore);
                ignore.forget();
            }
            return;
        }
    }
    let Some(doc) = get(&w, "document") else { return };
    let Some(ta) = call(&doc, "createElement", &["textarea".into()]) else { return };
    let _ = Reflect::set(&ta, &"value".into(), &t.into());
    if let Some(body) = get(&doc, "body") {
        call(&body, "appendChild", &[ta.clone()]);
        call(&ta, "select", &[]);
        call(&doc, "execCommand", &["copy".into()]);
        call(&body, "removeChild", &[ta]);
    }
}
/// A small copy button after a code; says so when done.
fn copy_btn(app: &mut FlunkiApp, ui: &mut Ui, code: &str) {
    if small_btn(ui, &format!("{COPY} Copy")).clicked() && !code.is_empty() {
        copy_text(ui.ctx(), code);
        app.lock_note = Some((format!("{CHECK} Copied {code}"), 1.6));
    }
}

/// Username and password, to log in or make an account (the web version).
fn login_box(app: &mut FlunkiApp, ui: &mut Ui, w: f32, why: &str) {
    ui.label(RichText::new(why).color(Color32::WHITE).size(15.0));
    ui.label(RichText::new("Your XP, stats and records are kept with your account, on any device.").color(MUTED).size(13.0));
    let busy = app.social.busy;
    text_field(ui, "loginuser", egui::TextEdit::singleline(&mut app.login_user).hint_text("Username").char_limit(net::USER_MAX).desired_width(w).margin(vec2(12.0, 10.0)).font(FontId::proportional(18.0)));
    let r = text_field(ui, "loginpass", egui::TextEdit::singleline(&mut app.login_pass).password(true).hint_text("Password").char_limit(net::PASS_MAX).desired_width(w).margin(vec2(12.0, 10.0)).font(FontId::proportional(18.0)));
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if !app.social.auth_err.is_empty() {
        ui.label(RichText::new(app.social.auth_err.clone()).color(BAD).size(14.0));
    }
    let mut go = None;
    if busy {
        ui.label(RichText::new("Checking…").color(MUTED));
    } else {
        ui.horizontal(|ui| {
            let hw = (w - 10.0) / 2.0;
            if big(ui, "LOG IN", true, hw).clicked() || enter {
                go = Some(false);
            }
            if big(ui, "SIGN UP", false, hw).clicked() {
                go = Some(true);
            }
        });
        ui.label(RichText::new(format!("New here? Pick a username ({}–{} letters or digits; it's your player's name) and a password of at least {} characters, then Sign up.", net::USER_MIN, net::USER_MAX, net::PASS_MIN)).color(MUTED).size(13.0));
    }
    if let Some(new) = go {
        // say what's wrong before asking the server
        let err = if let Err(e) = net::clean_user(&app.login_user) {
            Some(e)
        } else if new && app.login_pass.chars().count() < net::PASS_MIN {
            Some(format!("A password has at least {} characters", net::PASS_MIN))
        } else if app.login_pass.is_empty() {
            Some("Enter your password".to_string())
        } else {
            None
        };
        match err {
            Some(e) => app.social.auth_err = e,
            None => {
                let name = app.save.profile.name.clone();
                let (u, p) = (app.login_user.clone(), app.login_pass.clone());
                app.social.log_in(&u, &p, &name, new);
            }
        }
    }
}

fn account_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let mut close = false;
    sheet(ctx, sr, "account", sr.height() * 0.7, |ui, w| {
        if sheet_head(ui, "Account") {
            close = true;
        }
        if app.save.locked() {
            login_box(app, ui, w, "Log in to play online, add friends and earn XP.");
        } else if !app.save.logged_in() {
            // optional here: the phone has a player of its own
            login_box(app, ui, w, "Sign up to keep this player, its friends and progress under a username and password, and play as them on the web too.");
            ui.label(RichText::new("Logging in to another account switches to it; log out to get this phone's own player back.").color(MUTED).size(13.0));
        } else {
            ui.label(RichText::new(format!("{CHECK} Logged in as {}", app.save.user)).color(GOOD).size(16.0));
            close |= done_row(ui, w);
        }
    });
    if close {
        app.sheet = Sheet::None;
    }
}

fn done_row(ui: &mut Ui, w: f32) -> bool {
    big(ui, "DONE", true, w).clicked()
}

fn profile_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let mut close = false;
    sheet(ctx, sr, "profile", sr.height() * 0.5, |ui, w| {
        if sheet_head(ui, "Your player") {
            close = true;
        }
        if app.save.locked() {
            login_box(app, ui, w, "Log in to make your player your own.");
            return;
        }
        if app.save.logged_in() {
            // the username is the name
            ui.horizontal(|ui| {
                ui.label(RichText::new("Name").color(MUTED));
                ui.label(RichText::new(app.save.user.clone()).font(bold(18.0)).color(Color32::WHITE));
            });
        } else {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Name").color(MUTED));
            let r = text_field(ui, "name", egui::TextEdit::singleline(&mut app.name_edit).char_limit(16).desired_width(w - 70.0).margin(vec2(12.0, 9.0)).font(FontId::proportional(18.0)));
            if r.changed() {
                let n: String = app.name_edit.chars().filter(|c| !c.is_control()).collect();
                if !n.trim().is_empty() {
                    app.save.profile.name = n.trim().to_string();
                    app.save.store();
                }
            }
        });
        }
        let lv = app.save.level();
        let mut changed = false;
        let mut note: Option<(String, u32)> = None;
        let rows: [(&str, &[u32], fn(usize) -> Option<Unlock>, usize); 4] = [
            ("Shirt", &save::SWATCHES, |i| Some(Unlock::Shirt(i)), app.save.profile.color),
            ("Skin", &save::SKIN_OPTS, |_| None, app.save.profile.skin),
            ("Hair", &save::HAIR_OPTS, |i| Some(Unlock::Hair(i)), app.save.profile.hair),
            ("Shorts", &save::SHORTS_OPTS, |i| Some(Unlock::Shorts(i)), app.save.profile.shorts),
        ];
        for (ri, (name, opts, unl, cur)) in rows.iter().enumerate() {
            label_caps(ui, name);
            ui.horizontal_wrapped(|ui| {
                for (i, c) in opts.iter().enumerate() {
                    let need = unl(i).map_or(1, save::unlock_lv);
                    let locked = lv < need;
                    if swatch(ui, *c, i == *cur, locked).clicked() {
                        if locked {
                            note = Some((name.to_lowercase(), need));
                        } else {
                            let p = &mut app.save.profile;
                            match ri {
                                0 => p.color = i,
                                1 => p.skin = i,
                                2 => p.hair = i,
                                _ => p.shorts = i,
                            }
                            changed = true;
                        }
                    }
                }
            });
        }
        label_caps(ui, "Hat");
        ui.horizontal_wrapped(|ui| {
            for (i, n) in crate::scene::HAT_NAMES.iter().enumerate() {
                let need = save::unlock_lv(Unlock::Hat(i as u8));
                let locked = lv < need;
                if seg(ui, n, app.save.profile.hat == i as u8, locked, 60.0).clicked() {
                    if locked {
                        note = Some(("hat".into(), need));
                    } else {
                        app.save.profile.hat = i as u8;
                        changed = true;
                    }
                }
            }
        });
        label_caps(ui, "Voice");
        ui.horizontal_wrapped(|ui| {
            for (i, v) in save::VOICES.iter().enumerate() {
                let need = save::unlock_lv(Unlock::Voice(i));
                let locked = lv < need;
                if seg(ui, v.0, app.save.profile.voice == i, locked, 70.0).clicked() {
                    if locked {
                        note = Some(("voice".into(), need));
                    } else {
                        app.save.profile.voice = i;
                        changed = true;
                        let (txt, lang) = voice::QM_SAY[0];
                        voice::speak(txt, lang, v.1, v.2, app.audio.volume.max(0.1));
                    }
                }
            }
        });
        if let Some((what, need)) = note {
            app.lock_note = Some((format!("{LOCK} That {what} unlocks at level {need} · you are level {lv}"), 2.6));
        }
        if changed {
            app.save.store();
            app.hub_cheer = 1.1;
        }
        close |= done_row(ui, w);
    });
    if close {
        app.sheet = Sheet::None;
    }
}

fn progress_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let mut close = false;
    let l = save::level_of(app.save.xp);
    sheet(ctx, sr, "progress", sr.height() * 0.72, |ui, w| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Level {}", l.lv)).font(anton(34.0)).color(GOLD));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{} XP", app.save.xp)).color(MUTED));
            });
        });
        if app.save.locked() {
            ui.label(RichText::new(format!("{LOCK} Logged out, nothing is earned.")).color(WARN).size(14.0));
            if small_btn(ui, "Log in to earn XP").clicked() {
                app.sheet = Sheet::Account;
            }
        }
        let (r, _) = ui.allocate_exact_size(vec2(w, 10.0), Sense::hover());
        ui.painter().rect_filled(r, 5.0, Color32::from_white_alpha(25));
        let f = if l.need > 0 { l.into as f32 / l.need as f32 } else { 1.0 };
        ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width() * f, 10.0)), 5.0, GOLD);
        ui.label(RichText::new(format!("{} XP to level {}", l.need - l.into, l.lv + 1)).color(Color32::WHITE));
        ui.label(RichText::new("Earn XP by finishing matches (more for a win, each hit and every sneaky sip you get away with), drills, records and the tutorial.").color(MUTED).size(13.0));
        for lvn in 2..=save::TOP_UNLOCK {
            let items: Vec<&str> = save::UNLOCKS.iter().filter(|u| u.0 == lvn).map(|u| u.2).collect();
            if items.is_empty() {
                continue;
            }
            let got = l.lv >= lvn;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} Level {lvn}", if got { CHECK } else { LOCK })).color(if got { GOOD } else { MUTED }).font(bold(16.0)));
                ui.label(RichText::new(items.join(", ")).color(if got { Color32::WHITE } else { MUTED }).size(14.0));
            });
        }
        close |= done_row(ui, w);
    });
    if close {
        app.sheet = Sheet::None;
    }
}

fn train_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let mut act: Option<Drill> = None;
    let mut tut = false;
    let mut close = false;
    sheet(ctx, sr, "train", sr.height() * 0.75, |ui, w| {
        if sheet_head(ui, "Training") {
            close = true;
        }
        ui.label(RichText::new("Solo drills in the training hall. Fouls cost 2 seconds. Records go on the chalkboard.").color(MUTED).size(14.0));
        for (k, sub) in [(Drill::Throw, "10 throws, count your hits"), (Drill::Drink, "One bottle, as fast as you can without choking"), (Drill::Run, "Three runs, fetch and back")] {
            let pb = app.save.train_pb[k.idx()];
            let line = format!("{sub} · record {}", k.fmt(pb));
            if card_btn(ui, k.name(), &line, w, GOOD).clicked() {
                act = Some(k);
            }
        }
        let tl = if app.save.tut_done { "Replay the tutorial" } else { "New here? Play the tutorial" };
        if card_btn(ui, tl, "A guided practice round on the real pitch", w, GOLD).clicked() {
            tut = true;
        }
        close |= done_row(ui, w);
    });
    if let Some(k) = act {
        app.start_drill(k);
    } else if tut {
        app.start_tutorial();
    } else if close {
        app.sheet = Sheet::None;
    }
}

fn settings_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let mut close = false;
    sheet(ctx, sr, "settings", sr.height() * 0.75, |ui, w| {
        if sheet_head(ui, "Settings") {
            close = true;
        }
        let s = &mut app.save.settings;
        let mut dirty = false;
        let mut vol = s.volume as f32;
        if slider_row(ui, "Volume", &mut vol, 0.0, 100.0, |v| format!("{}%", v.round()), w) {
            s.volume = vol.round() as u32;
            dirty = true;
        }
        dirty |= slider_row(ui, "Flick power", &mut s.sens, 0.6, 1.6, |v| format!("{:.0}%", v * 100.0), w);
        dirty |= toggle(ui, &mut s.vibrate, "Vibrate on hits and fouls", w);
        dirty |= toggle(ui, &mut s.trail, "Ball trail", w);
        dirty |= toggle(ui, &mut s.voices, "Spoken quick messages", w);
        // on the web it's on only once the browser allows it; switching it on asks
        #[cfg(target_arch = "wasm32")]
        {
            let allowed = crate::app::web_notify_permission(false);
            if allowed != "unsupported" {
                let mut on = s.notify && allowed == "granted";
                if toggle(ui, &mut on, "Notify me of invites and messages", w) {
                    s.notify = on;
                    dirty = true;
                    if on {
                        crate::app::web_notify_permission(true);
                    }
                }
                if allowed == "denied" {
                    ui.label(RichText::new("Notifications are blocked for this site in your browser's settings").color(MUTED).size(13.0));
                }
            }
        }
        #[cfg(target_os = "android")]
        if toggle(ui, &mut s.notify, "Notify me of invites and messages", w) {
            dirty = true;
        }
        {
            label_caps(ui, "Account");
            if !app.save.logged_in() {
                if big(ui, "LOG IN OR SIGN UP", false, w).clicked() {
                    app.sheet = Sheet::Account;
                }
            } else {
                let user = app.save.user.clone();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Logged in as {user}")).color(Color32::WHITE).size(15.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if small_btn(ui, "Log out").clicked() {
                            app.log_out();
                        }
                    });
                });
            }
        }
        label_caps(ui, "About");
        ui.label(RichText::new(format!("FlunkieBal build {}", crate::net::APP_BUILD)).color(MUTED).size(14.0));
        update_row(app, ui, w);
        if big(ui, &format!("{BUG} REPORT A BUG"), false, w).clicked() {
            app.sheet = Sheet::Bug;
        }
        if dirty {
            app.save.store();
            app.audio.volume = app.save.settings.volume as f32 / 100.0;
        }
        close |= done_row(ui, w);
    });
    if close {
        app.sheet = Sheet::None;
    }
}

/// A newer version is out: say so, and get it from here.
fn update_row(app: &mut FlunkiApp, ui: &mut Ui, w: f32) {
    if !app.outdated() {
        return;
    }
    #[cfg(target_os = "android")]
    let state = crate::android::update_state();
    #[cfg(not(target_os = "android"))]
    let state = String::new();
    egui::Frame::NONE.fill(Color32::from_rgb(0x3a, 0x2a, 0x0a)).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_width(w - 20.0);
        ui.label(RichText::new(format!("{DOWNLOAD} A new version is out")).font(bold(16.0)).color(GOLD));
        let (line, can) = if let Some(p) = state.strip_prefix("get:") {
            (format!("Downloading… {p}%"), false)
        } else if state == "install" {
            ("Follow the installer to finish".to_string(), true)
        } else if let Some(e) = state.strip_prefix("err:") {
            (format!("That didn't work: {e}"), true)
        } else if cfg!(target_os = "android") {
            ("Update to keep playing online with everyone".to_string(), true)
        } else if cfg!(target_arch = "wasm32") {
            ("Reload to get it".to_string(), true)
        } else {
            ("Pull and rebuild to get it".to_string(), false)
        };
        ui.label(RichText::new(line).color(Color32::WHITE).size(14.0));
        if can && (cfg!(target_os = "android") || cfg!(target_arch = "wasm32")) && big(ui, if cfg!(target_arch = "wasm32") { "RELOAD" } else { "UPDATE NOW" }, true, w - 20.0).clicked() {
            app.update_app();
        }
    });
}

/// Tell us what went wrong. The game adds what it knows about itself.
fn bug_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let mut close = false;
    let mut send = false;
    sheet(ctx, sr, "bug", sr.height() * 0.75, |ui, w| {
        if sheet_head(ui, "Report a bug") {
            close = true;
        }
        ui.label(RichText::new("What happened, and what were you doing? The FlunkCie gets it with your game's version and its last log lines; nothing else.").color(MUTED).size(14.0));
        text_field(ui, "bug", egui::TextEdit::multiline(&mut app.bug_text).hint_text("The ball flew sideways when…").char_limit(crate::net::BUG_TEXT).desired_rows(5).desired_width(w));
        let busy = app.social.bug_busy;
        if let Some((ok, msg)) = &app.social.bug_res {
            let (t, c) = if *ok { (format!("{CHECK} Sent, thank you!"), GOOD) } else { (msg.clone(), BAD) };
            ui.label(RichText::new(t).color(c).size(14.0));
        }
        if big(ui, if busy { "SENDING…" } else { "SEND" }, true, w).clicked() && !busy && !app.bug_text.trim().is_empty() {
            send = true;
        }
        close |= done_row(ui, w);
    });
    if send {
        app.send_bug();
    }
    if app.social.bug_res.as_ref().map_or(false, |r| r.0) && !app.bug_text.is_empty() {
        app.bug_text.clear();
    }
    if close {
        app.social.bug_res = None;
        app.sheet = Sheet::None;
    }
}

fn rules_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let mut close = false;
    sheet(ctx, sr, "rules", sr.height() * 0.72, |ui, w| {
        if sheet_head(ui, "The rulebook") {
            close = true;
        }
        for (h, b) in [
            ("Throw", "Flick the ball up at the bottle in the middle. Throw while the moving ring is small for a bonus. Spin it in circles first for a curveball."),
            ("Hit it?", "When the bottle falls your team drinks. Hold DRINK, but let go before the gauge goes red or you choke and spill."),
            ("Run", "The other team runs: tap L and R in turn. One fetches the ball, one stands the bottle up, then both go back behind their line. Online, everyone taps for their own runner and holds DRINK for their own beer."),
            ("STOP", "When they are back, let go of your beer. Drinking after STOP is a foul."),
            ("The referee", "Fouls only count when the ref sees them. Watch the light on the grass: that is where he is looking. When he smokes, he sees nothing."),
            ("Strafbier", "A foul he sees costs a full extra beer. Knock over a beer with your throw and it's always seen: a Strafbier for whoever's beer it was, so aim for theirs, never your own."),
            ("Sneak sip", "Drink any time with SNEAK SIP. It takes a while, and if the ref looks your way you get a Strafbier."),
            ("Winning", "Players who empty their bottle leave the pitch. The first team with nobody left wins."),
        ] {
            ui.label(RichText::new(h).font(bold(16.0)).color(Color32::WHITE).size(17.0));
            ui.label(RichText::new(b).color(MUTED));
        }
        close |= done_row(ui, w);
    });
    if close {
        app.sheet = Sheet::None;
    }
}

fn stats_sheet(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let mut close = false;
    let mut reset = false;
    let all = app.save.matches.clone();
    let armed = app.reset_armed > 0.0;
    sheet(ctx, sr, "stats", sr.height() * 0.8, |ui, w| {
        if sheet_head(ui, "Stats") {
            close = true;
        }
        if app.save.locked() {
            ui.label(RichText::new(format!("{LOCK} Logged out, matches aren't kept.")).color(WARN).size(14.0));
            if small_btn(ui, "Log in to keep your stats").clicked() {
                app.sheet = Sheet::Account;
            }
        }
        if all.is_empty() {
            ui.label(RichText::new("No matches yet. Play one against the computer or online and your numbers show up here.").color(MUTED));
            close |= done_row(ui, w);
            return;
        }
        let mine: Vec<&save::MatchRec> = all.iter().filter(|m| m.win.is_some()).collect();
        let sum = |f: fn(&save::MatchRec) -> f32| mine.iter().map(|m| f(m)).sum::<f32>();
        let wins = mine.iter().filter(|m| m.win == Some(true)).count();
        let throws = sum(|m| m.throws as f32);
        let hits = sum(|m| m.hits as f32);
        let litres = sum(|m| m.litres);
        let pct = |a: f32, b: f32| if b > 0.0 { format!("{}%", (a / b * 100.0).round()) } else { "–".into() };
        let mut best = 0;
        let mut run = 0;
        for m in &mine {
            run = if m.win == Some(true) { run + 1 } else { 0 };
            best = best.max(run);
        }
        let hot = all.len() - mine.len();
        let fr: Vec<f32> = mine.iter().map(|m| m.fast_run).filter(|v| *v > 0.0).collect();
        let kpis = [
            ("Matches", mine.len().to_string(), if hot > 0 { format!("+{hot} pass-and-play") } else { "vs CPU and online".into() }),
            ("Win rate", pct(wins as f32, mine.len() as f32), format!("{wins} won · {} lost", mine.len() - wins)),
            ("Hit rate", pct(hits, throws), format!("{hits} hits from {throws} throws")),
            ("Drunk", format!("{litres:.1} L"), format!("≈ {} bottles", (litres / 0.5).round())),
            ("Strafbier", sum(|m| m.straf as f32).to_string(), "given to your team".into()),
            ("Ref missed", sum(|m| m.dodged as f32).to_string(), "fouls unseen".into()),
            ("Fastest run", fr.iter().cloned().fold(None, |a: Option<f32>, b| Some(a.map_or(b, |a| a.min(b)))).map_or("–".into(), |v| format!("{v:.1} s")), "hit to STOP".into()),
            ("Best streak", best.to_string(), "wins in a row".into()),
        ];
        let cw = (w - 10.0) / 2.0;
        for row in kpis.chunks(2) {
            ui.horizontal(|ui| {
                for (k, v, s) in row {
                    let (r, _) = ui.allocate_exact_size(vec2(cw, 66.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(r, 12.0, CARD);
                    p.text(r.left_top() + vec2(12.0, 8.0), Align2::LEFT_TOP, k.to_uppercase(), FontId::proportional(11.0), MUTED);
                    p.text(r.left_top() + vec2(12.0, 24.0), Align2::LEFT_TOP, v, anton(22.0), Color32::WHITE);
                    p.text(r.left_top() + vec2(12.0, 50.0), Align2::LEFT_TOP, s, FontId::proportional(11.0), MUTED);
                }
            });
        }
        // form: last ten
        label_caps(ui, "Form · last 10");
        ui.horizontal(|ui| {
            let last: Vec<&&save::MatchRec> = mine.iter().rev().take(10).collect::<Vec<_>>().into_iter().rev().collect();
            for _ in last.len()..10 {
                let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                ui.painter().rect_filled(r, 6.0, Color32::from_white_alpha(12));
            }
            for m in last {
                let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                let win = m.win == Some(true);
                ui.painter().rect_filled(r, 6.0, if win { GOLD } else { Color32::from_white_alpha(25) });
                ui.painter().text(r.center(), Align2::CENTER_CENTER, if win { "W" } else { "L" }, FontId::proportional(12.0), if win { INK } else { Color32::WHITE });
            }
        });
        // hit rate per match
        let ms: Vec<&&save::MatchRec> = mine.iter().filter(|m| m.throws > 0).collect();
        let ms: Vec<&&save::MatchRec> = ms.iter().rev().take(20).rev().cloned().collect();
        if ms.len() >= 2 {
            label_caps(ui, &format!("Hit rate per match · last {}", ms.len()));
            let (r, _) = ui.allocate_exact_size(vec2(w, 120.0), Sense::hover());
            let p = ui.painter();
            let (l, rt, t, b) = (r.left() + 34.0, r.right() - 8.0, r.top() + 8.0, r.bottom() - 16.0);
            let x = |i: usize| l + (rt - l) * i as f32 / (ms.len() - 1) as f32;
            let y = |v: f32| t + (b - t) * (1.0 - v);
            for v in [0.0f32, 0.5, 1.0] {
                p.line_segment([pos2(l, y(v)), pos2(rt, y(v))], Stroke::new(1.0, Color32::from_white_alpha(if v == 0.0 { 55 } else { 20 })));
                p.text(pos2(l - 6.0, y(v)), Align2::RIGHT_CENTER, format!("{}%", (v * 100.0) as i32), FontId::monospace(10.0), MUTED);
            }
            let avg = ms.iter().map(|m| m.hits as f32).sum::<f32>() / ms.iter().map(|m| m.throws as f32).sum::<f32>();
            p.add(egui::Shape::dashed_line(&[pos2(l, y(avg)), pos2(rt, y(avg))], Stroke::new(1.0, MUTED), 3.0, 4.0));
            p.text(pos2(rt, y(avg) - 4.0), Align2::RIGHT_BOTTOM, format!("avg {}%", (avg * 100.0).round()), FontId::monospace(10.0), MUTED);
            let pts: Vec<Pos2> = ms.iter().enumerate().map(|(i, m)| pos2(x(i), y(m.hits as f32 / m.throws as f32))).collect();
            p.add(egui::Shape::line(pts.clone(), Stroke::new(2.0, GOLD)));
            for (i, q) in pts.iter().enumerate() {
                let win = ms[i].win == Some(true);
                p.circle(*q, 4.0, if win { GOLD } else { INK }, Stroke::new(2.0, if win { INK } else { GOLD }));
            }
        }
        // by arena
        label_caps(ui, "By arena");
        egui::Grid::new("byarena").num_columns(4).spacing(vec2(14.0, 4.0)).show(ui, |ui| {
            for hd in ["Arena", "Played", "Won", "Hits"] {
                ui.label(RichText::new(hd).color(MUTED).size(13.0));
            }
            ui.end_row();
            for k in PITCHES {
                let ms: Vec<&&save::MatchRec> = mine.iter().filter(|m| m.arena == k.id()).collect();
                ui.label(k.name());
                if ms.is_empty() {
                    ui.label(RichText::new("0").color(MUTED));
                    ui.label(RichText::new("–").color(MUTED));
                    ui.label(RichText::new("–").color(MUTED));
                } else {
                    let w = ms.iter().filter(|m| m.win == Some(true)).count();
                    let th: u32 = ms.iter().map(|m| m.throws).sum();
                    let ht: u32 = ms.iter().map(|m| m.hits).sum();
                    ui.label(ms.len().to_string());
                    ui.label(pct(w as f32, ms.len() as f32));
                    ui.label(pct(ht as f32, th as f32));
                }
                ui.end_row();
            }
        });
        // recent
        label_caps(ui, "Recent");
        for m in all.iter().rev().take(8) {
            let res = match m.win {
                Some(true) => "W",
                Some(false) => "L",
                None => "–",
            };
            let what = match m.mode.as_str() {
                "hot" => format!("{} won · pass-and-play", m.winner),
                "online" => format!("{} vs {}", if m.win == Some(true) { "Win" } else { "Loss" }, if m.opp.is_empty() { "the computer".to_string() } else { m.opp.join(", ") }),
                _ => format!("{} vs CPU · {}", if m.win == Some(true) { "Win" } else { "Loss" }, game::DIFF_NAMES[m.diff.min(2)]),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(res).font(anton(18.0)).color(if m.win == Some(true) { GOLD } else { MUTED }));
                ui.vertical(|ui| {
                    ui.label(RichText::new(what).color(Color32::WHITE).size(14.0));
                    ui.label(RichText::new(format!("{} · {}:{:02} · {}/{} hits · {:.2} L", ArenaKey::from_id(&m.arena).name(), m.dur / 60, m.dur % 60, m.hits, m.throws, m.litres)).color(MUTED).size(12.0));
                });
            });
        }
        ui.horizontal(|ui| {
            if small_btn(ui, if armed { "Tap again to delete all stats" } else { "Reset stats" }).clicked() {
                reset = true;
            }
        });
        close |= done_row(ui, w);
    });
    if reset {
        if armed {
            app.save.matches.clear();
            app.save.store();
            app.reset_armed = 0.0;
        } else {
            app.reset_armed = 4.0;
        }
    }
    if close {
        app.sheet = Sheet::None;
    }
}

#[allow(dead_code)]
fn _unused(_: Ctrl) {}

#[cfg(test)]
mod font_tests {
    use super::*;
    /// Laying out every size up front leaves the atlas big enough that the game's own text doesn't grow it.
    #[test]
    fn warm_fonts_fill_the_atlas_once() {
        let ctx = egui::Context::default();
        setup_style(&ctx);
        ctx.all_styles_mut(|s| {
            s.visuals.text_options.font_hinting = false;
            s.visuals.text_options.subpixel_binning = false;
        });
        let mut input = egui::RawInput { max_texture_side: Some(4096), ..Default::default() };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(3.0);
        let mut sizes = Vec::new();
        for frame in 0..3 {
            let t = std::time::Instant::now();
            let out = ctx.run_ui(input.clone(), |ui| {
                if frame == 1 {
                    warm_fonts(ui.ctx(), 390.0);
                }
            });
            for (_, ds) in &out.textures_delta.set {
                for d in ds {
                    sizes.push((frame, d.image.size(), d.pos.is_none()));
                }
            }
            println!("frame {frame}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
        println!("atlas uploads: {sizes:?}, fill {:.2}", ctx.fonts(|f| f.font_atlas_fill_ratio()));
        // the HUD's text after that: nothing new for the atlas
        let out = ctx.run_ui(input.clone(), |ui| {
            let p = ui.painter();
            p.text(pos2(10.0, 10.0), Align2::LEFT_TOP, "HIT!", anton(banner_size(390.0)), Color32::WHITE);
            p.text(pos2(10.0, 90.0), Align2::LEFT_TOP, "Flick up to throw · spin in circles for a curveball", FontId::proportional(14.0), Color32::WHITE);
            ui.label(RichText::new("Fouls only count when the ref sees them. Watch the light on the grass: that is where he is looking. When he smokes, he sees nothing.").color(MUTED));
        });
        assert!(out.textures_delta.set.iter().flat_map(|(_, ds)| ds.iter()).all(|d| d.pos.is_some()), "no whole-atlas upload after warming");
    }
}
