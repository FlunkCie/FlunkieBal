//! The item shop: every piece of clothing, hairstyle, costume and shop hat, as cards on a full screen of its
//! own. The Featured tab changes every day and is cheaper that day (`net::featured`, `net::price_on`: the
//! server charges the same); costumes cost less than their pieces (`net::bundle_price`).
//! Tapping a card opens it big: buy it, wear it, or try it on (your player in the clubhouse wears it).

use super::{anton, bold};
use crate::app::{now_ms, FlunkiApp, Sheet};
use crate::audio::{self, Cue};
use crate::body;
use crate::icons::*;
use crate::net;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Ui};

/// Rarities: name, light and dark colour of their cards.
const RARITY: [(&str, Color32, Color32); 5] = [
    ("COMMON", Color32::from_rgb(0x9a, 0xa3, 0xb0), Color32::from_rgb(0x4b, 0x53, 0x63)),
    ("UNCOMMON", Color32::from_rgb(0x7c, 0xd3, 0x2b), Color32::from_rgb(0x23, 0x6b, 0x16)),
    ("RARE", Color32::from_rgb(0x3d, 0xc4, 0xff), Color32::from_rgb(0x15, 0x4f, 0xa6)),
    ("EPIC", Color32::from_rgb(0xd0, 0x6b, 0xff), Color32::from_rgb(0x58, 0x1f, 0xa8)),
    ("LEGENDARY", Color32::from_rgb(0xff, 0xb3, 0x3a), Color32::from_rgb(0xb0, 0x42, 0x13)),
];
const TABS: [&str; 9] = ["Featured", "Costumes", "Hair", "Face", "Top", "Trousers", "Shoes", "Back", "Hats"];
/// The tabs with clothes for one body: costumes and the slots (`body::SLOTS`, from tab 2).
fn body_tab(tab: usize) -> bool {
    (1..=7).contains(&tab)
}
/// The shop's pictures: pages of 8 × 8, 256 px each (see tools/icons_pack.py).
const PAGES: [&[u8]; 3] = [include_bytes!("../assets/shop0.png"), include_bytes!("../assets/shop1.png"), include_bytes!("../assets/shop2.png")];
/// The shop's day: days since 1970, UTC, like the server's (`net::price_on`).
fn today() -> u64 {
    (now_ms() / 86_400_000.0) as u64
}

/// Something for sale, as the shop shows it.
#[derive(Clone, Copy)]
pub(crate) struct Ware {
    pub id: u8,
    pub name: &'static str,
    pub rarity: u8,
    /// 0: free
    pub price: u32,
    /// the slot (`body::SLOTS`), or none for a hat or a costume
    pub slot: Option<usize>,
    pub rig: usize,
    /// its picture's number (`icon_of`)
    pub icon: usize,
}
fn ware(c: &net::Item) -> Ware {
    Ware { id: c.id, name: c.name, rarity: c.rarity, price: c.price, slot: Some(body::slot_of(c)), rig: body::rig_of(c), icon: (c.id - 32) as usize }
}
fn hats() -> impl Iterator<Item = Ware> {
    net::SHOP.iter().map(|&(h, price)| Ware { id: h, name: crate::scene::HAT_NAMES[h as usize], rarity: net::hat_rarity(price), price, slot: None, rig: 2, icon: net::CLOTHES.len() + h as usize - 1 })
}
/// The costumes, priced for what you have: the best piece's rarity, its pieces' price less `BUNDLE_OFF`.
fn costumes(app: &FlunkiApp) -> Vec<(Ware, net::Bundle)> {
    net::bundles()
        .into_iter()
        .map(|b| {
            let (price, _) = net::bundle_price(&b, &app.save.owned);
            let rarity = b.items.iter().filter_map(|i| net::clothes(*i)).map(|c| c.rarity).max().unwrap_or(0);
            let icon = net::CLOTHES.len() + 10 + net::COSTUMES.iter().position(|c| c.0 == b.set).unwrap_or(0);
            let rig = if b.set.starts_with("f/") { 1 } else { 0 };
            (Ware { id: b.id, name: b.name, rarity, price, slot: None, rig, icon }, b)
        })
        .collect()
}
/// What a costume's pieces cost one by one (the ones you don't have).
fn full_price(app: &FlunkiApp, b: &net::Bundle) -> u32 {
    net::bundle_price(b, &app.save.owned).1.iter().filter_map(|i| net::price_of(*i)).sum()
}
/// What it costs today: (price, the price without today's discount if there is one).
fn today_price(app: &FlunkiApp, w: &Ware) -> (u32, Option<u32>) {
    match net::bundle(w.id) {
        Some(b) => (w.price, Some(full_price(app, &b)).filter(|f| *f > w.price)),
        None => {
            let now = net::price_on(w.id, today()).unwrap_or(w.price);
            (now, Some(w.price).filter(|p| *p > now))
        }
    }
}
/// Today's picks (`net::featured`), for the shop.
fn featured() -> (Vec<Ware>, Vec<Ware>) {
    let (big, small) = net::featured(today());
    (big.iter().filter_map(|i| find(*i)).collect(), small.iter().filter_map(|i| find(*i)).collect())
}

