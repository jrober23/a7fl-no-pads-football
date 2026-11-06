//! Offensive formations and concepts, plus the defensive call sheet.
//!
//! 7 on 7 offenses field seven players: a quarterback, two down linemen and four
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

    /// The two men on the line. A flip runs the skill players to the other
    /// side; the guards stay where they are, because the side of the ball they
    /// are on is not what is being changed.
    pub fn is_line(self) -> bool {
        matches!(self, OffRole::Lg | OffRole::Rg)
    }

    /// True when a flip leaves him standing where he is. The guards are on the
    /// side of the ball they are on, and the two wide men already have a
    /// sideline each: sending them across the field was a man running eighty
    /// yards to do the job the man opposite him could do from where he stood.
    pub fn keeps_side(self) -> bool {
        matches!(self, OffRole::Lg | OffRole::Rg | OffRole::WideL | OffRole::WideR)
    }

    /// The man on the other side of the ball, for the jobs that change hands.
    pub fn opposite_side(self) -> Option<OffRole> {
        match self {
            OffRole::WideL => Some(OffRole::WideR),
            OffRole::WideR => Some(OffRole::WideL),
            _ => None,
        }
    }

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
    /// A formation the player drew: the number names the design in the book.
    Custom(u8),
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
            Formation::Custom(id) => crate::design::formation_name(id),
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

            (Formation::Custom(id), role) => crate::design::formation_spot(id, role),

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
    /// True when the design runs to the other side of the formation than it was
    /// drawn: what the audible's flip toggle sets.
    pub flip: bool,
    pub assignments: Vec<(OffRole, Assign)>,
}

impl Play {
    pub fn alignment(&self, role: OffRole) -> Vec2 {
        let a = self.formation.alignment(role);
        if self.flip && !role.keeps_side() {
            Vec2::new(a.x, -a.y)
        } else {
            a
        }
    }

    /// The same design run to the other side.
    ///
    /// Mirroring the waypoints is what keeps a corner route a corner route
    /// instead of quietly turning it into a post, and the two wide receivers
    /// take each other's mirrored job rather than changing sides: the design
    /// moves, the men do not. Everything that
    /// reads a play -- the diagrams, the routes, the blocking aims -- reads the
    /// flipped copy, so a flip can never advertise one thing and run another.
    pub fn flipped(&self) -> Play {
        let mut out = self.clone();
        out.flip = !self.flip;
        out.run_side = -self.run_side;
        out.assignments = self
            .assignments
            .iter()
            // The men on the line keep the job they were given. Their block is
            // not on one side of the ball or the other, so a flip that mirrored
            // it would be moving a guard for no reason.
            .map(|(role, a)| {
                if role.is_line() {
                    return (*role, a.clone());
                }
                // The two wide men do not run across the formation to reach the
                // other side of it: they keep the ground they are standing on
                // and take each other's job, mirrored.
                match role.opposite_side().and_then(|o| self.assignment(o)) {
                    Some(other) => (*role, mirror_assign(other)),
                    None => (*role, mirror_assign(a)),
                }
            })
            .collect();
        out
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
        flip: false,
        assignments: assignments.to_vec(),
    }
}

/// Mirror an assignment across the middle of the field: `y` is the across-field
/// axis, so negating it is the whole of a left-to-right flip.
fn mirror_assign(a: &Assign) -> Assign {
    let m = |p: &Vec2| Vec2::new(p.x, -p.y);
    match a {
        Assign::Route { points, depth } => Assign::Route {
            points: points.iter().map(m).collect(),
            depth: *depth,
        },
        Assign::Fake { aim, points, depth } => Assign::Fake {
            aim: m(aim),
            points: points.iter().map(m).collect(),
            depth: *depth,
        },
        Assign::Carry { points } => Assign::Carry { points: points.iter().map(m).collect() },
        Assign::QbCarry { points } => Assign::QbCarry { points: points.iter().map(m).collect() },
        Assign::RunBlock { aim, seal } => Assign::RunBlock { aim: m(aim), seal: *seal },
        Assign::ScreenBlock { aim } => Assign::ScreenBlock { aim: m(aim) },
        Assign::PassBlock { slide, max_depth } => Assign::PassBlock {
            slide: -*slide,
            max_depth: *max_depth,
        },
    }
}

/// The formations a book uses, in the order they first appear: the first layer
/// of the call sheet.
pub fn formations_in(book: &[Play]) -> Vec<Formation> {
    let mut out: Vec<Formation> = Vec::new();
    for p in book {
        if !out.contains(&p.formation) {
            out.push(p.formation);
        }
    }
    out
}

/// The plays a formation can run, by index into the book: the second layer of
/// the call sheet, and the set an audible is allowed to choose from.
pub fn plays_in(book: &[Play], formation: Formation) -> Vec<usize> {
    book.iter()
        .enumerate()
        .filter(|(_, p)| p.formation == formation)
        .map(|(i, _)| i)
        .collect()
}

