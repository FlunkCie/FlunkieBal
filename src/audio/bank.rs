//! What can be heard: every sound in the game, and how it's rendered (`sfx`, `amb`).

use super::dsp::{Clip, Rng};
use super::{amb, sfx};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cue {
    // ball, bottle, beers
    Throw,
    BounceGrass,
    BounceHard,
    BounceWood,
    BallBottle,
    BottleGrass,
    BottleHard,
    BottleSet,
    CanKnock,
    // the referee
    WhistleTwo,
    WhistleLong,
    WhistleFoul,
    // people
    Gulp,
    Cough,
    StepGrass,
    StepHard,
    StepWood,
    Cheer,
    Aww,
    Ooh,
    Clap,
    Claps,
    Crowd,
    Lighter,
    CanOpen,
    Slap,
    // little tunes
    HitSting,
    MissSting,
    WinSting,
    LoseSting,
    Spin,
    // the menus
    Tap,
    TapBig,
    Tick,
    Back,
    ToggleOn,
    ToggleOff,
    SwishUp,
    SwishDown,
    Chime,
    Pop,
    LevelUp,
    Denied,
    // now and then, round about
    Sparrow,
    Blackbird,
    Tit,
    Pigeon,
    BikeBell,
    Gull,
    CarPass,
    Train,
    Clink,
    Cutlery,
    Creak,
    Whoo,
    // always there (loops)
    Leaves,
    City,
    Wind,
    Room,
    Fridge,
    Vent,
    Traffic,
    Crickets,
    Babble,
    Music,
}
use Cue::*;

pub const ALL: [Cue; 65] = [
    Throw, BounceGrass, BounceHard, BounceWood, BallBottle, BottleGrass, BottleHard, BottleSet, CanKnock, WhistleTwo, WhistleLong, WhistleFoul, Gulp, Cough, StepGrass, StepHard, StepWood, Cheer, Aww, Ooh, Clap, Claps, Crowd, Lighter, CanOpen, Slap, HitSting, MissSting, WinSting, LoseSting, Spin, Tap, TapBig, Tick, Back, ToggleOn, ToggleOff, SwishUp, SwishDown, Chime, Pop, LevelUp, Denied, Sparrow, Blackbird, Tit, Pigeon, BikeBell, Gull, CarPass, Train, Clink, Cutlery, Creak, Whoo, Leaves, City, Wind, Room, Fridge, Vent, Traffic, Crickets, Babble, Music,
];
pub const MAX_VARS: usize = 8;

impl Cue {
    /// How many takes of it there are: played in turn at random, never the same twice running.
    pub fn vars(self) -> u32 {
        match self {
            StepGrass | StepHard | StepWood | Clap => 8,
            Gulp | Tap | Tick => 6,
            BounceGrass | BounceHard | BounceWood | BallBottle | Throw | Sparrow | Blackbird | Clink => 4,
            BottleGrass | BottleHard | CanKnock | Cough | Cheer | Aww | Ooh | Tit | Cutlery | Whoo | Creak => 3,
            HitSting | Pigeon | BikeBell | Gull | CarPass | Slap | BottleSet => 2,
            _ => 1,
        }
    }
    /// Plays round and round.
    pub fn is_loop(self) -> bool {
        matches!(self, Leaves | City | Wind | Room | Fridge | Vent | Traffic | Crickets | Babble | Music)
    }
    pub fn is_ui(self) -> bool {
        matches!(self, Tap | TapBig | Tick | Back | ToggleOn | ToggleOff | SwishUp | SwishDown | Chime | Pop | LevelUp | Denied)
    }
    /// Rendered in several goes (a few ms each), so no single frame waits long for it.
    pub fn steps(self) -> u32 {
        match self {
            Crowd => 16,
            Babble => 10,
            _ => 1,
        }
    }
}

