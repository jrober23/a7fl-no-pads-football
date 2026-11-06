//! The playbook the player designs: formations drawn as circles, fronts drawn as
//! X's, and the plays and calls that hang off them.
//!
//! The designs are kept as the player draws them, on a yard grid behind and ahead
//! of the line of scrimmage, and are turned into the [`Play`] and [`DefCall`] the
//! simulation runs only when a game asks for them. That means the editor can hold
//! a half-finished book without the league ever noticing, and the moment the book
//! is legal it is the book a club can take into a game.
//!
//! The line rules are enforced here rather than in the screen: two linemen on the
//! line of scrimmage and the quarterback behind them, which is the shape every
//! play in the league already has.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::math::Vec2;
use crate::playbook::{
    Assign, Book, Coverage, DefCall, DefFront, DefRole, Formation, OffRole, Play, PlayFamily, RunFit,
};

/// The shape a designed book has to be: two to five of each, one to nine under
/// each of them.
pub const MIN_FORMATIONS: usize = 2;
pub const MAX_FORMATIONS: usize = 5;
pub const MIN_PLAYS: usize = 1;
pub const MAX_PLAYS: usize = 9;

/// The grid the editor draws on, in yards from the line of scrimmage. `x` grows
/// toward the defense and `y` across the field. A spot is always a whole yard, so
/// a hand-drawn formation can be read back as a list of numbers.
pub const GRID_X: (i8, i8) = (-8, 12);
pub const GRID_Y: (i8, i8) = (-13, 13);

/// The four men a route can be drawn for, in the order their paths are kept.
pub const RECEIVERS: [OffRole; 4] = [OffRole::Back, OffRole::Slot, OffRole::WideL, OffRole::WideR];