/// The full 7 on 7 playbook. `l` in the mirrored designs is the role's side sign.
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

        // ---------------------- the scheme designs -------------------------
        // Each club's book is built out of these and the designs above: the
        // shared ones every club has, and the ones that are the club's own.
        play(
            "DIG DRIVE",
            "Dig / drive",
            PlayFamily::DropbackPass,
            Formation::Trips,
            2.6,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: 1.0, max_depth: -1.0 }),
                (Slot, route(&[(9.0, -3.0), (15.0, -7.0)], 15)),
                (WideL, route(&[(7.0, 9.0), (13.0, 15.0)], 12)),
                (WideR, route(&[(4.0, 3.0)], 4)),
            ],
        ),
        play(
            "SPOT",
            "Spot / hitch",
            PlayFamily::QuickPass,
            Formation::Doubles,
            1.2,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: -1.0, max_depth: -1.0 }),
                (Slot, route(&[(5.0, 1.5), (5.5, -1.0)], 5)),
                (WideL, route(&[(3.5, 6.0)], 3)),
                (WideR, route(&[(3.5, -6.0)], 3)),
            ],
        ),
        play(
            "Y CROSS",
            "Crosser",
            PlayFamily::DropbackPass,
            Formation::Trips,
            3.0,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, route(&[(3.0, 2.0)], 3)),
                (Slot, route(&[(12.0, -10.0), (17.0, 4.0)], 16)),
                (WideL, route(&[(20.0, -2.0), (32.0, -3.0)], 28)),
                (WideR, route(&[(10.0, 0.0), (9.0, -1.0)], 10)),
            ],
        ),
        play(
            "TUNNEL SCREEN",
            "Tunnel screen",
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
                (WideL, route(&[(18.0, -3.0)], 18)),
                (WideR, route(&[(-1.5, 1.0), (6.0, 2.0)], 0)),
            ],
        ),
        play(
            "PA BOMB",
            "Play action deep",
            PlayFamily::PlayAction,
            Formation::Doubles,
            2.0,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, fake(Vec2::new(2.0, 1.0), &[(20.0, 8.0), (34.0, 10.0)], 30)),
                (Slot, route(&[(20.0, -1.0), (34.0, -2.0)], 32)),
                (WideL, route(&[(16.0, 4.0), (30.0, 5.0)], 28)),
                (WideR, route(&[(6.0, 5.0)], 6)),
            ],
        ),
        play(
            "DOUBLE POST",
            "Double post",
            PlayFamily::DropbackPass,
            Formation::Spread,
            3.2,
            0,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Slot, route(&[(12.0, -2.0), (24.0, -8.0)], 22)),
                (WideL, route(&[(13.0, 2.0), (26.0, 8.0)], 24)),
                (WideR, route(&[(6.0, 5.0), (11.0, 8.0)], 9)),
            ],
        ),
        play(
            "RPO GLANCE",
            "RPO glance",
            PlayFamily::QuickPass,
            Formation::Pistol,
            1.0,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, run_block(1.5, 0.6, false)),
                (Rg, run_block(1.5, 2.2, false)),
                (Back, Assign::PassBlock { slide: 1.0, max_depth: -1.0 }),
                (Slot, run_block(2.5, 3.5, true)),
                (WideL, route(&[(6.0, -2.0)], 6)),
                (WideR, route(&[(9.0, -1.0)], 9)),
            ],
        ),
        play(
            "TRIPLE OPTION",
            "Option",
            PlayFamily::QbRun,
            Formation::Spread,
            0.0,
            1,
            &[
                (Qb, qb_carry(&[(-1.0, 0.0), (1.0, 2.5), (6.0, 5.0), (14.0, 6.0)])),
                (Lg, run_block(2.0, 0.5, false)),
                (Rg, run_block(3.0, 3.0, false)),
                (Back, run_block(3.0, 5.0, true)),
                (Slot, run_block(3.0, -4.0, true)),
                (WideL, route(&[(8.0, -2.0)], 8)),
                (WideR, route(&[(10.0, 2.0)], 10)),
            ],
        ),
        play(
            "DUO",
            "Duo",
            PlayFamily::RunInside,
            Formation::Bone,
            0.0,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: 1.0 }),
                (Lg, run_block(2.0, -0.6, false)),
                (Rg, run_block(3.2, 2.0, false)),
                (Back, carry(&[(-1.5, 0.0), (2.0, 0.5), (7.0, 1.0), (14.0, 0.5)])),
                (Slot, run_block(4.0, 3.5, true)),
                (WideL, run_block(1.0, -6.0, true)),
                (WideR, run_block(2.0, 9.0, true)),
            ],
        ),
        play(
            "LEAD ISO",
            "Isolation",
            PlayFamily::RunInside,
            Formation::Bone,
            0.0,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: 1.0 }),
                (Lg, run_block(2.5, 0.0, false)),
                (Rg, run_block(3.0, 2.0, false)),
                (Back, carry(&[(-1.0, 0.2), (2.0, 0.6), (8.0, 0.4), (15.0, 0.2)])),
                (Slot, run_block(2.5, 1.6, false)),
                (WideL, run_block(1.0, -5.5, true)),
                (WideR, run_block(2.0, 8.5, true)),
            ],
        ),
        play(
            "ZONE BOOT",
            "Bootleg",
            PlayFamily::PlayAction,
            Formation::Pistol,
            1.5,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
                (Lg, pass_pro(0.0)),
                (Rg, pass_pro(0.0)),
                (Back, fake(Vec2::new(2.0, 1.0), &[(1.0, 14.0), (12.0, 18.0)], 10)),
                (Slot, route(&[(8.0, 4.0), (16.0, -1.0)], 14)),
                (WideL, route(&[(4.0, -8.0)], 5)),
                (WideR, route(&[(16.0, 10.0)], 16)),
            ],
        ),
        play(
            "WIDE ZONE",
            "Outside zone",
            PlayFamily::RunOutside,
            Formation::Pistol,
            0.0,
            1,
            &[
                (Qb, Assign::PassBlock { slide: 0.0, max_depth: 1.0 }),
                (Lg, run_block(1.0, 1.0, false)),
                (Rg, run_block(1.5, 3.0, false)),
                (Back, carry(&[(-1.0, 0.4), (0.5, 3.5), (3.0, 7.0), (12.0, 8.0)])),
                (Slot, run_block(2.0, 5.0, true)),
                (WideL, run_block(1.0, -6.0, true)),
                (WideR, run_block(3.0, 10.0, true)),
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

/// The front a call lines up in: what the two edge players are doing with their
/// hands down. It is the first thing a defense is named by, and the first thing a
/// call sheet picks, so it is a real field on the call rather than a label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefFront {
    /// Both edges wide of the tackle box, rushing from outside in.
    Wide,
    /// Both edges head up on the guards, squeezing everything to the middle.
    Tite,
    /// One edge in the B gap, one wide: an unbalanced, spilling front.
    Over,
    /// A front the player drew: the number names the design in the book.
    Custom(u8),
}

impl DefFront {
    pub const ALL: [DefFront; 3] = [DefFront::Wide, DefFront::Tite, DefFront::Over];

    pub fn label(self) -> &'static str {
        match self {
            DefFront::Wide => "WIDE 9",
            DefFront::Tite => "TITE 5",
            DefFront::Over => "OVER 3",
            DefFront::Custom(id) => crate::design::front_name(id),
        }
    }

    /// The technique number a coach would give the edge on each side.
    pub fn technique(self, role: DefRole) -> u8 {
        match (self, role) {
            (DefFront::Wide, _) => 9,
            (DefFront::Tite, _) => 5,
            (DefFront::Over, DefRole::EdgeL) => 3,
            (DefFront::Over, _) => 9,
            (DefFront::Custom(_), _) => 5,
        }
    }

    /// Both edge techniques, left first, as the sheet prints them.
    pub fn techs(self) -> &'static str {
        match self {
            DefFront::Wide => "9 / 9",
            DefFront::Tite => "5 / 5",
            DefFront::Over => "3 / 9",
            DefFront::Custom(_) => "-- / --",
        }
    }

    /// What the front is trying to do with the run, in one word.
    pub fn line_call(self) -> &'static str {
        match self {
            DefFront::Wide => "CONTAIN",
            DefFront::Tite => "SQUEEZE",
            DefFront::Over => "SPILL",
            DefFront::Custom(_) => "BASE",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DefCall {
    pub name: &'static str,
    pub front: DefFront,
    pub coverage: Coverage,
    pub run_fit: RunFit,
    /// Defenders who rush the passer beyond the two edge players.
    pub blitzers: Vec<DefRole>,
    /// Extra rushers means fewer defenders in coverage; 7 total per side.
    pub press: bool,
    /// Drops drawn on the call sheet, one path a man, in yards past the line of
    /// scrimmage. A man with a path here goes where the player drew him instead
    /// of where the coverage would send him.
    pub drops: Vec<(DefRole, Vec<Vec2>)>,
}

impl DefCall {
    pub fn rushers(&self) -> usize {
        2 + self.blitzers.len()
    }

    /// What each of the seven is being asked to do, in the words on the card.
    /// Derived from the same front, coverage and blitz list the snap is played
    /// with, so the sheet cannot describe a defense the game does not run.
    pub fn assignment(&self, role: DefRole) -> &'static str {
        if self.blitzers.contains(&role) {
            return "BLITZ";
        }
        match role {
            DefRole::EdgeL | DefRole::EdgeR => match self.front {
                DefFront::Wide => "9 CONTAIN",
                DefFront::Tite => "5 SQUEEZE",
                DefFront::Over => {
                    if role == DefRole::EdgeL {
                        "3 SPILL"
                    } else {
                        "9 CONTAIN"
                    }
                }
                DefFront::Custom(_) => "RUSH",
            },
            DefRole::Mike | DefRole::Will => match self.coverage {
                Coverage::GoalLine => "FILL",
                c if c.is_man() => "MAN",
                _ => "HOOK",
            },
            DefRole::CbL | DefRole::CbR => {
                if self.press {
                    "PRESS"
                } else if self.coverage.is_man() {
                    "MAN"
                } else {
                    "ZONE"
                }
            }
            DefRole::Safety => match self.coverage {
                Coverage::Cover2 => "DEEP HALF",
                Coverage::Cover3 => "DEEP MIDDLE",
                Coverage::Cover4 => "DEEP QUARTER",
                Coverage::GoalLine => "ALLEY",
                _ => "FREE",
            },
        }
    }

    /// The read under the picture: front, coverage and how many are coming.
    pub fn read(&self) -> String {
        format!(
            "{} - {} - {} RUSH",
            self.front.label(),
            self.coverage.label(),
            self.rushers()
        )
    }
}

