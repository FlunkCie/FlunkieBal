//! What a person wears and how they're built: the pieces of `assets/people_*.glb` (see `tools/quaternius.py`)
//! put together by `Wear`, and builds (`BUILDS`: bones made wider or narrower, see `Rig::pose`).

use glam::Vec3;

/// What someone has on: body (`BODIES`), build (`BUILDS`), and per slot (`SLOTS`) a piece of clothing by
/// its number in `proto::CLOTHES` (0: none; a piece made for the other body counts as none too). Sent
/// online as `proto::Member::wear`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Wear {
    pub body: u8,
    pub build: u8,
    pub slots: [u8; 6],
}
impl Wear {
    pub fn to_bytes(self) -> [u8; 8] {
        let s = self.slots;
        [self.body, self.build, s[0], s[1], s[2], s[3], s[4], s[5]]
    }
    pub fn from_bytes(b: [u8; 8]) -> Wear {
        Wear { body: b[0] % BODIES.len() as u8, build: b[1] % BUILDS.len() as u8, slots: [b[2], b[3], b[4], b[5], b[6], b[7]] }
    }
    /// The rig (`Model::rigs`) this body is.
    pub fn rig(self) -> usize {
        BODIES[self.body as usize % BODIES.len()].1
    }
    /// What's worn in a slot, if it fits this body; a slot that can't be empty gets the first free piece.
    pub fn piece(self, slot: usize) -> Option<&'static Item> {
        let rig = self.rig();
        match crate::net::clothes(self.slots[slot]) {
            Some(c) if rig_of(c) == rig && slot_of(c) == slot => Some(c),
            _ if SLOTS[slot].2 => items(rig, slot).find(|c| c.price == 0),
            _ => None,
        }
    }
    /// The everyday clothes a new player has on.
    pub fn starter(body: u8) -> Wear {
        let mut w = Wear { body, build: 0, slots: [0; 6] };
        for slot in 0..SLOTS.len() {
            w.slots[slot] = items(w.rig(), slot).find(|c| c.price == 0).map_or(0, |c| c.id);
        }
        w
    }
    /// Anyone at all: for the people the game puts on the pitch.
    pub fn random(r: &mut impl FnMut() -> f32) -> Wear {
        let mut pick = |n: usize| ((r() * n as f32) as usize).min(n.saturating_sub(1));
        let body = pick(BODIES.len()) as u8;
        let mut w = Wear { body, build: pick(BUILDS.len()) as u8, slots: [0; 6] };
        for slot in 0..SLOTS.len() {
            let all: Vec<_> = items(w.rig(), slot).collect();
            // not everyone carries a backpack
            if all.is_empty() || (!SLOTS[slot].2 && pick(4) != 0) {
                continue;
            }
            w.slots[slot] = all[pick(all.len())].id;
        }
        w
    }
}

use crate::net::Item;
/// The rig a piece is made for.
pub fn rig_of(c: &Item) -> usize {
    if c.piece.starts_with("f/") { 1 } else { 0 }
}
pub fn slot_of(c: &Item) -> usize {
    SLOTS.iter().position(|s| c.piece.ends_with(s.1)).unwrap_or(0)
}
/// A rig's pieces for a slot.
pub fn items(rig: usize, slot: usize) -> impl Iterator<Item = &'static Item> {
    crate::net::CLOTHES.iter().filter(move |c| rig_of(c) == rig && slot_of(c) == slot)
}

/// name; the rig (0 male, 1 female)
pub const BODIES: [(&str, usize); 2] = [("Man", 0), ("Woman", 1)];
/// The slots: shown name, the end of their pieces' mesh names, whether something is always worn there.
/// The top takes the team's colour, trousers the shorts colour, hair the hair colour.
pub const SLOTS: [(&str, &str, bool); 6] = [("Hair", "_Hair", false), ("Face", "_Head", true), ("Top", "_Body", true), ("Trousers", "_Legs", true), ("Shoes", "_Feet", true), ("Back", "_Back", false)];

