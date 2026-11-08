//! The playbooks the player designs: formations drawn as circles, fronts drawn as
//! X's, the plays and calls that hang off them, and the coverage drawn behind each
//! call.
//!
//! A player keeps a shelf of books, each with its own name, and every book is
//! seven men on a yard grid with a job written on each spot. The designs are kept
//! as they are drawn and are turned into the [`Play`] and [`DefCall`] the
//! simulation runs only when a game asks for them, so a half-finished book on the
//! shelf never bothers the league: the moment a book is legal it is a book a club
//! can carry into a game.
//!
//! The line rules are enforced here rather than in the screen: two linemen on the
//! line of scrimmage and the quarterback behind them, which is the shape every
//! play in the league already has.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::math::Vec2;
use crate::playbook::{
    Assign, Book, Coverage, DefCall, DefFront, DefRole, Formation, OffRole, Play, PlayFamily, RunFit,
};

/// How many books the shelf holds. Each one gets a slot on the pad and a name.
pub const MAX_BOOKS: usize = 6;

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

/// A formation or front is named to the game by one number: the book it belongs
/// to in the high bits and its place in the book in the low three. Eight
/// formations a book is more than the five the editor allows, so the packing has
/// room to spare and the number fits a `u8` the same way a `Custom` id always has.
pub fn form_id(book: usize, formation: usize) -> u8 {
    ((book * 8) + formation) as u8
}

pub fn front_id(book: usize, front: usize) -> u8 {
    ((book * 8) + front) as u8
}

pub fn id_book(id: u8) -> usize {
    id as usize / 8
}

pub fn id_slot(id: u8) -> usize {
    id as usize % 8
}

/// One formation: seven spots on the grid and the job standing on each of them.
///
/// The spots are indexed by where they are on the field, not by the job, so any
/// of the seven can stand anywhere: the player picks who stands on a spot rather
/// than being handed a quarterback's spot and a guard's spot.
#[derive(Clone, Debug, PartialEq)]
pub struct FormationDesign {
    pub name: String,
    pub spots: [Vec2; 7],
    pub roles: [OffRole; 7],
}

/// The old name for [`FormationDesign`], kept short for the screens.
pub type DesignFormation = FormationDesign;

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
            roles: OffRole::ALL,
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
            roles: OffRole::ALL,
        }
    }

    /// The spot a job stands on.
    pub fn alignment(&self, role: OffRole) -> Vec2 {
        self.spots[self.slot_of(role)]
    }

    /// Which spot a job stands on, or his own index if the roles are somehow
    /// not a set of seven.
    pub fn slot_of(&self, role: OffRole) -> usize {
        self.roles
            .iter()
            .position(|r| *r == role)
            .unwrap_or_else(|| role.index())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignPlay {
    pub name: String,
    pub family: PlayFamily,
    /// The drawn path for each spot, in yards from the line of scrimmage. Empty
    /// means the man on that spot stays in and blocks.
    pub routes: [Vec<Vec2>; 7],
    /// The spot whose man takes the ball on a run. Any spot can be pointed at,
    /// so the handoff goes to the man the player picked rather than to whichever
    /// job is called the back: a tight end standing in the backfield carries it
    /// the same way the back does. Ignored on a pass.
    pub carrier: usize,
}

impl DesignPlay {
    pub fn blank(name: String) -> DesignPlay {
        DesignPlay {
            name,
            family: PlayFamily::QuickPass,
            routes: Default::default(),
            carrier: OffRole::Back.index(),
        }
    }

    /// A play copied out of a book the league already runs, with its waypoints
    /// moved from the man's shoulders onto the field.
    pub fn from_play(p: &Play, form: &DesignFormation) -> DesignPlay {
        let mut routes: [Vec<Vec2>; 7] = Default::default();
        for role in OffRole::ALL {
            let Some(points) = p.assignment(role).and_then(assign_points) else {
                continue;
            };
            let spot = form.alignment(role);
            let slot = form.slot_of(role);
            routes[slot] = points
                .iter()
                .map(|q| grid(Vec2::new(spot.x + q.x, spot.y + q.y)))
                .collect();
        }
        // The man who carries in the league's play carries here too: a copied
        // jet sweep still goes to the tight end, and a copied draw to the
        // quarterback.
        let carrier = p
            .carrier_role()
            .map(|role| form.slot_of(role))
            .unwrap_or_else(|| form.slot_of(OffRole::Back));
        DesignPlay {
            name: p.name.to_string(),
            family: p.family,
            routes,
            carrier,
        }
    }

    pub fn has_path(&self) -> bool {
        self.routes.iter().any(|r| !r.is_empty())
    }

    pub fn path(&self, slot: usize) -> &[Vec2] {
        self.routes.get(slot).map(|r| r.as_slice()).unwrap_or(&[])
    }

    /// The job of the man who takes the ball, read through the spot the player
    /// pointed at rather than through a job on its own.
    pub fn carrier_role(&self, form: &DesignFormation) -> OffRole {
        form.roles.get(self.carrier).copied().unwrap_or(OffRole::Back)
    }

    /// True when the ball is going to a man on the line, which the rules do not
    /// allow: a lineman is neither a back nor an eligible receiver.
    pub fn carrier_is_line(&self, form: &DesignFormation) -> bool {
        self.carrier_role(form).is_line()
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

/// One front: seven spots and the job standing on each, the same way a formation
/// is built, plus the coverage each of them plays behind it.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignFront {
    pub name: String,
    pub spots: [Vec2; 7],
    pub roles: [DefRole; 7],
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
            roles: DefRole::ALL,
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
            roles: DefRole::ALL,
        }
    }

    pub fn spot(&self, role: DefRole) -> Vec2 {
        self.spots[self.slot_of(role)]
    }

    pub fn slot_of(&self, role: DefRole) -> usize {
        self.roles
            .iter()
            .position(|r| *r == role)
            .unwrap_or_else(|| role.index())
    }
}