fn owns(app: &FlunkiApp, w: &Ware) -> bool {
    match (w.slot, net::bundle(w.id)) {
        (Some(_), _) => app.save.has_item(w.id),
        (None, Some(b)) => b.items.iter().all(|i| app.save.has_item(*i)),
        (None, None) => app.save.owned.contains(&w.id),
    }
}
fn wears(app: &FlunkiApp, w: &Ware) -> bool {
    let mine = body::Wear::from_bytes(app.save.profile.wear);
    match (w.slot, net::bundle(w.id)) {
        (Some(s), _) => mine.rig() == w.rig && mine.piece(s).is_some_and(|c| c.id == w.id),
        (None, Some(b)) => mine.rig() == w.rig && b.items.iter().all(|i| net::clothes(*i).is_some_and(|c| mine.piece(body::slot_of(c)).is_some_and(|p| p.id == *i))),
        (None, None) => app.save.profile.hat == w.id,
    }
}
/// Put it on: a piece in its slot, a costume whole (a piece for the other body changes bodies, in that
/// body's everyday clothes).
fn dressed(mut mine: body::Wear, w: &Ware) -> body::Wear {
    let items: Vec<u8> = match net::bundle(w.id) {
        Some(b) => b.items,
        None => vec![w.id],
    };
    if mine.rig() != w.rig {
        mine = body::Wear { build: mine.build, ..body::Wear::starter(w.rig as u8) };
    }
    if net::bundle(w.id).is_some() {
        // a costume has no backpack or hair of its own: off with what isn't part of it
        mine.slots = body::Wear::starter(w.rig as u8).slots;
        mine.slots[0] = 0;
        mine.slots[5] = 0;
    }
    for c in items.iter().filter_map(|i| net::clothes(*i)) {
        mine.slots[body::slot_of(c)] = c.id;
    }
    mine
}
/// `mine` with the shop's item `id` on (a piece, a costume or a hat), for trying it on.
pub(crate) fn tried_on(mine: body::Wear, hat: &mut u8, id: u8) -> body::Wear {
    match find(id).or_else(|| net::bundle(id).map(|b| Ware { id, name: "", rarity: 0, price: 0, slot: None, rig: if b.set.starts_with("f/") { 1 } else { 0 }, icon: 0 })) {
        Some(w) if w.slot.is_some() || net::bundle(id).is_some() => dressed(mine, &w),
        Some(w) => {
            *hat = w.id;
            mine
        }
        None => mine,
    }
}
fn wear_it(app: &mut FlunkiApp, w: &Ware) {
    match w.slot.or(net::bundle(w.id).map(|_| 0)) {
        Some(_) => app.save.profile.wear = dressed(body::Wear::from_bytes(app.save.profile.wear), w).to_bytes(),
        None => app.save.profile.hat = w.id,
    }
    app.save.store();
    app.hub_cheer = 1.1;
}

/// The picture of a ware, from the shop's pages.
pub(super) fn picture(ui: &Ui, tex: &[egui::TextureHandle], w: &Ware, r: Rect) {
    let p = ui.painter();
    let (page, k) = (w.icon / 64, (w.icon % 64) as f32);
    let Some(t) = tex.get(page) else { return };
    let (col, row) = (k % 8.0, (k / 8.0).floor());
    let uv = Rect::from_min_size(pos2(col / 8.0, row / 8.0), vec2(1.0 / 8.0, 1.0 / 8.0));
    let s = r.width().min(r.height());
    // a soft shadow under it, then the picture
    p.add(egui::Shape::ellipse_filled(pos2(r.center().x, r.center().y + s * 0.42), vec2(s * 0.3, s * 0.05), Color32::from_black_alpha(60)));
    p.image(t.id(), Rect::from_center_size(r.center(), vec2(s, s)), uv, Color32::WHITE);
}
/// The shop's pages, made into textures once.
fn pages(app: &mut FlunkiApp, ctx: &egui::Context) -> Vec<egui::TextureHandle> {
    if app.shop_tex.is_empty() {
        app.shop_tex = PAGES.iter().enumerate().filter_map(|(k, b)| crate::model::decode_png(b).map(|(w, h, px)| ctx.load_texture(format!("shop{k}"), egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px), egui::TextureOptions::LINEAR))).collect();
    }
    app.shop_tex.clone()
}

