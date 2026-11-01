//! Teams, players, attributes and A7FL uniform specs.
//!
//! Everything is generated deterministically from a seed so a saved game can be
//! replayed and rendered pixel-for-pixel.

use crate::math::{clamp, lerp, Rgb, Rng};

// ---------------------------------------------------------------------------
// Attributes
// ---------------------------------------------------------------------------

/// Madden-style 40..99 ratings. The simulation converts these to physical units
/// through the accessor methods below.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Attributes {
    pub speed: u8,
    pub accel: u8,
    pub agility: u8,
    pub strength: u8,
    pub catching: u8,
    pub accuracy: u8,
    pub power: u8,
    pub awareness: u8,
    pub pass_rush: u8,
    pub pass_block: u8,
    pub run_block: u8,
    pub tackling: u8,
    pub elusiveness: u8,
    pub break_tackle: u8,
    pub coverage: u8,
    pub discipline: u8,
    pub stamina: u8,
}

fn n(v: u8) -> f32 {
    v as f32 / 100.0
}

impl Attributes {
    /// Top speed in yards per second.
    pub fn top_speed(&self) -> f32 {
        lerp(5.4, 9.9, n(self.speed))
    }

    /// Acceleration in yards per second squared.
    pub fn acceleration(&self) -> f32 {
        lerp(7.0, 19.0, n(self.accel))
    }

    /// How quickly the player can change direction (radians of desired heading per second).
    pub fn turn_rate(&self) -> f32 {
        lerp(5.5, 12.5, n(self.agility))
    }

    /// Maximum air yards on a throw (not counting run after catch).
    pub fn arm_strength(&self) -> f32 {
        lerp(28.0, 62.0, n(self.power))
    }

    pub fn accuracy_factor(&self) -> f32 {
        n(self.accuracy)
    }

    pub fn catch_factor(&self) -> f32 {
        n(self.catching)
    }

    pub fn coverage_factor(&self) -> f32 {
        n(self.coverage)
    }

    pub fn tackle_factor(&self) -> f32 {
        n(self.tackling)
    }

    pub fn break_tackle_factor(&self) -> f32 {
        n(self.break_tackle)
    }

    pub fn elusiveness_factor(&self) -> f32 {
        n(self.elusiveness)
    }

    pub fn awareness_factor(&self) -> f32 {
        n(self.awareness)
    }

    pub fn stamina_factor(&self) -> f32 {
        n(self.stamina)
    }

    /// Composite used for depth charts, sorted best-first.
    pub fn overall(&self, pos: Position) -> u8 {
        let w: &[(u8, f32)] = match pos {
            Position::Qb => &[
                (self.accuracy, 3.0),
                (self.power, 2.0),
                (self.awareness, 3.0),
                (self.agility, 1.5),
                (self.speed, 1.0),
                (self.elusiveness, 1.0),
            ],
            Position::Ol => &[
                (self.pass_block, 3.0),
                (self.run_block, 3.0),
                (self.strength, 3.0),
                (self.awareness, 1.5),
                (self.agility, 0.5),
            ],
            Position::Rb => &[
                (self.speed, 2.5),
                (self.elusiveness, 2.5),
                (self.break_tackle, 2.0),
                (self.agility, 2.0),
                (self.catching, 1.5),
                (self.strength, 1.0),
            ],
            Position::Wr => &[
                (self.catching, 3.0),
                (self.speed, 3.0),
                (self.agility, 2.0),
                (self.awareness, 1.5),
                (self.elusiveness, 1.0),
            ],
            Position::Te => &[
                (self.catching, 2.5),
                (self.strength, 2.0),
                (self.run_block, 2.0),
                (self.speed, 1.5),
                (self.awareness, 1.0),
            ],
            Position::Dl => &[
                (self.pass_rush, 3.0),
                (self.strength, 2.5),
                (self.tackling, 2.0),
                (self.agility, 1.0),
            ],
            Position::Lb => &[
                (self.tackling, 3.0),
                (self.coverage, 2.0),
                (self.strength, 2.0),
                (self.speed, 2.0),
                (self.awareness, 1.0),
            ],
            Position::Db => &[
                (self.coverage, 3.5),
                (self.speed, 3.0),
                (self.agility, 2.0),
                (self.tackling, 1.5),
                (self.catching, 1.0),
            ],
        };
        let (sum, weight) = w
            .iter()
            .fold((0.0f32, 0.0f32), |(s, wt), (v, k)| (s + *v as f32 * *k, wt + *k));
        clamp((sum / weight).round(), 1.0, 99.0) as u8
    }
}