pub fn route_slot(role: OffRole) -> Option<usize> {
    RECEIVERS.iter().position(|r| *r == role)
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignFormation {
    pub name: String,
    /// One spot a man, indexed by [`OffRole::index`].
    pub spots: [Vec2; 7],
}

impl DesignFormation {
    /// A formation laid out from one the league already runs: a starting point,
    /// not a finished design.
    pub fn from_play(p: &Play) -> DesignFormation {
        let mut spots = [Vec2::new(0.0, 0.0); 7];
        for role in OffRole::ALL {
            spots[role.index()] = grid(p.alignment(role));
        }
        DesignFormation {
            name: p.formation.label().to_string(),
            spots,
        }
    }

    pub fn blank(index: usize, template: Formation) -> DesignFormation {
        let mut spots = [Vec2::new(0.0, 0.0); 7];
        for role in OffRole::ALL {
            spots[role.index()] = grid(template.alignment(role));
        }
        DesignFormation {
            name: format!("FORMATION {}", index + 1),
            spots,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignPlay {
    pub name: String,
    pub family: PlayFamily,
    /// The drawn path for each of [`RECEIVERS`], in yards from the line of
    /// scrimmage. Empty means he stays in and blocks.
    pub routes: [Vec<Vec2>; 4],
}

impl DesignPlay {
    pub fn blank(name: String) -> DesignPlay {
        DesignPlay {
            name,
            family: PlayFamily::QuickPass,
            routes: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        }
    }

    /// A play copied out of a book the league already runs, with its waypoints
    /// moved from the receiver's shoulders onto the field.
    pub fn from_play(p: &Play, form: &DesignFormation) -> DesignPlay {
        let mut routes: [Vec<Vec2>; 4] = Default::default();
        for role in RECEIVERS {
            let Some(slot) = route_slot(role) else { continue };
            let Some(points) = p.assignment(role).and_then(assign_points) else {
                continue;
            };
            let spot = form.spots[role.index()];
            routes[slot] = points
                .iter()
                .map(|q| grid(Vec2::new(spot.x + q.x, spot.y + q.y)))
                .collect();
        }
        DesignPlay {
            name: p.name.to_string(),
            family: p.family,
            routes,
        }
    }

    pub fn has_path(&self) -> bool {
        self.routes.iter().any(|r| !r.is_empty())
    }
}

fn assign_points(a: &Assign) -> Option<Vec<Vec2>> {
    match a {
        Assign::Route { points, .. } => Some(points.clone()),
        Assign::Fake { points, .. } => Some(points.clone()),
        Assign::Carry { points } => Some(points.clone()),
        Assign::QbCarry { points } => Some(points.clone()),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignFront {
    pub name: String,
    /// One spot a defender, indexed by [`DefRole::index`].
    pub spots: [Vec2; 7],
}

impl DesignFront {
    pub fn from_call(call: &DefCall) -> DesignFront {
        let mut spots = [Vec2::new(0.0, 0.0); 7];
        for (role, spot) in crate::playbook::defender_positions(call, 0.0, 1) {
            spots[role.index()] = grid(spot);
        }
        DesignFront {
            name: call.front.label().to_string(),
            spots,
        }
    }

    pub fn blank(index: usize) -> DesignFront {
        DesignFront {
            name: format!("FRONT {}", index + 1),
            spots: [
                Vec2::new(1.0, -2.0),
                Vec2::new(1.0, 2.0),
                Vec2::new(3.0, 0.0),
                Vec2::new(4.0, 3.0),
                Vec2::new(6.0, -10.0),
                Vec2::new(6.0, 10.0),
                Vec2::new(10.0, 0.0),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignCall {
    pub name: String,
    pub coverage: Coverage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignBook {
    pub name: String,
    /// The book it was started from, for the screen and for the save file.
    pub source: String,
    pub offense: Vec<DesignFormation>,
    /// The plays under each formation: `plays[i]` belongs to `offense[i]`.
    pub plays: Vec<Vec<DesignPlay>>,
    pub defense: Vec<DesignFront>,
    /// The calls under each front: `calls[i]` belongs to `defense[i]`.
    pub calls: Vec<Vec<DesignCall>>,
}

impl DesignBook {
    /// A sheet with the least a book can have: two formations, two fronts, and
    /// a play under each that the player draws himself.
    pub fn blank() -> DesignBook {
        let offense = vec![
            DesignFormation::blank(0, Formation::Pistol),
            DesignFormation::blank(1, Formation::Bone),
        ];
        let plays = offense
            .iter()
            .map(|f| vec![DesignPlay::blank(format!("{} 1", f.name))])
            .collect();
        let defense = vec![DesignFront::blank(0), DesignFront::blank(1)];
        let calls = defense
            .iter()
            .map(|_| vec![DesignCall { name: "COVER 1".to_string(), coverage: Coverage::ManFree }])
            .collect();
        DesignBook {
            name: "MY PLAYBOOK".to_string(),
            source: "SCRATCH".to_string(),
            offense,
            plays,
            defense,
            calls,
        }
    }

    /// A copy of a book the league already runs, trimmed to what a designed book
    /// is allowed to hold.
    pub fn from_built_in(source: Book) -> DesignBook {
        let mut offense: Vec<DesignFormation> = Vec::new();
        let mut plays: Vec<Vec<DesignPlay>> = Vec::new();
        for p in crate::playbook::playbook_for(source) {
            let name = p.formation.label().to_string();
            let i = match offense.iter().position(|f| f.name == name) {
                Some(i) => i,
                None => {
                    if offense.len() >= MAX_FORMATIONS {
                        continue;
                    }
                    offense.push(DesignFormation::from_play(&p));
                    plays.push(Vec::new());
                    offense.len() - 1
                }
            };
            if plays[i].len() >= MAX_PLAYS {
                continue;
            }
            plays[i].push(DesignPlay::from_play(&p, &offense[i]));
        }

        let mut defense: Vec<DesignFront> = Vec::new();
        let mut calls: Vec<Vec<DesignCall>> = Vec::new();
        for d in crate::playbook::defensive_playbook_for(source) {
            let name = d.front.label().to_string();
            let i = match defense.iter().position(|f| f.name == name) {
                Some(i) => i,
                None => {
                    if defense.len() >= MAX_FORMATIONS {
                        continue;
                    }
                    defense.push(DesignFront::from_call(&d));
                    calls.push(Vec::new());
                    defense.len() - 1
                }
            };
            if calls[i].len() >= MAX_PLAYS {
                continue;
            }
            calls[i].push(DesignCall {
                name: d.name.to_string(),
                coverage: d.coverage,
            });
        }

        // A book that came from a small scheme may not have a second formation or
        // front to its name. The blank sheet fills the gap so the counts the
        // editor enforces are the counts it starts with.
        while offense.len() < MIN_FORMATIONS {
            let f = DesignFormation::blank(offense.len(), Formation::Pistol);
            plays.push(vec![DesignPlay::blank(format!("{} 1", f.name))]);
            offense.push(f);
        }
        while defense.len() < MIN_FORMATIONS {
            let f = DesignFront::blank(defense.len());
            calls.push(vec![DesignCall { name: "COVER 1".to_string(), coverage: Coverage::ManFree }]);
            defense.push(f);
        }
        DesignBook {
            name: format!("{} BOOK", source.label()),
            source: source.label().to_string(),
            offense,
            plays,
            defense,
            calls,
        }
    }

    /// Everything that stops this book being run, in the order the editor would
    /// ask the player to fix it. An empty list means a club can take it into a
    /// game.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.offense.len() < MIN_FORMATIONS || self.offense.len() > MAX_FORMATIONS {
            out.push(format!(
                "OFFENSE NEEDS {} TO {} FORMATIONS",
                MIN_FORMATIONS, MAX_FORMATIONS
            ));
        }
        if self.defense.len() < MIN_FORMATIONS || self.defense.len() > MAX_FORMATIONS {
            out.push(format!(
                "DEFENSE NEEDS {} TO {} FRONTS",
                MIN_FORMATIONS, MAX_FORMATIONS
            ));
        }
        for (i, f) in self.offense.iter().enumerate() {
            if let Some(why) = formation_problem(&f.spots) {
                out.push(format!("{}: {}", f.name, why));
            }
            let plays = self.plays.get(i).map(|p| p.as_slice()).unwrap_or(&[]);
            if plays.len() < MIN_PLAYS || plays.len() > MAX_PLAYS {
                out.push(format!(
                    "{} NEEDS {} TO {} PLAYS",
                    f.name, MIN_PLAYS, MAX_PLAYS
                ));
            }
            for p in plays {
                if !p.has_path() {
                    out.push(format!("{}: EVERY PLAY NEEDS A DRAWN PATH", p.name));
                }
            }
        }
        for (i, f) in self.defense.iter().enumerate() {
            if let Some(why) = front_problem(&f.spots) {
                out.push(format!("{}: {}", f.name, why));
            }
            let calls = self.calls.get(i).map(|c| c.as_slice()).unwrap_or(&[]);
            if calls.len() < MIN_PLAYS || calls.len() > MAX_PLAYS {
                out.push(format!(
                    "{} NEEDS {} TO {} CALLS",
                    f.name, MIN_PLAYS, MAX_PLAYS
                ));
            }
        }
        out
    }

    pub fn is_legal(&self) -> bool {
        self.problems().is_empty()
    }

    /// The plays of one formation, kept the same length as the formation list.
    pub fn plays_of(&self, formation: usize) -> &[DesignPlay] {
        self.plays.get(formation).map(|p| p.as_slice()).unwrap_or(&[])
    }

    pub fn calls_of(&self, front: usize) -> &[DesignCall] {
        self.calls.get(front).map(|c| c.as_slice()).unwrap_or(&[])
    }
}

/// The two rules of an offensive formation, plus the one thing that makes a
/// drawing unreadable: two men standing on the same spot.
pub fn formation_problem(spots: &[Vec2; 7]) -> Option<&'static str> {
    let qb = spots[OffRole::Qb.index()];
    let lg = spots[OffRole::Lg.index()];
    let rg = spots[OffRole::Rg.index()];
    if lg.x != 0.0 || rg.x != 0.0 {
        return Some("TWO LINEMEN MUST BE ON THE LINE OF SCRIMMAGE");
    }
    if qb.x >= 0.0 {
        return Some("THE QUARTERBACK MUST BE BEHIND THE LINE");
    }
    if !inside(spots, false) {
        return Some("EVERY MAN HAS TO BE ON THE FIELD");
    }
    if crowded(spots) {
        return Some("TWO MEN CANNOT STAND ON ONE SPOT");
    }
    None
}

/// A front has no line rule of its own; it only has to be seven men in seven
/// places, on the field.
pub fn front_problem(spots: &[Vec2; 7]) -> Option<&'static str> {
    if !inside(spots, true) {
        return Some("EVERY MAN HAS TO BE ON THE FIELD");
    }
    if crowded(spots) {
        return Some("TWO MEN CANNOT STAND ON ONE SPOT");
    }
    None
}

fn inside(spots: &[Vec2; 7], defense: bool) -> bool {
    let (x0, x1) = if defense { (0, GRID_X.1) } else { GRID_X };
    spots.iter().all(|s| {
        s.x >= x0 as f32 && s.x <= x1 as f32 && s.y >= GRID_Y.0 as f32 && s.y <= GRID_Y.1 as f32
    })
}

fn crowded(spots: &[Vec2; 7]) -> bool {
    for i in 0..spots.len() {
        for j in (i + 1)..spots.len() {
            if spots[i] == spots[j] {
                return true;
            }
        }
    }
    false
}

/// A spot rounded onto the grid the editor draws, so a copied design can be
/// moved by hand afterwards.
fn grid(v: Vec2) -> Vec2 {
    let x = v.x.round().clamp(GRID_X.0 as f32, GRID_X.1 as f32);
    let y = v.y.round().clamp(GRID_Y.0 as f32, GRID_Y.1 as f32);
    Vec2::new(x, y)
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

fn store() -> &'static Mutex<Option<DesignBook>> {
    static BOOK: OnceLock<Mutex<Option<DesignBook>>> = OnceLock::new();
    BOOK.get_or_init(|| Mutex::new(None))
}

fn labels() -> &'static Mutex<(Vec<&'static str>, Vec<&'static str>)> {
    static LABELS: OnceLock<Mutex<(Vec<&'static str>, Vec<&'static str>)>> = OnceLock::new();
    LABELS.get_or_init(|| Mutex::new((Vec::new(), Vec::new())))
}

fn generation() -> u64 {
    static GEN: AtomicU64 = AtomicU64::new(0);
    GEN.load(Ordering::Relaxed)
}

fn bump() {
    static GEN: AtomicU64 = AtomicU64::new(0);
    GEN.fetch_add(1, Ordering::Relaxed);
}

pub fn book() -> Option<DesignBook> {
    store().lock().ok().and_then(|b| b.clone())
}

/// True when there is a book and it can be run: what the pickers ask before they
/// offer it.
pub fn has_book() -> bool {
    book().map(|b| b.is_legal()).unwrap_or(false)
}

/// Keep a book. The labels the playbook hands to the rest of the game are
/// settled here, against the names as they were when the book was saved.
pub fn set(book: Option<DesignBook>) {
    let names: Vec<&'static str> = book
        .as_ref()
        .map(|b| b.offense.iter().map(|f| leak(&f.name)).collect())
        .unwrap_or_default();
    let fronts: Vec<&'static str> = book
        .as_ref()
        .map(|b| b.defense.iter().map(|f| leak(&f.name)).collect())
        .unwrap_or_default();
    if let Ok(mut l) = labels().lock() {
        *l = (names, fronts);
    }
    if let Ok(mut b) = store().lock() {
        *b = book;
    }
    bump();
}

pub fn clear() {
    set(None);
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// The name of a designed formation, for the playbook card and the diagrams.
pub fn formation_name(id: u8) -> &'static str {
    labels()
        .lock()
        .ok()
        .and_then(|l| l.0.get(id as usize).copied())
        .unwrap_or("CUSTOM")
}

pub fn front_name(id: u8) -> &'static str {
    labels()
        .lock()
        .ok()
        .and_then(|l| l.1.get(id as usize).copied())
        .unwrap_or("CUSTOM")
}

/// Where a designed formation stands one man, relative to the line of scrimmage.
pub fn formation_spot(id: u8, role: OffRole) -> Vec2 {
    book()
        .and_then(|b| b.offense.get(id as usize).map(|f| f.spots[role.index()]))
        .unwrap_or(Vec2::new(0.0, 0.0))
}

/// Where a designed front stands one man, in field coordinates.
pub fn front_spot(id: u8, role: DefRole, los: f32, direction: i8) -> Vec2 {
    let s = book()
        .and_then(|b| b.defense.get(id as usize).map(|f| f.spots[role.index()]))
        .unwrap_or(Vec2::new(0.0, 0.0));
    Vec2::new(los + direction as f32 * s.x, s.y)
}

// ---------------------------------------------------------------------------
// Turning designs into the game's plays
// ---------------------------------------------------------------------------

/// The designed offense, as plays the simulation runs.
pub fn offense_plays() -> Vec<Play> {
    let Some(book) = book() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (fi, form) in book.offense.iter().enumerate() {
        let form_name = leak(&form.name);
        for (pi, p) in book.plays_of(fi).iter().enumerate() {
            let name = leak(&format!("{} {}", form.name, pi + 1));
            out.push(design_play(name, form_name, fi, form, p));
        }
    }
    out
}

fn design_play(name: &'static str, concept: &'static str, fi: usize, form: &DesignFormation, p: &DesignPlay) -> Play {
    let pass = p.family.is_pass();
    let drop = match p.family {
        PlayFamily::QuickPass | PlayFamily::Screen => 3.0,
        PlayFamily::DropbackPass | PlayFamily::PlayAction => 5.0,
        _ => 0.0,
    };
    let mut assignments: Vec<(OffRole, Assign)> = Vec::new();
    assignments.push((
        OffRole::Qb,
        Assign::PassBlock {
            slide: 0.0,
            max_depth: if pass { drop } else { 1.0 },
        },
    ));
    for role in [OffRole::Lg, OffRole::Rg] {
        assignments.push((
            role,
            if pass {
                Assign::PassBlock { slide: 0.0, max_depth: -1.0 }
            } else {
                Assign::RunBlock {
                    aim: Vec2::new(2.0, if role == OffRole::Lg { -1.0 } else { 1.0 }),
                    seal: false,
                }
            },
        ));
    }
    for role in RECEIVERS {
        let Some(slot) = route_slot(role) else { continue };
        let spot = form.spots[role.index()];
        let drawn = &p.routes[slot];
        if drawn.is_empty() {
            assignments.push((
                role,
                Assign::PassBlock { slide: 0.0, max_depth: -1.0 },
            ));
            continue;
        }
        let rel: Vec<Vec2> = drawn
            .iter()
            .map(|q| Vec2::new(q.x - spot.x, q.y - spot.y))
            .collect();
        if pass {
            let depth = rel
                .iter()
                .map(|q| q.x)
                .fold(0.0f32, f32::max)
                .max(0.0) as u8;
            assignments.push((role, Assign::Route { points: rel, depth }));
        } else if role == OffRole::Back {
            assignments.push((role, Assign::Carry { points: rel }));
        } else {
            assignments.push((
                role,
                Assign::RunBlock {
                    aim: rel.first().copied().unwrap_or(Vec2::new(1.0, 0.0)),
                    seal: false,
                },
            ));
        }
    }
    Play {
        name,
        concept,
        family: p.family,
        formation: Formation::Custom(fi as u8),
        drop,
        run_side: 0,
        flip: false,
        assignments,
    }
}

/// The designed defense, as calls the simulation runs.
pub fn defense_calls() -> Vec<DefCall> {
    let Some(book) = book() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (fi, _) in book.defense.iter().enumerate() {
        for c in book.calls_of(fi) {
            out.push(DefCall {
                name: leak(&c.name),
                front: DefFront::Custom(fi as u8),
                coverage: c.coverage,
                run_fit: RunFit::Balanced,
                blitzers: Vec::new(),
                press: c.coverage == Coverage::ManPress,
            });
        }
    }
    out
}

/// The AI's own copy of the designed offense, rebuilt when the book changes.
pub fn cached_offense() -> &'static [Play] {
    static CACHE: Mutex<Option<(u64, &'static [Play])>> = Mutex::new(None);
    let now = generation();
    if let Ok(mut c) = CACHE.lock() {
        if let Some((g, b)) = *c {
            if g == now {
                return b;
            }
        }
        let mut plays = offense_plays();
        if plays.is_empty() {
            // A club saved on this book before the book was cleared still has to
            // call something: the league's own plays are the floor.
            plays = crate::playbook::playbook();
        }
        let fresh: &'static [Play] = Box::leak(plays.into_boxed_slice());
        *c = Some((now, fresh));
        fresh
    } else {
        Box::leak(offense_plays().into_boxed_slice())
    }
}

pub fn cached_defense() -> &'static [DefCall] {
    static CACHE: Mutex<Option<(u64, &'static [DefCall])>> = Mutex::new(None);
    let now = generation();
    if let Ok(mut c) = CACHE.lock() {
        if let Some((g, b)) = *c {
            if g == now {
                return b;
            }
        }
        let mut calls = defense_calls();
        if calls.is_empty() {
            calls = crate::playbook::defensive_playbook();
        }
        let fresh: &'static [DefCall] = Box::leak(calls.into_boxed_slice());
        *c = Some((now, fresh));
        fresh
    } else {
        Box::leak(defense_calls().into_boxed_slice())
    }
}

// ---------------------------------------------------------------------------
// The save file
// ---------------------------------------------------------------------------

fn family_name(f: PlayFamily) -> &'static str {
    match f {
        PlayFamily::RunInside => "RunInside",
        PlayFamily::RunOutside => "RunOutside",
        PlayFamily::QbRun => "QbRun",
        PlayFamily::QuickPass => "QuickPass",
        PlayFamily::DropbackPass => "DropbackPass",
        PlayFamily::PlayAction => "PlayAction",
        PlayFamily::Screen => "Screen",
    }
}

fn parse_family(s: &str) -> Option<PlayFamily> {
    Some(match s {
        "RunInside" => PlayFamily::RunInside,
        "RunOutside" => PlayFamily::RunOutside,
        "QbRun" => PlayFamily::QbRun,
        "QuickPass" => PlayFamily::QuickPass,
        "DropbackPass" => PlayFamily::DropbackPass,
        "PlayAction" => PlayFamily::PlayAction,
        "Screen" => PlayFamily::Screen,
        _ => return None,
    })
}

fn coverage_name(c: Coverage) -> &'static str {
    match c {
        Coverage::ManFree => "ManFree",
        Coverage::ManPress => "ManPress",
        Coverage::Cover2 => "Cover2",
        Coverage::Cover3 => "Cover3",
        Coverage::Cover4 => "Cover4",
        Coverage::GoalLine => "GoalLine",
    }
}

fn parse_coverage(s: &str) -> Option<Coverage> {
    Some(match s {
        "ManFree" => Coverage::ManFree,
        "ManPress" => Coverage::ManPress,
        "Cover2" => Coverage::Cover2,
        "Cover3" => Coverage::Cover3,
        "Cover4" => Coverage::Cover4,
        "GoalLine" => Coverage::GoalLine,
        _ => return None,
    })
}

fn word(s: &str) -> String {
    s.replace(' ', "_")
}

fn unword(s: &str) -> String {
    s.replace('_', " ")
}

fn spot_text(p: Vec2) -> String {
    format!("{},{}", p.x as i32, p.y as i32)
}

fn parse_spot(s: &str) -> Option<Vec2> {
    let (x, y) = s.split_once(',')?;
    Some(Vec2::new(x.parse::<i32>().ok()? as f32, y.parse::<i32>().ok()? as f32))
}

/// The designed book as lines for the save file, after the clubs and kits.
pub fn text() -> String {
    let Some(book) = book() else {
        return String::new();
    };
    let mut out = String::from("# The playbook: formations, plays, fronts and calls.\n");
    out.push_str(&format!("book {} {}\n", word(&book.source), word(&book.name)));
    for (i, f) in book.offense.iter().enumerate() {
        let spots: Vec<String> = f.spots.iter().map(|s| spot_text(*s)).collect();
        out.push_str(&format!("off {} {} {}\n", i, word(&f.name), spots.join(" ")));
    }
    for (fi, plays) in book.plays.iter().enumerate() {
        for p in plays {
            // A semicolon between waypoints and a pipe between men: neither
            // can be a space, or the line would split inside a route.
            let mut parts = Vec::new();
            for route in &p.routes {
                parts.push(route.iter().map(|q| spot_text(*q)).collect::<Vec<_>>().join(";"));
            }
            out.push_str(&format!(
                "play {} {} {} {}\n",
                fi,
                word(&p.name),
                family_name(p.family),
                parts.join("|")
            ));
        }
    }
    for (i, f) in book.defense.iter().enumerate() {
        let spots: Vec<String> = f.spots.iter().map(|s| spot_text(*s)).collect();
        out.push_str(&format!("front {} {} {}\n", i, word(&f.name), spots.join(" ")));
    }
    for (fi, calls) in book.calls.iter().enumerate() {
        for c in calls {
            out.push_str(&format!(
                "call {} {} {}\n",
                fi,
                word(&c.name),
                coverage_name(c.coverage)
            ));
        }
    }
    out
}

/// Read the designed book back. A line that does not parse is skipped, on the
/// same rule as the rest of the save file: nothing a player typed by hand should
/// stop the game starting.
pub fn parse(text: &str) {
    let mut book: Option<DesignBook> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("book") => {
                let (Some(source), Some(name)) = (parts.next(), parts.next()) else {
                    continue;
                };
                book = Some(DesignBook {
                    name: unword(name),
                    source: unword(source),
                    offense: Vec::new(),
                    plays: Vec::new(),
                    defense: Vec::new(),
                    calls: Vec::new(),
                });
            }
            Some("off") => {
                let Some(b) = book.as_mut() else { continue };
                let (Some(_i), Some(name)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let spots: Vec<Vec2> = parts.filter_map(parse_spot).collect();
                if spots.len() != 7 {
                    continue;
                }
                let mut arr = [Vec2::new(0.0, 0.0); 7];
                arr.copy_from_slice(&spots);
                b.offense.push(DesignFormation {
                    name: unword(name),
                    spots: arr,
                });
                b.plays.push(Vec::new());
            }
            Some("front") => {
                let Some(b) = book.as_mut() else { continue };
                let (Some(_i), Some(name)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let spots: Vec<Vec2> = parts.filter_map(parse_spot).collect();
                if spots.len() != 7 {
                    continue;
                }
                let mut arr = [Vec2::new(0.0, 0.0); 7];
                arr.copy_from_slice(&spots);
                b.defense.push(DesignFront {
                    name: unword(name),
                    spots: arr,
                });
                b.calls.push(Vec::new());
            }
            Some("play") => {
                let Some(b) = book.as_mut() else { continue };
                let (Some(fi), Some(name), Some(fam)) = (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let (Some(fi), Some(family)) = (fi.parse::<usize>().ok(), parse_family(fam)) else {
                    continue;
                };
                let Some(routes) = parts.next() else { continue };
                let mut drawn: [Vec<Vec2>; 4] = Default::default();
                for (slot, lane) in routes.split('|').enumerate().take(4) {
                    drawn[slot] = lane.split(';').filter_map(parse_spot).collect();
                }
                if let Some(list) = b.plays.get_mut(fi) {
                    if list.len() < MAX_PLAYS {
                        list.push(DesignPlay {
                            name: unword(name),
                            family,
                            routes: drawn,
                        });
                    }
                }
            }
            Some("call") => {
                let Some(b) = book.as_mut() else { continue };
                let (Some(fi), Some(name), Some(cov)) = (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let (Some(fi), Some(coverage)) = (fi.parse::<usize>().ok(), parse_coverage(cov))
                else {
                    continue;
                };
                if let Some(list) = b.calls.get_mut(fi) {
                    if list.len() < MAX_PLAYS {
                        list.push(DesignCall {
                            name: unword(name),
                            coverage,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(book) = book {
        set(Some(book));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn store_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_book_copied_from_a_league_book_is_legal_once_its_plays_are_drawn() {
        let _store = store_lock();
        clear();
        let mut book = DesignBook::from_built_in(Book::WestCoast);
        assert!(book.offense.len() >= MIN_FORMATIONS);
        assert!(book.offense.len() <= MAX_FORMATIONS);
        assert!(book.defense.len() >= MIN_FORMATIONS);
        // The copied plays carry their routes, so the book is runnable as it
        // arrives: no drawing needed before a club can use it.
        assert!(book.problems().is_empty(), "{:?}", book.problems());
        assert!(book.plays.iter().all(|p| !p.is_empty()));
        // Every formation has the line the rulebook asks for.
        for f in &book.offense {
            assert_eq!(f.spots[OffRole::Lg.index()].x, 0.0);
            assert_eq!(f.spots[OffRole::Rg.index()].x, 0.0);
            assert!(f.spots[OffRole::Qb.index()].x < 0.0);
        }
        // ...and the plays it makes are plays the simulation can run.
        set(Some(book.clone()));
        let plays = offense_plays();
        assert!(!plays.is_empty());
        assert!(plays.iter().all(|p| p.assignments.len() == 7));
        assert!(plays.iter().any(|p| matches!(p.assignment(OffRole::WideL), Some(Assign::Route { .. }))));
        clear();
    }

    #[test]
    fn a_formation_without_two_men_on_the_line_is_refused() {
        let mut f = DesignFormation::blank(0, Formation::Pistol);
        assert!(formation_problem(&f.spots).is_none());
        // Slide a guard off the line: the book now breaks the rule of thumb.
        f.spots[OffRole::Lg.index()] = Vec2::new(-1.0, -1.0);
        assert_eq!(
            formation_problem(&f.spots),
            Some("TWO LINEMEN MUST BE ON THE LINE OF SCRIMMAGE")
        );
        let mut g = DesignFormation::blank(0, Formation::Pistol);
        g.spots[OffRole::Qb.index()] = Vec2::new(2.0, 0.0);
        assert_eq!(
            formation_problem(&g.spots),
            Some("THE QUARTERBACK MUST BE BEHIND THE LINE")
        );
    }

    #[test]
    fn the_shapes_the_editor_enforces_are_the_shapes_it_saves() {
        let _store = store_lock();
        clear();
        let mut full = DesignBook::from_built_in(Book::Balanced);
        // Trim to the limit the editor allows and save it.
        full.offense.truncate(MAX_FORMATIONS);
        full.plays.truncate(MAX_FORMATIONS);
        full.defense.truncate(MAX_FORMATIONS);
        full.calls.truncate(MAX_FORMATIONS);
        assert!(full.problems().is_empty(), "{:?}", full.problems());
        set(Some(full.clone()));

        let written = text();
        clear();
        assert!(book().is_none());
        parse(&written);

        let back = book().expect("the book came back empty");
        assert_eq!(back.offense.len(), full.offense.len());
        assert_eq!(back.defense.len(), full.defense.len());
        assert_eq!(back.offense[0].name, full.offense[0].name);
        assert_eq!(back.offense[0].spots, full.offense[0].spots);
        assert_eq!(back.plays[0].len(), full.plays[0].len());
        assert_eq!(back.calls[0].len(), full.calls[0].len());
        assert_eq!(back.plays[0][0].routes, full.plays[0][0].routes);
        assert!(has_book());
        clear();
    }

    #[test]
    fn a_designed_play_stands_where_it_was_drawn_and_is_not_in_a_built_formation() {
        let _store = store_lock();
        clear();
        let mut book = DesignBook::blank();
        // Draw the first play of the first formation: the back runs a flat, the
        // slot goes deep.
        let spots = book.offense[0].spots;
        book.plays[0][0].routes[0] = vec![Vec2::new(1.0, 3.0), Vec2::new(3.0, 6.0)];
        book.plays[0][0].routes[1] = vec![Vec2::new(6.0, 4.0), Vec2::new(14.0, 4.0)];
        book.plays[1][0].routes[0] = vec![Vec2::new(4.0, 0.0)];
        set(Some(book));

        let plays = offense_plays();
        let p = &plays[0];
        assert_eq!(p.formation, Formation::Custom(0));
        assert_eq!(p.alignment(OffRole::Qb), spots[OffRole::Qb.index()]);
        assert_eq!(p.alignment(OffRole::WideL), spots[OffRole::WideL.index()]);
        let route = match p.assignment(OffRole::Slot) {
            Some(Assign::Route { points, .. }) => points.clone(),
            other => panic!("the drawn route did not become a route: {other:?}"),
        };
        // The waypoints are relative to the slot's shoulders, not the line.
        let spot = spots[OffRole::Slot.index()];
        assert_eq!(route[1], Vec2::new(14.0 - spot.x, 4.0 - spot.y));
        assert_eq!(p.formation.label(), "FORMATION 1");
        clear();
    }

    #[test]
    fn a_count_outside_the_limits_is_a_problem() {
        let mut book = DesignBook::blank();
        book.offense.truncate(1);
        book.plays.truncate(1);
        let problems = book.problems();
        assert!(problems.iter().any(|p| p.contains("OFFENSE NEEDS")));
        let book = DesignBook::blank();
        for f in &book.offense {
            assert_eq!(f.spots[OffRole::Lg.index()].x, 0.0);
        }
    }
}