/// The card's face: rarity colours, a light from above, the picture, a dark band with name and price.
fn card(ui: &mut Ui, app: &FlunkiApp, tex: &[egui::TextureHandle], w: &Ware, r: Rect, big: bool) -> egui::Response {
    let resp = ui.allocate_rect(r, Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered() || resp.is_pointer_button_down_on());
    let r = r.expand(hover * 3.0);
    let (_, light, dark) = RARITY[w.rarity as usize % 5];
    let p = ui.painter();
    let round = CornerRadius::same(12);
    // the gradient, top light to bottom dark
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), light);
    m.colored_vertex(r.right_top(), light);
    m.colored_vertex(r.right_bottom(), dark);
    m.colored_vertex(r.left_bottom(), dark);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.rect_filled(r, round, dark);
    let inner = r.shrink(2.0);
    p.with_clip_rect(inner).add(egui::Shape::mesh(m));
    // a glow behind the picture
    let pic = Rect::from_min_max(r.min + vec2(8.0, 8.0), pos2(r.max.x - 8.0, r.max.y - if big { 64.0 } else { 52.0 }));
    for k in 0..6 {
        let f = 1.0 - k as f32 / 6.0;
        p.circle_filled(pic.center(), pic.width().min(pic.height()) * 0.5 * f, Color32::from_white_alpha((10.0 * (1.0 - f) + 4.0) as u8));
    }
    picture(ui, tex, w, pic);
    let p = ui.painter();
    // the band
    let band = Rect::from_min_max(pos2(r.left(), r.bottom() - if big { 58.0 } else { 46.0 }), r.max);
    p.rect_filled(band, CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 }, Color32::from_rgba_premultiplied(8, 10, 24, 215));
    p.rect_filled(Rect::from_min_max(pos2(r.left(), band.top() - 3.0), pos2(r.right(), band.top())), 0.0, light);
    let name = w.name.to_uppercase();
    p.text(pos2(band.left() + 9.0, band.top() + 5.0), Align2::LEFT_TOP, name, if big { anton(20.0) } else { bold(14.0) }, Color32::WHITE);
    let (now, was) = today_price(app, w);
    let (txt, col) = if wears(app, w) {
        (format!("{CHECK} WEARING"), Color32::from_rgb(0x8f, 0xf0, 0x8f))
    } else if owns(app, w) {
        (format!("{CHECK} OWNED"), Color32::from_rgb(0x8f, 0xf0, 0x8f))
    } else {
        (format!("{COIN} {now}"), Color32::from_rgb(0xff, 0xd8, 0x4a))
    };
    let font = bold(if big { 16.0 } else { 13.0 });
    let at = pos2(band.left() + 9.0, band.bottom() - 6.0);
    let price_r = p.text(at, Align2::LEFT_BOTTOM, txt, font.clone(), col);
    // a discount: the old price struck out next to the new, and a ribbon in the corner
    if let (Some(was), false) = (was, owns(app, w)) {
        let r2 = p.text(pos2(price_r.right() + 7.0, at.y), Align2::LEFT_BOTTOM, was.to_string(), font, Color32::from_white_alpha(140));
        p.line_segment([pos2(r2.left() - 1.0, r2.center().y), pos2(r2.right() + 1.0, r2.center().y)], Stroke::new(1.5, Color32::from_white_alpha(200)));
        let off = 100 - now * 100 / was.max(1);
        let tag = Rect::from_min_size(pos2(r.right() - if big { 66.0 } else { 52.0 }, r.top() + 8.0), vec2(if big { 58.0 } else { 46.0 }, if big { 28.0 } else { 22.0 }));
        p.rect_filled(tag, 6.0, Color32::from_rgb(0xe8, 0x2a, 0x3a));
        p.text(tag.center(), Align2::CENTER_CENTER, format!("-{off}%"), bold(if big { 16.0 } else { 13.0 }), Color32::WHITE);
    }
    // whose it is: the men's and the women's look alike in the picture
    if w.rig < 2 {
        p.text(pos2(band.right() - 8.0, band.bottom() - 6.0), Align2::RIGHT_BOTTOM, if w.rig == 1 { "WOMEN" } else { "MEN" }, bold(11.0), Color32::from_white_alpha(150));
    }
    p.rect_stroke(r, round, Stroke::new(if hover > 0.5 { 2.0 } else { 1.0 }, Color32::from_white_alpha(if hover > 0.5 { 200 } else { 60 })), StrokeKind::Inside);
    if resp.clicked() {
        audio::ui(Cue::Tap);
    }
    resp
}