pub fn defensive_playbook() -> Vec<DefCall> {
    vec![
        DefCall {
            name: "COVER 1 MAN",
            front: DefFront::Wide,
            coverage: Coverage::ManFree,
            run_fit: RunFit::Balanced,
            blitzers: vec![],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "PRESS BAIL",
            front: DefFront::Tite,
            coverage: Coverage::ManPress,
            run_fit: RunFit::Aggressive,
            blitzers: vec![],
            press: true,
            drops: Vec::new(),
        },
        DefCall {
            name: "COVER 2 ZONE",
            front: DefFront::Wide,
            coverage: Coverage::Cover2,
            run_fit: RunFit::Balanced,
            blitzers: vec![],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "COVER 3 ZONE",
            front: DefFront::Tite,
            coverage: Coverage::Cover3,
            run_fit: RunFit::Conservative,
            blitzers: vec![],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "COVER 4 ZONE",
            front: DefFront::Wide,
            coverage: Coverage::Cover4,
            run_fit: RunFit::Conservative,
            blitzers: vec![],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "MIKE BLITZ",
            front: DefFront::Tite,
            coverage: Coverage::ManFree,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Mike],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "WILL BLITZ",
            front: DefFront::Over,
            coverage: Coverage::ManFree,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Will],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "ALL OUT BLITZ",
            front: DefFront::Over,
            coverage: Coverage::ManFree,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Mike, DefRole::Will],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "GOAL LINE BEAR",
            front: DefFront::Over,
            coverage: Coverage::GoalLine,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Mike],
            press: true,
            drops: Vec::new(),
        },
        DefCall {
            name: "COVER 2 TAMPA",
            front: DefFront::Tite,
            coverage: Coverage::Cover2,
            run_fit: RunFit::Conservative,
            blitzers: vec![],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "COVER 0 BLITZ",
            front: DefFront::Over,
            coverage: Coverage::ManFree,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Mike, DefRole::Will],
            press: true,
            drops: Vec::new(),
        },
        DefCall {
            name: "FIRE ZONE",
            front: DefFront::Tite,
            coverage: Coverage::Cover3,
            run_fit: RunFit::Aggressive,
            blitzers: vec![DefRole::Will],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "COVER 3 MATCH",
            front: DefFront::Over,
            coverage: Coverage::Cover3,
            run_fit: RunFit::Balanced,
            blitzers: vec![],
            press: false,
            drops: Vec::new(),
        },
        DefCall {
            name: "HEAVY TITE",
            front: DefFront::Tite,
            coverage: Coverage::ManFree,
            run_fit: RunFit::Aggressive,
            blitzers: vec![],
            press: true,
            drops: Vec::new(),
        },
        DefCall {
            name: "COVER 4 QUARTERS",
            front: DefFront::Over,
            coverage: Coverage::Cover4,
            run_fit: RunFit::Conservative,
            blitzers: vec![],
            press: false,
            drops: Vec::new(),
        },
    ]
}

