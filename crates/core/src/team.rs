//! Teams, players, attributes and 7 on 7 uniform specs.
//!
//! Everything is generated deterministically from a seed so a saved game can be
//! replayed and rendered pixel-for-pixel.

use crate::playbook::Book;
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

    /// The ten numbers a man is judged by, on the same 40..99 scale the rest of
    /// the game uses. Two of them are not stored as such: throwing is what the arm
    /// and the accuracy add up to, route running is catching, agility and
    /// awareness, and block shedding is the first step of a pass rush.
    pub fn ratings(&self) -> [(&'static str, u8); 10] {
        let blend = |w: &[(u8, f32)]| -> u8 {
            let total: f32 = w.iter().map(|(_, k)| *k).sum();
            let sum: f32 = w.iter().map(|(v, k)| *v as f32 * k).sum();
            (sum / total).round().clamp(1.0, 99.0) as u8
        };
        [
            ("SPEED", self.speed),
            ("STRENGTH", self.strength),
            ("AGILITY", self.agility),
            ("AWARENESS", self.awareness),
            ("CATCHING", self.catching),
            (
                "THROWING",
                blend(&[(self.accuracy, 0.6), (self.power, 0.4)]),
            ),
            (
                "ROUTE RUNNING",
                blend(&[(self.catching, 0.4), (self.agility, 0.35), (self.awareness, 0.25)]),
            ),
            ("COVERAGE", self.coverage),
            ("TACKLING", self.tackling),
            ("BLOCK SHEDDING", blend(&[(self.pass_rush, 0.7), (self.strength, 0.3)])),
        ]
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
    /// Whether he pulls the balaclava down over his face. A couple of men a
    /// side do; it is decided once, when the roster is built, so every replay
    /// shows the same faces masked.
    pub balaclava: bool,
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

    /// 7 on 7 players wear jersey + shorts, no pads, so the visual silhouette is
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

/// 7 on 7 kit: sleeveless jersey, no shoulder pads, matching shorts and low socks.
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
    /// The club's mark as painted on the chest: its name in capitals, cut to
    /// what a chest can hold. Bytes rather than a string so a uniform stays
    /// `Copy`, which the render thread leans on.
    pub mark: [u8; 6],
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

// ---------------------------------------------------------------------------
// Ratings
// ---------------------------------------------------------------------------

/// What a club is worth on the field, on the same 40..99 scale as the men who
/// make it up: the seven starters on each side of the ball, averaged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamRatings {
    pub overall: u8,
    pub offense: u8,
    pub defense: u8,
}

/// The seven who start on each side of the ball in 7 on 7, and how many of each
/// the rating counts. The best at each position starts, so depth does not drag a
/// club's number down — the man on the field is what is being rated.
const OFFENSE_STARTERS: [(Position, usize); 5] = [
    (Position::Qb, 1),
    (Position::Ol, 2),
    (Position::Rb, 1),
    (Position::Wr, 2),
    (Position::Te, 1),
];
const DEFENSE_STARTERS: [(Position, usize); 3] =
    [(Position::Dl, 2), (Position::Lb, 2), (Position::Db, 3)];

fn mean(values: &[u8]) -> u8 {
    if values.is_empty() {
        return 0;
    }
    let sum: u32 = values.iter().map(|v| *v as u32).sum();
    (sum as f32 / values.len() as f32).round() as u8
}

fn starters(roster: &[Player], plan: &[(Position, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(pos, count) in plan {
        let mut picked: Vec<u8> = roster
            .iter()
            .filter(|p| p.pos == pos)
            .map(|p| p.overall())
            .collect();
        picked.sort_unstable_by(|a, b| b.cmp(a));
        out.extend(picked.into_iter().take(count));
    }
    out
}

impl TeamRatings {
    pub fn from_roster(roster: &[Player]) -> TeamRatings {
        let offense = starters(roster, &OFFENSE_STARTERS);
        let defense = starters(roster, &DEFENSE_STARTERS);
        let mut all = offense.clone();
        all.extend_from_slice(&defense);
        TeamRatings {
            offense: mean(&offense),
            defense: mean(&defense),
            overall: mean(&all),
        }
    }
}

// ---------------------------------------------------------------------------
// Edited depth charts
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Saved depth orders, by club seed and position row: player ids, best man first.
static DEPTH_EDITS: OnceLock<Mutex<HashMap<(u64, usize), Vec<u16>>>> = OnceLock::new();

fn depth_edits() -> &'static Mutex<HashMap<(u64, usize), Vec<u16>>> {
    DEPTH_EDITS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Save one position row of a club's chart, best man first.
pub fn set_depth_order(seed: u64, row: usize, ids: Vec<u16>) {
    if let Ok(mut m) = depth_edits().lock() {
        m.insert((seed, row), ids);
    }
}

/// The saved order for one position row, if that row has been edited.
pub fn depth_order(seed: u64, row: usize) -> Option<Vec<u16>> {
    depth_edits()
        .lock()
        .ok()
        .and_then(|m| m.get(&(seed, row)).cloned())
}

/// Every saved row for a club, flattened into one order. Rows nobody touched are
/// not in it, and those positions keep their generated order.
pub fn saved_depth(seed: u64) -> Vec<u16> {
    let Ok(m) = depth_edits().lock() else {
        return Vec::new();
    };
    let mut rows: Vec<((u64, usize), Vec<u16>)> = m
        .iter()
        .filter(|((s, _), _)| *s == seed)
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    rows.sort_by_key(|(k, _)| k.1);
    rows.into_iter().flat_map(|(_, v)| v).collect()
}

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
    /// The kit the front end chose for this side, if it chose one. `None` keeps
    /// the side's own rule: home shirt at home, white on the road.
    pub worn: Option<Uniform>,
    pub roster: Vec<Player>,
    /// The book this club runs, carried on the team so the coordinator on either
    /// sideline calls its own plays rather than the league's.
    pub book: Book,
    /// An edited depth order, by player id, best man first. Empty when nobody has
    /// touched the chart, and then the best man by rating starts.
    pub depth: Vec<u16>,
}

impl Team {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.city, self.name)
    }

    pub fn display_upper(&self) -> String {
        self.full_name().to_uppercase()
    }

    /// The club's number, from the men who will actually be on the field.
    pub fn ratings(&self) -> TeamRatings {
        TeamRatings::from_roster(&self.roster)
    }

    fn depth_by_pos(&self, pos: Position) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .roster
            .iter()
            .enumerate()
            .filter(|(_, p)| p.pos == pos)
            .map(|(i, _)| i)
            .collect();
        // A man the player moved up the chart starts, whatever the ratings say:
        // that is the whole point of being able to edit it.
        v.sort_by_key(|&i| {
            let rank = self.depth.iter().position(|&id| id == self.roster[i].id);
            (
                rank.unwrap_or(usize::MAX),
                std::cmp::Reverse(self.roster[i].overall()),
            )
        });
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
    // 7 on 7 rosters run 14 minimum, 35 max for inter-division play.
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
                balaclava: false,
            });
            id += 1;
        }
    }
    roster
}