/// The whole shop: draws over everything. While trying something on, only a bar at the bottom.
pub(crate) fn shop(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let tex = pages(app, ctx);
    if app.shop_trying {
        try_bar(app, ctx, sr);
        return;
    }
    let full = ctx.content_rect();
    let mut close = false;
    let mut pick = None;
    egui::Area::new(egui::Id::new("shop")).order(egui::Order::Foreground).fixed_pos(full.min).show(ctx, |ui| {
        ui.set_min_size(full.size());
        let p = ui.painter();
        // the backdrop: deep blue, lighter at the top, rays from the top centre
        let mut m = egui::Mesh::default();
        let (top, bot) = (Color32::from_rgb(0x24, 0x3f, 0xa8), Color32::from_rgb(0x0b, 0x10, 0x3a));
        m.colored_vertex(full.left_top(), top);
        m.colored_vertex(full.right_top(), top);
        m.colored_vertex(full.right_bottom(), bot);
        m.colored_vertex(full.left_bottom(), bot);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        p.add(egui::Shape::mesh(m));
        let o = pos2(full.center().x, full.top() - 40.0);
        for k in 0..9 {
            let a = -0.9 + k as f32 * 0.225;
            let d = vec2(a.sin(), a.cos()) * full.height() * 1.4;
            let side = vec2(a.cos(), -a.sin()) * 38.0;
            p.add(egui::Shape::convex_polygon(vec![o, o + d - side, o + d + side], Color32::from_white_alpha(if k % 2 == 0 { 9 } else { 5 }), Stroke::NONE));
        }
        let w = sr.width().min(760.0) - 24.0;
        let left = sr.center().x - w / 2.0;
        // header: back, title, coins
        let head = Rect::from_min_size(pos2(left, sr.top() + 10.0), vec2(w, 56.0));
        let back = Rect::from_min_size(head.left_center() - vec2(0.0, 21.0), vec2(42.0, 42.0));
        let br = ui.allocate_rect(back, Sense::click());
        let p = ui.painter();
        p.circle_filled(back.center(), 21.0, Color32::from_white_alpha(if br.hovered() { 50 } else { 28 }));
        p.text(back.center(), Align2::CENTER_CENTER, ARROW_LEFT, FontId::proportional(22.0), Color32::WHITE);
        if br.clicked() {
            audio::ui(Cue::Back);
            close = true;
        }
        let tpos = pos2(back.right() + 12.0, head.center().y);
        p.text(tpos + vec2(0.0, 3.0), Align2::LEFT_CENTER, "ITEM SHOP", anton(44.0), Color32::from_black_alpha(120));
        p.text(tpos, Align2::LEFT_CENTER, "ITEM SHOP", anton(44.0), Color32::WHITE);
        let coins = format!("{COINS} {}", app.save.coins);
        let cw = 26.0 + 11.0 * coins.chars().count() as f32;
        let pill = Rect::from_min_size(pos2(head.right() - cw, head.center().y - 18.0), vec2(cw, 36.0));
        p.rect_filled(pill, 18.0, Color32::from_black_alpha(110));
        p.rect_stroke(pill, 18.0, Stroke::new(1.5, Color32::from_rgb(0xff, 0xd8, 0x4a)), StrokeKind::Inside);
        p.text(pill.center(), Align2::CENTER_CENTER, coins, bold(18.0), Color32::from_rgb(0xff, 0xd8, 0x4a));
        // the day's change, counted down
        let left_ms = 86_400_000.0 - now_ms() % 86_400_000.0;
        let (hh, mm) = ((left_ms / 3_600_000.0) as u32, (left_ms / 60_000.0) as u32 % 60);
        p.text(pos2(left, head.bottom() + 4.0), Align2::LEFT_TOP, format!("New featured items in {hh}h {mm:02}m"), FontId::proportional(13.0), Color32::from_white_alpha(190));
        // tabs
        let mut x = left;
        let mut ty = head.bottom() + 26.0;
        for (k, t) in TABS.iter().enumerate() {
            let tw = 18.0 + 8.6 * t.len() as f32;
            // a second row when they don't fit
            if x + tw > left + w {
                x = left;
                ty += 36.0;
            }
            let r = Rect::from_min_size(pos2(x, ty), vec2(tw, 30.0));
            let resp = ui.allocate_rect(r, Sense::click());
            let on = app.shop_tab == k;
            let p = ui.painter();
            p.rect_filled(r, 15.0, if on { Color32::WHITE } else { Color32::from_white_alpha(if resp.hovered() { 40 } else { 22 }) });
            p.text(r.center(), Align2::CENTER_CENTER, t.to_uppercase(), bold(13.0), if on { Color32::from_rgb(0x12, 0x1c, 0x52) } else { Color32::WHITE });
            if resp.clicked() {
                audio::ui(Cue::Tick);
                app.shop_tab = k;
            }
            x += tw + 6.0;
        }
        let mut y = ty + 40.0;
        // whose clothes: the men's or the women's
        if body_tab(app.shop_tab) {
            for (k, (n, _)) in body::BODIES.iter().enumerate() {
                let r = Rect::from_min_size(pos2(left + k as f32 * 96.0, y), vec2(90.0, 28.0));
                let resp = ui.allocate_rect(r, Sense::click());
                let on = app.shop_body as usize == k;
                let p = ui.painter();
                p.rect_filled(r, 8.0, if on { Color32::from_rgb(0xff, 0xd8, 0x4a) } else { Color32::from_black_alpha(80) });
                p.text(r.center(), Align2::CENTER_CENTER, n.to_uppercase(), bold(13.0), if on { Color32::from_rgb(0x2a, 0x1a, 0x05) } else { Color32::WHITE });
                if resp.clicked() {
                    audio::ui(Cue::Tick);
                    app.shop_body = k as u8;
                }
            }
            y += 38.0;
        }
        if !app.save.online() {
            let r = Rect::from_min_size(pos2(left, y), vec2(w, 28.0));
            ui.painter().text(r.left_center(), Align2::LEFT_CENTER, format!("{LOCK} Log in to buy: coins come with an account"), bold(14.0), Color32::from_rgb(0xff, 0xd8, 0x4a));
            y += 30.0;
        } else if !app.social.err.is_empty() {
            ui.painter().text(pos2(left, y), Align2::LEFT_TOP, app.social.err.clone(), bold(14.0), Color32::from_rgb(0xff, 0x8a, 0x7a));
            y += 24.0;
        }
        // the cards
        let area = Rect::from_min_max(pos2(left - 6.0, y), pos2(left + w + 6.0, sr.bottom() - 8.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        egui::ScrollArea::vertical().id_salt(("shop", app.shop_tab, app.shop_body)).max_height(area.height()).show(&mut child, |ui| {
            ui.set_width(area.width());
            let x0 = ui.cursor().left() + 6.0;
            let mut y = ui.cursor().top();
            let cols = if w > 600.0 { 4 } else if w > 380.0 { 3 } else { 2 };
            let gap = 10.0;
            let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
            let mut grid = |ui: &mut Ui, wares: &[Ware], y: &mut f32, pick: &mut Option<Ware>| {
                for (k, wr) in wares.iter().enumerate() {
                    let r = Rect::from_min_size(pos2(x0 + (k % cols) as f32 * (cw + gap), *y + (k / cols) as f32 * (cw * 1.25 + gap)), vec2(cw, cw * 1.25));
                    if card(ui, app, &tex, wr, r, false).clicked() {
                        *pick = Some(*wr);
                    }
                }
                *y += ((wares.len() + cols - 1) / cols) as f32 * (cw * 1.25 + gap);
            };
            let section = |ui: &mut Ui, t: &str, y: &mut f32| {
                ui.painter().text(pos2(x0, *y), Align2::LEFT_TOP, t, anton(24.0), Color32::WHITE);
                *y += 34.0;
            };
            let mut chosen = None;
            if app.shop_tab == 0 {
                let (big, small) = featured();
                section(ui, "FEATURED", &mut y);
                ui.painter().text(pos2(x0 + 140.0, y - 26.0), Align2::LEFT_CENTER, format!("ALL {}% OFF TODAY", net::FEATURED_OFF), bold(16.0), Color32::from_rgb(0xff, 0x6a, 0x6a));
                let bw = (w - gap) / 2.0;
                // tall, but never taller than what's on screen
                let bh = (bw * 1.35).min(area.height() - 50.0).max(bw * 0.8);
                for (k, wr) in big.iter().enumerate() {
                    let r = Rect::from_min_size(pos2(x0 + k as f32 * (bw + gap), y), vec2(bw, bh));
                    if card(ui, app, &tex, wr, r, true).clicked() {
                        chosen = Some(*wr);
                    }
                }
                y += bh + gap + 8.0;
                section(ui, "DAILY", &mut y);
                grid(ui, &small, &mut y, &mut chosen);
            } else {
                let mut wares: Vec<Ware> = match app.shop_tab {
                    8 => hats().collect(),
                    1 => costumes(app).into_iter().map(|c| c.0).filter(|w| w.rig == app.shop_body as usize).collect(),
                    t => body::items(app.shop_body as usize, t - 2).map(ware).collect(),
                };
                // the best first, then by price
                wares.sort_by_key(|w| (std::cmp::Reverse(w.rarity), std::cmp::Reverse(w.price)));
                section(ui, &TABS[app.shop_tab].to_uppercase(), &mut y);
                if app.shop_tab == 1 {
                    ui.painter().text(pos2(x0 + 150.0, y - 26.0), Align2::LEFT_CENTER, format!("{}% OFF ALL PIECES TOGETHER", net::BUNDLE_OFF), bold(16.0), Color32::from_rgb(0xff, 0x6a, 0x6a));
                }
                grid(ui, &wares, &mut y, &mut chosen);
            }
            ui.allocate_rect(Rect::from_min_size(pos2(x0, ui.cursor().top()), vec2(w, (y - ui.cursor().top()).max(0.0) + 12.0)), Sense::hover());
            pick = chosen;
        });
    });
    if let Some(w) = pick {
        app.shop_pick = Some(w.id);
    }
    if app.shop_pick.is_some() {
        detail(app, ctx, sr, &tex);
    }
    if close {
        app.sheet = Sheet::None;
        app.shop_pick = None;
        app.try_on = None;
    }
}

fn find(id: u8) -> Option<Ware> {
    net::clothes(id).map(ware).or_else(|| hats().find(|h| h.id == id))
}
fn find_in(app: &FlunkiApp, id: u8) -> Option<Ware> {
    find(id).or_else(|| costumes(app).into_iter().find(|c| c.0.id == id).map(|c| c.0))
}

/// One ware, big, with what can be done with it.
fn detail(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect, tex: &[egui::TextureHandle]) {
    let Some(w) = app.shop_pick.and_then(|i| find_in(app, i)) else { return };
    let full = ctx.content_rect();
    let mut close = false;
    egui::Area::new(egui::Id::new("shop-detail")).order(egui::Order::Tooltip).fixed_pos(full.min).show(ctx, |ui| {
        ui.set_min_size(full.size());
        // tap outside: back to the shop
        if ui.allocate_rect(full, Sense::click()).clicked() {
            close = true;
        }
        ui.painter().rect_filled(full, 0.0, Color32::from_black_alpha(170));
        // as big as fits, portrait or landscape
        let cw = (sr.width() - 40.0).min(420.0).min((sr.height() - 24.0) / 1.55);
        let r = Rect::from_center_size(sr.center(), vec2(cw, cw * 1.55));
        let (rname, light, dark) = RARITY[w.rarity as usize % 5];
        let p = ui.painter();
        let mut m = egui::Mesh::default();
        m.colored_vertex(r.left_top(), light);
        m.colored_vertex(r.right_top(), light);
        m.colored_vertex(r.right_bottom(), dark);
        m.colored_vertex(r.left_bottom(), dark);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        p.rect_filled(r, 18.0, dark);
        p.with_clip_rect(r.shrink(3.0)).add(egui::Shape::mesh(m));
        // block clicks on the card itself
        ui.allocate_rect(r, Sense::click());
        let pic = Rect::from_min_size(r.min + vec2(20.0, 18.0), vec2(cw - 40.0, cw - 40.0));
        for k in 0..8 {
            let f = 1.0 - k as f32 / 8.0;
            ui.painter().circle_filled(pic.center(), pic.width() * 0.5 * f, Color32::from_white_alpha((12.0 * (1.0 - f) + 4.0) as u8));
        }
        picture(ui, tex, &w, pic);
        let p = ui.painter();
        let mut y = pic.bottom() + 6.0;
        p.text(pos2(r.left() + 20.0, y), Align2::LEFT_TOP, rname, bold(14.0), Color32::WHITE);
        y += 20.0;
        p.text(pos2(r.left() + 20.0, y), Align2::LEFT_TOP, w.name.to_uppercase(), anton(34.0), Color32::WHITE);
        y += 42.0;
        let whose = if w.rig == 1 { "women" } else { "men" };
        let what = match (w.slot, net::bundle(w.id)) {
            (Some(s), _) => format!("{} · for {whose}", body::SLOTS[s].0),
            (None, Some(b)) => format!("Costume: {} pieces · for {whose}", b.items.len()),
            (None, None) => "Hat".to_string(),
        };
        p.text(pos2(r.left() + 20.0, y), Align2::LEFT_TOP, what, FontId::proportional(15.0), Color32::from_white_alpha(220));
        let (now, was) = today_price(app, &w);
        if let (Some(was), false) = (was, owns(app, &w)) {
            p.text(pos2(r.left() + 20.0, y + 22.0), Align2::LEFT_TOP, format!("{COIN} {now} instead of {was}: you save {}", was - now), bold(15.0), Color32::from_rgb(0xff, 0xd8, 0x4a));
        }
        // buttons: try on · buy / wear
        let bh = 48.0;
        let row = Rect::from_min_size(pos2(r.left() + 16.0, r.bottom() - bh - 16.0), vec2(cw - 32.0, bh));
        let half = (row.width() - 10.0) / 2.0;
        let try_r = Rect::from_min_size(row.min, vec2(half, bh));
        let act_r = Rect::from_min_size(row.min + vec2(half + 10.0, 0.0), vec2(half, bh));
        let tr = ui.allocate_rect(try_r, Sense::click());
        let p = ui.painter();
        p.rect_filled(try_r, 12.0, Color32::from_black_alpha(if tr.hovered() { 150 } else { 110 }));
        p.text(try_r.center(), Align2::CENTER_CENTER, "TRY ON", anton(20.0), Color32::WHITE);
        if tr.clicked() {
            audio::ui(Cue::Tap);
            app.try_on = Some(w.id);
            app.shop_trying = true;
        }
        let (owned, worn) = (owns(app, &w), wears(app, &w));
        let can_buy = app.save.online() && app.social.ready && !app.social.buying && app.save.coins >= now;
        let (label, fill, col) = if worn && w.slot.is_none() && net::bundle(w.id).is_none() {
            ("TAKE OFF".to_string(), Color32::WHITE, Color32::from_rgb(0x12, 0x1c, 0x52))
        } else if worn {
            (format!("{CHECK} WEARING"), Color32::from_black_alpha(90), Color32::from_rgb(0x8f, 0xf0, 0x8f))
        } else if owned {
            ("WEAR".to_string(), Color32::WHITE, Color32::from_rgb(0x12, 0x1c, 0x52))
        } else if can_buy {
            (format!("BUY {COIN} {now}"), Color32::from_rgb(0xff, 0xd8, 0x4a), Color32::from_rgb(0x2a, 0x1a, 0x05))
        } else {
            (format!("{LOCK} {COIN} {now}"), Color32::from_black_alpha(90), Color32::from_white_alpha(170))
        };
        let ar = ui.allocate_rect(act_r, Sense::click());
        let p = ui.painter();
        p.rect_filled(act_r, 12.0, fill);
        p.text(act_r.center(), Align2::CENTER_CENTER, label, anton(20.0), col);
        if ar.clicked() {
            if worn && w.slot.is_none() && net::bundle(w.id).is_none() {
                // a hat comes off again
                audio::ui(Cue::Tick);
                app.save.profile.hat = 0;
                app.save.store();
            } else if worn {
            } else if owned {
                audio::ui(Cue::Tick);
                wear_it(app, &w);
            } else if can_buy {
                audio::ui(Cue::TapBig);
                app.social.buy(w.id);
            } else {
                audio::ui(Cue::Denied);
                let why = if !app.save.online() { "Log in to buy things".to_string() } else { format!("{COIN} {} more coins needed", now.saturating_sub(app.save.coins)) };
                app.lock_note = Some((why, 2.2));
            }
        }
    });
    if close {
        audio::ui(Cue::Back);
        app.shop_pick = None;
    }
}

/// Trying something on: the shop steps aside for the clubhouse, where your player wears it.
fn try_bar(app: &mut FlunkiApp, ctx: &egui::Context, sr: Rect) {
    let Some(w) = app.try_on.and_then(|i| find_in(app, i)) else {
        app.shop_trying = false;
        return;
    };
    egui::Area::new(egui::Id::new("shop-try")).order(egui::Order::Foreground).pivot(Align2::CENTER_BOTTOM).fixed_pos(pos2(sr.center().x, sr.bottom() - 12.0)).show(ctx, |ui| {
        let bw = (sr.width() - 24.0).min(460.0);
        let (r, _) = ui.allocate_exact_size(vec2(bw, 64.0), Sense::hover());
        let (_, light, dark) = RARITY[w.rarity as usize % 5];
        let p = ui.painter();
        p.rect_filled(r, 16.0, dark);
        p.rect_stroke(r, 16.0, Stroke::new(2.0, light), StrokeKind::Inside);
        p.text(r.left_center() + vec2(16.0, -9.0), Align2::LEFT_CENTER, "TRYING ON", bold(12.0), Color32::from_white_alpha(200));
        p.text(r.left_center() + vec2(16.0, 10.0), Align2::LEFT_CENTER, w.name.to_uppercase(), anton(20.0), Color32::WHITE);
        let back = Rect::from_min_size(pos2(r.right() - 132.0, r.top() + 10.0), vec2(120.0, 44.0));
        let resp = ui.allocate_rect(back, Sense::click());
        let p = ui.painter();
        p.rect_filled(back, 12.0, Color32::WHITE);
        p.text(back.center(), Align2::CENTER_CENTER, "BACK", anton(20.0), Color32::from_rgb(0x12, 0x1c, 0x52));
        if resp.clicked() {
            audio::ui(Cue::Back);
            app.shop_trying = false;
            app.try_on = None;
        }
    });
}

/// Your wardrobe, in the profile sheet: body, build, and per slot what you have (free or bought).
pub(super) fn locker(app: &mut FlunkiApp, ui: &mut Ui, w: f32) -> bool {
    let tex = pages(app, &ui.ctx().clone());
    let mut mine = body::Wear::from_bytes(app.save.profile.wear);
    let before = mine;
    let mut to_shop = None;
    let seg = |ui: &mut Ui, text: &str, on: bool, width: f32| -> bool {
        let (r, resp) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
        let p = ui.painter();
        p.rect_filled(r, 10.0, if on { super::GOLD } else { Color32::from_white_alpha(if resp.hovered() { 36 } else { 20 }) });
        p.text(r.center(), Align2::CENTER_CENTER, text, bold(14.0), if on { super::INK } else { Color32::WHITE });
        resp.clicked()
    };
    super::label_caps(ui, "Body");
    ui.horizontal_wrapped(|ui| {
        for (k, (n, _)) in body::BODIES.iter().enumerate() {
            if seg(ui, n, mine.body as usize == k, 90.0) && mine.body as usize != k {
                mine = body::Wear { build: mine.build, ..body::Wear::starter(k as u8) };
            }
        }
    });
    super::label_caps(ui, "Build");
    ui.horizontal_wrapped(|ui| {
        for (k, b) in body::BUILDS.iter().enumerate() {
            if seg(ui, b.name, mine.build as usize == k, 76.0) {
                mine.build = k as u8;
            }
        }
    });
    let tile = 66.0;
    for (slot, (name, _, always)) in body::SLOTS.iter().enumerate() {
        let have: Vec<&net::Item> = body::items(mine.rig(), slot).filter(|c| app.save.has_item(c.id)).collect();
        super::label_caps(ui, name);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let worn = mine.piece(slot).map(|c| c.id);
            if !always {
                let (r, resp) = ui.allocate_exact_size(vec2(tile, tile), Sense::click());
                let p = ui.painter();
                p.rect_filled(r, 10.0, Color32::from_white_alpha(18));
                p.text(r.center(), Align2::CENTER_CENTER, "None", bold(13.0), Color32::WHITE);
                if worn.is_none() {
                    p.rect_stroke(r, 10.0, Stroke::new(2.5, super::GOLD), StrokeKind::Inside);
                }
                if resp.clicked() {
                    audio::ui(Cue::Tick);
                    mine.slots[slot] = 0;
                }
            }
            for c in &have {
                let (r, resp) = ui.allocate_exact_size(vec2(tile, tile), Sense::click());
                let (_, light, dark) = RARITY[c.rarity as usize % 5];
                let p = ui.painter();
                p.rect_filled(r, 10.0, dark);
                p.rect_filled(Rect::from_min_size(r.min, vec2(tile, tile * 0.5)), CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }, light.gamma_multiply(0.55));
                picture(ui, &tex, &ware(c), r.shrink(3.0));
                if worn == Some(c.id) {
                    ui.painter().rect_stroke(r, 10.0, Stroke::new(2.5, super::GOLD), StrokeKind::Inside);
                }
                if resp.clicked() {
                    audio::ui(Cue::Tick);
                    mine.slots[slot] = c.id;
                }
                resp.on_hover_text(c.name);
            }
            // more of these in the shop
            let (r, resp) = ui.allocate_exact_size(vec2(tile, tile), Sense::click());
            let p = ui.painter();
            p.rect_stroke(r, 10.0, Stroke::new(1.5, Color32::from_white_alpha(70)), StrokeKind::Inside);
            p.text(r.center() - vec2(0.0, 8.0), Align2::CENTER_CENTER, SHOP, FontId::proportional(20.0), Color32::WHITE);
            p.text(r.center() + vec2(0.0, 16.0), Align2::CENTER_CENTER, "MORE", bold(11.0), Color32::WHITE);
            if resp.clicked() {
                audio::ui(Cue::Tap);
                to_shop = Some(slot + 2);
            }
        });
    }
    let _ = w;
    if let Some(tab) = to_shop {
        app.open_shop();
        app.shop_tab = tab;
        app.shop_body = mine.body;
    }
    if mine != before {
        app.save.profile.wear = mine.to_bytes();
        return true;
    }
    false
}
