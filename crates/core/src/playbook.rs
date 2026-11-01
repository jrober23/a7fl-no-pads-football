//! Offensive formations and concepts, plus the defensive call sheet.
//!
//! A7FL offenses field seven players: a quarterback, two down linemen and four
//! eligibles. The published rules note that teams "can run bone or pistol", so both
//! families are here along with trips, doubles and spread looks. All route
//! waypoints are relative to the receiver's alignment: `+x` is downfield, `+y` is
//! toward the wide (right) side of the formation, and designs use the sign of their
//! alignment so concepts mirror automatically.

use crate::math::Vec2;

/// The seven offensive jobs, in the order the personnel array is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OffRole {
    Qb,
    Lg,
    Rg,
    Back,
    Slot,
    WideL,
    WideR,
}

impl OffRole {
    pub const ALL: [OffRole; 7] = [
        OffRole::Qb,
        OffRole::Lg,
        OffRole::Rg,
        OffRole::Back,
        OffRole::Slot,
        OffRole::WideL,
        OffRole::WideR,
    ];

    pub fn index(self) -> usize {
        match self {
            OffRole::Qb => 0,
            OffRole::Lg => 1,
            OffRole::Rg => 2,
            OffRole::Back => 3,
            OffRole::Slot => 4,
            OffRole::WideL => 5,
            OffRole::WideR => 6,
        }
    }

    pub fn is_eligible(self) -> bool {
        !matches!(self, OffRole::Lg | OffRole::Rg | OffRole::Qb)
    }

    pub fn label(self) -> &'static str {
        match self {
            OffRole::Qb => "QB",
            OffRole::Lg => "LG",
            OffRole::Rg => "RG",
            OffRole::Back => "RB",
            OffRole::Slot => "TE",
            OffRole::WideL => "WR",
            OffRole::WideR => "WR",
        }
    }
}

/// The seven defensive jobs, in the order the personnel array is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefRole {
    EdgeL,
    EdgeR,
    Mike,
    Will,
    CbL,
    CbR,
    Safety,
}

impl DefRole {
    pub const ALL: [DefRole; 7] = [
        DefRole::EdgeL,
        DefRole::EdgeR,
        DefRole::Mike,
        DefRole::Will,
        DefRole::CbL,
        DefRole::CbR,
        DefRole::Safety,
    ];

    pub fn index(self) -> usize {
        match self {
            DefRole::EdgeL => 0,
            DefRole::EdgeR => 1,
            DefRole::Mike => 2,
            DefRole::Will => 3,
            DefRole::CbL => 4,
            DefRole::CbR => 5,
            DefRole::Safety => 6,
        }
    }

    pub fn is_lineman(self) -> bool {
        matches!(self, DefRole::EdgeL | DefRole::EdgeR)
    }

    pub fn is_box(self) -> bool {
        matches!(self, DefRole::EdgeL | DefRole::EdgeR | DefRole::Mike | DefRole::Will)
    }