/// A build: how much wider (x, z) or longer (y) each bone's own part of the body is, in the bone's axes
/// (y runs along the bone), and the size of the whole person.
pub struct Build {
    pub name: &'static str,
    pub scale: f32,
    parts: &'static [(&'static str, [f32; 3])],
}
impl Build {
    /// By bone name, either side (`upperarm` covers `upperarm_l` and `upperarm_r`).
    pub fn scale_of(&self, bone: &str) -> Vec3 {
        let base = bone.strip_suffix("_l").or_else(|| bone.strip_suffix("_r")).unwrap_or(bone);
        self.parts.iter().find(|p| p.0 == base).map(|p| Vec3::from_array(p.1)).unwrap_or(Vec3::ONE)
    }
}

pub const BUILDS: [Build; 5] = [
    Build { name: "Regular", scale: 1.0, parts: &[] },
    Build {
        name: "Stocky",
        scale: 0.95,
        parts: &[("spine_03", [1.14, 1.0, 1.1]), ("spine_02", [1.16, 1.0, 1.14]), ("spine_01", [1.14, 1.0, 1.12]), ("pelvis", [1.1, 1.0, 1.08]), ("upperarm", [1.16, 1.0, 1.16]), ("lowerarm", [1.14, 1.0, 1.14]), ("thigh", [1.14, 1.0, 1.14]), ("calf", [1.1, 1.0, 1.1])],
    },
    Build {
        name: "Slim",
        scale: 1.03,
        parts: &[("spine_03", [0.9, 1.0, 0.9]), ("spine_02", [0.88, 1.0, 0.88]), ("spine_01", [0.88, 1.0, 0.88]), ("pelvis", [0.92, 1.0, 0.92]), ("upperarm", [0.88, 1.0, 0.88]), ("lowerarm", [0.9, 1.0, 0.9]), ("thigh", [0.9, 1.0, 0.9]), ("calf", [0.92, 1.0, 0.92])],
    },
    Build { name: "Belly", scale: 1.0, parts: &[("spine_01", [1.25, 1.0, 1.4]), ("spine_02", [1.2, 1.0, 1.3]), ("pelvis", [1.1, 1.0, 1.15]), ("thigh", [1.06, 1.0, 1.06])] },
    Build {
        name: "Strong",
        scale: 1.03,
        parts: &[("spine_03", [1.2, 1.04, 1.15]), ("clavicle", [1.15, 1.1, 1.15]), ("upperarm", [1.25, 1.0, 1.25]), ("lowerarm", [1.2, 1.0, 1.2]), ("hand", [1.1, 1.1, 1.1])],
    },
];

/// Where things are on the head, in the head frame (metres, at the head bone, +Z forward), measured on the
/// heads (`measure_heads`).
pub const MOUTH: Vec3 = Vec3::new(0.0, 0.0, 0.11);
/// The middle of the skull: hats sit on it (see `scene::hat_frame`), name tags above it.
pub const CRANIUM: Vec3 = Vec3::new(0.0, 0.12, 0.0);
pub const CRANIUM_R: f32 = 0.115;

#[cfg(test)]
mod tests {
    #[test]
    fn measure_heads() {
        let m = crate::model::tests::load();
        for (r, rig) in m.rigs.iter().enumerate() {
            let p = rig.pose(&[glam::Quat::IDENTITY; crate::model::NKEYS], glam::Vec3::new(0.0, rig.legs.stand, 0.0), glam::Mat4::IDENTITY, glam::Quat::IDENTITY, &super::BUILDS[0]);
            let hinv = p.head.inverse();
            for name in ["Casual_Head", "Casual_Hair", "Suit_Hair"] {
                let Some((f, n)) = m.part(r, name) else { continue };
                let (mut lo, mut hi) = (glam::Vec3::splat(9.0), glam::Vec3::splat(-9.0));
                for &i in &m.indices[f as usize..(f + n) as usize] {
                    let v = &m.verts[i as usize];
                    let mut w = glam::Vec3::ZERO;
                    for k in 0..4 {
                        w += p.joints[v.joints[k] as usize].transform_point3(glam::Vec3::from_array(v.pos)) * v.weights[k];
                    }
                    let h = hinv.transform_point3(w);
                    lo = lo.min(h);
                    hi = hi.max(h);
                }
                println!("rig {r} {name}: {lo:.3?} {hi:.3?}");
            }
        }
    }
}