/// One call off a front: the coverage the seven play, whether the corners press,
/// and the drop drawn for each of them.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignCall {
    pub name: String,
    pub coverage: Coverage,
    /// Corners at the line with their hands on the receiver, rather than off
    /// with a cushion.
    pub press: bool,
    /// The drop drawn for each spot of the front, in yards past the line of
    /// scrimmage. Empty means the coverage decides where he goes.
    pub drops: [Vec<Vec2>; 7],
}

impl DesignCall {
    pub fn blank(name: String, coverage: Coverage) -> DesignCall {
        DesignCall {
            name,
            coverage,
            press: coverage == Coverage::ManPress,
            drops: Default::default(),
        }
    }

    pub fn has_drop(&self) -> bool {
        self.drops.iter().any(|d| !d.is_empty())
    }
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
            .map(|_| vec![DesignCall::blank("COVER 1".to_string(), Coverage::ManFree)])
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
                press: d.press,
                drops: Default::default(),
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
            calls.push(vec![DesignCall::blank(
                "COVER 1".to_string(),
                Coverage::ManFree,
            )]);
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
        if self.name.trim().is_empty() {
            out.push("THE BOOK NEEDS A NAME".to_string());
        }
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
            if let Some(why) = formation_problem_roles(&f.spots, &f.roles) {
                out.push(format!("{}: {}", f.name, why));
            }
            if !roles_are_seven(f.roles.iter().map(|r| r.index())) {
                out.push(format!("{}: TWO MEN CANNOT PLAY ONE JOB", f.name));
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
                } else if p.family.is_run() && p.path(p.carrier).is_empty() {
                    // The paths are drawn, but not on the man taking the ball:
                    // the run would have nobody to hand it to.
                    out.push(format!(
                        "{}: THE MAN TAKING THE HANDOFF NEEDS A PATH",
                        p.name
                    ));
                }
                if p.family.is_run() && p.carrier_is_line(f) {
                    out.push(format!("{}: ONLY A BACK CAN TAKE THE HANDOFF", p.name));
                }
                if p.name.trim().is_empty() {
                    out.push(format!("{}: A PLAY NEEDS A NAME", f.name));
                }
            }
        }
        for (i, f) in self.defense.iter().enumerate() {
            if let Some(why) = front_problem(&f.spots) {
                out.push(format!("{}: {}", f.name, why));
            }
            if !roles_are_seven(f.roles.iter().map(|r| r.index())) {
                out.push(format!("{}: TWO MEN CANNOT PLAY ONE JOB", f.name));
            }
            let calls = self.calls.get(i).map(|c| c.as_slice()).unwrap_or(&[]);
            if calls.len() < MIN_PLAYS || calls.len() > MAX_PLAYS {
                out.push(format!(
                    "{} NEEDS {} TO {} CALLS",
                    f.name, MIN_PLAYS, MAX_PLAYS
                ));
            }
            for c in calls {
                if c.name.trim().is_empty() {
                    out.push(format!("{}: A CALL NEEDS A NAME", f.name));
                }
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
    // The line rule follows the job, not the spot: whichever two men are wearing
    // the guards' numbers are the two who have to be on the line.
    formation_problem_roles(spots, &OffRole::ALL)
}

/// The same rules, read through the jobs standing on the spots.
pub fn formation_problem_roles(spots: &[Vec2; 7], roles: &[OffRole; 7]) -> Option<&'static str> {
    let at = |role: OffRole| {
        roles
            .iter()
            .position(|r| *r == role)
            .map(|i| spots[i])
            .unwrap_or(Vec2::ZERO)
    };
    let qb = at(OffRole::Qb);
    if at(OffRole::Lg).x != 0.0 || at(OffRole::Rg).x != 0.0 {
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

fn roles_are_seven(indices: impl Iterator<Item = usize>) -> bool {
    let mut seen = [false; 7];
    let mut n = 0;
    for i in indices {
        if i >= 7 || seen[i] {
            return false;
        }
        seen[i] = true;
        n += 1;
    }
    n == 7
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
// The shelf
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Library {
    books: Vec<DesignBook>,
    current: usize,
}

#[derive(Default)]
struct Labels {
    books: Vec<&'static str>,
    forms: BTreeMap<u8, &'static str>,
    fronts: BTreeMap<u8, &'static str>,
}

fn store() -> &'static Mutex<Option<Library>> {
    static BOOKS: OnceLock<Mutex<Option<Library>>> = OnceLock::new();
    BOOKS.get_or_init(|| Mutex::new(None))
}

fn labels() -> &'static Mutex<Labels> {
    static LABELS: OnceLock<Mutex<Labels>> = OnceLock::new();
    LABELS.get_or_init(|| Mutex::new(Labels::default()))
}

fn generation() -> u64 {
    static GEN: AtomicU64 = AtomicU64::new(0);
    GEN.load(Ordering::Relaxed)
}

fn bump() {
    static GEN: AtomicU64 = AtomicU64::new(0);
    GEN.fetch_add(1, Ordering::Relaxed);
}

fn lock_store() -> std::sync::MutexGuard<'static, Option<Library>> {
    store().lock().unwrap_or_else(|e| e.into_inner())
}

/// Settle the names the playbook hands to the rest of the game, and tell the
/// caches the shelf moved.
fn publish(snapshot: &Option<Library>) {
    let mut l = Labels::default();
    if let Some(lib) = snapshot {
        for (bi, b) in lib.books.iter().enumerate() {
            l.books.push(leak(&b.name));
            for (fi, f) in b.offense.iter().enumerate() {
                l.forms.insert(form_id(bi, fi), leak(&f.name));
            }
            for (fi, f) in b.defense.iter().enumerate() {
                l.fronts.insert(front_id(bi, fi), leak(&f.name));
            }
        }
    }
    if let Ok(mut labels) = labels().lock() {
        *labels = l;
    }
    bump();
}