/// The four divisions the league is split into. The exhibition picker walks
/// them in this order, four clubs in each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Division {
    North,
    South,
    East,
    West,
}

impl Division {
    pub const ALL: [Division; 4] =
        [Division::North, Division::South, Division::East, Division::West];

    pub fn label(self) -> &'static str {
        match self {
            Division::North => "NORTH",
            Division::South => "SOUTH",
            Division::East => "EAST",
            Division::West => "WEST",
        }
    }
}

/// One club as the menu sees it: everything needed to draw its badge and to
/// build its roster, without generating sixteen rosters to show a card.
#[derive(Clone, Debug)]
pub struct Club {
    pub city: &'static str,
    pub name: &'static str,
    pub abbr: &'static str,
    /// The book the club runs: its offense and its defense, the way the club it
    /// is drawn from runs its own.
    pub book: Book,
    pub division: Division,
    pub primary: Rgb,
    pub secondary: Rgb,
    pub accent: Rgb,
    pub pattern: JerseyPattern,
    pub seed: u64,
    pub level: i32,
    /// City where white is the home shirt and the colour is what the club
    /// travels in: Dallas and Miami do it the other way round from the league.
    pub white_home: bool,
}

impl Club {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.city, self.name)
    }

    /// The club's full roster, built from its seed. The customization screen
    /// shows a depth chart and a list, so it needs every man, not the three the
    /// card names.
    pub fn roster(&self) -> Vec<Player> {
        generate_roster(self.seed, self.level)
    }

    /// The card's numbers and the men worth naming, off one roster build: the
    /// menu shows sixteen of these, so it must not build thirty-two rosters.
    pub fn profile(&self, stars: usize) -> (TeamRatings, Vec<(Position, String, u8)>) {
        let roster = generate_roster(self.seed, self.level);
        let ratings = TeamRatings::from_roster(&roster);
        let mut men: Vec<(Position, String, u8)> = roster
            .iter()
            .map(|p| (p.pos, p.full_name(), p.overall()))
            .collect();
        men.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
        men.truncate(stars);
        (ratings, men)
    }
}