    pub fn label(self) -> &'static str {
        match self {
            DefRole::EdgeL => "EDGE",
            DefRole::EdgeR => "EDGE",
            DefRole::Mike => "MIKE",
            DefRole::Will => "WILL",
            DefRole::CbL => "CB",
            DefRole::CbR => "CB",
            DefRole::Safety => "SAF",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Formation {
    Pistol,
    Bone,
    Trips,
    Doubles,
    Spread,
    Empty,
}

impl Formation {
    pub fn label(self) -> &'static str {
        match self {
            Formation::Pistol => "PISTOL",
            Formation::Bone => "BONE",
            Formation::Trips => "TRIPS",
            Formation::Doubles => "DOUBLES",
            Formation::Spread => "SPREAD",
            Formation::Empty => "EMPTY",
        }
    }

    /// Alignment relative to the line of scrimmage: `x` negative is in the backfield.
    pub fn alignment(self, role: OffRole) -> Vec2 {
        use OffRole::*;
        match (self, role) {
            (Formation::Pistol, Qb) => Vec2::new(-3.2, 0.0),
            (Formation::Pistol, Back) => Vec2::new(-5.2, 0.7),
            (Formation::Pistol, Slot) => Vec2::new(-0.2, 3.0),
            (Formation::Pistol, WideL) => Vec2::new(0.0, -12.0),
            (Formation::Pistol, WideR) => Vec2::new(0.0, 12.5),

            (Formation::Bone, Qb) => Vec2::new(-2.2, 0.0),
            (Formation::Bone, Back) => Vec2::new(-4.6, -1.1),
            (Formation::Bone, Slot) => Vec2::new(-3.8, 1.4),
            (Formation::Bone, WideL) => Vec2::new(0.0, -11.5),
            (Formation::Bone, WideR) => Vec2::new(-0.6, 8.0),

            (Formation::Trips, Qb) => Vec2::new(-3.4, 0.0),
            (Formation::Trips, Back) => Vec2::new(-1.4, 2.0),
            (Formation::Trips, Slot) => Vec2::new(-0.4, 5.2),
            (Formation::Trips, WideL) => Vec2::new(0.0, -12.0),
            (Formation::Trips, WideR) => Vec2::new(0.0, 13.0),

            (Formation::Doubles, Qb) => Vec2::new(-3.0, 0.0),
            (Formation::Doubles, Back) => Vec2::new(-4.8, 1.2),
            (Formation::Doubles, Slot) => Vec2::new(-0.2, 3.4),
            (Formation::Doubles, WideL) => Vec2::new(0.0, -10.5),
            (Formation::Doubles, WideR) => Vec2::new(0.0, 11.0),

            (Formation::Spread, Qb) => Vec2::new(-3.6, 0.0),
            (Formation::Spread, Back) => Vec2::new(-2.0, 4.6),
            (Formation::Spread, Slot) => Vec2::new(0.0, -4.4),
            (Formation::Spread, WideL) => Vec2::new(0.0, -13.5),
            (Formation::Spread, WideR) => Vec2::new(0.0, 13.5),

            (Formation::Empty, Qb) => Vec2::new(-3.8, 0.0),
            (Formation::Empty, Back) => Vec2::new(0.0, -6.5),
            (Formation::Empty, Slot) => Vec2::new(0.0, 4.0),
            (Formation::Empty, WideL) => Vec2::new(0.0, -14.0),
            (Formation::Empty, WideR) => Vec2::new(0.0, 14.0),

            (_, Lg) => Vec2::new(-0.1, -0.62),
            (_, Rg) => Vec2::new(-0.1, 0.62),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayFamily {
    RunInside,
    RunOutside,
    QbRun,
    QuickPass,
    DropbackPass,
    PlayAction,
    Screen,
}

impl PlayFamily {
    pub fn is_run(self) -> bool {
        matches!(self, PlayFamily::RunInside | PlayFamily::RunOutside | PlayFamily::QbRun)
    }

    pub fn is_pass(self) -> bool {
        !self.is_run()
    }

    pub fn label(self) -> &'static str {
        match self {
            PlayFamily::RunInside => "RUN",
            PlayFamily::RunOutside => "RUN",
            PlayFamily::QbRun => "QB RUN",
            PlayFamily::QuickPass => "PASS",
            PlayFamily::DropbackPass => "PASS",
            PlayFamily::PlayAction => "PLAY ACTION",
            PlayFamily::Screen => "SCREEN",
        }
    }
}

/// What a blocker or receiver is asked to do on a play.
#[derive(Clone, Debug)]
pub enum Assign {
    /// Pass protection. `slide` shifts the pocket: -1 left, 0 set, +1 right.
    PassBlock { slide: f32, max_depth: f32 },
    /// Run blocking toward an aim point (relative to the LOS).
    RunBlock { aim: Vec2, seal: bool },
    /// A pass route; `depth` is the expected target depth in yards, used by the
    /// quarterback's progression and by the AI's play calling.
    Route { points: Vec<Vec2>, depth: u8 },
    /// The ball carrier's run path.
    Carry { points: Vec<Vec2> },
    /// The QB keeps it.
    QbCarry { points: Vec<Vec2> },
    /// Run a fake into the line, then release late.
    Fake { aim: Vec2, points: Vec<Vec2>, depth: u8 },
    /// Holds up on a screen, then blocks the first defender in the lane.
    ScreenBlock { aim: Vec2 },
}

impl Assign {
    pub fn route_depth(&self) -> Option<u8> {
        match self {
            Assign::Route { depth, .. } | Assign::Fake { depth, .. } => Some(*depth),
            _ => None,
        }
    }

    pub fn is_block(&self) -> bool {
        matches!(self, Assign::PassBlock { .. } | Assign::RunBlock { .. } | Assign::ScreenBlock { .. })
    }
}

#[derive(Clone, Debug)]
pub struct Play {
    pub name: &'static str,
    pub concept: &'static str,
    pub family: PlayFamily,
    pub formation: Formation,
    /// Depth of the quarterback's drop, in yards.
    pub drop: f32,
    pub run_side: i8,
    pub assignments: Vec<(OffRole, Assign)>,
}

impl Play {
    pub fn alignment(&self, role: OffRole) -> Vec2 {
        self.formation.alignment(role)
    }

    pub fn assignment(&self, role: OffRole) -> Option<&Assign> {
        self.assignments.iter().find(|(r, _)| *r == role).map(|(_, a)| a)
    }

    pub fn carrier_role(&self) -> Option<OffRole> {
        self.assignments.iter().find_map(|(r, a)| match a {
            Assign::Carry { .. } | Assign::QbCarry { .. } => Some(*r),
            _ => None,
        })
    }

    /// Deepest route on the play, used by the AI and the broadcast overlay.
    pub fn max_route_depth(&self) -> u8 {
        self.assignments
            .iter()
            .filter_map(|(_, a)| a.route_depth())
            .max()
            .unwrap_or(0)
    }

    pub fn short_name(&self) -> &'static str {
        self.name
    }
}

// ---------------------------------------------------------------------------
// Building blocks for play design
// ---------------------------------------------------------------------------

fn route(points: &[(f32, f32)], depth: u8) -> Assign {
    Assign::Route {
        points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
        depth,
    }
}

fn pass_pro(slide: f32) -> Assign {
    Assign::PassBlock { slide, max_depth: -1.0 }
}

fn run_block(aim_x: f32, aim_y: f32, seal: bool) -> Assign {
    Assign::RunBlock { aim: Vec2::new(aim_x, aim_y), seal }
}

fn carry(points: &[(f32, f32)]) -> Assign {
    Assign::Carry { points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect() }
}

fn qb_carry(points: &[(f32, f32)]) -> Assign {
    Assign::QbCarry { points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect() }
}

fn screen_block(x: f32, y: f32) -> Assign {
    Assign::ScreenBlock { aim: Vec2::new(x, y) }
}

fn fake(aim: Vec2, points: &[(f32, f32)], depth: u8) -> Assign {
    Assign::Fake {
        aim,
        points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
        depth,
    }
}

#[allow(clippy::too_many_arguments)]
fn play(
    name: &'static str,
    concept: &'static str,
    family: PlayFamily,
    formation: Formation,
    drop: f32,
    run_side: i8,
    assignments: &[(OffRole, Assign)],
) -> Play {
    Play {
        name,
        concept,
        family,
        formation,
        drop,
        run_side,
        assignments: assignments.to_vec(),
    }
}

/// The full A7FL playbook. `l` in the mirrored designs is the role's side sign.
pub fn playbook() -> Vec<Play> {
    use OffRole::*;
    vec![
        // ---------------------------- runs ---------------------------------
        play(
            "ZONE READ",
            "Inside zone",
            PlayFamily::RunInside,
            Formation::Pistol,
            0.0,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: 1.0 }),
                (Lg, run_block(2.0, -1.0, false)),
                (Rg, run_block(2.0, 1.4, false)),
                (Back, carry(&[(-1.0, 0.4), (2.5, 0.8), (6.0, 0.2), (14.0, 0.0)])),
                (Slot, run_block(2.5, 3.5, true)),
                (WideL, run_block(1.0, -6.0, true)),
                (WideR, run_block(1.0, 9.0, true)),
            ],
        ),
        play(
            "POWER O",
            "Power gap",
            PlayFamily::RunInside,
            Formation::Bone,
            0.0,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: 1.0 }),
                (Lg, run_block(1.5, 0.0, false)),
                (Rg, run_block(3.0, 2.6, false)),
                (Back, carry(&[(-1.5, 0.0), (2.0, 2.2), (9.0, 3.0), (16.0, 2.5)])),
                (Slot, run_block(3.5, 3.2, false)),
                (WideL, run_block(1.0, -5.0, true)),
                (WideR, run_block(2.0, 8.0, true)),
            ],
        ),
        play(
            "COUNTER TREY",
            "Counter",
            PlayFamily::RunInside,
            Formation::Bone,
            0.0,
            -1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: 1.0 }),
                (Lg, run_block(2.5, 2.6, false)),
                (Rg, run_block(3.0, 3.4, false)),
                (Back, carry(&[(-3.0, 1.6), (-3.5, -1.2), (1.0, -3.0), (7.0, -2.0), (15.0, -1.0)])),
                (Slot, run_block(4.0, 4.0, true)),
                (WideL, run_block(1.0, -6.5, true)),
                (WideR, run_block(3.0, 7.5, true)),
            ],
        ),
        play(
            "ROCKET SWEEP",
            "Outside zone",
            PlayFamily::RunOutside,
            Formation::Pistol,
            0.0,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: 1.0 }),
                (Lg, run_block(1.0, 1.0, false)),
                (Rg, run_block(1.5, 3.0, false)),
                (Back, carry(&[(-1.0, 0.6), (0.0, 4.5), (4.0, 6.5), (14.0, 6.0)])),
                (Slot, run_block(2.0, 5.5, true)),
                (WideL, run_block(0.0, -6.0, true)),
                (WideR, run_block(3.0, 8.0, true)),
            ],
        ),
        play(
            "JET FLY",
            "Jet sweep",
            PlayFamily::RunOutside,
            Formation::Trips,
            0.0,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: 1.0 }),
                (Lg, run_block(1.0, 0.0, false)),
                (Rg, run_block(1.0, 2.5, false)),
                (Back, run_block(2.0, 2.0, true)),
                (Slot, carry(&[(-1.0, 0.0), (1.0, 5.5), (5.0, 7.0), (16.0, 6.0)])),
                (WideL, run_block(3.0, -4.0, true)),
                (WideR, run_block(3.0, 6.0, true)),
            ],
        ),
        play(
            "QB DRAW",
            "Draw",
            PlayFamily::QbRun,
            Formation::Spread,
            0.0,
            0,
            &[
                (Qb, qb_carry(&[(-1.5, 0.0), (1.5, 0.0), (8.0, 0.6), (16.0, 0.0)])),
                (Lg, run_block(3.0, -1.4, false)),
                (Rg, run_block(3.0, 1.4, false)),
                (Back, run_block(4.0, 4.0, true)),
                (Slot, run_block(3.0, -4.0, true)),
                (WideL, route(&[(10.0, -1.0), (18.0, -1.5)], 12)),
                (WideR, route(&[(12.0, 1.0), (20.0, 2.0)], 14)),
            ],
        ),
        play(
            "QB KEEPER",
            "Keep right",
            PlayFamily::QbRun,
            Formation::Bone,
            0.0,
            1,
            &[
                (Qb, qb_carry(&[(-1.0, 0.0), (1.0, 3.0), (5.0, 6.0), (13.0, 7.5)])),
                (Lg, run_block(2.0, 0.5, false)),
                (Rg, run_block(3.0, 3.0, false)),
                (Back, fake(Vec2::new(2.0, -1.0), &[(0.0, 0.0)], 0)),
                (Slot, run_block(4.0, 5.0, true)),
                (WideL, route(&[(8.0, -2.0)], 8)),
                (WideR, run_block(4.0, 9.0, true)),
            ],
        ),
        play(
            "TRAP",
            "Trap",
            PlayFamily::RunInside,
            Formation::Bone,
            0.0,
            -1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: 1.0 }),
                (Lg, run_block(2.0, 1.5, false)),
                (Rg, run_block(2.0, -2.0, false)),
                (Back, carry(&[(-1.0, -0.5), (2.0, -1.5), (8.0, -2.0), (15.0, -1.0)])),
                (Slot, run_block(3.0, -3.0, false)),
                (WideL, run_block(2.0, -7.0, true)),
                (WideR, route(&[(6.0, 0.0), (12.0, 1.0)], 10)),
            ],
        ),

        // ---------------------------- quick passes -------------------------
        play(
            "SLANT FLAT",
            "Slant / flat",
            PlayFamily::QuickPass,
            Formation::Pistol,
            1.2,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, route(&[(-0.5, 2.5), (1.5, 5.5)], 3)),
                (Slot, route(&[(4.0, -3.5)], 6)),
                (WideL, route(&[(3.0, -3.0)], 5)),
                (WideR, route(&[(5.0, -3.0)], 6)),
            ],
        ),
        play(
            "CURL FLAT",
            "Curl / flat",
            PlayFamily::QuickPass,
            Formation::Pistol,
            1.5,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: -1.0, max_depth: -1.0 }),
                (Slot, route(&[(9.0, 1.0), (7.5, -1.0)], 9)),
                (WideL, route(&[(4.5, 7.0)], 5)),
                (WideR, route(&[(11.0, 1.0), (9.5, -1.5)], 11)),
            ],
        ),
        play(
            "SMASH",
            "Smash",
            PlayFamily::DropbackPass,
            Formation::Doubles,
            2.5,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: 1.0, max_depth: -1.0 }),
                (Slot, route(&[(7.0, 0.0), (5.5, -1.0)], 7)),
                (WideL, route(&[(9.0, 7.0)], 12)),
                (WideR, route(&[(7.0, -1.0), (6.0, -2.5)], 8)),
            ],
        ),
        play(
            "LEVELS",
            "Levels",
            PlayFamily::DropbackPass,
            Formation::Trips,
            2.0,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, route(&[(3.0, -14.0), (6.0, -6.0)], 6)),
                (Slot, route(&[(6.0, -3.5), (10.0, -2.0)], 10)),
                (WideL, route(&[(12.0, -1.0), (14.0, 0.0)], 14)),
                (WideR, route(&[(3.0, 4.0)], 4)),
            ],
        ),
        play(
            "MESH",
            "Mesh",
            PlayFamily::DropbackPass,
            Formation::Trips,
            2.5,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: 1.0, max_depth: -1.0 }),
                (Slot, route(&[(5.0, -12.0), (7.0, -20.0)], 8)),
                (WideL, route(&[(5.0, 14.0), (7.0, 21.0)], 8)),
                (WideR, route(&[(13.0, 0.0), (16.0, -1.0)], 16)),
            ],
        ),
        play(
            "FOUR VERTS",
            "Four verticals",
            PlayFamily::DropbackPass,
            Formation::Spread,
            3.0,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, route(&[(14.0, 1.5), (26.0, 2.0)], 24)),
                (Slot, route(&[(15.0, -2.0), (28.0, -2.5)], 26)),
                (WideL, route(&[(16.0, -3.0), (30.0, -3.0)], 28)),
                (WideR, route(&[(16.0, 3.0), (30.0, 3.0)], 28)),
            ],
        ),
        play(
            "POST WHEEL",
            "Post / wheel",
            PlayFamily::DropbackPass,
            Formation::Doubles,
            2.5,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, route(&[(1.0, 4.0), (12.0, 7.5)], 14)),
                (Slot, route(&[(10.0, -1.0), (24.0, -6.0)], 22)),
                (WideL, route(&[(10.0, 1.0), (22.0, 6.0)], 20)),
                (WideR, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
            ],
        ),
        play(
            "YANKEE",
            "Deep comeback",
            PlayFamily::DropbackPass,
            Formation::Doubles,
            3.0,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Slot, route(&[(8.0, -4.0), (12.0, -8.0)], 12)),
                (WideL, route(&[(18.0, 2.0), (16.0, 4.0)], 18)),
                (WideR, route(&[(20.0, 1.0), (18.0, 2.0)], 20)),
            ],
        ),
        play(
            "FLOOD",
            "Flood",
            PlayFamily::PlayAction,
            Formation::Trips,
            2.0,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, fake(Vec2::new(2.0, 1.0), &[(4.0, 6.0), (8.0, 7.0)], 6)),
                (Slot, route(&[(3.0, 8.0)], 4)),
                (WideL, route(&[(14.0, -2.0), (24.0, -4.0)], 22)),
                (WideR, route(&[(6.0, 9.0), (16.0, 10.0)], 14)),
            ],
        ),
        play(
            "PA BOOT",
            "Play action boot",
            PlayFamily::PlayAction,
            Formation::Bone,
            1.5,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, fake(Vec2::new(2.0, -2.0), &[(1.0, 16.0), (16.0, 20.0)], 12)),
                (Slot, route(&[(8.0, 3.0), (16.0, -2.0)], 14)),
                (WideL, route(&[(4.0, -9.0)], 5)),
                (WideR, route(&[(18.0, 12.0)], 18)),
            ],
        ),
        play(
            "RB SCREEN",
            "Screen",
            PlayFamily::Screen,
            Formation::Pistol,
            2.0,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, route(&[(-3.5, 3.5)], 0)),
                (Slot, screen_block(1.0, 6.0)),
                (WideL, route(&[(12.0, -6.0)], 12)),
                (WideR, screen_block(1.0, 7.0)),
            ],
        ),
        play(
            "WR SCREEN",
            "Receiver screen",
            PlayFamily::Screen,
            Formation::Trips,
            0.5,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, screen_block(0.5, 5.5)),
                (Slot, screen_block(1.0, 8.0)),
                (WideL, route(&[(16.0, -3.0)], 16)),
                (WideR, route(&[(-1.0, 1.0)], 0)),
            ],
        ),
        play(
            "GOAL FADE",
            "Fade",
            PlayFamily::DropbackPass,
            Formation::Doubles,
            2.0,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Slot, route(&[(7.0, 3.0), (12.0, 4.5)], 12)),
                (WideL, route(&[(2.0, -5.0), (12.0, -7.0)], 12)),
                (WideR, route(&[(1.0, 4.0), (10.0, 6.0)], 10)),
            ],
        ),
        play(
            "DAGGER",
            "Dagger",
            PlayFamily::DropbackPass,
            Formation::Trips,
            2.8,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: 1.0, max_depth: -1.0 }),
                (Slot, route(&[(10.0, -2.0), (13.0, -3.0)], 13)),
                (WideL, route(&[(14.0, -2.0), (26.0, -1.0)], 24)),
                (WideR, route(&[(5.0, 6.0)], 6)),
            ],
        ),
        play(
            "STICK",
            "Stick",
            PlayFamily::QuickPass,
            Formation::Doubles,
            1.2,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: -1.0, max_depth: -1.0 }),
                (Slot, route(&[(5.0, 2.0)], 5)),
                (WideL, route(&[(3.0, 6.0)], 3)),
                (WideR, route(&[(6.0, 1.0), (5.0, -0.5)], 6)),
            ],
        ),
        play(
            "DEEP SHOT",
            "Double go",
            PlayFamily::DropbackPass,
            Formation::Bone,
            3.5,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Slot, route(&[(18.0, 4.0), (30.0, 6.0)], 28)),
                (WideL, route(&[(20.0, -2.0), (32.0, -2.0)], 30)),
                (WideR, route(&[(19.0, 3.0), (31.0, 4.0)], 30)),
            ],
        ),
    ]
}