fn mutate<R>(f: impl FnOnce(&mut Option<Library>) -> R) -> R {
    let (out, snapshot) = {
        let mut guard = lock_store();
        let out = f(&mut guard);
        (out, guard.clone())
    };
    publish(&snapshot);
    out
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// The whole shelf, for a screen that wants to list it.
pub fn shelf() -> Vec<DesignBook> {
    lock_store()
        .as_ref()
        .map(|l| l.books.clone())
        .unwrap_or_default()
}

pub fn count() -> usize {
    lock_store().as_ref().map(|l| l.books.len()).unwrap_or(0)
}

pub fn current() -> Option<usize> {
    lock_store().as_ref().map(|l| l.current)
}

/// The book being edited.
pub fn book() -> Option<DesignBook> {
    let guard = lock_store();
    let lib = guard.as_ref()?;
    lib.books.get(lib.current).cloned()
}

pub fn book_at(index: usize) -> Option<DesignBook> {
    lock_store()
        .as_ref()
        .and_then(|l| l.books.get(index).cloned())
}

/// True when there is a legal book on the shelf: what the pickers ask before
/// they offer one.
pub fn has_book() -> bool {
    shelf().iter().any(|b| b.is_legal())
}

pub fn has_book_at(index: usize) -> bool {
    book_at(index).map(|b| b.is_legal()).unwrap_or(false)
}

pub fn has_book_named(_book: u8) -> bool {
    has_book()
}

/// Put a book on the shelf and make it the one being edited.
pub fn add(book: DesignBook) -> usize {
    mutate(|lib| {
        let shelf = lib.get_or_insert_with(|| Library {
            books: Vec::new(),
            current: 0,
        });
        if shelf.books.len() >= MAX_BOOKS {
            return shelf.current;
        }
        shelf.books.push(book);
        shelf.current = shelf.books.len() - 1;
        shelf.current
    })
}

/// Replace the book being edited. With an empty shelf this is how a book gets
/// there in the first place, which is what the tests and the old single-book
/// screens do.
pub fn set(book: Option<DesignBook>) {
    mutate(|lib| match book {
        Some(b) => {
            let shelf = lib.get_or_insert_with(|| Library {
                books: Vec::new(),
                current: 0,
            });
            if let Some(slot) = shelf.books.get_mut(shelf.current) {
                *slot = b;
            } else if shelf.books.len() < MAX_BOOKS {
                shelf.books.push(b);
                shelf.current = shelf.books.len() - 1;
            }
        }
        None => {
            *lib = None;
        }
    });
}

pub fn select(index: usize) {
    mutate(|lib| {
        if let Some(shelf) = lib {
            shelf.current = index.min(shelf.books.len().saturating_sub(1));
        }
    });
}

pub fn remove(index: usize) {
    mutate(|lib| {
        if let Some(shelf) = lib {
            if index < shelf.books.len() {
                shelf.books.remove(index);
            }
            if shelf.books.is_empty() {
                *lib = None;
            } else {
                shelf.current = shelf.current.min(shelf.books.len() - 1);
            }
        }
    });
}

pub fn clear() {
    mutate(|lib| *lib = None);
}

/// The name of a book on the shelf, for the picker.
pub fn book_name(index: usize) -> &'static str {
    labels()
        .lock()
        .ok()
        .and_then(|l| l.books.get(index).copied())
        .unwrap_or("MY PLAYBOOK")
}

/// The name of a designed formation, for the playbook card and the diagrams.
pub fn formation_name(id: u8) -> &'static str {
    labels()
        .lock()
        .ok()
        .and_then(|l| l.forms.get(&id).copied())
        .unwrap_or("CUSTOM")
}

pub fn front_name(id: u8) -> &'static str {
    labels()
        .lock()
        .ok()
        .and_then(|l| l.fronts.get(&id).copied())
        .unwrap_or("CUSTOM")
}

/// Where a designed formation stands one man, relative to the line of scrimmage.
pub fn formation_spot(id: u8, role: OffRole) -> Vec2 {
    book_at(id_book(id))
        .and_then(|b| b.offense.get(id_slot(id)).map(|f| f.alignment(role)))
        .unwrap_or(Vec2::new(0.0, 0.0))
}

/// Where a designed front stands one man, in field coordinates.
pub fn front_spot(id: u8, role: DefRole, los: f32, direction: i8) -> Vec2 {
    let s = book_at(id_book(id))
        .and_then(|b| b.defense.get(id_slot(id)).map(|f| f.spot(role)))
        .unwrap_or(Vec2::new(0.0, 0.0));
    Vec2::new(los + direction as f32 * s.x, s.y)
}

// ---------------------------------------------------------------------------
// Turning designs into the game's plays
// ---------------------------------------------------------------------------

/// The designed offense of one book, as plays the simulation runs.
pub fn offense_plays_at(index: usize) -> Vec<Play> {
    let Some(book) = book_at(index) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (fi, form) in book.offense.iter().enumerate() {
        for (pi, p) in book.plays_of(fi).iter().enumerate() {
            let name = if p.name.trim().is_empty() {
                leak(&format!("{} {}", form.name, pi + 1))
            } else {
                leak(&p.name)
            };
            let concept = leak(&form.name);
            out.push(design_play(
                name,
                concept,
                form_id(index, fi),
                form,
                p,
            ));
        }
    }
    out
}

/// The designed offense of the book being edited.
pub fn offense_plays() -> Vec<Play> {
    match current() {
        Some(i) => offense_plays_at(i),
        None => Vec::new(),
    }
}

