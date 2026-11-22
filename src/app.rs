//! The eframe app: state, input, camera, scene building, online glue. Screens live in `ui.rs`.

use crate::arenas::{self, ArenaKey, Env};
use crate::audio::Audio;
use crate::game::{self, AMode, Drill, Foot, Game, Mode, Person, Sfx, StatEv, State, ThrowSpec};
use crate::gfx::{DrawList, FrameData, Gfx, Globals, SceneCallback};
use crate::model::Model;
use crate::net::{self, FromServer, Input, Net, ToServer};
use crate::save::{self, MatchRec, Save, XpResult};
use crate::scene::{self, hex, Cam, Look, SceneOpts};
use crate::social::Got;
use crate::voice;
use egui::{pos2, vec2, Color32, FontFamily, FontId, Pos2, Rect};
use glam::{vec3, Mat4, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) const MOUSE: u64 = u64::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum View {
    Menu,
    Club,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Screen {
    Home,
    Play,
    Playing,
    Paused,
    Over,
    TrainOver,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sheet {
    None,
    Profile,
    Stats,
    Progress,
    Train,
    Settings,
    Rules,
    Friends,
    /// log in or sign up (the web version)
    Account,
    /// tell us what went wrong
    Bug,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PlayStep {
    Choose,
    Cpu,
    Hot,
    Online,
}

/// Which in-match menu is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MatchMenu {
    None,
    Chat,
    Emotes,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum CtlKind {
    Drink(usize),
    Pad(usize, Foot),
    Sneak,
    Qm,
    Emote,
}
#[derive(Clone, Copy)]
pub(crate) struct Ctl {
    pub kind: CtlKind,
    pub rect: Rect,
    pub round: bool,
}

pub(crate) struct Flick {
    pub pts: Vec<(f64, Pos2)>,
    pub cur: Pos2,
    pub s: Pos2,
    pub prev_ang: Option<f32>,
    pub acc: f32,
    pub armed: i32,
    pub spin_vis: f32,
}

pub(crate) enum PressUse {
    Flick,
    Drink(usize),
    Other,
}

pub(crate) struct Bubble {
    pub t: usize,
    pub i: usize,
    pub msg: usize,
    pub life: f32,
}

/// Running tallies for the stats page while a match is on.
#[derive(Default)]
pub(crate) struct StatsCur {
    pub rec: MatchRec,
    pub team: Option<usize>,
    pub t0: f64,
    pub last_mine: bool,
    pub run_start: f64,
    pub prev: [f32; 3],
}

/// The 3D render scales dynamic resolution goes between.
const SCALE_STEPS: [f32; 6] = [0.45, 0.55, 0.65, 0.75, 0.875, 1.0];

pub struct FlunkiApp {
    pub(crate) game: Game,
    pub(crate) model: Arc<Model>,
    pub(crate) audio: Audio,
    pub(crate) envs: HashMap<&'static str, Env>,
    pub(crate) save: Save,
    pub(crate) view: View,
    pub(crate) screen: Screen,
    pub(crate) sheet: Sheet,
    pub(crate) step: PlayStep,
    pub(crate) arena: ArenaKey,
    pub(crate) match_arena: ArenaKey,
    pub(crate) last_shown: ArenaKey,
    pub(crate) cam_pos: Vec3,
    pub(crate) cam_look: Vec3,
    pub(crate) cam_snap: bool,
    pub(crate) cam: Cam,
    pub(crate) flick: Option<Flick>,
    pub(crate) presses: Vec<(u64, PressUse)>,
    pub(crate) touch_seen: bool,
    pub(crate) aim_ball: Vec3,
    pub(crate) ring_r: f32,
    pub(crate) banner: Option<(game::Banner, f32)>,
    pub(crate) ref_mouth: Vec3,
    pub(crate) ref_tip: Vec3,
    pub(crate) heads: [[Vec3; 3]; 2],
    pub(crate) space_down: bool,
    pub(crate) hint: Option<(String, f32)>,
    pub(crate) time: f64,
    pub(crate) frame_list: DrawList,
    pub(crate) cam_override: Option<(Vec3, Vec3)>,
    pub(crate) hub_person: Person,
    pub(crate) hub_cheer: f32,
    pub(crate) last_xp: Option<XpResult>,
    pub(crate) stats_cur: Option<StatsCur>,
    pub(crate) train_res: Option<(Drill, f32, String, bool, Option<f32>)>,
    pub(crate) train_over_t: f32,
    pub(crate) over_t: f32,
    pub(crate) lock_note: Option<(String, f32)>,
    pub(crate) menu: MatchMenu,
    frames: u32,
    t0: f64,
    pub(crate) qm_sent: f64,
    pub(crate) emote_sent: f64,
    pub(crate) bubbles: Vec<Bubble>,
    pub(crate) qm_muted: bool,
    pub(crate) cpu_qm: Option<(usize, f32)>,
    pub(crate) reset_armed: f32,
    pub(crate) net: Option<Net>,
    pub(crate) social: crate::social::Social,
    pub(crate) net_err: String,
    /// the connection is kept open all the time: when to try again after it went, and how often it has
    net_retry: (f64, u32),
    pub(crate) code_in: String,
    pub(crate) name_edit: String,
    pub(crate) screen_rect: Rect,
    /// safe area (top, right, bottom, left) kept clear of the notch and the home bar
    pub(crate) safe: [f32; 4],
    pub(crate) safe_t: f64,
    /// test hook: walk the clubhouse player round in a circle at this speed
    pub(crate) lab_speed: f32,
    pub(crate) lab_ang: f32,
    pub(crate) lab_mode: String,
    /// Test hook: mesh kinds left out of the picture (performance checks).
    pub(crate) hide_meshes: Vec<u8>,
    pub(crate) fps_dt: f32,
    pub(crate) render_scale: f32,
    /// the refresh rate asked of the screen (Android), so it's asked again only when the setting changes
    #[cfg(target_os = "android")]
    fps_asked: Option<bool>,
    pub(crate) scale_lock: bool,
    /// dynamic resolution: how long it's been too slow and fast enough, and no step up before this
    scale_wait: (f32, f32),
    scale_up_after: f64,
    /// test hook: no egui animations
    no_anim: bool,
    /// the pixel density and banner size the fonts were laid out for up front (`ui::warm_fonts`)
    warmed: (f32, f32),
    /// a camera flight through the clubhouse door: (elapsed, from position, from look, into the hall?)
    pub(crate) fly: Option<(f32, Vec3, Vec3, bool)>,
    /// where we were last frame, to close menus when you move on
    pub(crate) last_nav: (Screen, View, PlayStep, Sheet),
    /// the login form
    pub(crate) login_user: String,
    pub(crate) login_pass: String,
    /// progression: last sent to the server, when, and whether the server's copy has been seen yet
    prog_sent: String,
    prog_t: f64,
    prog_synced: bool,
    /// what the phone's background check was last told (server, code, secret, on), and when we last
    /// looked for a tapped notification
    notes_told: (String, String, String, bool),
    open_t: f64,
    /// the bug report being written
    pub(crate) bug_text: String,
}

impl FlunkiApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let t0 = now_ms();
        let model = Arc::new(Model::load(include_bytes!("../assets/person.glb")).expect("person model"));
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            // a GPU validation error (browsers differ in what they accept) costs a frame, not the game
            rs.device.on_uncaptured_error(std::sync::Arc::new(|e: egui_wgpu::wgpu::Error| {
                log_str(&format!("gpu error: {e}"));
            }));
            let gfx = Gfx::new(rs, &model);
            rs.renderer.write().callback_resources.insert(gfx);
        }
        log_str(&format!("startup: model and renderer {:.0} ms", now_ms() - t0));
        crate::ui::setup_style(&cc.egui_ctx);
        let seed = (now_ms() * 1000.0) as u64 ^ 0x9e37_79b9;
        let mut game = Game::new(seed);
        game.to_menu();
        // every place is built up front so switching never stalls
        let mut envs = HashMap::new();
        for k in [ArenaKey::Vondel, ArenaKey::Roof, ArenaKey::Cafe, ArenaKey::Club, ArenaKey::Krakau, ArenaKey::Hub] {
            envs.insert(k.env_id(), arenas::build(k));
        }
        log_str(&format!("startup: places built at {:.0} ms", now_ms() - t0));
        let save = Save::load();
        let arena = ArenaKey::from_id(&save.arena);
        let mut app = FlunkiApp {
            game,
            model,
            audio: Audio::new(),
            envs,
            name_edit: save.profile.name.clone(),
            arena: if save.unlocked(save::Unlock::Arena(arena.id())) { arena } else { ArenaKey::Vondel },
            save,
            view: View::Menu,
            screen: Screen::Home,
            sheet: Sheet::None,
            step: PlayStep::Choose,
            match_arena: ArenaKey::Vondel,
            last_shown: ArenaKey::Vondel,
            cam_pos: vec3(0.0, 6.0, 14.0),
            cam_look: Vec3::ZERO,
            cam_snap: true,
            cam: Cam { pos: vec3(0.0, 6.0, 14.0), look: Vec3::ZERO, fov: 0.9, aspect: 1.0 },
            flick: None,
            presses: Vec::new(),
            touch_seen: false,
            aim_ball: Vec3::ZERO,
            ring_r: 1.0,
            banner: None,
            ref_mouth: game::REF_POS,
            ref_tip: game::REF_POS,
            heads: [[Vec3::ZERO; 3]; 2],
            space_down: false,
            hint: None,
            time: 0.0,
            frame_list: DrawList::default(),
            cam_override: None,
            hub_person: Person::new(arenas::HUB_O, 0.25),
            hub_cheer: 0.0,
            last_xp: None,
            stats_cur: None,
            train_res: None,
            train_over_t: 0.0,
            over_t: -1.0,
            lock_note: None,
            menu: MatchMenu::None,
            frames: 0,
            t0,
            qm_sent: -10.0,
            emote_sent: -10.0,
            bubbles: Vec::new(),
            qm_muted: false,
            cpu_qm: None,
            reset_armed: 0.0,
            net: None,
            social: Default::default(),
            net_err: String::new(),
            net_retry: (0.0, 0),
            code_in: String::new(),
            screen_rect: Rect::from_min_size(Pos2::ZERO, vec2(400.0, 800.0)),
            safe: [0.0; 4],
            safe_t: -1.0,
            lab_speed: 0.0,
            lab_ang: 0.0,
            lab_mode: String::new(),
            hide_meshes: Vec::new(),
            fps_dt: 0.016,
            render_scale: 1.0,
            #[cfg(target_os = "android")]
            fps_asked: None,
            scale_lock: false,
            scale_wait: (0.0, 0.0),
            scale_up_after: 0.0,
            no_anim: false,
            warmed: (0.0, 0.0),
            fly: None,
            last_nav: (Screen::Home, View::Menu, PlayStep::Choose, Sheet::None),
            login_user: String::new(),
            login_pass: String::new(),
            prog_sent: String::new(),
            prog_t: 0.0,
            prog_synced: false,
            notes_told: Default::default(),
            open_t: 0.0,
            bug_text: String::new(),
        };
        app.apply_volume();
        app
    }

    pub(crate) fn apply_volume(&mut self) {
        self.audio.volume = self.save.settings.volume as f32 / 100.0;
    }
    pub(crate) fn play(&mut self, s: Sfx) {
        self.audio.play(s);
        if self.save.settings.vibrate {
            match s {
                Sfx::Hit => voice::vibrate(40),
                Sfx::Straf => voice::vibrate(120),
                Sfx::Cough => voice::vibrate(200),
                _ => {}
            }
        }
    }

    // ---------- starting and leaving ----------
    pub(crate) fn start(&mut self, mode: Mode) {
        self.audio.unlock();
        self.leave_online();
        self.match_arena = self.arena;
        self.game.bounds = self.match_arena.bounds();
        self.game.start_match(mode, self.save.diff);
        self.after_start();
        self.stats_begin();
    }
    fn after_start(&mut self) {
        self.screen = Screen::Playing;
        self.sheet = Sheet::None;
        self.banner = None;
        self.flick = None;
        self.menu = MatchMenu::None;
        self.bubbles.clear();
        self.last_xp = None;
        self.over_t = -1.0;
        self.cam_snap = true;
    }
    pub(crate) fn start_tutorial(&mut self) {
        self.audio.unlock();
        self.leave_online();
        self.match_arena = ArenaKey::Vondel;
        self.game.bounds = None;
        self.game.start_tutorial();
        self.after_start();
        self.stats_cur = None;
    }
    pub(crate) fn start_drill(&mut self, k: Drill) {
        self.audio.unlock();
        if matches!(self.shown_arena(), ArenaKey::Hub | ArenaKey::Gym) {
            // walk through the door into the training hall
            self.fly = Some((0.0, self.cam_pos, self.cam_look, true));
        }
        self.leave_online();
        self.match_arena = ArenaKey::Gym;
        self.game.bounds = ArenaKey::Gym.bounds();
        self.game.begin_train(k);
        self.after_start();
        self.stats_cur = None;
        self.train_res = None;
    }
    pub(crate) fn go_home(&mut self, v: View) {
        if v == View::Club && self.shown_arena() == ArenaKey::Gym {
            self.fly = Some((0.0, self.cam_pos, self.cam_look, false));
        }
        self.leave_online();
        self.game.to_menu();
        self.game.bounds = None;
        self.view = v;
        self.screen = Screen::Home;
        self.sheet = Sheet::None;
        self.step = PlayStep::Choose;
        self.banner = None;
        self.flick = None;
        self.menu = MatchMenu::None;
        self.stats_cur = None;
        self.cam_snap = true;
    }

    /// Which place is on screen right now.
    pub(crate) fn shown_arena(&self) -> ArenaKey {
        let home = if self.view == View::Club { ArenaKey::Hub } else { ArenaKey::Vondel };
        match self.screen {
            Screen::Home => home,
            Screen::Play => match self.step {
                PlayStep::Choose => home,
                PlayStep::Online => match self.net.as_ref() {
                    Some(n) if n.in_room && !n.host() => ArenaKey::from_id(&n.arena),
                    Some(n) if n.in_room => self.arena,
                    _ => home,
                },
                _ => self.arena,
            },
            _ => self.match_arena,
        }
    }

    // ---------- looks ----------
    pub(crate) fn my_look(&self) -> [u8; 7] {
        let p = &self.save.profile;
        [p.color as u8, p.skin as u8, p.hair as u8, p.shorts as u8, p.voice as u8, p.hat, self.save.level().min(255) as u8]
    }
    pub(crate) fn look_of(l: &[u8; 7], i: usize) -> Look {
        let shirt = hex(save::SWATCHES[l[0] as usize % 8]);
        let mut lk = scene::player_look(shirt, i, i);
        lk.skin = hex(save::SKIN_OPTS[l[1] as usize % 6]);
        lk.hair = hex(save::HAIR_OPTS[l[2] as usize % 6]);
        lk.shorts = hex(save::SHORTS_OPTS[l[3] as usize % 6]);
        lk.hat = l[5].min(5);
        lk
    }
    fn looks(&self) -> [[Look; 3]; 2] {
        let g = &self.game;
        let blue = hex(0x2f86b8);
        let gold = hex(0xe8b02a);
        let mut out = [[scene::player_look(gold, 0, 0); 3]; 2];
        for t in 0..2 {
            for i in 0..3 {
                out[t][i] = scene::player_look(if t == 0 { gold } else { blue }, i, t * 3 + i);
            }
        }
        match g.mode {
            Mode::Cpu => {
                let me = self.my_look();
                let col = self.save.profile.color;
                for i in 0..3 {
                    out[0][i] = scene::player_look(hex(save::SWATCHES[col]), i, i);
                }
                if col == 5 {
                    for i in 0..3 {
                        out[1][i] = scene::player_look(gold, i, 3 + i); // the computer switches to gold if you wear blue
                    }
                }
                out[0][0] = Self::look_of(&me, 0);
            }
            Mode::Online => {
                if let Some(n) = self.net.as_ref() {
                    // a team wears its first player's colour; if both teams picked the same, the second one changes
                    let first = |t: u8| n.humans(t).first().map(|m| m.look[0] as usize);
                    let c0 = first(0).unwrap_or(0);
                    let mut c1 = first(1).unwrap_or(5);
                    if c1 == c0 {
                        c1 = if c0 == 5 { 0 } else { 5 };
                    }
                    for t in 0..2 {
                        let hs = n.humans(t as u8);
                        let col = hex(save::SWATCHES[if t == 0 { c0 } else { c1 }]);
                        for i in 0..3 {
                            let mut lk = scene::player_look(col, i, t * 3 + i);
                            if let Some(o) = n.owner(t, i) {
                                if i < hs.len() {
                                    lk = Self::look_of(&o.look, i);
                                    lk.shirt = scene::player_look(col, i, 0).shirt;
                                }
                            }
                            out[t][i] = lk;
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }

    // ---------- camera ----------
    fn cam_targets(&self, w: f32, h: f32) -> (Vec3, Vec3) {
        let g = &self.game;
        let t = g.clock;
        let portrait = h > w;
        if self.shown_arena() == ArenaKey::Hub {
            // the same framing as the JavaScript version: you in the middle of the mess
            let peek = self.sheet == Sheet::Profile || self.screen == Screen::Play;
            if peek {
                let o = arenas::HUB_O;
                return if portrait { (o + vec3(0.25, 1.25, 3.9), o + vec3(0.05, 0.3, 0.0)) } else { (o + vec3(0.75, 1.3, 3.5), o + vec3(0.75, 0.95, 0.0)) };
            }
            let o = arenas::HUB_O;
            return if portrait { (o + vec3(0.7 + (t * 0.21).sin() * 0.1, 1.6, 3.55), o + vec3(0.12, 0.62, 0.0)) } else { (o + vec3(0.4 + (t * 0.21).sin() * 0.12, 1.55, 3.15), o + vec3(0.95, 1.0, 0.0)) };
        }
        if matches!(self.screen, Screen::Home | Screen::Play) || g.mode == Mode::Demo {
            let a = t * 0.08;
            let r = if portrait { 15.0 } else { 13.5 };
            return (vec3(a.sin() * r, if portrait { 9.0 } else { 6.5 }, a.cos() * r), vec3(0.0, if portrait { -2.6 } else { -0.6 }, 0.0));
        }
        if g.train.as_ref().map_or(false, |tr| tr.kind == Drill::Drink) {
            let p = g.teams[0].homes[0];
            let s = g.teams[0].side;
            return (vec3(p.x + 0.55, 1.35, p.z - s * 2.7), vec3(p.x, 0.72, p.z));
        }
        if g.tut.map_or(false, |k| k.hold()) {
            let s = g.teams[0].side;
            return (vec3(0.0, if portrait { 11.0 } else { 8.0 }, s * if portrait { 11.5 } else { 11.0 }), vec3(0.0, 0.0, -s * 0.6));
        }
        let me = self.persp();
        let s = g.teams[me].side;
        let thrower_mine = g.my_players[g.turn][g.teams[g.turn].idx];
        if g.state == State::Aim && thrower_mine {
            let p = g.teams[g.turn].players[g.teams[g.turn].idx].pos;
            return (vec3(p.x, 1.66 + (t * 1.5).sin() * 0.006, p.z + g.teams[g.turn].side * 0.05), vec3(0.0, 0.2, 0.0));
        }
        // once thrown, `idx` is the next thrower already: a teammate's, when people share a team
        let threw_mine = g.thrown_by.map_or(false, |(t, i)| g.my_players[t][i]);
        if g.state == State::Flight && threw_mine {
            let bx = g.ball.x;
            return (vec3(bx * 0.4, 2.3, g.teams[g.turn].side * (game::LINE + 1.6)), vec3(bx * 0.5, 0.3, 0.0));
        }
        if matches!(g.state, State::Aim | State::CpuAim | State::Flight) {
            return (vec3(2.4 * s, 4.6, s * 11.8), vec3(0.0, 0.2, -s * 1.4));
        }
        (vec3(0.0, if portrait { 11.0 } else { 8.0 }, s * if portrait { 11.5 } else { 11.0 }), vec3(0.0, 0.0, -s * 0.6))
    }
    pub(crate) fn persp(&self) -> usize {
        let g = &self.game;
        match g.mode {
            Mode::Hot => g.turn,
            _ => g.my_team.unwrap_or(0),
        }
    }

    // ---------- controls ----------
    pub(crate) fn mine_team(&self, t: usize) -> bool {
        let g = &self.game;
        match g.mode {
            Mode::Hot => true,
            Mode::Demo => false,
            _ => g.my_team == Some(t),
        }
    }
    /// The players on team `t` this screen works (all of them offline, your own online).
    pub(crate) fn who(&self, t: usize) -> game::Who {
        self.game.my_players[t]
    }
    /// The one of yours whose DRINK button and pads are shown.
    pub(crate) fn lead(&self, t: usize) -> Option<usize> {
        self.game.teams[t].lead(self.who(t))
    }
    pub(crate) fn show_drink(&self, t: usize) -> bool {
        let g = &self.game;
        let who = self.who(t);
        // your own beer: once it's empty, your teammates drink on without you
        let beer = (0..3).any(|i| who[i] && g.teams[t].beers[i] > 0.0);
        let stop = (0..3).any(|i| who[i] && g.teams[t].dc[i].stop_show > 0.0);
        self.mine_team(t) && ((g.can_drink(t) && beer) || stop)
    }
    pub(crate) fn show_pads(&self, t: usize) -> bool {
        let g = &self.game;
        let who = self.who(t);
        // only the people whose players run get pads: before the hit the next two to throw, then the runners
        let runs = if g.state == State::Run { g.running_of(t, who) != (false, false) } else { g.will_run(t, who) };
        self.mine_team(t) && runs && !self.show_drink(t) && g.train.as_ref().map_or(true, |tr| tr.kind != Drill::Drink) && (g.can_run(t) || g.false_start_window(t) || (g.state == State::Run && g.hitter == Some(1 - t)))
    }
    pub(crate) fn qm_available(&self) -> bool {
        let g = &self.game;
        self.screen == Screen::Playing && matches!(g.mode, Mode::Cpu | Mode::Online) && g.train.is_none() && g.tut.is_none()
    }
    pub(crate) fn controls(&self, sr: Rect) -> Vec<Ctl> {
        let mut v = Vec::new();
        if self.screen != Screen::Playing {
            return v;
        }
        let g = &self.game;
        let w = sr.width();
        let hot = g.mode == Mode::Hot;
        for t in 0..2 {
            let cx = sr.left() + if hot { if t == 0 { w * 0.25 } else { w * 0.75 } } else { w * 0.5 };
            if self.show_drink(t) {
                let r = if hot { 56.0f32.min(w * 0.16) } else { 66.0 };
                v.push(Ctl { kind: CtlKind::Drink(t), rect: Rect::from_center_size(pos2(cx, sr.bottom() - 40.0 - r), vec2(r * 2.0, r * 2.0)), round: true });
            } else if self.show_pads(t) {
                // room for the round buttons on either side
                let pw = if hot { (w * 0.2).min(100.0) } else { (w / 2.0 - 83.0).clamp(70.0, 108.0) };
                for (f, sx) in [(Foot::L, -1.0f32), (Foot::R, 1.0)] {
                    let c = pos2(cx + sx * (pw * 0.5 + 5.0), sr.bottom() - 98.0);
                    v.push(Ctl { kind: CtlKind::Pad(t, f), rect: Rect::from_center_size(c, vec2(pw, 124.0)), round: false });
                }
            }
        }
        // the round buttons have their own corners, and stay there whatever else comes and goes
        let y = sr.bottom() - 98.0;
        if g.sneak_allowed() || g.sneak_progress().is_some() {
            v.push(Ctl { kind: CtlKind::Sneak, rect: Rect::from_center_size(pos2(sr.right() - 44.0, y), vec2(68.0, 68.0)), round: true });
        }
        if self.qm_available() {
            v.push(Ctl { kind: CtlKind::Qm, rect: Rect::from_center_size(pos2(sr.left() + 40.0, y), vec2(52.0, 52.0)), round: true });
            v.push(Ctl { kind: CtlKind::Emote, rect: Rect::from_center_size(pos2(sr.left() + 40.0, y - 62.0), vec2(52.0, 52.0)), round: true });
        }
        v
    }
    fn hit(&self, sr: Rect, p: Pos2) -> Option<CtlKind> {
        self.controls(sr).into_iter().find(|c| if c.round { c.rect.center().distance(p) <= c.rect.width() * 0.5 + 6.0 } else { c.rect.expand(6.0).contains(p) }).map(|c| c.kind)
    }

    // the same actions, whether this screen runs the match or mirrors the server's
    /// Online: send what the player did. Returns its number, which the snapshots acknowledge.
    fn send_input(&mut self, i: Input) -> u32 {
        self.game.pred.sent_n += 1;
        let n = self.game.pred.sent_n;
        if let Some(net) = self.net.as_mut() {
            net.send(ToServer::Input { n, i });
        }
        n
    }
    pub(crate) fn act_drink(&mut self, t: usize, hold: bool) {
        if self.game.mirror {
            let n = self.send_input(Input::Drink { hold, at: now_ms() });
            let g = &mut self.game;
            g.pred.hold_n = n;
            let can = g.state == State::Run && g.hitter == Some(t) && g.net_pause.is_none();
            for i in 0..3 {
                let beer = g.teams[t].beers[i] > 0.0;
                let d = &mut g.teams[t].dc[i];
                if !g.my_players[t][i] {
                    continue;
                }
                if !hold {
                    d.hold = false;
                } else if can && beer && d.cough <= 0.0 {
                    d.hold = true;
                    d.ht = 0.0;
                }
            }
            return;
        }
        let who = self.who(t);
        if hold {
            self.game.drink_press(t, who);
        } else {
            self.game.drink_release(t, who);
        }
    }
    pub(crate) fn act_pad(&mut self, t: usize, f: Foot) {
        if self.game.mirror {
            let n = self.send_input(Input::Pad { foot: f as u8, at: now_ms() });
            let who = self.who(t);
            let Some(k) = self.lead(t) else { return };
            let g = &mut self.game;
            g.pred.pad_n = n;
            let run = g.state == State::Run && g.hitter == Some(1 - t) && g.net_run && g.net_pause.is_none() && g.running_of(t, who) != (false, false);
            let (lock, last) = (g.teams[t].rc[k].lock, g.teams[t].rc[k].last);
            let stumble = last == Some(f);
            for i in (0..3).filter(|&i| who[i]) {
                let rc = &mut g.teams[t].rc[i];
                rc.down[f as usize] = 0.08;
                if run && lock <= 0.0 {
                    if stumble {
                        rc.msg = "Stumble! Alternate L and R".into();
                        rc.msg_t = 0.9;
                    }
                    rc.last = Some(f);
                }
            }
            if run && lock <= 0.0 && !stumble {
                self.game.pred.skip_step += 1;
                self.play(Sfx::Step);
            }
            return;
        }
        let who = self.who(t);
        self.game.pad_press(t, who, f);
    }
    pub(crate) fn act_sneak(&mut self) {
        if self.game.mirror {
            self.send_input(Input::Sneak);
            return;
        }
        self.game.start_sneak();
    }
    fn act_throw(&mut self, spec: ThrowSpec) {
        if self.game.mirror {
            if self.game.net_pause.is_some() {
                self.hint = Some(("The match is paused".into(), 1.4));
                return;
            }
            let n = self.send_input(Input::Throw { o: spec.origin.to_array(), yaw: spec.yaw, dist: spec.dist, curve: spec.curve, ring: spec.ring });
            self.game.predict_throw(spec, n);
            self.play(Sfx::Throw);
            return;
        }
        let t = self.game.turn;
        self.game.launch(t, spec);
    }

    fn press(&mut self, id: u64, p: Pos2, sr: Rect) {
        self.audio.unlock();
        if self.screen != Screen::Playing || self.sheet != Sheet::None || self.menu != MatchMenu::None {
            return;
        }
        if let Some(k) = self.hit(sr, p) {
            match k {
                CtlKind::Drink(t) => {
                    self.act_drink(t, true);
                    self.presses.push((id, PressUse::Drink(t)));
                }
                CtlKind::Pad(t, f) => {
                    self.act_pad(t, f);
                    self.presses.push((id, PressUse::Other));
                }
                CtlKind::Sneak => {
                    self.act_sneak();
                    self.presses.push((id, PressUse::Other));
                }
                CtlKind::Qm => {
                    self.menu = MatchMenu::Chat;
                    self.presses.push((id, PressUse::Other));
                }
                CtlKind::Emote => {
                    self.menu = MatchMenu::Emotes;
                    self.presses.push((id, PressUse::Other));
                }
            }
            return;
        }
        if self.game.local_aiming() && p.y > sr.top() + sr.height() * 0.35 && self.flick.is_none() {
            self.flick = Some(Flick { pts: vec![(self.time * 1000.0, p)], cur: p, s: p, prev_ang: None, acc: 0.0, armed: 0, spin_vis: 0.0 });
            self.presses.push((id, PressUse::Flick));
        }
    }
    fn moved(&mut self, id: u64, p: Pos2) {
        let Some((_, u)) = self.presses.iter().find(|(i, _)| *i == id) else { return };
        if !matches!(u, PressUse::Flick) {
            return;
        }
        let now = self.time * 1000.0;
        let mut spin = false;
        if let Some(f) = self.flick.as_mut() {
            let tt = f.pts.last().map_or(now, |l| l.0.max(now));
            f.pts.push((tt, p));
            while f.pts.len() > 2 && tt - f.pts[0].0 > 400.0 {
                f.pts.remove(0);
            }
            let d = p - f.s;
            if d.length_sq() >= 16.0 {
                let ang = d.y.atan2(d.x);
                if let Some(pa) = f.prev_ang {
                    let da = game::wrap_a(ang - pa);
                    if da.abs() < 1.1 {
                        f.acc += da;
                    }
                }
                f.prev_ang = Some(ang);
                f.s = p;
                let armed = if f.acc.abs() > std::f32::consts::TAU { f.acc.signum() as i32 } else { 0 };
                if armed != 0 && f.armed == 0 {
                    spin = true;
                }
                f.armed = armed;
            }
            f.cur = p;
        }
        if spin {
            self.play(Sfx::Spin);
        }
    }
    fn release(&mut self, id: u64, sr: Rect) {
        let Some(ix) = self.presses.iter().position(|(i, _)| *i == id) else { return };
        let (_, u) = self.presses.remove(ix);
        match u {
            PressUse::Drink(t) => self.act_drink(t, false),
            PressUse::Flick => self.release_flick(sr),
            PressUse::Other => {}
        }
    }
    pub(crate) fn release_flick(&mut self, sr: Rect) {
        let Some(f) = self.flick.take() else { return };
        if !self.game.local_aiming() {
            return;
        }
        let now = self.time * 1000.0;
        let last = *f.pts.last().unwrap();
        let mut first = *f.pts.iter().find(|p| last.0 - p.0 <= 85.0).unwrap_or(&f.pts[0]);
        if first.0 == last.0 && f.pts.len() > 1 {
            first = *f.pts.iter().rev().find(|p| p.0 < last.0).unwrap_or(&f.pts[0]);
        }
        let dt = ((last.0 - first.0) / 1000.0).max(0.016) as f32;
        let vx = (last.1.x - first.1.x) / dt;
        let vy = (last.1.y - first.1.y) / dt;
        let h = sr.height();
        if -vy < h * 0.9 || now - last.0 > 220.0 {
            self.hint = Some(("Flick it: swipe the ball up fast to throw".into(), 1.4));
            return;
        }
        let sp = -vy / h;
        let mut dist = (sp * 2.0 * self.save.settings.sens).clamp(1.2, 13.0);
        let b = self.aim_ball;
        let d = vec3(-b.x, 0.0, -b.z).normalize_or(Vec3::NEG_Z);
        let a = vx.atan2(-vy) * 0.5;
        let (rx, rz) = (-d.z, d.x);
        let dirx = d.x * a.cos() + rx * a.sin();
        let dirz = d.z * a.cos() + rz * a.sin();
        let mut yaw = dirx.atan2(dirz) + self.game.rng.gauss() * 0.02;
        dist *= 1.0 + self.game.rng.gauss() * 0.05;
        if self.game.tut.is_some() {
            // a gentle aim assist while learning
            yaw += game::wrap_a((-b.x).atan2(-b.z) - yaw) * 0.6;
            dist += ((b.x * b.x + b.z * b.z).sqrt() - 0.25 - dist) * 0.5;
        }
        self.act_throw(ThrowSpec { origin: b, yaw, dist, curve: -(f.armed as f32), ring: game::ring_idx(self.ring_r) });
        self.hint = None;
    }

    fn handle_input(&mut self, ctx: &egui::Context, sr: Rect) {
        let evs = ctx.input(|i| i.events.clone());
        let typing = ctx.egui_wants_keyboard_input();
        for ev in evs {
            match ev {
                egui::Event::Touch { id, phase, pos, .. } => {
                    self.touch_seen = true;
                    match phase {
                        egui::TouchPhase::Start => self.press(id.0, pos, sr),
                        egui::TouchPhase::Move => self.moved(id.0, pos),
                        egui::TouchPhase::End | egui::TouchPhase::Cancel => self.release(id.0, sr),
                    }
                }
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } if !self.touch_seen => {
                    if pressed {
                        self.press(MOUSE, pos, sr)
                    } else {
                        self.release(MOUSE, sr)
                    }
                }
                egui::Event::PointerMoved(pos) if !self.touch_seen => self.moved(MOUSE, pos),
                egui::Event::PointerGone if !self.touch_seen => self.release(MOUSE, sr),
                egui::Event::Key { key, pressed, repeat, .. } if !typing => self.key(key, pressed, repeat),
                _ => {}
            }
        }
    }
    fn key(&mut self, key: egui::Key, pressed: bool, repeat: bool) {
        if matches!(key, egui::Key::Escape | egui::Key::BrowserBack) && pressed {
            let handled = self.back();
            #[cfg(target_os = "android")]
            if !handled {
                crate::android::leave();
            }
            let _ = handled;
            return;
        }
        if self.screen != Screen::Playing {
            return;
        }
        let Some(t) = self.game.my_team.filter(|_| self.game.mode != Mode::Hot) else { return };
        match key {
            egui::Key::Space => {
                if pressed && !repeat && !self.space_down {
                    self.space_down = true;
                    self.act_drink(t, true);
                } else if !pressed && self.space_down {
                    self.space_down = false;
                    self.act_drink(t, false);
                }
            }
            egui::Key::ArrowLeft if pressed && !repeat => self.act_pad(t, Foot::L),
            egui::Key::ArrowRight if pressed && !repeat => self.act_pad(t, Foot::R),
            egui::Key::S if pressed && !repeat => self.act_sneak(),
            _ => {}
        }
    }
    /// Escape or the phone's back button: close what's open, step back one screen.
    pub(crate) fn back(&mut self) -> bool {
        if self.menu != MatchMenu::None {
            self.menu = MatchMenu::None;
            return true;
        }
        if self.sheet != Sheet::None {
            self.sheet = Sheet::None;
            return true;
        }
        match self.screen {
            Screen::Playing => self.screen = Screen::Paused,
            Screen::Paused => self.screen = Screen::Playing,
            Screen::Over | Screen::TrainOver => {
                let v = if self.game.train.is_some() { View::Club } else { self.view };
                self.go_home(v);
            }
            Screen::Play => {
                if self.step == PlayStep::Online && self.net.as_ref().map_or(false, |n| n.in_room) {
                    self.leave_online();
                } else if self.step != PlayStep::Choose {
                    self.step = PlayStep::Choose;
                } else {
                    self.screen = Screen::Home;
                }
            }
            Screen::Home => {
                if self.view == View::Club {
                    self.view = View::Menu;
                    self.cam_snap = true;
                } else {
                    return false;
                }
            }
        }
        true
    }

    // ---------- quick messages ----------
    pub(crate) fn send_qm(&mut self, i: usize) {
        if !self.save.unlocked(save::Unlock::Quick(i)) {
            return;
        }
        if self.time - self.qm_sent < 1.2 {
            return;
        }
        self.qm_sent = self.time;
        self.menu = MatchMenu::None;
        let me = self.net.as_ref().map(|n| n.id);
        self.show_qm(me, i);
        match self.game.mode {
            Mode::Online => {
                if let (Some(n), Some(id)) = (self.net.as_mut(), me) {
                    let _ = id;
                    n.send(ToServer::Qm { i: i as u8 });
                }
            }
            Mode::Cpu => {
                if self.game.rng.f() < 0.45 {
                    let reply = [0usize, 1, 6, 7, 8, 4][(self.game.rng.f() * 6.0) as usize % 6];
                    self.cpu_qm = Some((reply, 0.9 + self.game.rng.f() * 0.9));
                }
            }
            _ => {}
        }
    }
    /// Do an emote: right here, and on everyone else's screen.
    pub(crate) fn send_emote(&mut self, i: usize) {
        if !self.save.unlocked(save::Unlock::Emote(i)) {
            return;
        }
        if self.time - self.emote_sent < 1.0 {
            return;
        }
        self.emote_sent = self.time;
        self.menu = MatchMenu::None;
        let me = self.net.as_ref().map(|n| n.id);
        if !self.show_emote(me, i) {
            self.hint = Some(("Your players are busy right now".into(), 1.4));
            return;
        }
        if self.game.mode == Mode::Online {
            if let Some(n) = self.net.as_mut() {
                n.send(ToServer::Emote { i: i as u8 });
            }
        }
        // aiming, your own player is behind the camera
        if self.game.local_aiming() {
            self.hint = Some((format!("A teammate does: {}", crate::anim::EMOTES[i].0), 1.4));
        }
    }
    /// A member's team and the players with their name over them (all three offline).
    fn players_of(&self, id: u32) -> Option<(usize, game::Who)> {
        let n = self.net.as_ref()?;
        let m = n.members.iter().find(|m| m.id == id)?;
        let t = m.team as usize;
        Some((t, net::owned(&n.members, t, id)))
    }
    /// All of their own players do it (all three offline), each a beat after the last; busy ones sit it out.
    pub(crate) fn show_emote(&mut self, from: Option<u32>, i: usize) -> bool {
        let (t, who) = match (self.game.mode, from, self.net.as_ref()) {
            (Mode::Online, Some(id), Some(n)) => {
                if self.qm_muted && id != n.id {
                    return false;
                }
                let Some(tw) = self.players_of(id) else { return false };
                tw
            }
            _ => (self.game.my_team.unwrap_or(0), game::ALL),
        };
        let g = &mut self.game;
        let aiming = matches!(g.state, State::Aim | State::CpuAim) && g.turn == t;
        let idx = g.teams[t].idx;
        let free: Vec<usize> = (0..3).filter(|p| who[*p] && !g.teams[t].out[*p] && !(aiming && idx == *p) && g.teams[t].players[*p].anim.mode == crate::game::AMode::Idle).collect();
        for (n, &p) in free.iter().enumerate() {
            let a = &mut g.teams[t].players[p].anim;
            a.emote = i as u8 + 1;
            // a negative time is a wait: the pose only starts at 0
            a.emote_t = -0.25 * n as f32;
        }
        !free.is_empty()
    }
    /// Show (and say) a quick message. `from` is a member id online; None means this player offline.
    pub(crate) fn show_qm(&mut self, from: Option<u32>, i: usize) {
        let (t, p, voice_ix) = match (self.game.mode, from, self.net.as_ref()) {
            (Mode::Online, Some(id), Some(n)) => {
                let Some(m) = n.members.iter().find(|m| m.id == id) else { return };
                if self.qm_muted && id != n.id {
                    return;
                }
                let voice = m.look[4] as usize;
                let Some((t, who)) = self.players_of(id) else { return };
                // over their own player, the one still on the pitch if they can
                let k = (0..3).find(|&i| who[i] && !self.game.teams[t].out[i]).or_else(|| (0..3).find(|&i| who[i])).unwrap_or(0);
                (t, k, voice)
            }
            _ => (0, 0, self.save.profile.voice),
        };
        self.add_bubble(t, p, i, voice_ix);
    }
    pub(crate) fn add_bubble(&mut self, t: usize, p: usize, i: usize, voice_ix: usize) {
        self.bubbles.retain(|b| !(b.t == t && b.i == p));
        self.bubbles.push(Bubble { t, i: p, msg: i, life: 2.6 });
        if self.save.settings.voices && (cfg!(target_arch = "wasm32") || cfg!(target_os = "android")) {
            let (txt, lang) = voice::QM_SAY[i % 9];
            let (_, pitch, rate) = save::VOICES[voice_ix % 4];
            voice::speak(txt, lang, pitch, rate, self.audio.volume.max(0.05));
        } else {
            self.play(Sfx::Touch(0.25));
        }
    }

    // ---------- stats ----------
    fn stats_begin(&mut self) {
        let g = &self.game;
        if g.train.is_some() || g.tut.is_some() {
            self.stats_cur = None;
            return;
        }
        let team = if g.mode == Mode::Hot { None } else { g.my_team };
        let mode = match g.mode {
            Mode::Hot => "hot",
            Mode::Online => "online",
            _ => "cpu",
        };
        let mut rec = MatchRec { t: now_ms(), mode: mode.into(), diff: g.diff, arena: self.match_arena.id().into(), ..Default::default() };
        if let (Some(n), Some(t)) = (self.net.as_ref(), team) {
            rec.opp = n.humans(1 - t as u8).iter().map(|m| m.name.clone()).take(3).collect();
        }
        self.stats_cur = Some(StatsCur { rec, team, t0: self.time, prev: team.map_or([0.0; 3], |t| g.teams[t].beers), ..Default::default() });
    }
    fn stats_tick(&mut self) {
        let evs = std::mem::take(&mut self.game.stat_ev);
        let Some(c) = self.stats_cur.as_mut() else { return };
        let g = &self.game;
        for e in evs {
            match e {
                StatEv::Throw { team, idx } => {
                    c.last_mine = Some(team) == c.team && g.my_players[team][idx];
                    if c.last_mine {
                        c.rec.throws += 1;
                    }
                }
                StatEv::Hit { team, ring, curve } => {
                    if Some(team) == c.team && c.last_mine {
                        c.rec.hits += 1;
                        match ring {
                            3 => c.rec.exc += 1,
                            2 => c.rec.great += 1,
                            1 => c.rec.nice += 1,
                            _ => {}
                        }
                        if curve {
                            c.rec.curve += 1;
                        }
                    }
                    if c.team.is_some() && Some(team) != c.team {
                        c.run_start = self.time;
                    }
                }
                StatEv::Stop => {
                    if c.run_start > 0.0 {
                        let d = (self.time - c.run_start) as f32;
                        if d > 1.0 && d < 120.0 && (c.rec.fast_run == 0.0 || d < c.rec.fast_run) {
                            c.rec.fast_run = (d * 10.0).round() / 10.0;
                        }
                        c.run_start = 0.0;
                    }
                }
                StatEv::Straf { team } => {
                    if Some(team) == c.team {
                        c.rec.straf += 1
                    }
                }
                StatEv::Unseen { team } => {
                    if Some(team) == c.team {
                        c.rec.dodged += 1
                    }
                }
                StatEv::Sneak { team } => {
                    if Some(team) == c.team {
                        c.rec.sneaks += 1
                    }
                }
                StatEv::Over { .. } => {}
            }
        }
        if let Some(t) = c.team {
            let b = g.teams[t].beers;
            for i in 0..3 {
                if g.my_players[t][i] {
                    let d = c.prev[i] - b[i];
                    if d > 0.0 && d < 0.5 {
                        c.rec.litres += d * 0.5; // a bottle is half a litre
                    }
                }
            }
            c.prev = b;
        }
    }
    /// The match is over: file it and hand out XP.
    fn stats_end(&mut self, winner: usize) {
        let Some(mut c) = self.stats_cur.take() else { return };
        let g = &self.game;
        if g.mirror {
            // a guest only sees the scoreboard: count the team's throws and hits
            if let Some(t) = c.team {
                c.rec.throws = g.teams[t].throws;
                c.rec.hits = g.teams[t].hits;
            }
        }
        let mut parts = Vec::new();
        let n = match c.team {
            None => {
                parts.push("Pass-and-play match 25".into());
                25
            }
            Some(t) => {
                let mut n = 40;
                parts.push("Match 40".to_string());
                if winner == t {
                    n += 60;
                    parts.push("Win 60".into());
                }
                if c.rec.hits > 0 {
                    n += 10 * c.rec.hits;
                    parts.push(format!("{} {} {}", c.rec.hits, if c.rec.hits > 1 { "hits" } else { "hit" }, 10 * c.rec.hits));
                }
                if c.rec.sneaks > 0 {
                    n += 15 * c.rec.sneaks;
                    parts.push(format!("{} sneaky sip{} {}", c.rec.sneaks, if c.rec.sneaks > 1 { "s" } else { "" }, 15 * c.rec.sneaks));
                }
                n
            }
        };
        c.rec.win = c.team.map(|t| t == winner);
        c.rec.winner = g.teams[winner].name.into();
        c.rec.dur = (self.time - c.t0) as u32;
        c.rec.litres = (c.rec.litres * 100.0).round() / 100.0;
        if self.save.locked() {
            // logged out: played, not kept
            self.last_xp = None;
            return;
        }
        self.save.matches.push(c.rec);
        let extra = self.save.matches.len().saturating_sub(save::STATS_MAX);
        if extra > 0 {
            self.save.matches.drain(..extra);
        }
        self.last_xp = Some(self.save.add_xp(n, parts));
    }
    fn train_end(&mut self, k: Drill, v: f32, note: String) {
        let prev = self.save.train_pb[k.idx()];
        let rec = match prev {
            None => true,
            Some(p) => {
                if k == Drill::Throw {
                    v > p
                } else {
                    v < p
                }
            }
        };
        if self.save.locked() {
            self.last_xp = None;
            self.train_res = Some((k, v, note, false, prev));
            return;
        }
        if rec {
            self.save.train_pb[k.idx()] = Some(v);
        }
        let parts = if rec { vec!["Drill 25".to_string(), "Record 25".to_string()] } else { vec!["Drill 25".to_string()] };
        self.last_xp = Some(self.save.add_xp(if rec { 50 } else { 25 }, parts));
        self.train_res = Some((k, v, note, rec, prev));
        self.save.store();
    }
    pub(crate) fn finish_tutorial(&mut self) {
        if !self.save.tut_done {
            self.save.tut_done = true;
            let r = self.save.add_xp(50, vec!["Tutorial 50".into()]);
            let (title, sub) = if self.save.locked() { ("Tutorial complete".into(), "Log in to earn XP".into()) } else { (format!("+{} XP", r.gain), "Tutorial complete".into()) };
            self.banner = Some((game::Banner { title, sub, dur: 1.8, straf: false }, 0.0));
        }
        self.save.store();
    }

    // ---------- online ----------
    pub(crate) fn net_connect(&mut self) -> bool {
        if self.save.locked() {
            self.net_err = "Log in to play online".into();
            return false;
        }
        if self.net.as_ref().map_or(false, |n| n.sock.state() < 2) {
            return true;
        }
        match Net::connect(&save::server(), self.time) {
            Ok(n) => {
                self.net = Some(n);
                self.net_err.clear();
                true
            }
            Err(e) => {
                self.net_err = e;
                false
            }
        }
    }
    /// The connection to the server is always there (once you may play online), so hosting, joining and
    /// finding a match don't wait for one. When it goes it comes back by itself, sooner at first.
    fn net_keep(&mut self) {
        if self.save.locked() {
            if let Some(n) = self.net.take() {
                n.sock.close();
            }
            return;
        }
        if self.net.is_none() && self.time >= self.net_retry.0 {
            let tries = self.net_retry.1;
            self.net_retry = (self.time + (1.0 + tries as f64 * 2.0).min(15.0), tries + 1);
            if let Ok(n) = Net::connect(&save::server(), self.time) {
                self.net = Some(n);
            }
        }
        if self.net.as_ref().map_or(false, |n| n.sock.state() == 1 && n.last_rx > n.opened) {
            self.net_retry.1 = 0;
        }
    }
    /// Our friend code and secret, if we have them for this server.
    fn account_here(&self) -> (String, String) {
        let a = &self.save.account;
        if a.0 == save::server() {
            (a.1.clone(), a.2.clone())
        } else {
            Default::default()
        }
    }
    pub(crate) fn net_host(&mut self) {
        if !self.net_connect() {
            return;
        }
        let (name, look, me) = (self.save.profile.name.clone(), self.my_look(), self.account_here());
        self.net.as_mut().unwrap().send(ToServer::Host { v: net::PROTOCOL, name, look, me });
    }
    pub(crate) fn net_join(&mut self, code: &str) {
        if !self.net_connect() {
            return;
        }
        let (name, look, me) = (self.save.profile.name.clone(), self.my_look(), self.account_here());
        self.net.as_mut().unwrap().send(ToServer::Join { v: net::PROTOCOL, code: code.to_string(), name, look, me });
    }
    pub(crate) fn net_list(&mut self) {
        if !self.net_connect() {
            return;
        }
        self.net.as_mut().unwrap().send(ToServer::List);
    }
    pub(crate) fn net_open(&mut self, open: bool) {
        if let Some(n) = self.net.as_mut() {
            n.open = open;
            n.send(ToServer::Open { open });
        }
    }
    pub(crate) fn leave_online(&mut self) {
        if let Some(n) = self.net.as_mut() {
            if n.in_room {
                n.send(ToServer::Leave);
            }
            // the connection stays, for the next match
            n.leave_room();
            if n.reconnect.is_some() {
                // it was down while in the room: start afresh rather than resume a seat we just gave up
                n.sock.close();
                self.net = None;
                self.net_retry.0 = 0.0;
            }
        }
        self.game.mirror = false;
        self.game.net_pause = None;
        self.game.pred = Default::default();
    }
    pub(crate) fn net_set_team(&mut self, t: u8) {
        if let Some(n) = self.net.as_mut() {
            n.send(ToServer::Team { team: t });
        }
    }
    /// Host: ask the server to start (or restart) the match.
    pub(crate) fn net_start(&mut self) {
        if let Some(n) = self.net.as_mut() {
            n.send(ToServer::Start);
        }
    }
    /// The online match can't go on for us: back to the online screen, and say why.
    fn net_out(&mut self, msg: String) {
        let was_playing = matches!(self.screen, Screen::Playing | Screen::Paused | Screen::Over) && self.game.mode == Mode::Online;
        if let Some(n) = self.net.as_mut() {
            if n.reconnect.is_some() || n.sock.state() == 2 {
                // a connection that's down starts afresh in the background
                n.sock.close();
                self.net = None;
                self.net_retry.0 = 0.0;
            } else {
                n.leave_room();
            }
        }
        self.game.mirror = false;
        self.game.net_pause = None;
        self.game.pred = Default::default();
        if was_playing {
            self.go_home(View::Menu);
            self.screen = Screen::Play;
            self.step = PlayStep::Online;
        }
        self.net_err = msg;
    }
    fn social_poll(&mut self) {
        let status = match self.net.as_ref() {
            Some(n) if n.in_room && n.phase == net::PHASE_PLAYING => net::STATUS_MATCH,
            Some(n) if n.in_room => net::STATUS_LOBBY,
            _ => net::STATUS_FREE,
        };
        let url = save::server();
        // with a password, never a new account by just connecting: a session that's gone is logged out
        let only = save::needs_login() || self.save.logged_in();
        if only && !self.save.user.is_empty() && self.save.account.0 != url {
            // logged in on another server: not here
            self.save.log_out();
        }
        let (code, secret) = if self.save.account.0 == url { (self.save.account.1.clone(), self.save.account.2.clone()) } else { (String::new(), String::new()) };
        let name = self.save.profile.name.clone();
        for got in self.social.poll(self.time, &url, (&code, &secret), &name, status, only) {
            match got {
                Got::Me { code: c, secret: s, user, name } => {
                    // with a password, the save keeps the username as typed: it's the player's name
                    let user = if user.is_empty() { user } else { name };
                    if !user.is_empty() && !self.save.logged_in() && c != code && !save::needs_login() {
                        // a phone logs in to an account of its own: keep its own player for when it logs out
                        self.save.device = Some((self.save.account.clone(), self.save.progress(), self.save.rev));
                    }
                    if !user.is_empty() && c != code {
                        // another account than the one this progression belongs to
                        self.save.rev = 0;
                    }
                    if (c.as_str(), s.as_str(), user.as_str()) != (code.as_str(), secret.as_str(), self.save.user.as_str()) {
                        self.save.account = (url.clone(), c, s);
                        self.save.user = user;
                        self.save.store();
                    }
                    self.prog_synced = false;
                    self.login_pass.clear();
                }
                Got::Progress { rev, data } => self.take_progress(rev, data),
                Got::LoggedOut => {
                    self.save.log_out();
                    self.name_edit = self.save.profile.name.clone();
                }
            }
        }
        if self.save.logged_in() && !self.save.locked() && self.save.profile.name != self.save.user {
            // logged in, the username is the name, whatever the progression said
            self.save.profile.name = self.save.user.clone();
            self.name_edit = self.save.user.clone();
            self.save.store();
        }
        self.push_progress();
        // a message from someone: a word on screen, unless you're in the chat
        if let Some((who, text)) = self.social.toast.take() {
            let short: String = text.chars().take(40).collect();
            self.banner = Some((game::Banner { title: who, sub: short, dur: 2.4, straf: false }, 0.0));
            self.play(Sfx::Touch(0.3));
        }
    }
    /// The server's copy of our progression, just after logging in: take it, or keep ours if it's newer.
    fn take_progress(&mut self, rev: u64, data: String) {
        self.prog_synced = true;
        if data.is_empty() {
            // a new account: what was earned before logging in was needed is its start
            if let Some(old) = self.save.legacy.take() {
                self.save.set_progress(&old);
                self.name_edit = self.save.profile.name.clone();
            }
        } else if self.save.rev <= rev && self.save.set_progress(&data) {
            self.save.rev = rev;
            self.prog_sent = data;
            self.name_edit = self.save.profile.name.clone();
            self.save.store();
            return;
        }
        // ours is newer, or the server has none yet: it gets ours
        self.save.rev = self.save.rev.max(rev);
        self.prog_sent.clear();
        self.save.store();
    }
    /// Logged in with a password: keep the server's copy of the progression up to date.
    fn push_progress(&mut self) {
        if !self.save.logged_in() || self.save.locked() || !self.social.ready || !self.prog_synced || self.time - self.prog_t < 2.0 {
            return;
        }
        self.prog_t = self.time;
        let data = self.save.progress();
        if data != self.prog_sent {
            self.save.rev += 1;
            self.save.store();
            self.social.send(ToServer::SProgress { rev: self.save.rev, data: data.clone() });
            self.prog_sent = data;
        }
    }
    pub(crate) fn log_out(&mut self) {
        self.leave_online();
        self.social.log_out();
        self.save.log_out();
        self.name_edit = self.save.profile.name.clone();
        self.prog_sent.clear();
        self.prog_synced = false;
    }
    /// Say yes to an invite: off to that room's lobby.
    pub(crate) fn accept_invite(&mut self) {
        let Some(i) = self.social.invite.take() else { return };
        self.join_invited(&i.room);
    }
    /// Off to an invite's room, from wherever we are (a match too: a tapped notification).
    fn join_invited(&mut self, room: &str) {
        self.leave_online();
        self.game.to_menu();
        self.game.bounds = None;
        self.view = View::Menu;
        self.sheet = Sheet::None;
        self.screen = Screen::Play;
        self.step = PlayStep::Online;
        self.net_join(room);
    }

    // ---------- notifications, updates, bug reports ----------
    /// The server has a newer build than this one.
    pub(crate) fn outdated(&self) -> bool {
        self.social.latest > net::APP_BUILD
    }
    /// Get the newest version: on a phone the APK is downloaded and installed, in a browser the page is
    /// loaded again (the server always has the newest).
    pub(crate) fn update_app(&mut self) {
        #[cfg(target_os = "android")]
        crate::android::update(net::APK_URL);
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = web_sys::window() {
            let _ = w.location().reload();
        }
    }
    /// Notifications: who the phone checks for in the background, what came in, and what was tapped.
    fn notes_poll(&mut self) {
        let notes = std::mem::take(&mut self.social.notes);
        let on = self.save.settings.notify && !self.save.locked();
        #[cfg(target_os = "android")]
        {
            let url = save::server();
            let a = &self.save.account;
            let told = if on && a.0 == url && !a.1.is_empty() { (net::http_url(&url), a.1.clone(), a.2.clone(), true) } else { Default::default() };
            if told != self.notes_told {
                if told.3 && !self.notes_told.3 {
                    crate::android::ask_notify();
                }
                crate::android::notes_account(&told.0, &told.1, &told.2, told.3);
                self.notes_told = told;
            }
            for n in &notes {
                crate::android::notify(n);
            }
            if self.time - self.open_t > 0.5 {
                self.open_t = self.time;
                let open = crate::android::take_open();
                if let Some(o) = &open {
                    log_str(&format!("opened from a notification: {o}"));
                }
                match open {
                    Some(o) if o.starts_with("room:") => {
                        self.social.invite = None;
                        self.join_invited(&o[5..]);
                    }
                    Some(o) if o == "update" => self.sheet = Sheet::Settings,
                    Some(o) if o.starts_with("chat:") => {
                        self.sheet = Sheet::Friends;
                        if self.social.ready {
                            self.social.open(&o[5..]);
                        }
                    }
                    _ => {}
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        if on {
            for n in &notes {
                web_notify(n);
            }
        }
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        let _ = (notes, on);
    }
    /// What goes with a bug report besides what the player wrote.
    pub(crate) fn bug_info(&self) -> String {
        let g = &self.game;
        let platform = if cfg!(target_os = "android") {
            "android"
        } else if cfg!(target_arch = "wasm32") {
            "web"
        } else {
            "desktop"
        };
        #[cfg(target_arch = "wasm32")]
        let agent = web_sys::window().and_then(|w| w.navigator().user_agent().ok()).unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        let agent = String::new();
        let online = self.net.as_ref().map_or("no connection".to_string(), |n| format!("socket={} room={} phase={} rtt={:.0}ms reconnecting={}", n.sock.state(), if n.in_room { n.code.as_str() } else { "-" }, n.phase, n.rtt * 1000.0, n.reconnect.is_some()));
        let mut s = format!(
            "build {} ({}), protocol {}, newest {}\n{agent}\nscreen={:?} sheet={:?} mode={:?} state={:?} turn={} mirror={} arena={:?}\n{online}\nfps={:.0} scale={:.2} logged_in={} friends_ready={}\n--- log ---\n",
            net::APP_BUILD,
            platform,
            net::PROTOCOL,
            self.social.latest,
            self.screen,
            self.sheet,
            g.mode,
            g.state,
            g.turn,
            g.mirror,
            self.match_arena,
            1.0 / self.fps_dt.max(1e-3),
            self.render_scale,
            self.save.logged_in(),
            self.social.ready
        );
        for l in recent_log() {
            s.push_str(&l);
            s.push('\n');
        }
        let max = net::BUG_INFO;
        if s.chars().count() > max {
            // the newest lines matter most
            s = s.chars().rev().take(max).collect::<Vec<_>>().into_iter().rev().collect();
        }
        s
    }
    pub(crate) fn send_bug(&mut self) {
        let text: String = self.bug_text.trim().chars().take(net::BUG_TEXT).collect();
        let info = self.bug_info();
        if !self.social.send_bug(self.time, text, info) {
            self.social.bug_res = Some((false, "Can't reach the server right now, try again in a moment".into()));
        }
    }
    fn net_poll(&mut self) {
        // offline nobody reads these
        self.game.net_ev.clear();
        self.net_keep();
        let now = self.time;
        let Some(n) = self.net.as_mut() else { return };
        if !n.try_reconnect(now) {
            self.net_out("Couldn't get back into the match: the connection stayed down".into());
            return;
        }
        n.flush();
        let mut inbox = Vec::new();
        while let Some(s) = n.sock.recv() {
            inbox.push(s);
        }
        if !inbox.is_empty() {
            n.last_rx = now;
        }
        // back from a sleep there's a pile of snapshots; only the last is worth hearing
        let snaps = inbox.iter().filter(|s| s.0.starts_with("{\"t\":\"snap\"")).count();
        let mut seen = 0;
        for (s, at) in inbox {
            let Ok(m) = serde_json::from_str::<FromServer>(&s) else {
                log_str(&format!("unreadable message from the server: {}", s.chars().take(80).collect::<String>()));
                continue;
            };
            if let FromServer::Pong { c } = m {
                let Some(n) = self.net.as_mut() else { return };
                if c == 0.0 {
                    continue; // the socket's own keepalive
                }
                let r = ((at - c) / 1000.0).max(0.0) as f32;
                n.rtt = if n.rtt == 0.0 { r } else { n.rtt * 0.7 + r * 0.3 };
                self.game.pred.rtt = n.rtt.min(0.5);
            } else if matches!(m, FromServer::Snap { .. }) {
                seen += 1;
                self.net_snap(m, seen == snaps || snaps < 4);
            } else {
                self.net_msg(m);
            }
        }
        let Some(n) = self.net.as_mut() else { return };
        // a dead connection doesn't always say so
        let st = n.sock.state();
        let stale = st == 1 && now - n.last_rx > net::DEAD_AFTER;
        let lost = st == 2 || stale || (st == 0 && now - n.opened > 8.0 && n.reconnect.is_none());
        if lost && n.reconnect.is_none() {
            if n.in_room && !n.token.is_empty() {
                log_str(&format!("connection lost (stale={stale}); reconnecting"));
                n.sock.close();
                n.reconnect = Some(net::Reconnect { since: now, next: now, tries: 0, started: now });
            } else {
                // not in a match: nothing to tell, it just comes back in the background
                n.sock.close();
                self.net = None;
                return;
            }
        }
        if n.sock.state() == 1 && now - n.ping_t > 1.0 {
            n.ping_t = now;
            let (s, d) = (self.game.pred.last_q, self.game.pred.digest);
            n.send(ToServer::Hb { c: now_ms(), r: n.rtt, s, d });
        }
        // the host's choices in the lobby
        if n.host() && n.phase != net::PHASE_PLAYING {
            let (arena, diff) = (self.arena.id(), self.save.diff as u8);
            if n.arena != arena || n.diff != diff {
                n.arena = arena.into();
                n.diff = diff;
                n.send(ToServer::Config { arena: arena.into(), diff });
            }
        }
    }
    /// Take a snapshot. A new match number means the server has started a new match.
    fn net_snap(&mut self, m: FromServer, sound: bool) {
        let FromServer::Snap { a, s } = m else { return };
        let new_match = s.m != self.game.pred.match_id;
        if new_match {
            let Some(n) = self.net.as_ref() else { return };
            self.match_arena = ArenaKey::from_id(&n.arena);
            self.game.bounds = self.match_arena.bounds();
            self.game.mirror_begin(s.m);
            self.game.banners.clear();
            net::set_sides(&mut self.game, &n.members, Some(n.id));
            self.after_start();
            self.stats_begin();
        }
        let was = self.game.state;
        let sxs = self.game.apply_snapshot(&s, a);
        if sound {
            for x in sxs {
                self.play(x);
            }
        }
        if self.game.state == State::Over && was != State::Over && self.screen == Screen::Playing {
            self.over_t = 1.5;
        }
    }
    fn net_msg(&mut self, m: FromServer) {
        let Some(n) = self.net.as_mut() else { return };
        match m {
            FromServer::Welcome { code, id, token, resumed } => {
                n.code = code;
                n.id = id;
                n.token = token;
                n.in_room = true;
                n.reconnect = None;
                self.net_err.clear();
                if resumed {
                    // nothing we pressed before the break counts any more
                    self.game.pred.sent_n = 0;
                    self.game.pred.hold_n = 0;
                    self.game.pred.pad_n = 0;
                    self.game.pred.throw_n = 0;
                    self.game.pred.last_q = 0;
                    self.banner = Some((game::Banner { title: "Back in".into(), sub: "Connection restored".into(), dur: 1.4, straf: false }, 0.0));
                }
            }
            FromServer::Roster { members, host, arena, diff, phase, open } => {
                n.members = members;
                n.host_id = host;
                n.arena = arena;
                n.diff = diff;
                n.phase = phase;
                n.open = open;
                if !n.host() {
                    self.arena = ArenaKey::from_id(&n.arena);
                }
                if phase != net::PHASE_LOBBY && self.game.mirror {
                    net::set_sides(&mut self.game, &n.members, Some(n.id));
                }
            }
            FromServer::Status { l } => n.status = l,
            FromServer::Notice { title, sub } => {
                self.banner = Some((game::Banner { title, sub, dur: 2.2, straf: false }, 0.0));
            }
            FromServer::Reject { n: which, why } => {
                let p = &mut self.game.pred;
                if p.throw_n == which {
                    p.throw_n = 0;
                }
                self.hint = Some((why, 1.6));
            }
            FromServer::Rooms { rooms } => n.rooms = rooms,
            FromServer::Err { msg, fatal } => {
                if fatal {
                    self.net_out(msg);
                } else if self.screen == Screen::Playing {
                    self.hint = Some((msg, 1.8));
                } else {
                    self.net_err = msg;
                }
            }
            FromServer::Closed { why } => self.net_out(why),
            FromServer::Pong { .. } => {}
            FromServer::Latest { build } => self.social.latest = build,
            FromServer::Emote { i, from } => {
                if (i as usize) < crate::anim::EMOTES.len() {
                    self.show_emote(Some(from), i as usize);
                }
            }
            FromServer::Qm { i, from } => {
                if (i as usize) < voice::QM.len() {
                    self.show_qm(Some(from), i as usize);
                }
            }
            _ => {}
        }
    }

    /// Animate everyone: players follow the ball with their eyes while it is in play.
    fn animate_all(&mut self, dt: f32) {
            let g = &mut self.game;
            let live = matches!(g.state, State::Aim | State::CpuAim | State::Flight | State::Run | State::Stopped);
            let ball = if g.local_aiming() { self.aim_ball } else { g.ball };
            let (turn, idx) = (g.turn, g.teams[g.turn].idx);
            for ti in 0..2 {
                for i in 0..3 {
                    let aiming = matches!(g.state, State::Aim | State::CpuAim) && ti == turn && i == idx;
                    let look = if !live || g.teams[ti].out[i] {
                        None
                    } else if aiming {
                        Some(Vec3::new(0.0, 0.2, 0.0))
                    } else {
                        Some(ball)
                    };
                    crate::anim::animate(&mut g.teams[ti].players[i], dt, look, (ti * 3 + i) as f32 * 0.37);
                }
            }
            crate::anim::animate(&mut g.referee, dt, None, 2.9);
        }

    // ---------- drawing ----------
    /// The light for what's on screen; in the clubhouse building it changes as you walk into the hall.
    fn scene_look(&self, k: ArenaKey) -> arenas::Look {
        if matches!(k, ArenaKey::Hub | ArenaKey::Gym) {
            let z = self.cam.pos.z;
            let g = ((14.4 - z) / 2.0).clamp(0.0, 1.0);
            return arenas::blend(arenas::look(ArenaKey::Hub), arenas::look(ArenaKey::Gym), g * g * (3.0 - 2.0 * g));
        }
        arenas::look(k)
    }
    fn globals(&self, lk: arenas::Look) -> Globals {
        let sun = lk.sun_dir.normalize();
        let e = |c: [f32; 3]| [c[0], c[1], c[2], 0.0];
        let (light_vp, near) = if lk.spot {
            // a bare bulb: shadows spread out from it, straight down over the whole room
            let p = lk.lamps[0].0;
            let view = Mat4::look_at_rh(p, p - Vec3::Y, Vec3::Z);
            (Mat4::perspective_rh(2.7, 1.0, 0.05, 14.0) * view, 0.05)
        } else {
            // the sun (or a ceiling full of lamps): the shadow map follows what the camera looks at
            let c = vec3(self.cam_look.x, 0.0, self.cam_look.z);
            let view = Mat4::look_at_rh(c + sun * 40.0, c, if sun.y > 0.95 { Vec3::Z } else { Vec3::Y });
            (Mat4::orthographic_rh(-16.0, 16.0, -16.0, 16.0, 1.0, 90.0) * view, 0.0)
        };
        let mut lp = [[0.0; 4]; crate::gfx::NLIGHTS];
        let mut lc = [[0.0; 4]; crate::gfx::NLIGHTS];
        for (i, l) in lk.lamps.iter().enumerate() {
            lp[i] = l.0.extend(l.1).to_array();
            lc[i] = e(l.2);
        }
        let mut rooms = [[0.0; 4]; 4];
        for (i, r) in lk.rooms.iter().enumerate() {
            if let Some((mn, mx)) = r {
                rooms[i * 2] = mn.extend(0.0).to_array();
                rooms[i * 2 + 1] = mx.extend(1.0).to_array();
            }
        }
        Globals {
            view_proj: self.cam.view_proj().to_cols_array_2d(),
            light_vp: light_vp.to_cols_array_2d(),
            cam_pos: self.cam.pos.extend(1.0).to_array(),
            sun_dir: sun.extend(near).to_array(),
            sun_col: e(lk.sun_col),
            sky_top: e(lk.sky_top),
            sky_hor: e(lk.sky_hor),
            ground_col: e(lk.ground),
            fog: lk.fog,
            misc: [(self.time % 3600.0) as f32, 1.0 / 2048.0, lk.exposure, lk.ambient],
            grade: lk.grade,
            lp,
            lc,
            rooms,
        }
    }
}

impl eframe::App for FlunkiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t_start = now_ms();
        // how long the first frames take: nothing reads the phone's input queue meanwhile
        if self.frames < 4 {
            self.frames += 1;
            log_str(&format!("startup: frame {} begins at {:.0} ms", self.frames, now_ms() - self.t0));
        }
        let ctx = ui.ctx().clone();
        if self.no_anim {
            ctx.all_styles_mut(|s| s.animation_time = 0.0);
        }
        let full = ctx.content_rect();
        self.time = ctx.input(|i| i.time);
        if self.time - self.safe_t > 0.5 {
            self.safe_t = self.time;
            self.safe = safe_insets();
        }
        let t_insets = now_ms();
        // everything you read or tap stays inside the safe area; the 3D picture fills the screen
        let sr = Rect::from_min_max(full.min + vec2(self.safe[3], self.safe[0]), full.max - vec2(self.safe[1], self.safe[2]));
        self.screen_rect = sr;
        let (w, h) = (full.width().max(1.0), full.height().max(1.0));
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.05);
        self.fps_dt = self.fps_dt * 0.9 + ctx.input(|i| i.unstable_dt) * 0.1;
        self.time = ctx.input(|i| i.time);
        // every font size once, before anyone opens a page (and again after turning the phone)
        let warm = (ctx.pixels_per_point(), crate::ui::banner_size(sr.width()));
        if self.frames >= 2 && warm != self.warmed {
            // one bitmap per glyph instead of four (sub-pixel positions), and no hinting on a sharp
            // screen: a quarter of the work, and what's laid out here is then all there is to draw.
            // A change only counts from the next frame (it clears the atlas), so lay out after that
            let hint = ctx.pixels_per_point() < 1.75;
            let to = ctx.global_style().visuals.text_options;
            if to.font_hinting != hint || to.subpixel_binning {
                ctx.all_styles_mut(|s| {
                    s.visuals.text_options.font_hinting = hint;
                    s.visuals.text_options.subpixel_binning = false;
                });
            } else {
                let t = now_ms();
                crate::ui::warm_fonts(&ctx, sr.width());
                self.warmed = warm;
                log_str(&format!("fonts laid out in {:.0} ms", now_ms() - t));
            }
        }
        let t_warm = now_ms();
        self.handle_input(&ctx, sr);
        let t_input = now_ms();
        self.net_poll();
        let t_net = now_ms();
        self.social_poll();
        self.notes_poll();
        let t_in = now_ms();

        // simulate (an online match keeps going while you look at the pause menu)
        // a drill waits until you have walked through the door
        let walking_in = matches!(self.fly, Some((t, _, _, true)) if t < 1.7);
        if (self.screen != Screen::Paused || self.game.mode == Mode::Online) && !walking_in {
            self.game.tick(dt, self.ref_mouth, self.ref_tip);
        }
        let sfx = std::mem::take(&mut self.game.sfx);
        for s in sfx {
            self.play(s);
        }
        if let Some(b) = self.game.banners.drain(..).last() {
            self.banner = Some((b, 0.0));
        }
        if let Some((b, age)) = self.banner.as_mut() {
            *age += dt;
            if *age > b.dur + 0.35 {
                self.banner = None;
            }
        }
        for o in [&mut self.hint, &mut self.lock_note] {
            if let Some((_, t)) = o.as_mut() {
                *t -= dt;
                if *t <= 0.0 {
                    *o = None;
                }
            }
        }
        self.reset_armed = (self.reset_armed - dt).max(0.0);
        self.stats_tick();
        // the match ended
        let mut over_now = self.game.show_over;
        if self.over_t >= 0.0 {
            self.over_t -= dt;
            if self.over_t < 0.0 {
                over_now = true;
            }
        }
        if over_now {
            self.game.show_over = false;
            self.over_t = -1.0;
            if matches!(self.screen, Screen::Playing | Screen::Paused) {
                if let Some(w) = self.game.win {
                    self.stats_end(w);
                }
                self.screen = Screen::Over;
            }
        }
        if let Some((k, v, note)) = self.game.train_result.take() {
            self.train_end(k, v, note);
            self.train_over_t = 0.9;
        }
        if self.train_over_t > 0.0 {
            self.train_over_t -= dt;
            if self.train_over_t <= 0.0 && self.screen == Screen::Playing {
                self.screen = Screen::TrainOver;
            }
        }
        if let Some((m, t)) = self.cpu_qm.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                let m = *m;
                self.cpu_qm = None;
                let i = self.game.teams[1].active_idx(self.game.teams[1].idx);
                let v = (self.game.rng.f() * 4.0) as usize;
                self.add_bubble(1, i, m, v);
            }
        }
        for b in self.bubbles.iter_mut() {
            b.life -= dt;
        }
        self.bubbles.retain(|b| b.life > 0.0);
        if !self.game.local_aiming() && self.flick.is_some() {
            self.flick = None;
        }
        self.animate_all(dt);
        // the clubhouse player
        let shown = self.shown_arena();
        if shown == ArenaKey::Hub {
            self.hub_person.vis.treadmill = self.lab_speed;
            if self.lab_speed > 0.0 {
                self.hub_person.yaw = 1.57;
            }
            let lab = self.lab_mode.clone();
            let a = &mut self.hub_person.anim;
            a.sip = lab == "sip";
            a.point = lab == "point";
            a.smoking = lab == "smoke" || lab == "drag";
            if lab == "drag" {
                a.smoke_t = 0.5;
            } else if a.smoking {
                a.smoke_t += dt;
            }
            if lab == "throw" && (a.throw_t < 0.0) {
                a.throw_t = 0.0;
            }
            if self.hub_cheer <= 0.0 && matches!(lab.as_str(), "bend" | "sit" | "cheer") {
                a.mode = match lab.as_str() {
                    "bend" => AMode::Bend,
                    "sit" => AMode::Sit,
                    _ => AMode::Cheer,
                };
            } else if self.hub_cheer > 0.0 {
                self.hub_cheer -= dt;
                a.mode = AMode::Cheer;
            } else {
                a.mode = AMode::Idle;
            }
            a.cheer_t += dt;
            a.phase += dt;
            crate::anim::animate(&mut self.hub_person, dt, None, 0.5);
        }
        if shown != self.last_shown {
            let same_building = matches!(shown, ArenaKey::Hub | ArenaKey::Gym) && matches!(self.last_shown, ArenaKey::Hub | ArenaKey::Gym);
            self.last_shown = shown;
            if !same_building {
                self.cam_snap = true;
                self.fly = None;
            }
        }

        // camera
        let (tp, tl) = self.cam_override.unwrap_or_else(|| self.cam_targets(w, h));
        let k = 1.0 - (-3.0 * dt).exp();
        if let Some((t0, fp, fl, inward)) = self.fly {
            // through the barn door: a smooth curve from where the camera was to where it is going
            const DUR: f32 = 2.4;
            let t1 = t0 + dt;
            let u = (t1 / DUR).clamp(0.0, 1.0);
            let e = u * u * (3.0 - 2.0 * u);
            let door = arenas::HUB_O + vec3(arenas::DOOR_X, 1.45, -4.6);
            // a spline that goes straight through the doorway: in front of it, then behind it
            let dz = if inward { vec3(0.0, 0.0, 1.3) } else { vec3(0.0, 0.0, -1.3) };
            let pts = [fp, door + dz, door - dz, tp];
            let path = |e: f32| -> Vec3 {
                let f = e.clamp(0.0, 1.0) * 3.0;
                let i = (f.floor() as usize).min(2);
                let s = f - i as f32;
                let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
                let p3 = if i == 2 { pts[3] } else { pts[i + 2] };
                let (p1, p2) = (pts[i], pts[i + 1]);
                let (s2, s3) = (s * s, s * s * s);
                0.5 * (p1 * 2.0 + (p2 - p0) * s + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * s2 + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * s3)
            };
            self.cam_pos = path(e);
            let mut dir = path(e + 0.08) - self.cam_pos;
            dir.y = 0.0;
            let ahead = self.cam_pos + dir.normalize_or_zero() * 3.0 + vec3(0.0, -0.25, 0.0);
            let w0 = (1.0 - e / 0.3).clamp(0.0, 1.0);
            let w1 = ((e - 0.7) / 0.3).clamp(0.0, 1.0);
            let w0 = w0 * w0 * (3.0 - 2.0 * w0);
            let w1 = w1 * w1 * (3.0 - 2.0 * w1);
            self.cam_look = ahead.lerp(fl, w0).lerp(tl, w1);
            self.fly = if u >= 1.0 { None } else { Some((t1, fp, fl, inward)) };
            self.cam_snap = false;
        } else if self.cam_snap {
            self.cam_pos = tp;
            self.cam_look = tl;
            self.cam_snap = false;
        } else {
            self.cam_pos = self.cam_pos.lerp(tp, k);
            self.cam_look = self.cam_look.lerp(tl, k);
        }
        let portrait = h > w;
        self.cam = Cam { pos: self.cam_pos, look: self.cam_look, fov: (if portrait { 66.0f32 } else { 50.0 }).to_radians(), aspect: w / h };

        // the ball follows your finger while you aim
        let aiming = self.game.local_aiming() && self.screen == Screen::Playing;
        if aiming {
            self.ring_r = 1.0 - ((self.game.clock * 0.7) % 1.0) * 0.75;
            let rest = pos2(sr.center().x, sr.bottom() - (140.0f32).min(h * 0.18));
            let target = self.flick.as_ref().map_or(rest, |f| f.cur);
            // from where the camera is going, not where it is on its way: a quick flick while it
            // still glides over from the last view would throw from the wrong spot (or be turned down)
            let at = Cam { pos: tp, look: tl, ..self.cam };
            let world = at.screen_to_world((target.x - full.left()) / w, (target.y - full.top()) / h, 1.5);
            if self.flick.is_some() || self.aim_ball.distance(world) > 3.0 {
                self.aim_ball = world;
            } else {
                self.aim_ball = self.aim_ball.lerp(world, 1.0 - (-12.0 * dt).exp());
            }
            if let Some(f) = self.flick.as_mut() {
                f.spin_vis += f.armed as f32 * 16.0 * dt;
            }
        }

        // build and paint the 3D scene
        let mut list = std::mem::take(&mut self.frame_list);
        list.clear();
        let building = matches!(shown, ArenaKey::Hub | ArenaKey::Gym);
        if let Some(env) = self.envs.get(shown.env_id()) {
            env.draw(&mut list, self.cam.pos);
        }
        arenas::dynamic(&mut list, shown, self.game.clock, self.cam.pos);
        if building {
            arenas::chalk_records(&mut list, &self.save.train_pb);
        }
        if shown == ArenaKey::Hub {
            let lk = Self::look_of(&self.my_look(), 0);
            scene::build_hub_player(&mut list, &self.model, &self.hub_person, &lk);
        } else {
            let opts = SceneOpts {
                looks: self.looks(),
                drill: self.game.train.as_ref().map(|t| t.kind),
                aim_ball: aiming.then_some(self.aim_ball),
                aim_spin: self.flick.as_ref().map_or(0.0, |f| f.acc + f.spin_vis),
                curve_armed: self.flick.as_ref().map_or(false, |f| f.armed != 0),
                ring_r: aiming.then_some(self.ring_r),
                hide_thrower: aiming,
                trail: self.save.settings.trail,
            };
            let out = scene::build(&mut list, &self.model, &self.game, &self.cam, &opts);
            self.ref_mouth = out.ref_mouth;
            self.ref_tip = out.ref_tip;
            self.heads = out.heads;
        }
        let lk = self.scene_look(shown);
        if !self.hide_meshes.is_empty() {
            let hide = self.hide_meshes.clone();
            list.opaque.retain(|(m, _)| !hide.contains(&(*m as u8)));
        }
        // dynamic resolution: a phone that can't keep up renders the 3D a little softer. In a few steps,
        // and only once it's been slow (or fast) a while: every new size rebuilds all render targets
        if !self.scale_lock {
            self.render_scale = self.render_scale.min(self.save.settings.quality().max_scale);
        }
        #[cfg(target_os = "android")]
        if self.fps_asked != Some(self.save.settings.max_fps) {
            self.fps_asked = Some(self.save.settings.max_fps);
            crate::android::max_fps(self.save.settings.max_fps);
        }
        if self.time > 4.0 && !self.scale_lock {
            let fps = 1.0 / self.fps_dt.max(1e-4);
            let (slow, fast) = &mut self.scale_wait;
            if fps < 40.0 {
                *slow += dt;
                *fast = 0.0;
            } else if fps > 56.0 {
                *fast += dt;
                *slow = 0.0;
            } else {
                *slow = 0.0;
                *fast = 0.0;
            }
            let at = SCALE_STEPS.iter().rposition(|s| *s <= self.render_scale + 1e-3).unwrap_or(0);
            let top = SCALE_STEPS.iter().rposition(|s| *s <= self.save.settings.quality().max_scale + 1e-3).unwrap_or(0);
            if *slow > 1.0 && at > 0 {
                self.render_scale = SCALE_STEPS[at - 1];
                // having had to go down, going back up waits longer
                self.scale_up_after = self.time + 15.0;
                self.scale_wait = (0.0, 0.0);
            } else if *fast > 4.0 && at < top && self.time > self.scale_up_after {
                self.render_scale = SCALE_STEPS[at + 1];
                self.scale_wait = (0.0, 0.0);
            }
        }
        // the park's lawn grows blades; a phone that's struggling gets fewer of them first
        let q = self.save.settings.quality();
        let grass = if shown.env_id() != ArenaKey::Vondel.id() || self.hide_meshes.contains(&15) { 0.0 } else { q.grass.min(if self.render_scale < 0.72 { 0.6 } else { 1.0 }) };
        let t_scene = now_ms();
        let fd = Arc::new(FrameData { globals: self.globals(lk), list: list.clone(), clear: lk.clear, scale: self.render_scale, grass, leaves_far: q.leaves_far, shadows: q.shadows, bloom: q.bloom });
        self.frame_list = list;
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(full, SceneCallback(fd)));
        let painter = ui.painter().clone();
        crate::ui::draw(self, &ctx, &painter, sr, full);
        let t_ui = now_ms();
        // a hitch, in words: which part of this frame took the time, or how long since the last one
        // (that gap is the GPU, texture uploads, the browser). Shows in the console and in logcat
        let gap = ctx.input(|i| i.unstable_dt) * 1000.0;
        if (t_ui - t_start > slow_ms() || gap > 80.0) && self.frames >= 4 {
            log_str(&format!(
                "slow frame: {:.0} ms here (input+net {:.0} = insets {:.0} fonts {:.0} input {:.0} net {:.0} friends {:.0}, match+scene {:.0}, ui {:.0}), {:.0} ms since the last; {:?} {:?} {:?}",
                t_ui - t_start,
                t_in - t_start,
                t_insets - t_start,
                t_warm - t_insets,
                t_input - t_warm,
                t_net - t_input,
                t_in - t_net,
                t_scene - t_in,
                t_ui - t_scene,
                gap,
                self.screen,
                self.sheet,
                self.game.state
            ));
        }
        // moving to another screen closes whatever was open on the old one
        let (ls, lv, lp, lsh) = self.last_nav;
        if (self.screen, self.view) != (ls, lv) || (self.screen == Screen::Play && self.step != lp) {
            if self.sheet == lsh {
                self.sheet = Sheet::None;
            }
            self.menu = MatchMenu::None;
            self.lock_note = None;
            self.hint = None;
            if self.screen != Screen::Play {
                self.step = PlayStep::Choose;
            }
            if let Some(n) = self.net.as_mut() {
                n.rooms.clear();
            }
        }
        if self.sheet != Sheet::None {
            self.menu = MatchMenu::None;
        }
        self.last_nav = (self.screen, self.view, self.step, self.sheet);
        self.debug_cmds(sr);
        #[cfg(not(target_arch = "wasm32"))]
        shot(&ctx);
        SCREEN.with(|c| c.set(if self.screen == Screen::Home && self.view == View::Menu && self.sheet == Sheet::None { 0 } else { 1 }));
        ctx.request_repaint();
    }
}

/// The headline sizes there are. Every size is a row of glyphs in egui's atlas, and a full atlas is
/// thrown away and drawn again (a hitch), so the big font comes in these sizes only.
pub(crate) const ANTON_SIZES: [f32; 10] = [12.0, 14.0, 17.0, 20.0, 24.0, 28.0, 34.0, 44.0, 56.0, 64.0];
pub(crate) fn anton_size(size: f32) -> f32 {
    ANTON_SIZES.iter().copied().min_by(|a, b| (a - size).abs().total_cmp(&(b - size).abs())).unwrap_or(size)
}
pub(crate) fn anton(size: f32) -> FontId {
    FontId::new(anton_size(size), FontFamily::Name("anton".into()))
}
pub(crate) fn fade(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

// ---------- test hook: window.flunki_cmd("…") queues commands for the next frame ----------
thread_local! {
    /// 0 when the main menu is showing (the Android back button then leaves the app).
    pub static SCREEN: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    pub static CMDS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    /// A screenshot asked for by `shot:path`.
    static SHOT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}
#[cfg(not(target_arch = "wasm32"))]
fn shot(ctx: &egui::Context) {
    let Some(path) = SHOT.with(|s| s.borrow_mut().take()) else { return };
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
        let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
        out.extend(img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b()]));
        let _ = std::fs::write(&path, out);
        log_str(&format!("shot {path}"));
    })));
}
/// Test hook on the desktop: with FLUNKI_STDIN set, each line on stdin is a `flunki_cmd`.
#[cfg(not(target_arch = "wasm32"))]
static STDIN_CMDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
#[cfg(not(target_arch = "wasm32"))]
pub fn stdin_cmds() {
    if std::env::var_os("FLUNKI_STDIN").is_some() {
        std::thread::spawn(|| {
            for l in std::io::stdin().lines().map_while(Result::ok) {
                if let Ok(mut q) = STDIN_CMDS.lock() {
                    q.push(l);
                }
            }
        });
    }
}
impl FlunkiApp {
    fn debug_cmds(&mut self, sr: Rect) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut q) = STDIN_CMDS.lock() {
            CMDS.with(|c| c.borrow_mut().append(&mut q));
        }
        // test hook on a phone: `adb shell run-as <app> sh -c 'echo state > files/cmds'`
        #[cfg(target_os = "android")]
        if let Some(p) = crate::android::data_dir().map(|d| d.join("cmds")) {
            if let Ok(t) = std::fs::read_to_string(&p) {
                let _ = std::fs::remove_file(&p);
                CMDS.with(|c| c.borrow_mut().extend(t.lines().map(str::to_string)));
            }
        }
        let cmds: Vec<String> = CMDS.with(|c| std::mem::take(&mut *c.borrow_mut()));
        for c in cmds {
            let (cmd, arg) = c.split_once(':').unwrap_or((c.as_str(), ""));
            match cmd {
                "cpu" => self.start(Mode::Cpu),
                "hot" => self.start(Mode::Hot),
                "menu" => self.go_home(View::Menu),
                "club" => self.go_home(View::Club),
                "back" => {
                    self.back();
                }
                "sheet" => {
                    self.sheet = match arg {
                        "profile" => Sheet::Profile,
                        "stats" => Sheet::Stats,
                        "progress" => Sheet::Progress,
                        "train" => Sheet::Train,
                        "settings" => Sheet::Settings,
                        "rules" => Sheet::Rules,
                        "friends" => Sheet::Friends,
                        "account" => Sheet::Account,
                        "bug" => Sheet::Bug,
                        _ => Sheet::None,
                    }
                }
                "play" => {
                    self.screen = Screen::Play;
                    self.step = match arg {
                        "cpu" => PlayStep::Cpu,
                        "hot" => PlayStep::Hot,
                        "online" => PlayStep::Online,
                        _ => PlayStep::Choose,
                    };
                }
                "arena" => self.arena = ArenaKey::from_id(arg),
                "xp" => {
                    if let Ok(v) = arg.parse() {
                        self.save.xp = v;
                        self.save.store();
                    }
                }
                "name" => {
                    self.save.profile.name = arg.into();
                    self.name_edit = arg.into();
                    self.save.store();
                }
                // test hooks for logging in: `login:user,pass`, `signup:user,pass`, `logout`
                "login" | "signup" => {
                    let (u, p) = arg.split_once(',').unwrap_or((arg, ""));
                    let name = self.save.profile.name.clone();
                    self.social.log_in(u, p, &name, cmd == "signup");
                }
                "logout" => self.log_out(),
                // test hook: no fading in, for screenshots from a headless browser (few frames)
                "noanim" => self.no_anim = true,
                "netdrop" => {
                    // test hook: the connection breaks, the way a tunnel breaks it
                    if let Some(n) = self.net.as_ref() {
                        n.sock.close();
                    }
                }
                "host" => self.net_host(),
                "join" => self.net_join(arg),
                "start" => self.net_start(),
                "team" => self.net_set_team(arg.parse().unwrap_or(0)),
                "drill" => self.start_drill(match arg {
                    "drink" => Drill::Drink,
                    "run" => Drill::Run,
                    _ => Drill::Throw,
                }),
                "tut" => self.start_tutorial(),
                "tutnext" => {
                    if self.game.tut == Some(game::Tut::Intro) {
                        self.game.set_tut(game::Tut::Throw)
                    } else if self.game.tut == Some(game::Tut::Ref) {
                        self.game.set_tut(game::Tut::Done)
                    }
                }
                "qm" => self.send_qm(arg.parse().unwrap_or(0)),
                "emote" => self.send_emote(arg.parse().unwrap_or(0)),
                "throw" | "miss" => {
                    if self.game.local_aiming() {
                        let t = self.game.turn;
                        let o = self.game.teams[t].players[self.game.teams[t].idx].hand();
                        let yaw = (-o.x).atan2(-o.z) + if cmd == "miss" { 0.3 } else { 0.0 };
                        let dist = (o.x * o.x + o.z * o.z).sqrt() - 0.3;
                        self.act_throw(ThrowSpec { origin: o, yaw, dist, curve: 0.0, ring: 2 });
                    }
                }
                "flicktest" => {
                    if self.game.local_aiming() {
                        let now = self.time * 1000.0;
                        let x = sr.center().x;
                        let y0 = sr.bottom() - 120.0;
                        let pts = (0..6).map(|i| (now - 80.0 + i as f64 * 16.0, pos2(x + i as f32 * 2.0, y0 - i as f32 * 70.0))).collect::<Vec<_>>();
                        let cur = pts.last().unwrap().1;
                        self.flick = Some(Flick { pts, cur, s: cur, prev_ang: None, acc: 0.0, armed: 0, spin_vis: 0.0 });
                        self.release_flick(sr);
                        log_str(&format!("flicktest → state {:?}", self.game.state));
                    }
                }
                "drink" | "nodrink" => {
                    let t = self.game.my_team.unwrap_or(0);
                    self.act_drink(t, cmd == "drink")
                }
                "l" | "r" => {
                    let t = self.game.my_team.unwrap_or(0);
                    self.act_pad(t, if cmd == "l" { Foot::L } else { Foot::R })
                }
                "ff" => {
                    // fast-forward the match (tests run in a very slow browser)
                    let secs: f32 = arg.parse().unwrap_or(1.0);
                    for _ in 0..(secs * 60.0) as usize {
                        self.game.tick(1.0 / 60.0, self.ref_mouth, self.ref_tip);
                        self.animate_all(1.0 / 60.0);
                    }
                }
                "lab" => {
                    // lab:<pose> or lab:walk:<m/s>
                    let mut it = arg.split(':');
                    self.lab_mode = it.next().unwrap_or("").into();
                    self.lab_speed = if self.lab_mode == "walk" { it.next().and_then(|v| v.parse().ok()).unwrap_or(1.4) } else { 0.0 };
                    if self.lab_speed == 0.0 {
                        self.hub_person.pos = arenas::HUB_O;
                        self.hub_person.yaw = 0.25;
                    }
                }
                "labff" => {
                    let secs: f32 = arg.parse().unwrap_or(0.5);
                    for _ in 0..(secs * 60.0) as usize {
                        let a = &mut self.hub_person.anim;
                        if a.throw_t >= 0.0 {
                            a.throw_t += 1.0 / 60.0;
                            if a.throw_t > crate::anim::THROW_LEN {
                                a.throw_t = -1.0;
                            }
                        }
                        self.hub_person.vis.treadmill = self.lab_speed;
                        crate::anim::animate(&mut self.hub_person, 1.0 / 60.0, None, 0.5);
                    }
                }
                "friendadd" => self.social.send(ToServer::FriendAdd { code: arg.into() }),
                "friendsay" => {
                    self.social.say_in = arg.into();
                    self.social.say_open_chat();
                }
                "friendchat" => self.social.open(arg),
                "friendstats" => self.social.open_stats_for(arg),
                "friendinvite" => {
                    let room = self.net.as_ref().map_or(String::new(), |n| n.code.clone());
                    self.social.send(ToServer::Invite { to: arg.into(), room });
                }
                "inviteyes" => self.accept_invite(),
                "friendstate" => log_str(&format!("social ready={} code={} friends={:?} requests={:?} sent={:?} invite={} err={}", self.social.ready, self.social.code, self.social.friends.iter().map(|f| (f.name.as_str(), f.online, f.status, f.unread)).collect::<Vec<_>>(), self.social.requests.len(), self.social.sent.len(), self.social.invite.as_ref().map_or("", |i| i.room.as_str()), self.social.err)),
                "sneak" => self.act_sneak(),
                "straf" => {
                    let t = self.game.my_team.unwrap_or(0);
                    self.game.judge(t, game::Foul::Can);
                }
                "win" => {
                    let t = self.game.my_team.unwrap_or(0);
                    if !self.game.mirror {
                        self.game.teams[t].beers = [0.0, 0.0, 0.004];
                    }
                }
                "cam" => {
                    let v: Vec<f32> = arg.split(',').filter_map(|x| x.parse().ok()).collect();
                    self.cam_override = (v.len() == 6).then(|| (vec3(v[0], v[1], v[2]), vec3(v[3], v[4], v[5])));
                    self.cam_snap = true;
                }
                "scale" => {
                    self.scale_lock = true;
                    self.render_scale = arg.parse().unwrap_or(1.0);
                }
                "quality" => self.save.settings.quality = arg.parse::<u8>().unwrap_or(2).min(2),
                "hide" => self.hide_meshes = arg.split(',').filter_map(|x| x.parse().ok()).collect(),
                "fps" => log_str(&format!("fps {:.1}", 1.0 / self.fps_dt.max(1e-4))),
                // the desktop: save what's on screen as a PPM picture
                #[cfg(not(target_arch = "wasm32"))]
                "shot" => SHOT.with(|s| *s.borrow_mut() = Some(arg.to_string())),
                "state" => {
                    let n = self.net.as_ref().map(|n| format!("net(id={} host={} room={} members={:?} phase={} rtt={:.3} link={:?} pause={:?} skipped={} missed={} err={})", n.id, n.host(), n.code, n.members.iter().map(|m| (m.name.clone(), m.team, m.away)).collect::<Vec<_>>(), n.phase, n.rtt, n.link(self.game.pred.snap_age, self.game.mirror), self.game.net_pause, self.game.pred.skipped_snaps, self.game.pred.missed_ev, self.net_err)).unwrap_or_else(|| format!("nonet err={}", self.net_err));
                    log_str(&format!(
                        "screen={:?} view={:?} sheet={:?} state={:?} mode={:?} mirror={} turn={} beers={:?}/{:?} straf={}/{} xp={} lv={} matches={} pb={:?} tut={:?} my_team={:?} {}",
                        self.screen, self.view, self.sheet, self.game.state, self.game.mode, self.game.mirror, self.game.turn, self.game.teams[0].beers, self.game.teams[1].beers, self.game.teams[0].straf, self.game.teams[1].straf, self.save.xp, self.save.level(), self.save.matches.len(), self.save.train_pb, self.game.tut, self.game.my_team, n
                    ))
                }
                _ => {}
            }
        }
    }
}
/// The last lines logged, for a bug report.
static RECENT: std::sync::Mutex<std::collections::VecDeque<String>> = std::sync::Mutex::new(std::collections::VecDeque::new());
const RECENT_LINES: usize = 40;
fn keep_line(s: &str) {
    if let Ok(mut r) = RECENT.lock() {
        if r.len() >= RECENT_LINES {
            r.pop_front();
        }
        r.push_back(format!("{:.1} {}", now_ms() / 1000.0 % 100000.0, s.chars().take(300).collect::<String>()));
    }
}
pub(crate) fn recent_log() -> Vec<String> {
    RECENT.lock().map(|r| r.iter().cloned().collect()).unwrap_or_default()
}
#[cfg(target_arch = "wasm32")]
pub(crate) fn log_str(s: &str) {
    keep_line(s);
    web_sys::console::log_1(&s.into());
}
#[cfg(target_os = "android")]
pub(crate) fn log_str(s: &str) {
    keep_line(s);
    log::info!("{s}");
}
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn log_str(s: &str) {
    keep_line(s);
    println!("{s}");
}
/// A browser notification, while the tab isn't being looked at. Notification may not exist at all
/// (an iPhone outside a home-screen app): everything goes through `Reflect`.
#[cfg(target_arch = "wasm32")]
fn web_notify(n: &net::Note) {
    use js_sys::Reflect;
    use wasm_bindgen::JsCast;
    let Some(w) = web_sys::window() else { return };
    if !w.document().map_or(false, |d| d.hidden()) {
        return;
    }
    let Some(cls) = Reflect::get(&w, &"Notification".into()).ok().and_then(|c| c.dyn_into::<js_sys::Function>().ok()) else { return };
    if Reflect::get(&cls, &"permission".into()).ok().and_then(|p| p.as_string()).as_deref() != Some("granted") {
        return;
    }
    let (title, body) = note_words(n);
    let opts = js_sys::Object::new();
    let _ = Reflect::set(&opts, &"body".into(), &body.into());
    let _ = Reflect::set(&opts, &"tag".into(), &format!("flunkiebal-{}", n.id).into());
    let args = js_sys::Array::of2(&title.into(), &opts);
    if let Ok(o) = Reflect::construct(&cls, &args) {
        // tapping it brings the game's tab to the front
        let f = js_sys::Function::new_no_args("window.focus(); this.close();");
        let _ = Reflect::set(&o, &"onclick".into(), &f);
    }
}
/// Ask the browser for notifications. Called from a tap, which browsers want for this.
#[cfg(target_arch = "wasm32")]
pub(crate) fn web_notify_permission(ask: bool) -> String {
    use js_sys::Reflect;
    use wasm_bindgen::JsCast;
    let Some(w) = web_sys::window() else { return "unsupported".into() };
    let Some(cls) = Reflect::get(&w, &"Notification".into()).ok().and_then(|c| c.dyn_into::<js_sys::Function>().ok()) else { return "unsupported".into() };
    let now = Reflect::get(&cls, &"permission".into()).ok().and_then(|p| p.as_string()).unwrap_or_default();
    if ask && now == "default" {
        if let Ok(f) = Reflect::get(&cls, &"requestPermission".into()).and_then(|f| f.dyn_into::<js_sys::Function>().map_err(Into::into)) {
            let _ = f.call0(&cls);
        }
    }
    now
}
/// A notification in words: (title, text).
pub(crate) fn note_words(n: &net::Note) -> (String, String) {
    if n.kind == net::NOTE_INVITE {
        (format!("{} invites you", n.name), format!("Come play FlunkieBal: match {}", n.room))
    } else {
        (n.name.clone(), n.text.clone())
    }
}
/// Safe-area insets from the page (CSS env()) and, in the Android app, from the display cutout.
#[cfg(target_arch = "wasm32")]
fn safe_insets() -> [f32; 4] {
    use wasm_bindgen::JsCast;
    let Some(w) = web_sys::window() else { return [0.0; 4] };
    let f = js_sys::Reflect::get(&w, &"flunkiInsets".into()).ok().and_then(|f| f.dyn_into::<js_sys::Function>().ok());
    let s = f.and_then(|f| f.call0(&w).ok()).and_then(|v| v.as_string()).unwrap_or_default();
    let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if v.len() == 4 {
        [v[0].clamp(0.0, 80.0), v[1].clamp(0.0, 80.0), v[2].clamp(0.0, 60.0), v[3].clamp(0.0, 80.0)]
    } else {
        [0.0; 4]
    }
}
#[cfg(target_os = "android")]
fn safe_insets() -> [f32; 4] {
    crate::android::insets()
}
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn safe_insets() -> [f32; 4] {
    [0.0; 4]
}
#[cfg(target_arch = "wasm32")]
pub(crate) fn now_ms() -> f64 {
    js_sys::Date::now()
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

/// A frame slower than this is logged (`FLUNKI_SLOW_MS` on the desktop, for looking closer).
fn slow_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(v) = std::env::var("FLUNKI_SLOW_MS").ok().and_then(|v| v.parse().ok()) {
        return v;
    }
    40.0
}