// ---------------------------------------------------------------------------
// The books
// ---------------------------------------------------------------------------

/// A club's book: the offense it runs and the defense it plays.
///
/// Seven of them, and each is a whole identity rather than a label: the offense
/// picks its designs out of the catalog above, and the defense picks its calls.
/// Every club in the league has one, the way every NFL club does, and the player
/// may run any book over his own club's side.
///
/// Every book carries the same handful of fundamentals -- the shared plays are
/// what let two clubs line up against each other and still recognise the game --
/// and adds the designs that are its own. Some designs turn up in more than one
/// book, because a mesh concept is a mesh concept whoever draws it up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Book {
    /// Short, timed, underneath: the ball comes out on rhythm.
    WestCoast,
    /// Four wide, mesh and verticals, the whole field spread.
    AirRaid,
    /// Deep shots and play action off a heavy run look.
    Vertical,
    /// The quarterback as a runner, and the option.
    Spread,
    /// Gap runs, isolation, and the play action off them.
    Power,
    /// Outside zone, boots and the cut block.
    Zone,
    /// Nothing left out: the whole catalog, both ways.
    Balanced,
    /// A book the player designed himself: the number is its slot on the shelf.
    Custom(u8),
}

impl Book {
    /// The league's own books. The player's shelf is listed by the menu rather
    /// than here, because it changes while the game is running.
    pub const ALL: [Book; 7] = [
        Book::WestCoast,
        Book::AirRaid,
        Book::Vertical,
        Book::Spread,
        Book::Power,
        Book::Zone,
        Book::Balanced,
    ];

    pub fn index(self) -> usize {
        match self {
            Book::WestCoast => 0,
            Book::AirRaid => 1,
            Book::Vertical => 2,
            Book::Spread => 3,
            Book::Power => 4,
            Book::Zone => 5,
            Book::Balanced => 6,
            Book::Custom(slot) => 7 + slot as usize,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Book::WestCoast => "WEST COAST",
            Book::AirRaid => "AIR RAID",
            Book::Vertical => "VERTICAL",
            Book::Spread => "SPREAD OPTION",
            Book::Power => "POWER RUN",
            Book::Zone => "ZONE RUN",
            Book::Balanced => "BALANCED",
            Book::Custom(slot) => crate::design::book_name(slot as usize),
        }
    }