// ---------------------------------------------------------------------------
// Players
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Position {
    Qb,
    Ol,
    Rb,
    Wr,
    Te,
    Dl,
    Lb,
    Db,
}

impl Position {
    pub fn abbr(self) -> &'static str {
        match self {
            Position::Qb => "QB",
            Position::Ol => "OL",
            Position::Rb => "RB",
            Position::Wr => "WR",
            Position::Te => "TE",
            Position::Dl => "DL",
            Position::Lb => "LB",
            Position::Db => "DB",
        }
    }

    pub fn is_skill(self) -> bool {
        matches!(self, Position::Qb | Position::Rb | Position::Wr | Position::Te)
    }

    pub fn is_offense(self) -> bool {
        matches!(self, Position::Qb | Position::Ol | Position::Rb | Position::Wr | Position::Te)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinTone {
    Porcelain,
    Fair,
    Tan,
    Olive,
    Brown,
    Deep,
}

impl SkinTone {
    pub fn rgb(self) -> Rgb {
        match self {
            SkinTone::Porcelain => Rgb::hex(0xF0C8A8),
            SkinTone::Fair => Rgb::hex(0xE0AC7E),
            SkinTone::Tan => Rgb::hex(0xC68863),
            SkinTone::Olive => Rgb::hex(0xA9714B),
            SkinTone::Brown => Rgb::hex(0x82502F),
            SkinTone::Deep => Rgb::hex(0x5B3419),
        }
    }

    fn from_roll(r: f32) -> SkinTone {
        match r {
            x if x < 0.14 => SkinTone::Deep,
            x if x < 0.40 => SkinTone::Brown,
            x if x < 0.60 => SkinTone::Olive,
            x if x < 0.78 => SkinTone::Tan,
            x if x < 0.92 => SkinTone::Fair,
            _ => SkinTone::Porcelain,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HairStyle {
    Bald,
    Buzz,
    Fade,
    Locs,
    TwistOut,
    HighTop,
}

impl HairStyle {
    pub fn all() -> [HairStyle; 6] {
        [
            HairStyle::Bald,
            HairStyle::Buzz,
            HairStyle::Fade,
            HairStyle::Locs,
            HairStyle::TwistOut,
            HairStyle::HighTop,
        ]
    }
}

#[derive(Clone, Debug)]
pub struct Player {
    pub id: u16,
    pub first: String,
    pub last: String,
    pub number: u8,
    pub pos: Position,
    pub attrs: Attributes,
    /// Height in inches.
    pub height: u8,
    pub weight: u16,
    pub skin: SkinTone,
    pub hair: HairStyle,
    pub hair_color: Rgb,
    /// 0.9 (slight) .. 1.1 (thick) body width multiplier for the renderer.
    pub build: f32,
    /// 0.0 = clean shaven, 1.0 = full beard, for the face textures.
    pub beard: f32,
}

impl Player {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first, self.last)
    }

    pub fn short_name(&self) -> String {
        format!("{} {}", self.first.chars().next().unwrap_or('?'), self.last)
    }

    pub fn last_name(&self) -> &str {
        &self.last
    }

    pub fn overall(&self) -> u8 {
        self.attrs.overall(self.pos)
    }

    /// A7FL players wear jersey + shorts, no pads, so the visual silhouette is
    /// narrower than an NFL player's and depends on real body size.
    pub fn shoulder_width(&self) -> f32 {
        lerp(0.44, 0.62, (self.weight as f32 - 165.0) / 110.0) * self.build
    }

    pub fn height_yards(&self) -> f32 {
        self.height as f32 / 36.0
    }
}

// ---------------------------------------------------------------------------
// Uniforms
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JerseyPattern {
    Solid,
    ShoulderStripe,
    Sash,
    SidePanel,
    Hoops,
}

/// A7FL kit: sleeveless jersey, no shoulder pads, matching shorts and low socks.
#[derive(Clone, Copy, Debug)]
pub struct Uniform {
    pub jersey: Rgb,
    pub jersey_accent: Rgb,
    pub numbers: Rgb,
    pub shorts: Rgb,
    pub shorts_accent: Rgb,
    pub socks: Rgb,
    pub shoes: Rgb,
    pub pattern: JerseyPattern,
}

impl Uniform {
    pub fn readable_number_color(&self) -> Rgb {
        if self.jersey.luma() > 145.0 {
            Rgb::hex(0x14161A)
        } else {
            self.numbers
        }
    }
}

// ---------------------------------------------------------------------------
// Teams
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Team {
    pub id: u8,
    pub city: String,
    pub name: String,
    pub abbr: String,
    pub primary: Rgb,
    pub secondary: Rgb,
    pub accent: Rgb,
    pub home: Uniform,
    pub away: Uniform,
    pub roster: Vec<Player>,
}

impl Team {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.city, self.name)
    }

    pub fn display_upper(&self) -> String {
        self.full_name().to_uppercase()
    }

    fn depth_by_pos(&self, pos: Position) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .roster
            .iter()
            .enumerate()
            .filter(|(_, p)| p.pos == pos)
            .map(|(i, _)| i)
            .collect();
        v.sort_by_key(|&i| std::cmp::Reverse(self.roster[i].overall()));
        v
    }

    pub fn qb(&self) -> usize {
        self.depth_by_pos(Position::Qb)[0]
    }

    /// 7 offensive players: QB, 2 down linemen, a back and 3 more eligibles.
    pub fn offense_personnel(&self) -> OffensePersonnel {
        let qb = self.qb();
        let ol = self.depth_by_pos(Position::Ol);
        let rb = self.depth_by_pos(Position::Rb);
        let wr = self.depth_by_pos(Position::Wr);
        let te = self.depth_by_pos(Position::Te);

        let lg = ol[0];
        let rg = *ol.get(1).unwrap_or(&ol[0]);
        let back = *rb.first().unwrap_or(&wr[0]);
        // Prefer a tight end for the in-line slot, then fill with receivers.
        let slot = *te.first().unwrap_or(&wr[0]);
        let mut wide: Vec<usize> = wr.iter().copied().take(3).collect();
        while wide.len() < 2 {
            match te.get(wide.len()) {
                Some(&t) => wide.push(t),
                None => break,
            }
        }
        let wide_left = wide.first().copied().unwrap_or(lg);
        let wide_right = wide.get(1).copied().unwrap_or(rg);

        OffensePersonnel { players: [qb, lg, rg, back, slot, wide_left, wide_right] }
    }

    /// 7 defenders: 2 linemen, 2 linebackers, 3 defensive backs.
    pub fn defense_personnel(&self) -> DefensePersonnel {
        let dl = self.depth_by_pos(Position::Dl);
        let lb = self.depth_by_pos(Position::Lb);
        let db = self.depth_by_pos(Position::Db);
        let players = [
            dl[0],
            *dl.get(1).unwrap_or(&dl[0]),
            lb[0],
            *lb.get(1).unwrap_or(&lb[0]),
            db[0],
            *db.get(1).unwrap_or(&db[0]),
            *db.get(2).unwrap_or(&db[0]),
        ];
        DefensePersonnel { players }
    }

    pub fn player(&self, idx: usize) -> &Player {
        &self.roster[idx]
    }
}