// ---------------------------------------------------------------------------
// Defense
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// Man across the board with one deep help defender.
    ManFree,
    /// Man with press technique at the line.
    ManPress,
    /// Two deep safeties, three underneath.
    Cover2,
    /// Three deep, two underneath.
    Cover3,
    /// Four across the deep thirds/quarters, soft.
    Cover4,
    /// Bear front, everybody tight, stop the run.
    GoalLine,
}

impl Coverage {
    pub fn label(self) -> &'static str {
        match self {
            Coverage::ManFree => "COVER 1",
            Coverage::ManPress => "PRESS MAN",
            Coverage::Cover2 => "COVER 2",
            Coverage::Cover3 => "COVER 3",
            Coverage::Cover4 => "COVER 4",
            Coverage::GoalLine => "GOAL LINE",
        }
    }

    pub fn deep_defenders(self) -> usize {
        match self {
            Coverage::ManFree | Coverage::ManPress => 1,
            Coverage::Cover2 => 2,
            Coverage::Cover3 => 3,
            Coverage::Cover4 => 4,
            Coverage::GoalLine => 0,
        }
    }

    pub fn is_man(self) -> bool {
        matches!(self, Coverage::ManFree | Coverage::ManPress)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunFit {
    Aggressive,
    Balanced,
    Conservative,
}

#[derive(Clone, Debug)]
pub struct DefCall {
    pub name: &'static str,
    pub coverage: Coverage,
    pub run_fit: RunFit,
    /// Defenders who rush the passer beyond the two edge players.
    pub blitzers: Vec<DefRole>,
    /// Extra rushers means fewer defenders in coverage; 7 total per side.
    pub press: bool,
}

impl DefCall {
    pub fn rushers(&self) -> usize {
        2 + self.blitzers.len()
    }
}

pub fn defensive_playbook() -> Vec<DefCall> {
    vec![
        DefCall {
            name: "COVER 1 MAN",
            coverage: Coverage::ManFree,
            run_fit: RunFit::Balanced,
            blitzers: vec![],
            press: false,
        },
        DefCall {
            name: "PRESS BAIL",
            coverage: Coverage::ManPress,
            run_fit: RunFit::Aggressive,
            blitzers: vec![],
            press: true,
        },
        DefCall {
            name: "COVER 2 ZONE",
            coverage: Coverage::Cover2,
            run_fit: RunFit::Balanced,
            blitzers: vec![],
            press: false,
        },
        DefCall {
            name: "COVER 3 ZONE",
            coverage: Coverage::Cover3,
            run_fit: RunFit::Conservative,
            blitzers: vec![],
            press: false,
        },
        DefCall {
            name: "COVER 4 ZONE",
            coverage: Coverage::Cover4,
            run_fit: RunFit::Conservative,
            blitzers: vec![],
            press: false,
        },
        DefCall {
            name: "MIKE BLITZ",
            coverage: Coverage::ManFree,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Mike],
            press: false,
        },
        DefCall {
            name: "WILL BLITZ",
            coverage: Coverage::ManFree,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Will],
            press: false,
        },
        DefCall {
            name: "ALL OUT BLITZ",
            coverage: Coverage::ManFree,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Mike, DefRole::Will],
            press: false,
        },
        DefCall {
            name: "GOAL LINE BEAR",
            coverage: Coverage::GoalLine,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Mike],
            press: true,
        },
    ]
}