/// The club's mark for the chest: the first word of its name, in capitals, cut
/// to the six cells that fit across a jersey without running under an arm.
fn club_mark(name: &str) -> [u8; 6] {
    let mut out = [0u8; 6];
    let word = name.split_whitespace().next().unwrap_or("");
    for (i, b) in word.bytes().take(6).enumerate() {
        out[i] = b.to_ascii_uppercase();
    }
    out
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
    mark: [u8; 6],
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
        mark,
    }
}

/// The two shirts a club owns: its colour and its white, in the order the club
/// counts them. Most of the league wears the colour at home; Dallas and Miami
/// wear white there and the colour on the road.
///
/// Built without a roster, so a menu can show both kits for every club on the
/// card without generating sixteen teams to do it.
pub fn club_kits(spec: &Club) -> (Uniform, Uniform) {
    let colour = build_uniform(
        spec.primary,
        spec.accent,
        if spec.primary.luma() > 150.0 { spec.primary.scale(0.2) } else { Rgb::WHITE },
        spec.primary.scale(0.55),
        spec.accent,
        Rgb::WHITE,
        Rgb::hex(0x15171B),
        spec.pattern,
        club_mark(spec.name),
    );
    let white = build_uniform(
        Rgb::hex(0xF2F4F6),
        spec.primary,
        spec.primary.scale(0.55),
        spec.primary.scale(0.75),
        spec.accent,
        spec.primary,
        Rgb::hex(0x15171B),
        spec.pattern,
        club_mark(spec.name),
    );
    if spec.white_home {
        (white, colour)
    } else {
        (colour, white)
    }
}

pub fn team_from_spec(spec: Club) -> Team {
    let roster = generate_roster(spec.seed, spec.level);
    let (home, away) = club_kits(&spec);
    let mut team = Team {
        id: 0,
        city: spec.city.to_string(),
        name: spec.name.to_string(),
        abbr: spec.abbr.to_string(),
        primary: spec.primary,
        secondary: spec.secondary,
        accent: spec.accent,
        home,
        away,
        worn: None,
        roster,
        book: spec.book,
            depth: saved_depth(spec.seed),
    };

    // A couple of men a side wear the balaclava: two of the seven who take the
    // field on offense and one on defense, chosen per club from the men who
    // actually start. Everybody used to wear one, which made a team read as a
    // single masked unit; two a side reads as a few individuals instead. Picked
    // once, here, so a replay shows the same faces.
    let off = team.offense_personnel();
    let def = team.defense_personnel();
    let mut rng = Rng::new(spec.seed ^ 0xBACA_1A7A);
    let pick = |rng: &mut Rng, n: usize| (rng.f32() * n as f32) as usize % n;
    let a = pick(&mut rng, 7);
    // A second starter, never the same man twice.
    let b = (a + 1 + pick(&mut rng, 6)) % 7;
    let c = pick(&mut rng, 7);
    for i in [off.players[a], off.players[b], def.players[c]] {
        team.roster[i].balaclava = true;
    }
    team
}

