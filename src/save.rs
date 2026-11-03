//! Everything kept on this device: your player, XP, match history, drill records and settings.
//! Stored as one JSON document in localStorage (web); kept in memory on desktop.
//! The web version logs in with a username and password (`needs_login`): its progression belongs to
//! that account, is kept on the server too, and nothing is earned while logged out.

use serde::{Deserialize, Serialize};

pub const SWATCHES: [u32; 8] = [0xe8b02a, 0xe8742a, 0xd43c3c, 0xe05a9a, 0x8a5ad0, 0x2f86b8, 0x2aa89a, 0x4caa3c];
pub const SW_NAMES: [&str; 8] = ["Gold", "Orange", "Red", "Pink", "Purple", "Blue", "Teal", "Green"];
pub const SKIN_OPTS: [u32; 6] = [0xf3cfb0, 0xe2b597, 0xc98e66, 0x9a6544, 0x6e4329, 0x47291a];
pub const SKIN_NAMES: [&str; 6] = ["Very light", "Light", "Medium", "Tan", "Brown", "Dark"];
pub const HAIR_OPTS: [u32; 6] = [0x161210, 0x3a2414, 0x6b4424, 0xb08a4f, 0xe0c48a, 0xa8401e];
pub const HAIR_NAMES: [&str; 6] = ["Black", "Dark brown", "Brown", "Dark blond", "Blond", "Ginger"];
pub const SHORTS_OPTS: [u32; 6] = [0x1c1c20, 0x3a4458, 0x2d3b2a, 0x6b5a44, 0xe8e2d0, 0x7a1e24];
pub const SHORTS_NAMES: [&str; 6] = ["Black", "Navy", "Olive", "Khaki", "White", "Burgundy"];
/// name, pitch, speed
pub const VOICES: [(&str, f32, f32); 4] = [("Deep", 0.62, 0.95), ("Low", 0.82, 1.0), ("Normal", 1.0, 1.02), ("Bright", 1.22, 1.05)];

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub color: usize,
    pub skin: usize,
    pub hair: usize,
    pub shorts: usize,
    pub voice: usize,
    pub hat: u8,
}
impl Default for Profile {
    fn default() -> Self {
        Profile { name: "Player".into(), color: 0, skin: 1, hair: 1, shorts: 1, voice: 1, hat: 0 }
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct MatchRec {
    pub t: f64,
    pub mode: String, // cpu · hot · online
    pub diff: usize,
    pub arena: String,
    pub win: Option<bool>,
    pub winner: String,
    pub throws: u32,
    pub hits: u32,
    pub exc: u32,
    pub great: u32,
    pub nice: u32,
    pub curve: u32,
    pub straf: u32,
    pub dodged: u32,
    pub sneaks: u32,
    pub litres: f32,
    pub fast_run: f32,
    pub dur: u32,
    pub opp: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub volume: u32,
    pub vibrate: bool,
    pub sens: f32,
    pub trail: bool,
    pub voices: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Settings { volume: 80, vibrate: true, sens: 1.0, trail: true, voices: true }
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Save {
    pub profile: Profile,
    pub xp: u32,
    pub matches: Vec<MatchRec>,
    pub train_pb: [Option<f32>; 3], // throw (hits), drink (s), run (s)
    pub tut_done: bool,
    pub settings: Settings,
    pub arena: String,
    pub diff: usize,
    /// who you are to the friends system on that server: (server, code, secret)
    pub account: (String, String, String),
    /// logged in with a password as this username, as typed (web), on the server in `account`; it's
    /// also the player's name
    pub user: String,
    /// counts up with every change to the progression, to tell it from the server's copy
    pub rev: u64,
    /// progression from before logging in was needed, for the first account made here
    pub legacy: Option<String>,
    /// logged in where it's optional (the phone): its own account, progression and rev, back on logging out
    pub device: Option<((String, String, String), String, u64)>,
}

/// What of the save belongs to the account and is kept on the server for it.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Progress {
    profile: Profile,
    xp: u32,
    matches: Vec<MatchRec>,
    train_pb: [Option<f32>; 3],
    tut_done: bool,
}
/// The latest matches the server keeps; the rest stay on this device only.
const SYNC_MATCHES: usize = 100;

/// Must you log in to earn anything and to play online? On the web, where a device is no one in
/// particular; `FLUNKI_LOGIN=1` tries it out on the desktop.
pub fn needs_login() -> bool {
    #[cfg(target_arch = "wasm32")]
    return true;
    #[cfg(not(target_arch = "wasm32"))]
    return std::env::var("FLUNKI_LOGIN").map_or(false, |v| v == "1");
}

pub const SERVER: &str = "wss://flunkiebal.flunkcie.nl";

/// Where online matches and friends are, always. `FLUNKI_SERVER` points the desktop build elsewhere,
/// for trying out a server of your own.
pub fn server() -> String {
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    if let Ok(s) = std::env::var("FLUNKI_SERVER") {
        return s;
    }
    SERVER.into()
}
const KEY: &str = "flunkiebal-save";
/// Where saves lived under earlier names; read if there is no new save yet.
#[cfg(target_arch = "wasm32")]
const OLD_KEYS: [&str; 2] = ["flunkieball-save", "flunki-rs-save"];
pub const STATS_MAX: usize = 300;

impl Save {
    pub fn load() -> Save {
        let mut s: Save = read().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if s.arena.is_empty() {
            s.arena = "vondel".into();
        }
        s.diff = s.diff.min(2);
        if needs_login() && s.user.is_empty() && (s.xp > 0 || !s.matches.is_empty() || s.train_pb.iter().any(|p| p.is_some())) {
            // earned before logging in was needed: put aside for the first account made on this device
            s.legacy = Some(s.progress());
            s.reset_progress();
            s.store();
        }
        s
    }
    /// Logged out where logging in is needed: nothing is earned.
    pub fn locked(&self) -> bool {
        needs_login() && (self.user.is_empty() || self.account.0 != server())
    }
    /// The progression, as the server keeps it.
    pub fn progress(&self) -> String {
        let from = self.matches.len().saturating_sub(SYNC_MATCHES);
        let p = Progress { profile: self.profile.clone(), xp: self.xp, matches: self.matches[from..].to_vec(), train_pb: self.train_pb, tut_done: self.tut_done };
        serde_json::to_string(&p).unwrap_or_default()
    }
    /// Take over the progression the server kept; false if it can't be read.
    pub fn set_progress(&mut self, data: &str) -> bool {
        let Ok(p) = serde_json::from_str::<Progress>(data) else { return false };
        self.profile = p.profile;
        self.xp = p.xp;
        self.matches = p.matches;
        self.train_pb = p.train_pb;
        self.tut_done = p.tut_done;
        true
    }
    fn reset_progress(&mut self) {
        self.set_progress("{}");
    }
    /// Logged in with a username and password (on any platform).
    pub fn logged_in(&self) -> bool {
        !self.user.is_empty()
    }
    /// Log out: forget the account and what it had earned, keep the settings. Where logging in is
    /// optional, the device's own player is back.
    pub fn log_out(&mut self) {
        self.user.clear();
        match self.device.take() {
            Some((account, progress, rev)) => {
                self.account = account;
                self.rev = rev;
                if !self.set_progress(&progress) {
                    self.reset_progress();
                }
            }
            None => {
                self.account = Default::default();
                self.rev = 0;
                self.reset_progress();
            }
        }
        self.store();
    }
    pub fn store(&self) {
        if let Ok(t) = serde_json::to_string(self) {
            write(&t);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn read() -> Option<String> {
    let st = web_sys::window()?.local_storage().ok()??;
    st.get_item(KEY).ok().flatten().or_else(|| OLD_KEYS.iter().find_map(|k| st.get_item(k).ok().flatten()))
}
#[cfg(target_arch = "wasm32")]
fn write(t: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(KEY, t);
    }
}
#[cfg(target_os = "android")]
fn read() -> Option<String> {
    std::fs::read_to_string(crate::android::data_dir()?.join("save.json")).ok()
}
#[cfg(target_os = "android")]
fn write(t: &str) {
    // never a half-written save, whenever the phone decides to end us
    let Some(d) = crate::android::data_dir() else { return };
    let tmp = d.join("save.tmp");
    if std::fs::write(&tmp, t).is_ok() {
        let _ = std::fs::rename(&tmp, d.join("save.json"));
    }
}
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn read() -> Option<String> {
    None
}
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn write(_t: &str) {}

// ---------- progression ----------
/// The highest level anything unlocks at; levels themselves go on for ever.
pub const TOP_UNLOCK: u32 = 10;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unlock {
    Hat(u8),
    Shirt(usize),
    Hair(usize),
    Shorts(usize),
    Arena(&'static str),
    Voice(usize),
    /// a quick message, by its number in `voice::QM`
    Quick(usize),
    /// an emote, by its number in `anim::EMOTES`
    Emote(usize),
}
/// Whatever isn't in here you have from the start: three quick messages (Proost!, Nice throw!, Drink up!)
/// and two emotes (Wave, Clap).
pub const UNLOCKS: [(u32, Unlock, &str); 33] = [
    (2, Unlock::Hat(1), "Cap"),
    (2, Unlock::Shirt(6), "Teal shirt"),
    (2, Unlock::Hair(4), "Blond hair"),
    (2, Unlock::Shorts(3), "Khaki shorts"),
    (3, Unlock::Arena("roof"), "Science Park dak"),
    (3, Unlock::Voice(0), "Deep voice"),
    (3, Unlock::Shirt(7), "Green shirt"),
    (4, Unlock::Hat(2), "Bucket hat"),
    (4, Unlock::Shirt(3), "Pink shirt"),
    (4, Unlock::Shorts(4), "White shorts"),
    (5, Unlock::Arena("cafe"), "Café de Krul"),
    (5, Unlock::Hair(5), "Ginger hair"),
    (6, Unlock::Hat(3), "Sunglasses"),
    (6, Unlock::Shirt(4), "Purple shirt"),
    (6, Unlock::Shorts(5), "Burgundy shorts"),
    (7, Unlock::Arena("club"), "Duplex"),
    (8, Unlock::Hat(4), "Headband"),
    (9, Unlock::Voice(3), "Bright voice"),
    (10, Unlock::Hat(5), "Flunk King crown"),
    (2, Unlock::Arena("evening"), "Vondelpark evening"),
    (9, Unlock::Arena("krakau"), "Krakau Hostel"),
    (2, Unlock::Quick(8), "“GG”"),
    (2, Unlock::Emote(6), "Bow emote"),
    (3, Unlock::Quick(4), "“Oops…”"),
    (4, Unlock::Quick(6), "Laugh message"),
    (4, Unlock::Emote(5), "Facepalm emote"),
    (5, Unlock::Quick(5), "“Hurry up!”"),
    (5, Unlock::Emote(4), "Flex emote"),
    (6, Unlock::Quick(7), "Clap message"),
    (7, Unlock::Emote(1), "Buzzball emote"),
    (8, Unlock::Quick(3), "“Ref, are you blind?”"),
    (8, Unlock::Emote(0), "Smoke emote"),
    (9, Unlock::Emote(2), "Dance emote"),
];
/// XP needed to go from level l to l+1.
pub fn xp_for(l: u32) -> u32 {
    100 + 40 * (l - 1)
}
#[derive(Clone, Copy, Debug)]
pub struct Level {
    pub lv: u32,
    pub into: u32,
    pub need: u32,
}
pub fn level_of(mut xp: u32) -> Level {
    let mut l = 1;
    while xp >= xp_for(l) {
        xp -= xp_for(l);
        l += 1;
    }
    Level { lv: l, into: xp, need: xp_for(l) }
}
pub fn unlock_lv(u: Unlock) -> u32 {
    UNLOCKS.iter().find(|x| x.1 == u).map_or(1, |x| x.0)
}

#[derive(Clone, Debug)]
pub struct XpResult {
    pub gain: u32,
    pub before: Level,
    pub after: Level,
    pub unlocked: Vec<&'static str>,
    pub parts: Vec<String>,
}

impl Save {
    pub fn level(&self) -> u32 {
        level_of(self.xp).lv
    }
    pub fn unlocked(&self, u: Unlock) -> bool {
        self.level() >= unlock_lv(u)
    }
    pub fn add_xp(&mut self, n: u32, parts: Vec<String>) -> XpResult {
        let n = if self.locked() { 0 } else { n };
        let before = level_of(self.xp);
        self.xp += n;
        let after = level_of(self.xp);
        let unlocked = UNLOCKS.iter().filter(|u| u.0 > before.lv && u.0 <= after.lv).map(|u| u.2).collect();
        self.store();
        XpResult { gain: n, before, after, unlocked, parts }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quick_messages_and_emotes_progress() {
        use crate::anim::EMOTES;
        use crate::voice::QM;
        let new = Save::default();
        assert_eq!(new.level(), 1);
        let have = |s: &Save, f: fn(usize) -> Unlock, n: usize| (0..n).filter(|i| s.unlocked(f(*i))).collect::<Vec<_>>();
        assert_eq!(have(&new, Unlock::Quick, QM.len()), vec![0, 1, 2], "three messages to start with");
        assert_eq!(have(&new, Unlock::Emote, EMOTES.len()), vec![3, 7], "wave and clap to start with");
        let mut top = Save::default();
        top.xp = 1_000_000;
        assert_eq!(have(&top, Unlock::Quick, QM.len()).len(), QM.len());
        assert_eq!(have(&top, Unlock::Emote, EMOTES.len()).len(), EMOTES.len());
        // and levelling up says what you've got
        let mut s = Save::default();
        let r = s.add_xp(xp_for(1), vec![]);
        assert!(r.unlocked.contains(&"Bow emote") && r.unlocked.contains(&"“GG”"), "{:?}", r.unlocked);
    }
    #[test]
    fn progress_round_trip() {
        let mut s = Save::default();
        s.xp = 321;
        s.profile.name = "Anna".into();
        s.train_pb[1] = Some(4.5);
        s.matches = (0..SYNC_MATCHES + 20).map(|i| MatchRec { hits: i as u32, ..Default::default() }).collect();
        let data = s.progress();
        assert!(data.len() < crate::net::MAX_PROGRESS, "{} bytes", data.len());
        let mut t = Save::default();
        t.settings.volume = 10;
        assert!(t.set_progress(&data));
        assert_eq!((t.xp, t.profile.name.as_str(), t.train_pb[1]), (321, "Anna", Some(4.5)));
        assert_eq!(t.matches.len(), SYNC_MATCHES, "the latest ones");
        assert_eq!(t.matches[0].hits, 20);
        assert_eq!(t.settings.volume, 10, "settings stay this device's");
        assert!(!t.set_progress("nonsense"));
    }
    #[test]
    fn one_server() {
        assert_eq!(server(), SERVER);
        // settings from when you could pick a server still load
        let s: Settings = serde_json::from_str(r#"{"volume":30,"server":"ws://10.0.0.5:8787"}"#).unwrap();
        assert_eq!(s.volume, 30);
    }
    use super::*;
    #[test]
    fn levels() {
        assert_eq!(level_of(0).lv, 1);
        assert_eq!(level_of(99).lv, 1);
        assert_eq!(level_of(100).lv, 2);
        assert_eq!(level_of(100 + 140).lv, 3);
        assert_eq!(level_of(100000).lv, 69, "no top level");
        assert_eq!(unlock_lv(Unlock::Arena("club")), 7);
        assert_eq!(unlock_lv(Unlock::Shirt(0)), 1);
    }
}