/// Defensive alignment relative to the line of scrimmage. `los` is the canonical
/// spot and `direction` the offense's attack direction.
pub fn defensive_alignment(
    role: DefRole,
    coverage: Coverage,
    los: f32,
    direction: i8,
    play_dir: i8,
) -> Vec2 {
    // Work in "offense forward" space then convert.
    let fwd = direction as f32;
    let to_field = |along: f32, lateral: f32| {
        Vec2::new(los + fwd * along, lateral + play_dir as f32 * 0.0)
    };
    match role {
        DefRole::EdgeL => to_field(0.9, -1.7),
        DefRole::EdgeR => to_field(0.9, 1.7),
        DefRole::Mike => to_field(4.5, -0.4),
        DefRole::Will => to_field(5.0, 2.4),
        DefRole::CbL => to_field(match coverage {
            Coverage::ManPress => 1.0,
            _ => 6.5,
        }, -10.5),
        DefRole::CbR => to_field(match coverage {
            Coverage::ManPress => 1.0,
            _ => 6.5,
        }, 10.5),
        DefRole::Safety => to_field(
            match coverage {
                Coverage::Cover2 => 16.0,
                Coverage::Cover3 => 15.0,
                Coverage::Cover4 => 18.0,
                Coverage::GoalLine => 8.0,
                _ => 17.0,
            },
            0.0,
        ),
    }
}