    /// What the picker says about it, in one line.
    pub fn blurb(self) -> &'static str {
        match self {
            Book::WestCoast => "TIMED, UNDERNEATH, ON RHYTHM: THE BALL COMES OUT QUICK",
            Book::AirRaid => "FOUR WIDE, MESH AND VERTS: THE WHOLE FIELD, EVERY SNAP",
            Book::Vertical => "DEEP SHOTS AND PLAY ACTION OFF A HEAVY RUN LOOK",
            Book::Spread => "THE QUARTERBACK RUNS IT: OPTION, DRAW AND THE RPO",
            Book::Power => "GAP RUNS, ISOLATION, AND THE PLAY ACTION OFF THEM",
            Book::Zone => "OUTSIDE ZONE, BOOTS AND THE CUT BLOCK",
            Book::Balanced => "EVERY DESIGN IN THE LEAGUE, BOTH SIDES OF THE BALL",
            Book::Custom(_) => "THE BOOK YOU DREW: FORMATIONS, PLAYS, FRONTS AND CALLS",
        }
    }

    /// The designs in the book. The shared fundamentals are listed in each one,
    /// because they are what every club in the league runs.
    pub fn plays(self) -> &'static [&'static str] {
        match self {
            Book::WestCoast => &[
                "ZONE READ",
                "POWER O",
                "CURL FLAT",
                "SMASH",
                "FOUR VERTS",
                "RB SCREEN",
                "SLANT FLAT",
                "LEVELS",
                "STICK",
                "DAGGER",
                "FLOOD",
                "DIG DRIVE",
                "SPOT",
            ],
            Book::AirRaid => &[
                "ZONE READ",
                "POWER O",
                "CURL FLAT",
                "SMASH",
                "FOUR VERTS",
                "RB SCREEN",
                "SLANT FLAT",
                "MESH",
                "LEVELS",
                "STICK",
                "DAGGER",
                "WR SCREEN",
                "Y CROSS",
                "TUNNEL SCREEN",
            ],
            Book::Vertical => &[
                "ZONE READ",
                "POWER O",
                "CURL FLAT",
                "SMASH",
                "FOUR VERTS",
                "RB SCREEN",
                "DEEP SHOT",
                "POST WHEEL",
                "YANKEE",
                "DAGGER",
                "FLOOD",
                "GOAL FADE",
                "PA BOMB",
                "DOUBLE POST",
            ],
            Book::Spread => &[
                "ZONE READ",
                "POWER O",
                "CURL FLAT",
                "SMASH",
                "FOUR VERTS",
                "RB SCREEN",
                "QB DRAW",
                "QB KEEPER",
                "JET FLY",
                "SLANT FLAT",
                "MESH",
                "WR SCREEN",
                "RPO GLANCE",
                "TRIPLE OPTION",
            ],
            Book::Power => &[
                "ZONE READ",
                "POWER O",
                "CURL FLAT",
                "SMASH",
                "FOUR VERTS",
                "RB SCREEN",
                "COUNTER TREY",
                "TRAP",
                "QB KEEPER",
                "PA BOOT",
                "FLOOD",
                "DEEP SHOT",
                "DUO",
                "LEAD ISO",
            ],
            Book::Zone => &[
                "ZONE READ",
                "POWER O",
                "CURL FLAT",
                "SMASH",
                "FOUR VERTS",
                "RB SCREEN",
                "ROCKET SWEEP",
                "JET FLY",
                "FLOOD",
                "PA BOOT",
                "DEEP SHOT",
                "ZONE BOOT",
                "WIDE ZONE",
            ],
            Book::Balanced => &[],
            Book::Custom(_) => &[],
        }
    }

    /// The defensive calls in the book, on the same terms.
    pub fn calls(self) -> &'static [&'static str] {
        match self {
            Book::WestCoast => &[
                "COVER 1 MAN",
                "COVER 3 ZONE",
                "MIKE BLITZ",
                "COVER 2 ZONE",
                "COVER 2 TAMPA",
                "PRESS BAIL",
            ],
            Book::AirRaid => &[
                "COVER 1 MAN",
                "COVER 3 ZONE",
                "MIKE BLITZ",
                "COVER 4 ZONE",
                "COVER 4 QUARTERS",
                "COVER 2 ZONE",
            ],
            Book::Vertical => &[
                "COVER 1 MAN",
                "COVER 3 ZONE",
                "MIKE BLITZ",
                "COVER 0 BLITZ",
                "PRESS BAIL",
                "GOAL LINE BEAR",
            ],
            Book::Spread => &[
                "COVER 1 MAN",
                "COVER 3 ZONE",
                "MIKE BLITZ",
                "WILL BLITZ",
                "FIRE ZONE",
                "COVER 2 ZONE",
            ],
            Book::Power => &[
                "COVER 1 MAN",
                "COVER 3 ZONE",
                "MIKE BLITZ",
                "HEAVY TITE",
                "GOAL LINE BEAR",
                "PRESS BAIL",
            ],
            Book::Zone => &[
                "COVER 1 MAN",
                "COVER 3 ZONE",
                "MIKE BLITZ",
                "COVER 3 MATCH",
                "COVER 4 ZONE",
                "COVER 2 ZONE",
            ],
            Book::Balanced => &[],
            Book::Custom(_) => &[],
        }
    }
}