/// One sound being rendered, step by step.
pub struct Job {
    pub cue: Cue,
    pub var: u32,
    step: u32,
    clip: Clip,
}
impl Job {
    pub fn new(cue: Cue, var: u32) -> Job {
        Job { cue, var, step: 0, clip: Clip::default() }
    }
    /// Do the next part; the clip once it's done.
    pub fn step(&mut self) -> Option<Clip> {
        let r = &mut Rng::new(self.cue as u32 * 7919 + self.var * 104729 + self.step * 31);
        let (cue, var, k) = (self.cue, self.var, self.step);
        let c = &mut self.clip;
        let v = var as usize;
        match cue {
            Crowd => sfx::crowd(c, r, k),
            Babble => amb::babble(c, r, k),
            _ => {
                *c = match cue {
                    Throw => sfx::throw(r),
                    BounceGrass => sfx::bounce(r, 0),
                    BounceHard => sfx::bounce(r, 1),
                    BounceWood => sfx::bounce(r, 2),
                    BallBottle => sfx::ball_bottle(r),
                    BottleGrass => sfx::bottle_fall(r, false),
                    BottleHard => sfx::bottle_fall(r, true),
                    BottleSet => sfx::bottle_set(r),
                    CanKnock => sfx::can_knock(r),
                    WhistleTwo => sfx::whistle(r, &[(0.0, 0.16), (0.25, 0.5)], 1.0),
                    WhistleLong => sfx::whistle(r, &[(0.0, 0.95)], 0.9),
                    WhistleFoul => sfx::whistle(r, &[(0.0, 0.11), (0.17, 0.28), (0.36, 1.15)], 1.3),
                    Gulp => sfx::gulp(r),
                    Cough => sfx::cough(r, v),
                    StepGrass => sfx::step(r, 0),
                    StepHard => sfx::step(r, 1),
                    StepWood => sfx::step(r, 2),
                    Cheer => sfx::shout(r, 0),
                    Aww => sfx::shout(r, 1),
                    Ooh => sfx::shout(r, 2),
                    Whoo => sfx::shout(r, 3),
                    Clap => sfx::clap(r),
                    Claps => sfx::claps(r),
                    Lighter => sfx::lighter(r),
                    CanOpen => sfx::can_open(r),
                    Slap => sfx::slap(r),
                    HitSting => sfx::hit_sting(r, v),
                    MissSting => sfx::miss_sting(r),
                    WinSting => sfx::win_sting(r),
                    LoseSting => sfx::lose_sting(r),
                    Spin => sfx::spin(r),
                    Tap => sfx::tap(r, 1.0),
                    TapBig => sfx::tap_big(r),
                    Tick => sfx::tick(r),
                    Back => sfx::tap(r, 0.68),
                    ToggleOn => sfx::toggle(r, true),
                    ToggleOff => sfx::toggle(r, false),
                    SwishUp => sfx::swish(r, true),
                    SwishDown => sfx::swish(r, false),
                    Chime => sfx::chime(r),
                    Pop => sfx::pop(r),
                    LevelUp => sfx::level_up(r),
                    Denied => sfx::denied(r),
                    Sparrow => amb::sparrow(r),
                    Blackbird => amb::blackbird(r),
                    Tit => amb::tit(r),
                    Pigeon => amb::pigeon(r),
                    BikeBell => amb::bike_bell(r),
                    Gull => amb::gull(r),
                    CarPass => amb::car_pass(r, v),
                    Train => amb::train(r),
                    Clink => amb::clink(r),
                    Cutlery => amb::cutlery(r),
                    Creak => amb::creak(r),
                    Leaves => amb::leaves(r),
                    City => amb::city(r),
                    Wind => amb::wind(r),
                    Room => amb::room(r),
                    Fridge => amb::fridge(r),
                    Vent => amb::vent(r),
                    Traffic => amb::traffic(r),
                    Crickets => amb::crickets(r),
                    Music => amb::music(r),
                    Crowd | Babble => unreachable!(),
                };
            }
        }
        self.step += 1;
        (self.step >= cue.steps()).then(|| std::mem::take(&mut self.clip))
    }
}