fn design_play(
    name: &'static str,
    concept: &'static str,
    id: u8,
    form: &DesignFormation,
    p: &DesignPlay,
) -> Play {
    let pass = p.family.is_pass();
    let drop = match p.family {
        PlayFamily::QuickPass | PlayFamily::Screen => 3.0,
        PlayFamily::DropbackPass | PlayFamily::PlayAction => 5.0,
        _ => 0.0,
    };
    let mut assignments: Vec<(OffRole, Assign)> = Vec::new();
    for slot in 0..7 {
        let role = form.roles[slot];
        let spot = form.spots[slot];
        let drawn = &p.routes[slot];
        let rel: Vec<Vec2> = drawn
            .iter()
            .map(|q| Vec2::new(q.x - spot.x, q.y - spot.y))
            .collect();
        match role {
            // Whoever the player pointed at takes the ball: the handoff goes to
            // the man, not to the job he is wearing, so the tight end standing in
            // the backfield carries it exactly as the back does, and a picked
            // quarterback keeps it.
            _ if !pass && slot == p.carrier && !drawn.is_empty() => {
                if role == OffRole::Qb {
                    assignments.push((role, Assign::QbCarry { points: rel }));
                } else {
                    assignments.push((role, Assign::Carry { points: rel }));
                }
            }
            OffRole::Qb => {
                assignments.push((
                    role,
                    Assign::PassBlock {
                        slide: 0.0,
                        max_depth: if pass { drop } else { 1.0 },
                    },
                ));
            }
            OffRole::Lg | OffRole::Rg => {
                assignments.push((
                    role,
                    if pass {
                        Assign::PassBlock {
                            slide: 0.0,
                            max_depth: -1.0,
                        }
                    } else {
                        Assign::RunBlock {
                            aim: rel
                                .first()
                                .copied()
                                .unwrap_or(Vec2::new(
                                    2.0,
                                    if spot.y < 0.0 { -1.0 } else { 1.0 },
                                )),
                            seal: false,
                        }
                    },
                ));
            }
            _ => {
                if pass {
                    if drawn.is_empty() {
                        assignments.push((
                            role,
                            Assign::PassBlock {
                                slide: 0.0,
                                max_depth: -1.0,
                            },
                        ));
                    } else {
                        let depth = rel
                            .iter()
                            .map(|q| q.x)
                            .fold(0.0f32, f32::max)
                            .max(0.0)
                            .ceil() as u8;
                        assignments.push((role, Assign::Route { points: rel, depth }));
                    }
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
        }
    }
    Play {
        name,
        concept,
        family: p.family,
        formation: Formation::Custom(id),
        drop,
        run_side: 0,
        flip: false,
        assignments,
    }
}

/// The designed defense of one book, as calls the simulation runs.
pub fn defense_calls_at(index: usize) -> Vec<DefCall> {
    let Some(book) = book_at(index) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (fi, front) in book.defense.iter().enumerate() {
        for c in book.calls_of(fi) {
            let mut drops: Vec<(DefRole, Vec<Vec2>)> = Vec::new();
            for slot in 0..7 {
                if !c.drops[slot].is_empty() {
                    drops.push((front.roles[slot], c.drops[slot].clone()));
                }
            }
            out.push(DefCall {
                name: leak(&c.name),
                front: DefFront::Custom(front_id(index, fi)),
                coverage: c.coverage,
                run_fit: RunFit::Balanced,
                blitzers: Vec::new(),
                press: c.press || c.coverage == Coverage::ManPress,
                drops,
            });
        }
    }
    out
}

/// The designed defense of the book being edited.
pub fn defense_calls() -> Vec<DefCall> {
    match current() {
        Some(i) => defense_calls_at(i),
        None => Vec::new(),
    }
}

/// The AI's own copy of one book's offense, rebuilt when the shelf changes.
pub fn cached_offense(index: usize) -> &'static [Play] {
    static CACHE: Mutex<Vec<Option<(u64, &'static [Play])>>> = Mutex::new(Vec::new());
    cached(
        &CACHE,
        index,
        offense_plays_at,
        crate::playbook::playbook as fn() -> Vec<Play>,
    )
}

pub fn cached_defense(index: usize) -> &'static [DefCall] {
    static CACHE: Mutex<Vec<Option<(u64, &'static [DefCall])>>> = Mutex::new(Vec::new());
    cached(
        &CACHE,
        index,
        defense_calls_at,
        crate::playbook::defensive_playbook as fn() -> Vec<DefCall>,
    )
}

fn cached<T: Clone + 'static>(
    cache: &Mutex<Vec<Option<(u64, &'static [T])>>>,
    index: usize,
    build: fn(usize) -> Vec<T>,
    fallback: fn() -> Vec<T>,
) -> &'static [T] {
    let now = generation();
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() <= index {
        guard.resize(index + 1, None);
    }
    if let Some((g, cached)) = guard[index] {
        if g == now {
            return cached;
        }
    }
    let mut made = build(index);
    if made.is_empty() {
        // A club saved on this book before the shelf changed still has to call
        // something: the league's own book is the floor.
        made = fallback();
    }
    let fresh: &'static [T] = Box::leak(made.into_boxed_slice());
    guard[index] = Some((now, fresh));
    fresh
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
    s.trim().replace(' ', "_")
}

fn unword(s: &str) -> String {
    s.replace('_', " ")
}

fn spot_text(p: Vec2) -> String {
    format!("{},{},", p.x as i32, p.y as i32)
}

fn parse_spot(s: &str) -> Option<Vec2> {
    let s = s.trim_end_matches(',');
    let (x, y) = s.split_once(',')?;
    Some(Vec2::new(
        x.parse::<i32>().ok()? as f32,
        y.parse::<i32>().ok()? as f32,
    ))
}

fn routes_text(routes: &[Vec<Vec2>; 7]) -> String {
    // A semicolon between waypoints and a pipe between men: neither can be a
    // space, or the line would split inside a route.
    routes
        .iter()
        .map(|r| r.iter().map(|q| spot_text(*q)).collect::<Vec<_>>().join(";"))
        .collect::<Vec<_>>()
        .join("|")
}

fn parse_routes(text: &str) -> [Vec<Vec2>; 7] {
    let mut out: [Vec<Vec2>; 7] = Default::default();
    for (slot, lane) in text.split('|').enumerate().take(7) {
        out[slot] = lane.split(';').filter_map(parse_spot).collect();
    }
    out
}

/// The shelf as lines for the save file, after the clubs and kits.
pub fn text() -> String {
    let shelf = shelf();
    if shelf.is_empty() {
        return String::new();
    }
    let current = current().unwrap_or(0);
    let mut out = String::from("# The playbooks: books, formations, plays, fronts and calls.\n");
    out.push_str(&format!("books {} {}\n", shelf.len(), current));
    for (bi, book) in shelf.iter().enumerate() {
        out.push_str(&format!(
            "book2 {} {} {}\n",
            bi,
            word(if book.source.is_empty() {
                "SCRATCH"
            } else {
                &book.source
            }),
            word(&book.name)
        ));
        for (i, f) in book.offense.iter().enumerate() {
            let spots: Vec<String> = f.spots.iter().map(|s| spot_text(*s)).collect();
            let roles: Vec<String> = f.roles.iter().map(|r| r.index().to_string()).collect();
            out.push_str(&format!(
                "off2 {} {} {} {} {}\n",
                bi,
                i,
                word(&f.name),
                spots.join(" "),
                roles.join(" ")
            ));
        }
        for (fi, plays) in book.plays.iter().enumerate() {
            for p in plays {
                out.push_str(&format!(
                    "play2 {} {} {} {} {} {}\n",
                    bi,
                    fi,
                    word(&p.name),
                    family_name(p.family),
                    routes_text(&p.routes),
                    p.carrier
                ));
            }
        }
        for (i, f) in book.defense.iter().enumerate() {
            let spots: Vec<String> = f.spots.iter().map(|s| spot_text(*s)).collect();
            let roles: Vec<String> = f.roles.iter().map(|r| r.index().to_string()).collect();
            out.push_str(&format!(
                "front2 {} {} {} {} {}\n",
                bi,
                i,
                word(&f.name),
                spots.join(" "),
                roles.join(" ")
            ));
        }
        for (fi, calls) in book.calls.iter().enumerate() {
            for c in calls {
                out.push_str(&format!(
                    "call2 {} {} {} {} {} {}\n",
                    bi,
                    fi,
                    word(&c.name),
                    coverage_name(c.coverage),
                    u8::from(c.press),
                    routes_text(&c.drops)
                ));
            }
        }
    }
    out
}

/// Read the shelf back. Lines that do not parse are skipped, on the same rule as
/// the rest of the save file: nothing a player typed by hand should stop the
/// game starting. The single-book lines a previous version wrote are read too,
/// so a book drawn before the shelf existed is still there.
pub fn parse(text: &str) {
    let mut books: Vec<Option<DesignBook>> = Vec::new();
    let mut legacy: Option<DesignBook> = None;
    let mut current: usize = 0;

    fn entry(books: &mut Vec<Option<DesignBook>>, slot: usize) -> &mut DesignBook {
        while books.len() <= slot {
            let n = books.len();
            books.push(Some(DesignBook {
                name: format!("BOOK {}", n + 1),
                source: "SCRATCH".to_string(),
                offense: Vec::new(),
                plays: Vec::new(),
                defense: Vec::new(),
                calls: Vec::new(),
            }));
        }
        if books[slot].is_none() {
            books[slot] = Some(DesignBook {
                name: format!("BOOK {}", slot + 1),
                source: "SCRATCH".to_string(),
                offense: Vec::new(),
                plays: Vec::new(),
                defense: Vec::new(),
                calls: Vec::new(),
            });
        }
        books[slot].as_mut().expect("just filled")
    }

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("books") => {
                let _ = parts.next();
                if let Some(c) = parts.next().and_then(|s| s.parse::<usize>().ok()) {
                    current = c;
                }
            }
            Some("book2") => {
                let (Some(bi), Some(source), Some(name)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let Some(bi) = bi.parse::<usize>().ok() else {
                    continue;
                };
                let b = entry(&mut books, bi);
                b.source = unword(source);
                b.name = unword(name);
            }
            Some("off2") | Some("front2") => {
                let defense = line.starts_with("front2");
                let (Some(bi), Some(_i), Some(name)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let Some(bi) = bi.parse::<usize>().ok() else {
                    continue;
                };
                let rest: Vec<&str> = parts.collect();
                if rest.len() != 14 {
                    continue;
                }
                let spots: Vec<Vec2> = rest[..7].iter().filter_map(|s| parse_spot(s)).collect();
                let roles: Vec<usize> = rest[7..]
                    .iter()
                    .filter_map(|s| s.parse::<usize>().ok())
                    .collect();
                if spots.len() != 7 || roles.len() != 7 {
                    continue;
                }
                let mut arr = [Vec2::new(0.0, 0.0); 7];
                arr.copy_from_slice(&spots);
                if roles.iter().any(|r| *r >= 7) {
                    continue;
                }
                let b = entry(&mut books, bi);
                if defense {
                    let mut roles_out = DefRole::ALL;
                    for (slot, r) in roles.iter().enumerate() {
                        roles_out[slot] = DefRole::ALL[*r];
                    }
                    b.defense.push(DesignFront {
                        name: unword(name),
                        spots: arr,
                        roles: roles_out,
                    });
                    b.calls.push(Vec::new());
                } else {
                    let mut roles_out = OffRole::ALL;
                    for (slot, r) in roles.iter().enumerate() {
                        roles_out[slot] = OffRole::ALL[*r];
                    }
                    b.offense.push(DesignFormation {
                        name: unword(name),
                        spots: arr,
                        roles: roles_out,
                    });
                    b.plays.push(Vec::new());
                }
            }
            Some("play2") => {
                let (Some(bi), Some(fi), Some(name), Some(fam)) =
                    (parts.next(), parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let (Some(bi), Some(fi), Some(family)) = (
                    bi.parse::<usize>().ok(),
                    fi.parse::<usize>().ok(),
                    parse_family(fam),
                ) else {
                    continue;
                };
                let Some(routes) = parts.next() else { continue };
                // The spot taking the handoff is written after the paths, and
                // defaulted to the back for a play saved before the pick existed.
                let carrier = parts
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|c| *c < 7)
                    .unwrap_or(OffRole::Back.index());
                let b = entry(&mut books, bi);
                if let Some(list) = b.plays.get_mut(fi) {
                    if list.len() < MAX_PLAYS {
                        list.push(DesignPlay {
                            name: unword(name),
                            family,
                            routes: parse_routes(routes),
                            carrier,
                        });
                    }
                }
            }
            Some("call2") => {
                let (Some(bi), Some(fi), Some(name), Some(cov), Some(press), Some(drops)) = (
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                ) else {
                    continue;
                };
                let (Some(bi), Some(fi), Some(coverage)) = (
                    bi.parse::<usize>().ok(),
                    fi.parse::<usize>().ok(),
                    parse_coverage(cov),
                ) else {
                    continue;
                };
                let b = entry(&mut books, bi);
                if let Some(list) = b.calls.get_mut(fi) {
                    if list.len() < MAX_PLAYS {
                        list.push(DesignCall {
                            name: unword(name),
                            coverage,
                            press: press != "0",
                            drops: parse_routes(drops),
                        });
                    }
                }
            }
            // The single-book lines of the first version.
            Some("book") => {
                let (Some(source), Some(name)) = (parts.next(), parts.next()) else {
                    continue;
                };
                legacy = Some(DesignBook {
                    name: unword(name),
                    source: unword(source),
                    offense: Vec::new(),
                    plays: Vec::new(),
                    defense: Vec::new(),
                    calls: Vec::new(),
                });
            }
            Some("off") => {
                let Some(b) = legacy.as_mut() else { continue };
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
                    roles: OffRole::ALL,
                });
                b.plays.push(Vec::new());
            }
            Some("front") => {
                let Some(b) = legacy.as_mut() else { continue };
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
                    roles: DefRole::ALL,
                });
                b.calls.push(Vec::new());
            }
            Some("play") => {
                let Some(b) = legacy.as_mut() else { continue };
                let (Some(fi), Some(name), Some(fam)) = (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let (Some(fi), Some(family)) = (fi.parse::<usize>().ok(), parse_family(fam)) else {
                    continue;
                };
                let Some(routes) = parts.next() else { continue };
                // The old lanes were the four eligible jobs in the order the
                // book listed them; they are the same four spots by default.
                let lanes: Vec<&str> = routes.split('|').collect();
                let mut drawn: [Vec<Vec2>; 7] = Default::default();
                for (n, lane) in lanes.into_iter().take(4).enumerate() {
                    let slot = [3usize, 4, 5, 6][n];
                    drawn[slot] = lane.split(';').filter_map(parse_spot).collect();
                }
                if let Some(list) = b.plays.get_mut(fi) {
                    if list.len() < MAX_PLAYS {
                        list.push(DesignPlay {
                            name: unword(name),
                            family,
                            routes: drawn,
                            carrier: OffRole::Back.index(),
                        });
                    }
                }
            }
            Some("call") => {
                let Some(b) = legacy.as_mut() else { continue };
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
                        list.push(DesignCall::blank(unword(name), coverage));
                    }
                }
            }
            _ => {}
        }
    }

    let mut shelf: Vec<DesignBook> = books.into_iter().flatten().collect();
    // A book from the old format is a book, unless the file also has a shelf.
    if shelf.is_empty() {
        if let Some(old) = legacy {
            shelf.push(old);
        }
    }
    shelf.truncate(MAX_BOOKS);
    if shelf.is_empty() {
        return;
    }
    let current = current.min(shelf.len() - 1);
    mutate(|lib| {
        *lib = Some(Library {
            books: shelf,
            current,
        });
    });
}