/// The designs every club in the league has, whatever its book.
pub const SHARED_PLAYS: [&str; 6] =
    ["ZONE READ", "POWER O", "CURL FLAT", "SMASH", "FOUR VERTS", "RB SCREEN"];

/// The calls every club in the league has, whatever its book.
/// The calls every club carries, whatever it hangs its hat on. Goal line is on
/// the list because a book without one has no defense to call inside the five,
/// and the fallback was a man-to-man call that hands over the touchdown instead.
pub const SHARED_CALLS: [&str; 4] =
    ["COVER 1 MAN", "COVER 3 ZONE", "MIKE BLITZ", "GOAL LINE BEAR"];

/// The offense one club's book gives it, in catalog order so the call sheet
/// reads the same way every week.
pub fn playbook_for(book: Book) -> Vec<Play> {
    if let Book::Custom(slot) = book {
        let drawn = crate::design::offense_plays_at(slot as usize);
        // A book that is not legal yet is not a book: the league's own plays are
        // the fallback rather than an empty call sheet.
        return if drawn.is_empty() { playbook() } else { drawn };
    }
    let all = playbook();
    if book == Book::Balanced {
        return all;
    }
    let names = book.plays();
    all.into_iter()
        .filter(|p| names.contains(&p.name) || SHARED_PLAYS.contains(&p.name))
        .collect()
}

/// The defense one club's book gives it, on the same terms.
pub fn defensive_playbook_for(book: Book) -> Vec<DefCall> {
    if let Book::Custom(slot) = book {
        let drawn = crate::design::defense_calls_at(slot as usize);
        return if drawn.is_empty() {
            defensive_playbook()
        } else {
            drawn
        };
    }
    let all = defensive_playbook();
    if book == Book::Balanced {
        return all;
    }
    let names = book.calls();
    // The shared calls come with every book whether the book bothers to list
    // them or not: the fundamentals are the league's, not the scheme's.
    all.into_iter()
        .filter(|d| names.contains(&d.name) || SHARED_CALLS.contains(&d.name))
        .collect()
}