#[cfg(test)]
mod balaclava_tests {
    use super::*;

    #[test]
    fn a_couple_of_men_a_side_wear_the_balaclava() {
        for club in clubs() {
            let team = team_from_spec(club);
            let off = team.offense_personnel();
            let def = team.defense_personnel();
            let masked = |idx: &[usize; 7]| {
                idx.iter()
                    .filter(|&&i| team.roster[i].balaclava)
                    .count()
            };
            assert_eq!(masked(&off.players), 2, "{} offense", team.abbr);
            assert_eq!(masked(&def.players), 1, "{} defense", team.abbr);
        }
    }
}

/// The sixteen clubs of the league, four to a division. Cities are real; every
/// nickname, mark and colour set here is invented, so nothing on a jersey
/// belongs to a club somebody already owns.
pub fn clubs() -> Vec<Club> {
    use JerseyPattern::*;
    vec![
        // North
        Club { city: "Boston", name: "Lanterns", abbr: "BOS", division: Division::North, primary: Rgb::hex(0x0B3D2E), secondary: Rgb::hex(0x082A20), accent: Rgb::hex(0xE8B44A), pattern: ShoulderStripe, book: Book::Spread, seed: 0xA7F1_0001, white_home: false, level: 8 },
        Club { city: "Chicago", name: "Foundry", abbr: "CHI", division: Division::North, primary: Rgb::hex(0x8C2F1B), secondary: Rgb::hex(0x5E1F12), accent: Rgb::hex(0xF2C14E), pattern: Solid, book: Book::Power, seed: 0xA7F1_0002, white_home: false, level: 5 },
        Club { city: "Detroit", name: "Assembly", abbr: "DET", division: Division::North, primary: Rgb::hex(0x1F3A5F), secondary: Rgb::hex(0x14263F), accent: Rgb::hex(0xFF6B35), pattern: SidePanel, book: Book::Vertical, seed: 0xA7F1_0003, white_home: false, level: 3 },
        Club { city: "Minneapolis", name: "Northwind", abbr: "MIN", division: Division::North, primary: Rgb::hex(0x2E4A7D), secondary: Rgb::hex(0x1B2E50), accent: Rgb::hex(0xC8E6FF), pattern: Hoops, book: Book::WestCoast, seed: 0xA7F1_0004, white_home: false, level: 6 },
        // South
        Club { city: "Atlanta", name: "Fireflies", abbr: "ATL", division: Division::South, primary: Rgb::hex(0x3B1F5E), secondary: Rgb::hex(0x261340), accent: Rgb::hex(0xFFD166), pattern: Sash, book: Book::AirRaid, seed: 0xA7F1_0005, white_home: false, level: 4 },
        Club { city: "Miami", name: "Riptide", abbr: "MIA", division: Division::South, primary: Rgb::hex(0x0FA3B1), secondary: Rgb::hex(0x0B6E77), accent: Rgb::hex(0xF6F7EB), pattern: Hoops, book: Book::Spread, seed: 0xA7F1_0006, white_home: true, level: 3 },
        Club { city: "Dallas", name: "Rustlers", abbr: "DAL", division: Division::South, primary: Rgb::hex(0x7A2E1E), secondary: Rgb::hex(0x4E1C12), accent: Rgb::hex(0xE9C46A), pattern: ShoulderStripe, book: Book::Power, seed: 0xA7F1_0007, white_home: true, level: 7 },
        Club { city: "Nashville", name: "Brass", abbr: "NSH", division: Division::South, primary: Rgb::hex(0xA8761F), secondary: Rgb::hex(0x6E4C12), accent: Rgb::hex(0x1B1B1B), pattern: Solid, book: Book::Power, seed: 0xA7F1_0008, white_home: false, level: 5 },
        // East
        Club { city: "New York", name: "Beacons", abbr: "NYC", division: Division::East, primary: Rgb::hex(0x1B2A6B), secondary: Rgb::hex(0x101A45), accent: Rgb::hex(0xF4D35E), pattern: ShoulderStripe, book: Book::WestCoast, seed: 0xA7F1_0009, white_home: false, level: 6 },
        Club { city: "Philadelphia", name: "Anvils", abbr: "PHI", division: Division::East, primary: Rgb::hex(0x2B2B2B), secondary: Rgb::hex(0x161616), accent: Rgb::hex(0xC0C0C0), pattern: SidePanel, book: Book::Zone, seed: 0xA7F1_000A, white_home: false, level: 8 },
        Club { city: "Washington", name: "Monuments", abbr: "WAS", division: Division::East, primary: Rgb::hex(0x12284C), secondary: Rgb::hex(0x0A1930), accent: Rgb::hex(0xD62828), pattern: Solid, book: Book::AirRaid, seed: 0xA7F1_000B, white_home: false, level: 4 },
        Club { city: "Charlotte", name: "Aviators", abbr: "CLT", division: Division::East, primary: Rgb::hex(0x2F5D8C), secondary: Rgb::hex(0x1C3A59), accent: Rgb::hex(0x9BD1F0), pattern: Hoops, book: Book::Power, seed: 0xA7F1_000C, white_home: false, level: 2 },
        // West
        Club { city: "Los Angeles", name: "Comets", abbr: "LAX", division: Division::West, primary: Rgb::hex(0x5B2A86), secondary: Rgb::hex(0x3A1A57), accent: Rgb::hex(0xC8FF00), pattern: Sash, book: Book::WestCoast, seed: 0xA7F1_000D, white_home: false, level: 3 },
        Club { city: "Phoenix", name: "Sandstorm", abbr: "PHX", division: Division::West, primary: Rgb::hex(0xB5651D), secondary: Rgb::hex(0x7A4413), accent: Rgb::hex(0xF2E8CF), pattern: ShoulderStripe, book: Book::AirRaid, seed: 0xA7F1_000E, white_home: false, level: 6 },
        Club { city: "Seattle", name: "Rainmakers", abbr: "SEA", division: Division::West, primary: Rgb::hex(0x14532D), secondary: Rgb::hex(0x0C3520), accent: Rgb::hex(0x7FD1AE), pattern: Solid, book: Book::Zone, seed: 0xA7F1_000F, white_home: false, level: 5 },
        Club { city: "Denver", name: "Altitude", abbr: "DEN", division: Division::West, primary: Rgb::hex(0x3C4F76), secondary: Rgb::hex(0x27334C), accent: Rgb::hex(0xE0E7F1), pattern: SidePanel, book: Book::WestCoast, seed: 0xA7F1_0010, white_home: false, level: 7 },
        // North, the second four
        Club { city: "Indianapolis", name: "Speedway", abbr: "IND", division: Division::North, primary: Rgb::hex(0x1C2E4A), secondary: Rgb::hex(0x101B2E), accent: Rgb::hex(0xF2C14E), pattern: SidePanel, book: Book::AirRaid, seed: 0xA7F1_0011, white_home: false, level: 6 },
        Club { city: "Columbus", name: "Quarry", abbr: "CMH", division: Division::North, primary: Rgb::hex(0x8A5A2B), secondary: Rgb::hex(0x5C3B1C), accent: Rgb::hex(0xF0E6D2), pattern: Solid, book: Book::Balanced, seed: 0xA7F1_0012, white_home: false, level: 4 },
        Club { city: "Milwaukee", name: "Iron", abbr: "MIL", division: Division::North, primary: Rgb::hex(0x2B2F36), secondary: Rgb::hex(0x191C21), accent: Rgb::hex(0xE8B44A), pattern: ShoulderStripe, book: Book::Vertical, seed: 0xA7F1_0013, white_home: false, level: 5 },
        Club { city: "Buffalo", name: "Gales", abbr: "BUF", division: Division::North, primary: Rgb::hex(0x274B7A), secondary: Rgb::hex(0x18304F), accent: Rgb::hex(0xD9E6F2), pattern: Hoops, book: Book::Vertical, seed: 0xA7F1_0014, white_home: false, level: 3 },
        // South, the second four
        Club { city: "Houston", name: "Derricks", abbr: "HOU", division: Division::South, primary: Rgb::hex(0x22303C), secondary: Rgb::hex(0x141E26), accent: Rgb::hex(0xE86A2C), pattern: Sash, book: Book::WestCoast, seed: 0xA7F1_0015, white_home: false, level: 7 },
        Club { city: "New Orleans", name: "Revelers", abbr: "NOL", division: Division::South, primary: Rgb::hex(0x4B1D5E), secondary: Rgb::hex(0x30123D), accent: Rgb::hex(0xF2C14E), pattern: Hoops, book: Book::AirRaid, seed: 0xA7F1_0016, white_home: false, level: 5 },
        Club { city: "Memphis", name: "Blues", abbr: "MEM", division: Division::South, primary: Rgb::hex(0x1B3A6B), secondary: Rgb::hex(0x102444), accent: Rgb::hex(0x9BD1F0), pattern: Solid, book: Book::Balanced, seed: 0xA7F1_0017, white_home: false, level: 4 },
        Club { city: "Tampa", name: "Storm", abbr: "TPA", division: Division::South, primary: Rgb::hex(0x0F5C63), secondary: Rgb::hex(0x0A3D42), accent: Rgb::hex(0xF4E3B2), pattern: SidePanel, book: Book::Vertical, seed: 0xA7F1_0018, white_home: false, level: 6 },
        // East, the second four
        Club { city: "Baltimore", name: "Clippers", abbr: "BAL", division: Division::East, primary: Rgb::hex(0x1F2A44), secondary: Rgb::hex(0x121A2C), accent: Rgb::hex(0xE8573F), pattern: ShoulderStripe, book: Book::Power, seed: 0xA7F1_0019, white_home: false, level: 5 },
        Club { city: "Richmond", name: "Founders", abbr: "RIC", division: Division::East, primary: Rgb::hex(0x5A2A2A), secondary: Rgb::hex(0x3A1A1A), accent: Rgb::hex(0xE8DCC8), pattern: Solid, book: Book::Balanced, seed: 0xA7F1_001A, white_home: false, level: 3 },
        Club { city: "Providence", name: "Sparrows", abbr: "PVD", division: Division::East, primary: Rgb::hex(0x2E5D3A), secondary: Rgb::hex(0x1C3A24), accent: Rgb::hex(0xF2E8CF), pattern: Hoops, book: Book::Balanced, seed: 0xA7F1_001B, white_home: false, level: 4 },
        Club { city: "Syracuse", name: "Squall", abbr: "SYR", division: Division::East, primary: Rgb::hex(0x3C4F76), secondary: Rgb::hex(0x27334C), accent: Rgb::hex(0xCFE3F5), pattern: SidePanel, book: Book::Balanced, seed: 0xA7F1_001C, white_home: false, level: 2 },
        // West, the second four
        Club { city: "San Diego", name: "Tide", abbr: "SDG", division: Division::West, primary: Rgb::hex(0x0E5A78), secondary: Rgb::hex(0x0A3E53), accent: Rgb::hex(0xF6E9C9), pattern: Sash, book: Book::Vertical, seed: 0xA7F1_001D, white_home: false, level: 6 },
        Club { city: "Sacramento", name: "Delta", abbr: "SAC", division: Division::West, primary: Rgb::hex(0x3A6B4A), secondary: Rgb::hex(0x244630), accent: Rgb::hex(0xE9C46A), pattern: Solid, book: Book::Balanced, seed: 0xA7F1_001E, white_home: false, level: 4 },
        Club { city: "Portland", name: "Timberline", abbr: "POR", division: Division::West, primary: Rgb::hex(0x1E4A34), secondary: Rgb::hex(0x12301F), accent: Rgb::hex(0xE0E7F1), pattern: ShoulderStripe, book: Book::Zone, seed: 0xA7F1_001F, white_home: false, level: 5 },
        Club { city: "Salt Lake", name: "Summit", abbr: "SLC", division: Division::West, primary: Rgb::hex(0x4A3A6B), secondary: Rgb::hex(0x2C2244), accent: Rgb::hex(0xF2F2EC), pattern: Hoops, book: Book::Zone, seed: 0xA7F1_0020, white_home: false, level: 7 },
    ]
}