/// The saved store is one thing, so every test that touches it takes this lock.
#[cfg(test)]
pub(crate) fn test_store_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    fn store_lock() -> MutexGuard<'static, ()> {
        test_store_lock()
    }

    /// Give every play in the book a drawn path, so the book is legal.
    fn draw_paths(book: &mut DesignBook) {
        for (fi, plays) in book.plays.iter_mut().enumerate() {
            let spot = book.offense[fi].spots[3];
            for p in plays {
                if !p.has_path() {
                    p.routes[3] = vec![Vec2::new(spot.x + 2.0, spot.y + 1.0)];
                }
            }
        }
    }

    #[test]
    fn a_book_copied_from_a_league_book_is_legal_and_runs() {
        let _store = store_lock();
        clear();
        let book = DesignBook::from_built_in(Book::WestCoast);
        assert!(book.offense.len() >= MIN_FORMATIONS);
        assert!(book.offense.len() <= MAX_FORMATIONS);
        assert!(book.defense.len() >= MIN_FORMATIONS);
        assert!(book.problems().is_empty(), "{:?}", book.problems());
        assert!(book.plays.iter().all(|p| !p.is_empty()));
        assert!(book.plays_of(0).iter().all(|p| p.has_path()));

        let i = add(book);
        let plays = offense_plays_at(i);
        assert!(!plays.is_empty());
        assert!(plays.iter().all(|p| p.assignments.len() == 7));
        assert!(plays
            .iter()
            .any(|p| matches!(p.assignment(OffRole::WideL), Some(Assign::Route { .. }))));
        // The formation is named by the book it belongs to, not by a single slot.
        assert_eq!(plays[0].formation, Formation::Custom(form_id(i, 0)));
        clear();
    }

    #[test]
    fn a_formation_without_two_men_on_the_line_is_refused() {
        let f = DesignFormation::blank(0, Formation::Pistol);
        assert!(formation_problem_roles(&f.spots, &f.roles).is_none());
        let mut g = f.clone();
        g.spots[OffRole::Lg.index()] = Vec2::new(-1.0, -1.0);
        assert_eq!(
            formation_problem_roles(&g.spots, &g.roles),
            Some("TWO LINEMEN MUST BE ON THE LINE OF SCRIMMAGE")
        );
        let mut h = f.clone();
        h.spots[OffRole::Qb.index()] = Vec2::new(2.0, 0.0);
        assert_eq!(
            formation_problem_roles(&h.spots, &h.roles),
            Some("THE QUARTERBACK MUST BE BEHIND THE LINE")
        );
    }

    #[test]
    fn the_line_rule_follows_the_job_and_not_the_spot() {
        let _store = store_lock();
        clear();
        let mut form = DesignFormation::blank(0, Formation::Pistol);
        // The guards' jobs move onto the wide men's spots, and the wide men's
        // jobs onto the guards' spots.
        let lg = form.slot_of(OffRole::Lg);
        let wide = form.slot_of(OffRole::WideL);
        form.roles.swap(lg, wide);
        // In the pistol the wide man is on the line, so the guard's job still
        // has a man on the line and the rule still holds.
        assert_eq!(form.alignment(OffRole::Lg).x, 0.0);
        assert!(formation_problem_roles(&form.spots, &form.roles).is_none());
        // Walk the man wearing the guard's job off the line: the spot the guard
        // was born on is still on the line, and the rule still breaks.
        let guard_now = form.slot_of(OffRole::Lg);
        form.spots[guard_now] = Vec2::new(-1.0, -1.0);
        assert_eq!(
            formation_problem_roles(&form.spots, &form.roles),
            Some("TWO LINEMEN MUST BE ON THE LINE OF SCRIMMAGE")
        );

        // A route is drawn on the spot the man is standing on, not on the job's
        // old home, so a job moved around the board keeps his own path.
        let mut book = DesignBook::blank();
        draw_paths(&mut book);
        let slot = book.offense[0].slot_of(OffRole::Back);
        let spot = book.offense[0].spots[slot];
        book.offense[0].roles.swap(slot, 0);
        let slot = book.offense[0].slot_of(OffRole::Back);
        let spot_now = book.offense[0].spots[slot];
        book.plays[0][0].routes[slot] = vec![Vec2::new(spot_now.x + 4.0, spot_now.y + 2.0)];
        let i = add(book);
        let plays = offense_plays_at(i);
        match plays[0].assignment(OffRole::Back) {
            Some(Assign::Route { points, .. }) => {
                assert_eq!(points[0], Vec2::new(4.0, 2.0))
            }
            Some(Assign::Carry { points }) => assert_eq!(points[0], Vec2::new(4.0, 2.0)),
            other => panic!("the drawn route did not become a path: {other:?}"),
        }
        let _ = spot;
        clear();
    }

    #[test]
    fn the_man_the_player_points_at_takes_the_handoff() {
        let _store = store_lock();
        clear();
        let mut book = DesignBook::blank();
        draw_paths(&mut book);
        // The tight end stands in the backfield beside the back, and the ball is
        // going to him: the carrier is a spot on the drawing, not the job that
        // happens to be called Back.
        let te = book.offense[0].slot_of(OffRole::Slot);
        let back = book.offense[0].slot_of(OffRole::Back);
        book.offense[0].spots[te] = Vec2::new(-4.0, 2.0);
        let back_spot = book.offense[0].spots[back];
        book.plays[0][0].family = PlayFamily::RunInside;
        book.plays[0][0].carrier = te;
        book.plays[0][0].routes = Default::default();
        book.plays[0][0].routes[te] = vec![Vec2::new(-2.0, 2.0), Vec2::new(7.0, 0.0)];
        book.plays[0][0].routes[back] = vec![Vec2::new(back_spot.x + 2.0, back_spot.y)];
        assert!(book.problems().is_empty(), "{:?}", book.problems());
        assert_eq!(book.plays[0][0].carrier_role(&book.offense[0]), OffRole::Slot);

        let i = add(book);
        let plays = offense_plays_at(i);
        assert_eq!(
            plays[0].carrier_role(),
            Some(OffRole::Slot),
            "the ball goes to the tight end, not to the back"
        );
        assert!(
            matches!(plays[0].assignment(OffRole::Slot), Some(Assign::Carry { .. })),
            "his drawn path is the ball: {:?}",
            plays[0].assignment(OffRole::Slot)
        );
        assert!(
            matches!(plays[0].assignment(OffRole::Back), Some(Assign::RunBlock { .. })),
            "the back blocks on a run that is not his: {:?}",
            plays[0].assignment(OffRole::Back)
        );
        // The path is written relative to his own feet: (-2, 2) and (7, 0) drawn
        // from a man standing on (-4, 2).
        assert_eq!(
            plays[0].assignment(OffRole::Slot).and_then(assign_points),
            Some(vec![Vec2::new(2.0, 0.0), Vec2::new(11.0, -2.0)]),
            "the drawn path is the run"
        );
        clear();
    }

    #[test]
    fn a_run_whose_carrier_has_no_path_is_refused() {
        let _store = store_lock();
        clear();
        let mut book = DesignBook::blank();
        draw_paths(&mut book);
        // Paths are drawn, but none of them is on the man taking the ball.
        book.plays[0][0].family = PlayFamily::RunInside;
        book.plays[0][0].carrier = 4;
        book.plays[0][0].routes[4] = Vec::new();
        let problems = book.problems();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("THE MAN TAKING THE HANDOFF NEEDS A PATH")),
            "{problems:?}"
        );
        clear();
    }

    #[test]
    fn the_handoff_survives_the_save_file() {
        let _store = store_lock();
        clear();
        let mut book = DesignBook::blank();
        draw_paths(&mut book);
        book.plays[0][0].family = PlayFamily::RunInside;
        book.plays[0][0].routes[5] = vec![Vec2::new(-1.0, -9.0)];
        book.plays[0][0].carrier = 5;
        add(book);
        let written = text();
        clear();
        parse(&written);
        let back = book_at(0).expect("the book comes back");
        assert_eq!(
            back.plays[0][0].carrier, 5,
            "the man taking the handoff is written down with the play"
        );
        assert_eq!(back.plays[0][0].carrier_role(&back.offense[0]), OffRole::WideL);
        clear();
    }

    #[test]
    fn a_play_can_be_named_and_the_name_is_what_the_game_calls_it() {
        let _store = store_lock();
        clear();
        let mut book = DesignBook::blank();
        draw_paths(&mut book);
        book.plays[0][0].name = "MESH RIGHT".to_string();
        book.plays[0][0].routes[3] = vec![Vec2::new(4.0, 1.0)];
        book.offense[0].name = "TRIPS LEFT".to_string();
        assert!(book.problems().is_empty(), "{:?}", book.problems());
        let i = add(book);
        let plays = offense_plays_at(i);
        assert_eq!(plays[0].name, "MESH RIGHT");
        assert_eq!(plays[0].concept, "TRIPS LEFT");
        assert_eq!(formation_name(form_id(i, 0)), "TRIPS LEFT");
        clear();
    }

    #[test]
    fn a_drawn_coverage_becomes_a_drop_the_defense_can_play() {
        let _store = store_lock();
        clear();
        let mut book = DesignBook::blank();
        draw_paths(&mut book);
        let slot = book.defense[0].slot_of(DefRole::Mike);
        let spot = book.defense[0].spots[slot];
        book.calls[0][0].drops[slot] = vec![Vec2::new(spot.x + 6.0, spot.y + 2.0)];
        book.calls[0][0].name = "MIKE DROP".to_string();
        book.calls[0][0].press = true;
        assert!(book.problems().is_empty(), "{:?}", book.problems());
        let i = add(book);
        let calls = defense_calls_at(i);
        assert_eq!(calls[0].name, "MIKE DROP");
        assert!(calls[0].press);
        let (role, path) = calls[0]
            .drops
            .iter()
            .find(|(r, _)| *r == DefRole::Mike)
            .expect("the drawn drop was lost");
        assert_eq!(*role, DefRole::Mike);
        assert_eq!(path, &vec![Vec2::new(spot.x + 6.0, spot.y + 2.0)]);
        clear();
    }

    #[test]
    fn several_books_live_on_the_shelf_side_by_side() {
        let _store = store_lock();
        clear();
        let mut a = DesignBook::blank();
        draw_paths(&mut a);
        a.name = "AIR BOOK".to_string();
        a.plays[0][0].name = "AIR MESH".to_string();
        a.plays[0][0].routes[0] = vec![Vec2::new(3.0, 1.0)];
        let mut b = DesignBook::blank();
        draw_paths(&mut b);
        b.name = "RUN BOOK".to_string();
        b.plays[0][0].name = "POWER LEFT".to_string();
        b.plays[0][0].routes[0] = vec![Vec2::new(1.0, 0.0)];
        let ia = add(a);
        let ib = add(b);
        assert_ne!(ia, ib);
        assert_eq!(count(), 2);
        assert_eq!(book_name(ia), "AIR BOOK");
        assert_eq!(book_name(ib), "RUN BOOK");
        assert_eq!(formation_name(form_id(ib, 0)), "FORMATION 1");
        // The two books make different plays, and the caches tell them apart.
        let pa = cached_offense(ia);
        let pb = cached_offense(ib);
        assert_ne!(pa[0].name, pb[0].name);
        assert_eq!(pa[0].formation, Formation::Custom(form_id(ia, 0)));
        assert_eq!(pb[0].formation, Formation::Custom(form_id(ib, 0)));
        // Removing one leaves the other where it was.
        remove(ia);
        assert_eq!(count(), 1);
        assert_eq!(book_name(0), "RUN BOOK");
        clear();
    }

    #[test]
    fn the_shelf_survives_the_save_file() {
        let _store = store_lock();
        clear();
        let mut made = DesignBook::from_built_in(Book::Balanced);
        made.offense.truncate(MAX_FORMATIONS);
        made.plays.truncate(MAX_FORMATIONS);
        made.defense.truncate(MAX_FORMATIONS);
        made.calls.truncate(MAX_FORMATIONS);
        made.name = "BACKYARD".to_string();
        made.plays[0][0].name = "MY BEST PLAY".to_string();
        made.calls[0][0].press = true;
        made.offense[0].roles.swap(0, 3);
        assert!(made.problems().is_empty(), "{:?}", made.problems());
        add(made.clone());

        let written = text();
        clear();
        assert!(book().is_none());
        parse(&written);

        let back = book().expect("the book came back empty");
        assert_eq!(back.name, "BACKYARD");
        assert_eq!(back.offense.len(), made.offense.len());
        assert_eq!(back.defense.len(), made.defense.len());
        assert_eq!(back.offense[0].name, made.offense[0].name);
        assert_eq!(back.offense[0].spots, made.offense[0].spots);
        assert_eq!(back.offense[0].roles, made.offense[0].roles);
        assert_eq!(back.plays[0][0].name, "MY BEST PLAY");
        assert_eq!(back.plays[0][0].routes, made.plays[0][0].routes);
        assert_eq!(back.calls[0][0].press, true);
        assert!(has_book());
        clear();
    }

    #[test]
    fn a_book_from_the_first_version_still_loads() {
        let _store = store_lock();
        clear();
        let old = "\
book WestCoast MY_PLAYBOOK
off 0 FORMATION_1 0,-3, 0,-1, 0,1, -4,0, -1,3, 0,-12, 0,12,
play 0 ZONE_READ QuickPass 1,0;3,0|4,2||
front 0 FRONT_1 1,-2, 1,2, 3,0, 4,3, 6,-10, 6,10, 10,0,
call 0 COVER_1 ManFree
";
        parse(old);
        let b = book().expect("the old book did not load");
        assert_eq!(b.name, "MY PLAYBOOK");
        assert_eq!(b.offense.len(), 1);
        assert_eq!(b.plays[0].len(), 1);
        // The four old lanes land on the four eligible spots.
        assert_eq!(b.plays[0][0].routes[3].len(), 2);
        assert_eq!(b.plays[0][0].routes[4].len(), 1);
        assert_eq!(b.defense[0].roles, DefRole::ALL);
        clear();
    }

    #[test]
    fn a_count_outside_the_limits_is_a_problem() {
        let mut book = DesignBook::blank();
        book.offense.truncate(1);
        book.plays.truncate(1);
        let problems = book.problems();
        assert!(problems.iter().any(|p| p.contains("OFFENSE NEEDS")));
    }
}