/// Position a defense lines up at, used by the renderer for pre-snap shots.
pub fn defender_positions(call: &DefCall, los: f32, direction: i8) -> Vec<(DefRole, Vec2)> {
    DefRole::ALL
        .iter()
        .map(|&r| (r, defensive_alignment(r, call.coverage, los, direction, 0)))
        .collect()
}

/// Cheap description of an offensive personnel grouping for the play-call screen.
pub fn personnel_label(formation: Formation) -> &'static str {
    match formation {
        Formation::Pistol => "1 RB, 1 TE, 2 WR",
        Formation::Bone => "2 RB, 1 TE, 1 WR",
        Formation::Trips => "1 RB, 1 TE, 2 WR (trips)",
        Formation::Doubles => "1 RB, 1 TE, 2 WR (doubles)",
        Formation::Spread => "1 RB, 3 WR",
        Formation::Empty => "4 WR",
    }
}

/// Which receiver a route's `l` mirroring should treat as being on the left.
pub fn route_side(role: OffRole) -> i8 {
    match role {
        OffRole::WideL | OffRole::Slot => -1,
        OffRole::Back | OffRole::WideR => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_play_has_seven_assignments_and_one_carrier() {
        let book = playbook();
        assert!(book.len() >= 20, "playbook too thin: {}", book.len());
        for p in &book {
            assert_eq!(p.assignments.len(), 7, "{} has {} assignments", p.name, p.assignments.len());
            for role in OffRole::ALL {
                assert!(p.assignment(role).is_some(), "{} missing {}", p.name, role.label());
            }
            if p.family.is_run() {
                assert!(p.carrier_role().is_some(), "{} has no ball carrier", p.name);
            } else {
                let routes = p
                    .assignments
                    .iter()
                    .filter(|(_, a)| a.route_depth().is_some())
                    .count();
                assert!(routes >= 2, "{} has no real routes", p.name);
            }
        }
    }

    #[test]
    fn formations_are_legal_sets() {
        for f in [
            Formation::Pistol,
            Formation::Bone,
            Formation::Trips,
            Formation::Doubles,
            Formation::Spread,
            Formation::Empty,
        ] {
            let lg = f.alignment(OffRole::Lg);
            let rg = f.alignment(OffRole::Rg);
            assert!((lg.y - rg.y).abs() > 1.0, "{f:?} linemen too close");
            assert_eq!((lg.x, rg.x), (-0.1, -0.1), "{f:?} linemen off the ball");
            let qb = f.alignment(OffRole::Qb);
            assert!(qb.x <= -2.0 && qb.x >= -5.0, "{f:?} QB depth illegal");
            assert!(qb.y.abs() < 1.0, "{f:?} QB not between the guards");
            // Every receiver has to line up inside the 37.5 yard width.
            for role in [OffRole::WideL, OffRole::WideR, OffRole::Slot, OffRole::Back] {
                let p = f.alignment(role);
                assert!(p.y.abs() < 18.75, "{f:?} {} off the field", role.label());
            }
        }
    }

    #[test]
    fn defenses_field_seven_with_a_legal_rush_count() {
        for d in defensive_playbook() {
            assert!(d.rushers() >= 2 && d.rushers() <= 4, "{} rush count", d.name);
            assert!(d.coverage.deep_defenders() <= 7 - d.rushers() + 2);
        }
    }
}