/// The league, rosters and all, in the order the clubs are listed.
pub fn league_teams() -> Vec<Team> {
    clubs().into_iter().map(team_from_spec).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_club_is_rated_on_the_player_scale() {
        for club in clubs() {
            let (ratings, stars) = club.profile(3);
            for v in [ratings.overall, ratings.offense, ratings.defense] {
                assert!((40..=99).contains(&v), "{} rated {v}", club.abbr);
            }
            assert_eq!(stars.len(), 3, "{} named {} men", club.abbr, stars.len());
            for (pos, name, rating) in &stars {
                assert!(!pos.abbr().is_empty());
                assert!(!name.is_empty());
                assert!((40..=99).contains(rating), "{} rated {rating}", club.abbr);
            }
            // The best man on the card is at least as good as the side he plays
            // on: a club whose star is below its own average is mis-rated.
            assert!(
                stars[0].2 >= ratings.offense.min(ratings.defense),
                "{}: star {} under the side's {}",
                club.abbr,
                stars[0].2,
                ratings.offense.min(ratings.defense)
            );
        }
    }

    #[test]
    fn ratings_separate_the_clubs() {
        let mut overalls: Vec<u8> = league_teams().iter().map(|t| t.ratings().overall).collect();
        overalls.sort_unstable();
        overalls.dedup();
        assert!(
            overalls.len() >= 6,
            "sixteen clubs collapsed into {} ratings: {overalls:?}",
            overalls.len()
        );
    }

    #[test]
    fn rosters_are_legal_and_unique() {
        let teams = league_teams();
        assert_eq!(teams.len(), 32);
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

    /// Dallas and Miami wear white at home and their colour on the road; every
    /// other club is the other way round.
    #[test]
    fn two_cities_wear_white_at_home() {
        let all = clubs();
        let white = |c: Rgb| c.luma() > 200.0;
        let mut white_homes = Vec::new();
        for club in &all {
            let (home, away) = club_kits(club);
            assert_eq!(
                white(home.jersey),
                club.white_home,
                "{} is kitted out the wrong way round",
                club.abbr
            );
            // One shirt is the colour and the other is the white, whichever
            // way round the club counts them.
            assert_ne!(white(home.jersey), white(away.jersey), "{}", club.abbr);
            if club.white_home {
                white_homes.push(club.abbr);
            }
        }
        white_homes.sort_unstable();
        assert_eq!(white_homes, ["DAL", "MIA"]);
        // The team itself, not just the menu's copy of the kits.
        let dal = all.iter().find(|c| c.abbr == "DAL").expect("Dallas");
        let team = team_from_spec(dal.clone());
        assert!(white(team.home.jersey), "Dallas does not wear white at home");
        assert!(team.worn.is_none(), "a fresh team already has a shirt on");
    }

    #[test]
        fn the_league_is_thirty_two_clubs_in_four_divisions() {
        let all = clubs();
        assert_eq!(all.len(), 32);
        for d in Division::ALL {
            let n = all.iter().filter(|c| c.division == d).count();
            assert_eq!(n, 8, "{} has {n} clubs", d.label());
        }
        let mut marks: Vec<&str> = all.iter().map(|c| c.abbr).collect();
        marks.sort_unstable();
        marks.dedup();
        assert_eq!(marks.len(), 32, "two clubs share a mark");
        let mut names: Vec<String> = all.iter().map(|c| c.full_name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 32, "two clubs share a name");
        // The menu picks by index, so the built league has to be in the same
        // order as the club list it draws.
        let teams = league_teams();
        assert_eq!(teams.len(), all.len());
        for (i, club) in all.iter().enumerate() {
            assert_eq!(teams[i].full_name(), club.full_name(), "club {i} is out of order");
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