/// Defensive alignment relative to the line of scrimmage. `los` is the canonical
/// spot and `direction` the offense's attack direction.
///
/// The front decides where the two edge players line up, which is the difference
/// between a wide nine rushing from outside the tackle box and a tight five head
/// up on the guard. It moves them on the field, so the card and the snap agree.
pub fn defensive_alignment(
    role: DefRole,
    call: &DefCall,
    los: f32,
    direction: i8,
    play_dir: i8,
) -> Vec2 {
    // A front the player drew is already in the yards the editor works in.
    if let DefFront::Custom(id) = call.front {
        return crate::design::front_spot(id, role, los, direction);
    }
    // Work in "offense forward" space then convert.
    let fwd = direction as f32;
    let coverage = call.coverage;
    let to_field = |along: f32, lateral: f32| {
        Vec2::new(los + fwd * along, lateral + play_dir as f32 * 0.0)
    };
    let (edge_l, edge_r) = match call.front {
        // Measured against the base alignment the defense used before fronts
        // existed (0.9 yards upfield, 1.7 either side): a wide nine is half a yard
        // wider, a tight five is a quarter tighter, and the over front is the one
        // deliberately unbalanced look. Bigger shifts looked better on the card and
        // were measured to wipe out long touchdowns, so they are kept small.
        DefFront::Wide => (to_field(0.8, -1.95), to_field(0.8, 1.95)),
        DefFront::Tite => (to_field(0.9, -1.45), to_field(0.9, 1.45)),
        DefFront::Over => (to_field(0.85, -1.35), to_field(0.8, 1.95)),
        // A designed front never reaches here: it returned its drawn spots
        // above. The arm exists so the match stays exhaustive.
        DefFront::Custom(_) => (to_field(0.8, -1.95), to_field(0.8, 1.95)),
    };
    match role {
        DefRole::EdgeL => edge_l,
        DefRole::EdgeR => edge_r,
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
        .map(|&r| (r, defensive_alignment(r, call, los, direction, 0)))
        .collect()
}

/// The fronts the book is built on, in the order a coach installs them.
pub fn fronts_in(book: &[DefCall]) -> Vec<DefFront> {
    let mut out: Vec<DefFront> = Vec::new();
    for d in book {
        if !out.contains(&d.front) {
            out.push(d.front);
        }
    }
    out
}

/// The calls in one front, in book order.
pub fn def_calls_in(book: &[DefCall], front: DefFront) -> Vec<usize> {
    book.iter()
        .enumerate()
        .filter(|(_, d)| d.front == front)
        .map(|(i, _)| i)
        .collect()
}

/// Cheap description of an offensive personnel grouping for the play-call screen.
pub fn personnel_label(formation: Formation) -> &'static str {
    // Short enough to fit the formation column: the longer wording ran under the
    // edge of the box and read as a rendering fault.
    match formation {
        Formation::Pistol => "1RB 1TE 2WR",
        Formation::Bone => "2RB 1TE 1WR",
        Formation::Trips => "TRIPS 1RB 2WR",
        Formation::Doubles => "DBLS 1RB 2WR",
        Formation::Spread => "1RB 3WR",
        Formation::Empty => "4WR",
        Formation::Custom(_) => "DESIGNED",
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
    fn a_flipped_play_keeps_the_outside_men_where_they_are() {
        for p in playbook() {
            let f = p.flipped();
            assert_eq!(f.flip, !p.flip, "{} did not flip", p.name);
            for role in OffRole::ALL {
                let a = p.alignment(role);
                let b = f.alignment(role);
                assert!((a.x - b.x).abs() < 1e-6, "{} {role:?} moved upfield", p.name);
                if role.keeps_side() {
                    // The men on the line and the men on the sidelines stay on
                    // the side they lined up on. The insides are the ones who
                    // change sides.
                    assert!((a.y - b.y).abs() < 1e-6, "{} {role:?} moved on a flip", p.name);
                } else {
                    assert!((a.y + b.y).abs() < 1e-6, "{} {role:?} did not mirror", p.name);
                }
            }
            // The outside men do not change sides: they take each other's job,
            // mirrored, from where they are already standing.
            let left_was = waypoints(p.assignment(OffRole::WideL).expect("no wide left job"));
            let right_was = waypoints(p.assignment(OffRole::WideR).expect("no wide right job"));
            let left_now = waypoints(f.assignment(OffRole::WideL).expect("wide left lost his job"));
            let right_now = waypoints(f.assignment(OffRole::WideR).expect("wide right lost his job"));
            assert_eq!(left_now.len(), right_was.len(), "{}: the jobs did not swap", p.name);
            assert_eq!(right_now.len(), left_was.len(), "{}: the jobs did not swap", p.name);
            for (now, was) in left_now.iter().zip(right_was.iter()) {
                assert!(
                    (now.x - was.x).abs() < 1e-4 && (now.y + was.y).abs() < 1e-4,
                    "{}: the wide left did not take the other job: {now:?} from {was:?}",
                    p.name
                );
            }
            for (now, was) in right_now.iter().zip(left_was.iter()) {
                assert!(
                    (now.x - was.x).abs() < 1e-4 && (now.y + was.y).abs() < 1e-4,
                    "{}: the wide right did not take the other job: {now:?} from {was:?}",
                    p.name
                );
            }
            // Flipping twice is the play that was drawn, not a mirrored copy of a
            // mirrored copy that has quietly drifted.
            let back = f.flipped();
            assert_eq!(back.flip, p.flip);
            for role in OffRole::ALL {
                let a = back.alignment(role);
                let b = p.alignment(role);
                assert!((a.y - b.y).abs() < 1e-4, "{} {role:?} did not come back", p.name);
                let got = waypoints(back.assignment(role).expect("no job"));
                let want = waypoints(p.assignment(role).expect("no job"));
                assert_eq!(got.len(), want.len(), "{} {role:?} came back different", p.name);
                for (g, w) in got.iter().zip(want.iter()) {
                    assert!(
                        (g.x - w.x).abs() < 1e-4 && (g.y - w.y).abs() < 1e-4,
                        "{} {role:?} came back different",
                        p.name
                    );
                }
            }
        }
    }

    /// The waypoints an assignment runs, for comparing a job with its mirror.
    fn waypoints(a: &Assign) -> Vec<Vec2> {
        match a {
            Assign::Route { points, .. }
            | Assign::Fake { points, .. }
            | Assign::Carry { points }
            | Assign::QbCarry { points } => points.clone(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn every_formation_has_plays_an_audible_can_reach() {
        let book = playbook();
        let formations = formations_in(&book);
        assert!(formations.len() >= 4, "only {} formations", formations.len());
        for f in formations {
            let plays = plays_in(&book, f);
            assert!(plays.len() >= 2, "{f:?} has {} plays to audible to", plays.len());
            for i in plays {
                assert_eq!(book[i].formation, f);
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

    /// The sheet is only layered if every call sits in exactly one front, and a
    /// card is only honest if every man on it has a job.
    #[test]
    fn every_front_has_calls_and_every_call_has_a_job_for_all_seven() {
        let book = defensive_playbook();
        let fronts = fronts_in(&book);
        assert_eq!(fronts.len(), DefFront::ALL.len(), "one column per front");
        let mut total = 0;
        for f in fronts {
            let calls = def_calls_in(&book, f);
            assert!(calls.len() >= 3, "{f:?} only has {} calls", calls.len());
            total += calls.len();
            for i in calls {
                for role in DefRole::ALL {
                    assert!(
                        !book[i].assignment(role).is_empty(),
                        "{} has no job for {role:?}",
                        book[i].name
                    );
                }
            }
        }
        assert_eq!(total, book.len(), "a call belongs to one front and one front only");
    }

    #[test]
    fn the_front_changes_where_the_edges_line_up() {
        let los = 50.0;
        let mut wide = defensive_playbook()[0].clone();
        wide.front = DefFront::Wide;
        let mut tite = wide.clone();
        tite.front = DefFront::Tite;
        let edge = |call: &DefCall, role| defensive_alignment(role, call, los, 1, 0);
        assert!(
            edge(&wide, DefRole::EdgeL).y.abs() > edge(&tite, DefRole::EdgeL).y.abs() + 0.3,
            "a wide nine has to line up wider than a tight five: {:.2} vs {:.2}",
            edge(&wide, DefRole::EdgeL).y,
            edge(&tite, DefRole::EdgeL).y
        );
        // And the over front is deliberately unbalanced: one three, one nine.
        let mut over = wide.clone();
        over.front = DefFront::Over;
        let (l, r) = (edge(&over, DefRole::EdgeL), edge(&over, DefRole::EdgeR));
        assert!(
            (l.y.abs() - r.y.abs()).abs() > 0.5,
            "the over front has to be unbalanced: {:.1} vs {:.1}",
            l.y,
            r.y
        );
    }

    #[test]
    fn defenses_field_seven_with_a_legal_rush_count() {
        for d in defensive_playbook() {
            assert!(d.rushers() >= 2 && d.rushers() <= 4, "{} rush count", d.name);
            assert!(d.coverage.deep_defenders() <= 7 - d.rushers() + 2);
        }
    }
}


#[cfg(test)]
mod book_tests {
    use super::*;

    /// Every club's book is a book: the shared fundamentals, enough designs to
    /// call a game from, and more than one formation to line up in.
    #[test]
    fn every_book_has_the_shared_plays_and_enough_of_its_own() {
        // A designed book is the player's, not a league scheme: it has whatever
        // he drew in it.
        for book in Book::ALL {
            let off = playbook_for(book);
            let def = defensive_playbook_for(book);
            assert!(off.len() >= 12, "{} has {} plays", book.label(), off.len());
            for name in SHARED_PLAYS {
                assert!(
                    off.iter().any(|p| p.name == name),
                    "{} is missing the shared play {name}",
                    book.label()
                );
            }
            let forms = formations_in(&off);
            assert!(forms.len() >= 3, "{} has {} formations", book.label(), forms.len());
            let runs = off.iter().filter(|p| p.family.is_run()).count();
            let passes = off.iter().filter(|p| p.family.is_pass()).count();
            assert!(runs >= 2 && passes >= 4, "{} is one-sided", book.label());
            assert!(def.len() >= 4, "{} has {} calls", book.label(), def.len());
            for name in SHARED_CALLS {
                assert!(
                    def.iter().any(|d| d.name == name),
                    "{} is missing the shared call {name}",
                    book.label()
                );
            }
        }
    }

    /// A scheme book is not the whole catalog wearing a new name: each one is
    /// missing designs that an unrelated one has.
    #[test]
    fn two_books_are_not_the_same_book() {
        for a in Book::ALL {
            for b in Book::ALL {
                // A designed book is not a scheme: it is whatever was drawn,
                // which on a machine with no drawing in it is the whole catalog.
                if a == b || a == Book::Balanced || b == Book::Balanced {
                    continue;
                }
                let (one, two) = (playbook_for(a), playbook_for(b));
                assert!(
                    one.iter().any(|p| !two.iter().any(|q| q.name == p.name)),
                    "{} and {} run exactly the same plays",
                    a.label(),
                    b.label()
                );
            }
        }
    }

    /// A name in a book that is not in the catalog would silently shorten the
    /// book, and a duplicate name would make two designs the same design.
    #[test]
    fn every_named_design_exists_and_names_are_unique() {
        let all = playbook();
        let calls = defensive_playbook();
        let mut seen: Vec<&str> = Vec::new();
        for p in &all {
            assert!(!seen.contains(&p.name), "two designs called {}", p.name);
            seen.push(p.name);
            // Every design gives every job something to do, or a man stands
            // there while the play is run.
            for role in OffRole::ALL {
                assert!(
                    p.assignment(role).is_some(),
                    "{} asks nothing of {}",
                    p.name,
                    role.label()
                );
            }
        }
        let mut seen: Vec<&str> = Vec::new();
        for d in &calls {
            assert!(!seen.contains(&d.name), "two calls called {}", d.name);
            seen.push(d.name);
        }
        for book in Book::ALL {
            for name in book.plays() {
                assert!(
                    all.iter().any(|p| p.name == *name),
                    "{} names {name}",
                    book.label()
                );
            }
            for name in book.calls() {
                assert!(
                    calls.iter().any(|d| d.name == *name),
                    "{} calls {name}",
                    book.label()
                );
            }
        }
    }

    /// A book's defense says what it is: fronts and coverages rather than the
    /// same shape six times.
    #[test]
    fn a_book_covers_more_than_one_coverage() {
        for book in Book::ALL {
            let def = defensive_playbook_for(book);
            let mut covs: Vec<Coverage> = Vec::new();
            let mut fronts: Vec<DefFront> = Vec::new();
            for d in &def {
                if !covs.contains(&d.coverage) {
                    covs.push(d.coverage);
                }
                if !fronts.contains(&d.front) {
                    fronts.push(d.front);
                }
            }
            assert!(covs.len() >= 2, "{} plays one coverage", book.label());
            assert!(fronts.len() >= 2, "{} plays one front", book.label());
        }
    }
}