/// Seven on-field offensive players, indexed to match `OffRole::index()`:
/// `[QB, LG, RG, RB, TE, WR-L, WR-R]`.
#[derive(Clone, Copy, Debug)]
pub struct OffensePersonnel {
    pub players: [usize; 7],
}

impl OffensePersonnel {
    pub fn qb(&self) -> usize {
        self.players[0]
    }

    pub fn back(&self) -> usize {
        self.players[3]
    }

    /// Everyone who can legally catch a pass.
    pub fn eligibles(&self) -> [usize; 4] {
        [self.players[3], self.players[4], self.players[5], self.players[6]]
    }
}

/// Seven on-field defenders, indexed to match `DefRole::index()`:
/// `[EDGE-L, EDGE-R, MIKE, WILL, CB-L, CB-R, SAF]`.
#[derive(Clone, Copy, Debug)]
pub struct DefensePersonnel {
    pub players: [usize; 7],
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

const FIRST_NAMES: &[&str] = &[
    "Dante", "Marcus", "Tyrell", "Javon", "Andre", "Elijah", "Tariq", "Malik", "Kareem", "Devon",
    "Isaiah", "Rashad", "Xavier", "Lamar", "Tre", "Damon", "Cortez", "Micah", "Jamal", "Antoine",
    "Bryce", "Cole", "Darius", "Ezekiel", "Garrett", "Hakeem", "Ibrahim", "Jalen", "Keon", "Lorenzo",
    "Mekhi", "Nasir", "Omar", "Princeton", "Quentin", "Rico", "Samir", "Tavian", "Ulysses", "Vince",
    "Wes", "Yusuf", "Zaire", "Amari", "Bishop", "Caleb", "Deshawn", "Emeka", "Frankie", "Gus",
];

const LAST_NAMES: &[&str] = &[
    "Whitfield", "Boatwright", "Okonkwo", "Delgado", "Rasheed", "Mercer", "Castillo", "Bennett",
    "Okafor", "Sandoval", "Kowalski", "Duval", "Ellis", "Freeman", "Grady", "Hollins", "Ivey",
    "Jefferson", "Kincaid", "Lawson", "Maddox", "Novak", "Ortega", "Pryor", "Quintero", "Reyes",
    "Stafford", "Truitt", "Underwood", "Vega", "Wynn", "Xiong", "Yates", "Zamora", "Ashby",
    "Brantley", "Cotuato", "Dunbar", "Escobar", "Fontaine", "Guevara", "Halstead", "Ishmael",
    "Jeter", "Kelsey", "Lindqvist", "Mabry", "Nakamura", "Ordonez", "Pemberton",
];

const HAIR_COLORS: &[Rgb] = &[
    Rgb::hex(0x141010),
    Rgb::hex(0x241A12),
    Rgb::hex(0x3A2418),
    Rgb::hex(0x4A3220),
    Rgb::hex(0x6B4A2A),
];

fn attr(rng: &mut Rng, base: i32, spread: i32) -> u8 {
    clamp((base + rng.range_i32(-spread, spread)) as f32, 41.0, 99.0) as u8
}

fn profile(pos: Position, rng: &mut Rng, level: i32) -> Attributes {
    let l = level;
    match pos {
        Position::Qb => Attributes {
            speed: attr(rng, 64 + l, 6),
            accel: attr(rng, 66 + l, 6),
            agility: attr(rng, 68 + l, 6),
            strength: attr(rng, 54 + l, 7),
            catching: attr(rng, 40, 8),
            accuracy: attr(rng, 74 + l, 8),
            power: attr(rng, 71 + l, 8),
            awareness: attr(rng, 73 + l, 7),
            pass_rush: attr(rng, 22, 8),
            pass_block: attr(rng, 30, 8),
            run_block: attr(rng, 32, 8),
            tackling: attr(rng, 34, 8),
            elusiveness: attr(rng, 62 + l, 8),
            break_tackle: attr(rng, 56 + l, 8),
            coverage: attr(rng, 24, 8),
            discipline: attr(rng, 74 + l, 8),
            stamina: attr(rng, 68 + l, 8),
        },
        Position::Ol => Attributes {
            speed: attr(rng, 51 + l / 2, 5),
            accel: attr(rng, 56 + l / 2, 6),
            agility: attr(rng, 58 + l / 2, 6),
            strength: attr(rng, 82 + l, 8),
            catching: attr(rng, 34, 6),
            accuracy: attr(rng, 30, 5),
            power: attr(rng, 40, 6),
            awareness: attr(rng, 68 + l, 8),
            pass_rush: attr(rng, 30, 8),
            pass_block: attr(rng, 76 + l, 8),
            run_block: attr(rng, 78 + l, 8),
            tackling: attr(rng, 38, 8),
            elusiveness: attr(rng, 40, 8),
            break_tackle: attr(rng, 50, 8),
            coverage: attr(rng, 28, 6),
            discipline: attr(rng, 70 + l, 8),
            stamina: attr(rng, 70 + l, 8),
        },
        Position::Rb => Attributes {
            speed: attr(rng, 78 + l, 7),
            accel: attr(rng, 80 + l, 7),
            agility: attr(rng, 79 + l, 7),
            strength: attr(rng, 62 + l, 8),
            catching: attr(rng, 70 + l, 8),
            accuracy: attr(rng, 30, 6),
            power: attr(rng, 36, 6),
            awareness: attr(rng, 68 + l, 8),
            pass_rush: attr(rng, 22, 6),
            pass_block: attr(rng, 48 + l, 8),
            run_block: attr(rng, 42, 8),
            tackling: attr(rng, 36, 8),
            elusiveness: attr(rng, 80 + l, 7),
            break_tackle: attr(rng, 74 + l, 8),
            coverage: attr(rng, 30, 6),
            discipline: attr(rng, 70 + l, 8),
            stamina: attr(rng, 70 + l, 8),
        },
        Position::Wr => Attributes {
            speed: attr(rng, 84 + l, 6),
            accel: attr(rng, 83 + l, 6),
            agility: attr(rng, 82 + l, 7),
            strength: attr(rng, 52 + l, 8),
            catching: attr(rng, 80 + l, 8),
            accuracy: attr(rng, 30, 6),
            power: attr(rng, 34, 6),
            awareness: attr(rng, 70 + l, 8),
            pass_rush: attr(rng, 20, 6),
            pass_block: attr(rng, 34, 8),
            run_block: attr(rng, 38 + l / 2, 8),
            tackling: attr(rng, 32, 8),
            elusiveness: attr(rng, 78 + l, 8),
            break_tackle: attr(rng, 58 + l, 8),
            coverage: attr(rng, 32, 6),
            discipline: attr(rng, 70 + l, 8),
            stamina: attr(rng, 70 + l, 8),
        },
        Position::Te => Attributes {
            speed: attr(rng, 70 + l, 7),
            accel: attr(rng, 72 + l, 7),
            agility: attr(rng, 70 + l, 7),
            strength: attr(rng, 74 + l, 8),
            catching: attr(rng, 74 + l, 8),
            accuracy: attr(rng, 28, 5),
            power: attr(rng, 34, 6),
            awareness: attr(rng, 68 + l, 8),
            pass_rush: attr(rng, 24, 6),
            pass_block: attr(rng, 60 + l, 8),
            run_block: attr(rng, 66 + l, 8),
            tackling: attr(rng, 40, 8),
            elusiveness: attr(rng, 60 + l, 8),
            break_tackle: attr(rng, 68 + l, 8),
            coverage: attr(rng, 30, 6),
            discipline: attr(rng, 70 + l, 8),
            stamina: attr(rng, 70 + l, 8),
        },
        Position::Dl => Attributes {
            speed: attr(rng, 72 + l, 7),
            accel: attr(rng, 76 + l, 7),
            agility: attr(rng, 72 + l, 7),
            strength: attr(rng, 80 + l, 8),
            catching: attr(rng, 34, 8),
            accuracy: attr(rng, 28, 5),
            power: attr(rng, 32, 6),
            awareness: attr(rng, 68 + l, 8),
            pass_rush: attr(rng, 79 + l, 8),
            pass_block: attr(rng, 30, 8),
            run_block: attr(rng, 34, 8),
            tackling: attr(rng, 76 + l, 8),
            elusiveness: attr(rng, 50, 8),
            break_tackle: attr(rng, 50, 8),
            coverage: attr(rng, 40, 8),
            discipline: attr(rng, 68 + l, 8),
            stamina: attr(rng, 68 + l, 8),
        },
        Position::Lb => Attributes {
            speed: attr(rng, 74 + l, 7),
            accel: attr(rng, 77 + l, 7),
            agility: attr(rng, 74 + l, 7),
            strength: attr(rng, 74 + l, 8),
            catching: attr(rng, 50 + l, 8),
            accuracy: attr(rng, 28, 5),
            power: attr(rng, 32, 6),
            awareness: attr(rng, 71 + l, 8),
            pass_rush: attr(rng, 68 + l, 8),
            pass_block: attr(rng, 30, 8),
            run_block: attr(rng, 34, 8),
            tackling: attr(rng, 80 + l, 8),
            elusiveness: attr(rng, 54, 8),
            break_tackle: attr(rng, 58, 8),
            coverage: attr(rng, 68 + l, 8),
            discipline: attr(rng, 70 + l, 8),
            stamina: attr(rng, 70 + l, 8),
        },
        Position::Db => Attributes {
            speed: attr(rng, 82 + l, 6),
            accel: attr(rng, 83 + l, 6),
            agility: attr(rng, 82 + l, 7),
            strength: attr(rng, 56 + l, 8),
            catching: attr(rng, 62 + l, 8),
            accuracy: attr(rng, 28, 5),
            power: attr(rng, 32, 6),
            awareness: attr(rng, 72 + l, 8),
            pass_rush: attr(rng, 40, 8),
            pass_block: attr(rng, 28, 6),
            run_block: attr(rng, 32, 6),
            tackling: attr(rng, 68 + l, 8),
            elusiveness: attr(rng, 60 + l, 8),
            break_tackle: attr(rng, 48, 8),
            coverage: attr(rng, 80 + l, 8),
            discipline: attr(rng, 72 + l, 8),
            stamina: attr(rng, 72 + l, 8),
        },
    }
}

fn body(pos: Position, rng: &mut Rng) -> (u8, u16, f32) {
    match pos {
        Position::Qb => (rng.range_i32(73, 76) as u8, rng.range_i32(195, 225) as u16, rng.range(0.97, 1.05)),
        Position::Ol => (rng.range_i32(74, 77) as u8, rng.range_i32(265, 320) as u16, rng.range(1.06, 1.16)),
        Position::Rb => (rng.range_i32(68, 73) as u8, rng.range_i32(185, 215) as u16, rng.range(0.98, 1.08)),
        Position::Wr => (rng.range_i32(70, 76) as u8, rng.range_i32(175, 205) as u16, rng.range(0.9, 0.99)),
        Position::Te => (rng.range_i32(74, 77) as u8, rng.range_i32(230, 260) as u16, rng.range(1.02, 1.12)),
        Position::Dl => (rng.range_i32(73, 77) as u8, rng.range_i32(245, 290) as u16, rng.range(1.02, 1.14)),
        Position::Lb => (rng.range_i32(71, 76) as u8, rng.range_i32(215, 245) as u16, rng.range(1.0, 1.08)),
        Position::Db => (rng.range_i32(69, 74) as u8, rng.range_i32(180, 205) as u16, rng.range(0.92, 1.0)),
    }
}

fn generate_roster(seed: u64, level: i32) -> Vec<Player> {
    let mut rng = Rng::new(seed);
    // A7FL rosters run 14 minimum, 35 max for inter-division play.
    let plan: &[(Position, usize)] = &[
        (Position::Qb, 2),
        (Position::Ol, 4),
        (Position::Rb, 2),
        (Position::Wr, 4),
        (Position::Te, 2),
        (Position::Dl, 3),
        (Position::Lb, 3),
        (Position::Db, 4),
    ];
    let mut used_numbers: Vec<u8> = Vec::new();
    let mut roster = Vec::new();
    let mut id = 0u16;
    for &(pos, count) in plan {
        // Starters are a little better than the depth behind them.
        for i in 0..count {
            let lvl = if i == 0 { level } else { level - rng.range_i32(2, 9) };
            let attrs = profile(pos, &mut rng, lvl);
            let (height, weight, build) = body(pos, &mut rng);
            let number = loop {
                let candidate = match pos {
                    Position::Qb => rng.range_i32(1, 19) as u8,
                    Position::Ol => rng.range_i32(50, 79) as u8,
                    _ => rng.range_i32(1, 49) as u8,
                };
                if !used_numbers.contains(&candidate) {
                    used_numbers.push(candidate);
                    break candidate;
                }
            };
            let skin = SkinTone::from_roll(rng.f32());
            let hair = *rng.pick(&HairStyle::all()).unwrap_or(&HairStyle::Fade);
            roster.push(Player {
                id,
                first: rng.pick(FIRST_NAMES).unwrap_or(&"Dante").to_string(),
                last: rng.pick(LAST_NAMES).unwrap_or(&"Whitfield").to_string(),
                number,
                pos,
                attrs,
                height,
                weight,
                skin,
                hair,
                hair_color: *rng.pick(HAIR_COLORS).unwrap_or(&Rgb::hex(0x241A12)),
                build,
                beard: if rng.chance(0.45) { rng.range(0.3, 1.0) } else { 0.0 },
            });
            id += 1;
        }
    }
    roster
}

struct TeamSpec {
    city: &'static str,
    name: &'static str,
    abbr: &'static str,
    primary: Rgb,
    secondary: Rgb,
    accent: Rgb,
    pattern: JerseyPattern,
    seed: u64,
    level: i32,
}

fn build_uniform(
    jersey: Rgb,
    jersey_accent: Rgb,
    numbers: Rgb,
    shorts: Rgb,
    shorts_accent: Rgb,
    socks: Rgb,
    shoes: Rgb,
    pattern: JerseyPattern,
) -> Uniform {
    Uniform {
        jersey,
        jersey_accent,
        numbers,
        shorts,
        shorts_accent,
        socks,
        shoes,
        pattern,
    }
}

fn team_from_spec(spec: TeamSpec) -> Team {
    let roster = generate_roster(spec.seed, spec.level);
    let home = build_uniform(
        spec.primary,
        spec.accent,
        if spec.primary.luma() > 150.0 { spec.primary.scale(0.2) } else { Rgb::WHITE },
        spec.primary.scale(0.55),
        spec.accent,
        Rgb::WHITE,
        Rgb::hex(0x15171B),
        spec.pattern,
    );
    let away = build_uniform(
        Rgb::hex(0xF2F4F6),
        spec.primary,
        spec.primary.scale(0.55),
        spec.primary.scale(0.75),
        spec.accent,
        spec.primary,
        Rgb::hex(0x15171B),
        spec.pattern,
    );
    Team {
        id: 0,
        city: spec.city.to_string(),
        name: spec.name.to_string(),
        abbr: spec.abbr.to_string(),
        primary: spec.primary,
        secondary: spec.secondary,
        accent: spec.accent,
        home,
        away,
        roster,
    }
}

/// The ten fictional clubs of the A7FL-inspired league. Real A7FL markets, made-up
/// nicknames so nothing here touches a real team's marks.
pub fn league_teams() -> Vec<Team> {
    let specs = vec![
        TeamSpec {
            city: "Newark",
            name: "Nighthawks",
            abbr: "NEW",
            primary: Rgb::hex(0x111318),
            secondary: Rgb::hex(0x2A2E36),
            accent: Rgb::hex(0xF2C14E),
            pattern: JerseyPattern::ShoulderStripe,
            seed: 0xA7F1_0001,
            level: 8,
        },
        TeamSpec {
            city: "Paterson",
            name: "Forge",
            abbr: "PAT",
            primary: Rgb::hex(0x4A5568),
            secondary: Rgb::hex(0x2D3748),
            accent: Rgb::hex(0xFF6B35),
            pattern: JerseyPattern::SidePanel,
            seed: 0xA7F1_0002,
            level: 5,
        },
        TeamSpec {
            city: "Jersey City",
            name: "Havoc",
            abbr: "JC",
            primary: Rgb::hex(0x5B2A86),
            secondary: Rgb::hex(0x3A1A57),
            accent: Rgb::hex(0xC8FF00),
            pattern: JerseyPattern::Sash,
            seed: 0xA7F1_0003,
            level: 2,
        },
        TeamSpec {
            city: "Baltimore",
            name: "Iron",
            abbr: "BAL",
            primary: Rgb::hex(0x2B2D42),
            secondary: Rgb::hex(0x8D99AE),
            accent: Rgb::hex(0xE63946),
            pattern: JerseyPattern::ShoulderStripe,
            seed: 0xA7F1_0004,
            level: 4,
        },
        TeamSpec {
            city: "Washington",
            name: "Sentinels",
            abbr: "WAS",
            primary: Rgb::hex(0x12284C),
            secondary: Rgb::hex(0x0A1930),
            accent: Rgb::hex(0xD62828),
            pattern: JerseyPattern::Solid,
            seed: 0xA7F1_0005,
            level: 6,
        },
        TeamSpec {
            city: "Miami",
            name: "Riptide",
            abbr: "MIA",
            primary: Rgb::hex(0x0FA3B1),
            secondary: Rgb::hex(0x0B6E77),
            accent: Rgb::hex(0xF6F7EB),
            pattern: JerseyPattern::Hoops,
            seed: 0xA7F1_0006,
            level: 3,
        },
        TeamSpec {
            city: "Cincinnati",
            name: "Stampede",
            abbr: "CIN",
            primary: Rgb::hex(0xA4161A),
            secondary: Rgb::hex(0x6B0F12),
            accent: Rgb::hex(0xF2C14E),
            pattern: JerseyPattern::ShoulderStripe,
            seed: 0xA7F1_0007,
            level: 7,
        },
        TeamSpec {
            city: "Las Vegas",
            name: "Vipers",
            abbr: "LV",
            primary: Rgb::hex(0xC0C6CC),
            secondary: Rgb::hex(0x8A929C),
            accent: Rgb::hex(0xD7263D),
            pattern: JerseyPattern::SidePanel,
            seed: 0xA7F1_0008,
            level: 1,
        },
        TeamSpec {
            city: "Camden",
            name: "Cyclones",
            abbr: "CAM",
            primary: Rgb::hex(0x1B998B),
            secondary: Rgb::hex(0x12726A),
            accent: Rgb::hex(0xEDF7F6),
            pattern: JerseyPattern::Sash,
            seed: 0xA7F1_0009,
            level: 0,
        },
        TeamSpec {
            city: "Trenton",
            name: "Thunder",
            abbr: "TRE",
            primary: Rgb::hex(0xE0A526),
            secondary: Rgb::hex(0xA87A16),
            accent: Rgb::hex(0x1A1C20),
            pattern: JerseyPattern::Solid,
            seed: 0xA7F1_000A,
            level: 3,
        },
    ];
    specs
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let mut t = team_from_spec(s);
            t.id = i as u8;
            t
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rosters_are_legal_and_unique() {
        let teams = league_teams();
        assert_eq!(teams.len(), 10);
        for t in &teams {
            assert!(t.roster.len() >= 14, "{} roster too small", t.abbr);
            let mut numbers: Vec<u8> = t.roster.iter().map(|p| p.number).collect();
            numbers.sort_unstable();
            numbers.dedup();
            assert_eq!(numbers.len(), t.roster.len(), "duplicate numbers on {}", t.abbr);
            for p in &t.roster {
                for v in [
                    p.attrs.speed,
                    p.attrs.accuracy,
                    p.attrs.tackling,
                    p.attrs.coverage,
                ] {
                    assert!((41..=99).contains(&v), "attribute out of range: {v}");
                }
            }
        }
    }

    #[test]
    fn personnel_gives_seven_a_side() {
        let teams = league_teams();
        let t = &teams[0];
        let off = t.offense_personnel();
        let mut off_ids = off.players.to_vec();
        off_ids.sort_unstable();
        off_ids.dedup();
        assert_eq!(off_ids.len(), 7, "duplicate offensive personnel");
        assert_eq!(t.player(off.players[1]).pos, Position::Ol);
        assert_eq!(t.player(off.players[2]).pos, Position::Ol);
        assert_eq!(off.qb(), off.players[0]);

        let def = t.defense_personnel();
        let mut def_ids = def.players.to_vec();
        def_ids.sort_unstable();
        def_ids.dedup();
        assert_eq!(def_ids.len(), 7, "duplicate defensive personnel");
    }

    #[test]
    fn generation_is_deterministic() {
        let a = league_teams();
        let b = league_teams();
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.roster.len(), y.roster.len());
            for (p, q) in x.roster.iter().zip(y.roster.iter()) {
                assert_eq!(p.number, q.number);
                assert_eq!(p.attrs, q.attrs);
                assert_eq!(p.last, q.last);
            }
        }
    }

    #[test]
    fn physical_conversions_are_sane() {
        let fast = Attributes { speed: 95, power: 90, ..Default::default() };
        let slow = Attributes { speed: 50, power: 40, ..Default::default() };
        assert!(fast.top_speed() > slow.top_speed() + 1.5);
        assert!(fast.arm_strength() > slow.arm_strength() + 10.0);
        assert!(fast.top_speed() < 10.5, "nobody runs 21 mph in pads-free 7s");
    }
}
