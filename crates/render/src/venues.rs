//! The grounds the game is played on.
//!
//! A venue is the building around the field: the concrete is still the
//! broadcast's, but the world outside it changes. There are two practice
//! grounds, one for each of the sixteen clubs — each with something of its own
//! city on the skyline — and four one-off grounds the league takes the game to.
//!
//! Everything here is generated from a fixed seed, so a ground looks the same
//! on every replay of every play at it.

use a7fl_core::math::{Rgb, Vec3};
use a7fl_core::team::{clubs, Team};

use crate::scene::{Lighting, Mesh};

/// Yards of field between the goal lines, and half the width of the paint. The
/// building starts outside these.
const FIELD_LEN: f32 = 120.0;
/// Half the width of the paint, taken from the field itself: anything hung off
/// these lines has to clear the sidelines, or it hangs over the field.
const HALF_WIDTH: f32 = a7fl_core::field::FIELD_HALF_WIDTH;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The field the game shipped with: open air, a full house.
    Practice,
    /// The same field with the roof on and the stands empty.
    ClosedPractice,
    /// One of the sixteen clubs' own grounds, by club index.
    Home(usize),
    /// A dome on the sea bed, with the water and the fish outside it.
    Underwater,
    /// Red dirt, mesas and a water tower; nothing else for miles.
    Outback,
    /// The ocean down one side, a hotel down the other.
    Beach,
    /// The league's final: both clubs in both end zones.
    Championship,
}

/// How many grounds the picker offers.
pub const COUNT: usize = 22;

const HOME_START: usize = 2;

#[derive(Clone, Copy, Debug)]
pub struct Venue {
    index: usize,
}

impl Venue {
    pub fn from_index(i: usize) -> Venue {
        Venue { index: i % COUNT }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn kind(&self) -> Kind {
        match self.index {
            0 => Kind::Practice,
            1 => Kind::ClosedPractice,
            i if i < HOME_START + 16 => Kind::Home(i - HOME_START),
            18 => Kind::Underwater,
            19 => Kind::Outback,
            20 => Kind::Beach,
            _ => Kind::Championship,
        }
    }

    /// The name on the picker and on the scoreboard bug.
    pub fn name(&self) -> String {
        match self.kind() {
            Kind::Practice => "THE PRACTICE FIELD".to_string(),
            Kind::ClosedPractice => "THE CLOSED PRACTICE".to_string(),
            Kind::Home(k) => cities()
                .get(k)
                .map(|c| c.suffix.to_string())
                .unwrap_or_else(|| "THE HOME GROUND".to_string()),
            Kind::Underwater => "THE DEEP".to_string(),
            Kind::Outback => "THE BACK OF BEYOND".to_string(),
            Kind::Beach => "THE STRAND".to_string(),
            Kind::Championship => "THE CHAMPIONSHIP".to_string(),
        }
    }

    /// The line under the name: whose ground it is, or what it is.
    pub fn sub(&self) -> String {
        match self.kind() {
            Kind::Practice => "OPEN AIR, FULL HOUSE".to_string(),
            Kind::ClosedPractice => "ROOF ON, STANDS EMPTY".to_string(),
            Kind::Home(k) => clubs()
                .get(k)
                .map(|c| c.city.to_uppercase())
                .unwrap_or_else(|| "HOME".to_string()),
            Kind::Underwater => "A CLEAR DOME ON THE SEA BED".to_string(),
            Kind::Outback => "RED DIRT, NO NEIGHBOURS".to_string(),
            Kind::Beach => "OCEAN ONE SIDE, HOTEL THE OTHER".to_string(),
            Kind::Championship => "BOTH END ZONES, BOTH CLUBS".to_string(),
        }
    }

    /// One sentence for the picker, so a ground can be chosen without seeing it.
    pub fn blurb(&self) -> &'static str {
        match self.kind() {
            Kind::Practice => "The field the game is learned on: open sky, a crowd in the bowl.",
            Kind::ClosedPractice => "The same field with the roof down and nobody in the seats.",
            Kind::Home(k) => cities().get(k).map(|c| c.blurb).unwrap_or("A home ground."),
            Kind::Underwater => "A clear dome overhead, the sea beyond it, and the fish swimming past.",
            Kind::Outback => "A field cut into the red earth, with the mesas on the horizon.",
            Kind::Beach => "Breakers down one sideline, a hotel down the other, palms over both.",
            Kind::Championship => "The final: both clubs painted into both end zones.",
        }
    }

    pub fn has_crowd(&self) -> bool {
        !matches!(self.kind(), Kind::ClosedPractice)
    }


    /// Sky gradient, top to horizon. The horizon doubles as the fog colour, so
    /// distant scenery fades into the weather of its own ground.
    pub fn sky(&self) -> [Rgb; 3] {
        match self.kind() {
            Kind::Practice => [Rgb::hex(0x0A1428), Rgb::hex(0x20364E), Rgb::hex(0x7E8F96)],
            Kind::ClosedPractice => [Rgb::hex(0x0B0F15), Rgb::hex(0x1A222D), Rgb::hex(0x3C4654)],
            Kind::Home(k) => cities()
                .get(k)
                .map(|c| c.sky)
                .unwrap_or([Rgb::hex(0x0A1428), Rgb::hex(0x20364E), Rgb::hex(0x7E8F96)]),
            Kind::Underwater => [Rgb::hex(0x031C29), Rgb::hex(0x06455F), Rgb::hex(0x0B6E86)],
            Kind::Outback => [Rgb::hex(0x3E7FB5), Rgb::hex(0xD9A66B), Rgb::hex(0xE8C08A)],
            Kind::Beach => [Rgb::hex(0x1E7FC4), Rgb::hex(0x63B6DE), Rgb::hex(0xD8F0F6)],
            Kind::Championship => [Rgb::hex(0x120A2A), Rgb::hex(0x3B1E63), Rgb::hex(0xD9A441)],
        }
    }

    /// The light in this building. Still one sun for the whole game — a ground
    /// changes the fog and the bounce off the turf, not the shading model.
    pub fn lighting(&self) -> Lighting {
        let sky = self.sky();
        let mut light = Lighting::default();
        light.fog = sky[2];
        match self.kind() {
            Kind::Underwater => {
                light.ambient = 0.62;
                light.fog_density = 0.013;
                light.bounce = Vec3::new(0.0, 0.0, 1.0);
                light.bounce_strength = 0.06;
            }
            Kind::Outback => {
                light.ambient = 0.46;
                light.fog_density = 0.008;
                light.bounce = Vec3::new(0.1, 0.0, -1.0).normalize_or_zero();
                light.bounce_strength = 0.16;
            }
            Kind::Beach => {
                light.ambient = 0.56;
                light.fog_density = 0.009;
            }
            Kind::Championship => {
                light.ambient = 0.50;
                light.fog_density = 0.011;
            }
            _ => {}
        }
        light
    }
}

// ---------------------------------------------------------------------------
// The sixteen home grounds
// ---------------------------------------------------------------------------

/// One club's ground: what its city puts on the skyline, and the weather it
/// puts over it. The club colours still dress the field and the bowl; this is
/// the city behind them.
#[derive(Clone, Copy)]
struct City {
    suffix: &'static str,
    blurb: &'static str,
    /// How many towers are worth building.
    towers: u8,
    /// Low brick blocks: rowhouses, warehouses, honky-tonks.
    brick: u8,
    palms: u8,
    pines: u8,
    /// Chimneys and stacks.
    stacks: u8,
    /// A stretch of water beyond one stand.
    water: bool,
    /// A tall lit spire over the rest.
    spire: bool,
    sky: [Rgb; 3],
}

fn cities() -> Vec<City> {
    fn hex(v: u32) -> Rgb {
        Rgb::hex(v)
    }
    vec![
        // North
        City { suffix: "THE HARBOUR", blurb: "Brick rowhouses, a harbour full of masts, and the wind off the water.", towers: 3, brick: 6, palms: 0, pines: 0, stacks: 1, water: true, spire: false, sky: [hex(0x0A1428), hex(0x243A4E), hex(0x8A9AA0)] },
        City { suffix: "FOUNDRY YARD", blurb: "Steel over the old works: stacks, gantries and a train that never stops.", towers: 2, brick: 5, palms: 0, pines: 0, stacks: 4, water: false, spire: false, sky: [hex(0x141018), hex(0x3A2A2A), hex(0xB08A6A)] },
        City { suffix: "ASSEMBLY GROUNDS", blurb: "A factory town's field: brick, chimneys and the plant running nights.", towers: 2, brick: 6, palms: 0, pines: 0, stacks: 3, water: true, spire: false, sky: [hex(0x101820), hex(0x2E3C46), hex(0x93A3A6)] },
        City { suffix: "NORTHWIND FIELD", blurb: "Glass towers joined by skybridges, and pines standing in the cold.", towers: 4, brick: 1, palms: 0, pines: 8, stacks: 0, water: true, spire: false, sky: [hex(0x081C2E), hex(0x1E3C56), hex(0xAFC4CE)] },
        // South
        City { suffix: "FIREFLY PARK", blurb: "A warm night, glass towers, and the trees full of light.", towers: 4, brick: 2, palms: 0, pines: 6, stacks: 0, water: false, spire: false, sky: [hex(0x1A1030), hex(0x3E2A5E), hex(0xC79A8A)] },
        City { suffix: "RIPTIDE SHORES", blurb: "Pastel hotels, palms over the walk, and the ocean past the boards.", towers: 3, brick: 1, palms: 10, pines: 0, stacks: 0, water: true, spire: false, sky: [hex(0x123C5E), hex(0x3E8FB0), hex(0xF6E9C9)] },
        City { suffix: "RUSTLER RANCH", blurb: "Stockyards and derricks: low brick, tall steel, and a lot of sky.", towers: 3, brick: 5, palms: 0, pines: 0, stacks: 2, water: false, spire: false, sky: [hex(0x2E1A2E), hex(0x6E3E36), hex(0xE0A878)] },
        City { suffix: "BRASS HALL", blurb: "Neon down the street, a lit spire over it, and music in the walls.", towers: 3, brick: 4, palms: 0, pines: 0, stacks: 1, water: true, spire: true, sky: [hex(0x181026), hex(0x3A2448), hex(0xC98E6A)] },
        // East
        City { suffix: "BEACON HEIGHTS", blurb: "The city at night: a wall of windows and a light on top of it.", towers: 6, brick: 2, palms: 0, pines: 0, stacks: 0, water: true, spire: true, sky: [hex(0x080E22), hex(0x1A2A4E), hex(0x8E9AB6)] },
        City { suffix: "ANVIL WORKS", blurb: "Brick, iron and a foundry glow: the oldest ground in the league.", towers: 2, brick: 6, palms: 0, pines: 0, stacks: 3, water: true, spire: false, sky: [hex(0x0E1218), hex(0x2A2E34), hex(0x9AA0A6)] },
        City { suffix: "MONUMENT GROUNDS", blurb: "Marble and glass, a lit obelisk, and the capital behind the stand.", towers: 3, brick: 2, palms: 0, pines: 4, stacks: 0, water: true, spire: true, sky: [hex(0x0A1626), hex(0x22364E), hex(0xB6BCC2)] },
        City { suffix: "AVIATOR FIELD", blurb: "Hangars and a control tower, with the city lights on the horizon.", towers: 3, brick: 3, palms: 0, pines: 5, stacks: 1, water: false, spire: false, sky: [hex(0x0C1A2E), hex(0x24405C), hex(0xA6B4BC)] },
        // West
        City { suffix: "COMET BOWL", blurb: "Palms, hills and a city that runs all the way to the ridge.", towers: 5, brick: 1, palms: 8, pines: 0, stacks: 0, water: false, spire: false, sky: [hex(0x140E2A), hex(0x4A2E5E), hex(0xE0A070)] },
        City { suffix: "SANDSTORM MESA", blurb: "The desert at dusk: mesas, cactus and one bright strip of sky.", towers: 1, brick: 1, palms: 2, pines: 0, stacks: 0, water: false, spire: false, sky: [hex(0x2A1A38), hex(0x8A4A46), hex(0xE8B478)] },
        City { suffix: "RAINMAKER FIELD", blurb: "Pines, water and glass: the rain comes in off the sound.", towers: 4, brick: 1, palms: 0, pines: 9, stacks: 0, water: true, spire: false, sky: [hex(0x0C1A20), hex(0x223C44), hex(0x8AA4A8)] },
        City { suffix: "ALTITUDE PARK", blurb: "The mountains stand over the stand, and the ball carries up here.", towers: 2, brick: 1, palms: 0, pines: 8, stacks: 0, water: false, spire: false, sky: [hex(0x0A1A30), hex(0x244260), hex(0xC8D2DC)] },
    ]
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------


/// A tower with windows on the face turned toward the field.
fn tower(mesh: &mut Mesh, at: Vec3, half: Vec3, face: f32, color: Rgb, rand: &mut dyn FnMut() -> f32) {
    mesh.box_shaded(at, half, 0.0, color, 1.0);
    // A roof lip, so the boxes do not all end on the same hard line.
    mesh.box_shaded(
        Vec3::new(at.x, at.y, at.z + half.z + 0.25),
        Vec3::new(half.x * 1.06, half.y * 1.06, 0.25),
        0.0,
        color.scale(0.8),
        1.0,
    );
    // Window courses: two rows of lights for every four yards of height.
    let y_face = at.y - face * (half.y + 0.03);
    let rows = ((half.z * 2.0 / 2.6) as usize).clamp(1, 14);
    let cols = ((half.x * 2.0 / 2.2) as usize).clamp(1, 8);
    for row in 0..rows {
        for col in 0..cols {
            let lit = rand() < 0.5;
            let color = if lit {
                Rgb::hex(0xF2D08A)
            } else {
                Rgb::hex(0x2A3440)
            };
            let x = at.x - half.x + 1.0 + col as f32 * 2.2;
            let z = 1.4 + row as f32 * 2.6;
            if z > half.z * 2.0 - 1.0 {
                continue;
            }
            let w = 0.55;
            let h = 0.75;
            mesh.quad_shaded(
                Vec3::new(x - w, y_face, z),
                Vec3::new(x + w, y_face, z),
                Vec3::new(x + w, y_face, z + h),
                Vec3::new(x - w, y_face, z + h),
                color,
                1.0,
            );
        }
    }
}

/// A low block of brick: a rowhouse, a warehouse, a honky-tonk.
fn brick_block(mesh: &mut Mesh, at: Vec3, half: Vec3, face: f32, color: Rgb, rand: &mut dyn FnMut() -> f32) {
    mesh.box_shaded(at, half, 0.0, color, 1.0);
    // A flat roof with a parapet, and a couple of dark windows.
    mesh.box_shaded(
        Vec3::new(at.x, at.y, at.z + half.z + 0.18),
        Vec3::new(half.x * 1.02, half.y * 1.02, 0.18),
        0.0,
        color.scale(0.72),
        1.0,
    );
    let y_face = at.y - face * (half.y + 0.03);
    let cols = ((half.x * 2.0 / 2.6) as usize).clamp(1, 5);
    for col in 0..cols {
        let x = at.x - half.x + 1.2 + col as f32 * 2.6;
        let lit = rand() < 0.35;
        let color = if lit { Rgb::hex(0xE8BE72) } else { Rgb::hex(0x20262E) };
        mesh.quad_shaded(
            Vec3::new(x - 0.55, y_face, 1.4),
            Vec3::new(x + 0.55, y_face, 1.4),
            Vec3::new(x + 0.55, y_face, 2.5),
            Vec3::new(x - 0.55, y_face, 2.5),
            color,
            1.0,
        );
    }
}

fn palm(mesh: &mut Mesh, at: Vec3, height: f32, rand: &mut dyn FnMut() -> f32) {
    let top = Vec3::new(at.x, at.y, at.z + height);
    mesh.cylinder(at, top, 0.22, 0.14, 5, Rgb::hex(0x5A4A32));
    let green = Rgb::hex(0x2E6B3A);
    let n = 7;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU + rand() * 0.4;
        let dir = Vec3::new(a.cos(), a.sin(), 0.0);
        let side = Vec3::new(-a.sin(), a.cos(), 0.0);
        let tip = top + dir * (2.0 + rand() * 0.9) - Vec3::new(0.0, 0.0, 1.2 + rand() * 0.7);
        mesh.quad_shaded(
            top + dir * 0.15,
            top + dir * 0.15 + side * 0.35,
            tip + side * 0.15,
            tip,
            green,
            1.0,
        );
    }
}

fn pine(mesh: &mut Mesh, at: Vec3, height: f32, rand: &mut dyn FnMut() -> f32) {
    let base = Vec3::new(at.x, at.y, at.z);
    mesh.cylinder(base, base + Vec3::new(0.0, 0.0, height * 0.25), 0.2, 0.14, 5, Rgb::hex(0x4A3A28));
    let green = Rgb::hex(0x1E4A32);
    for tier in 0..3 {
        let z = height * (0.2 + tier as f32 * 0.24);
        let r = (height * 0.26) * (1.0 - tier as f32 * 0.24) + rand() * 0.2;
        mesh.cylinder(
            base + Vec3::new(0.0, 0.0, z),
            base + Vec3::new(0.0, 0.0, z + height * 0.33),
            r,
            0.0,
            6,
            green,
        );
    }
}

fn rock(mesh: &mut Mesh, at: Vec3, size: f32, color: Rgb, rand: &mut dyn FnMut() -> f32) {
    let yaw = rand() * std::f32::consts::TAU;
    mesh.box_shaded(
        Vec3::new(at.x, at.y, at.z + size * 0.35),
        Vec3::new(size, size * (0.5 + rand() * 0.4), size * 0.6),
        yaw,
        color,
        1.0,
    );
}

fn water(mesh: &mut Mesh, x0: f32, x1: f32, y0: f32, y1: f32, color: Rgb) {
    mesh.quad_shaded(
        Vec3::new(x0, y0, 0.01),
        Vec3::new(x1, y0, 0.01),
        Vec3::new(x1, y1, 0.01),
        Vec3::new(x0, y1, 0.01),
        color,
        1.0,
    );
    // A few long highlight streaks, so the plane is not one flat colour.
    let mut y = y0;
    while y < y1 {
        let lerp = (y - y0) / (y1 - y0).max(1.0);
        mesh.quad_shaded(
            Vec3::new(x0, y, 0.02),
            Vec3::new(x1, y, 0.02),
            Vec3::new(x1, y + 0.35, 0.02),
            Vec3::new(x0, y + 0.35, 0.02),
            color.lerp(Rgb::WHITE, lerp * 0.28),
            1.0,
        );
        y += 2.4;
    }
}

fn bunting(mesh: &mut Mesh, x0: f32, x1: f32, y: f32, z: f32, colors: [Rgb; 3]) {
    let mut x = x0;
    let mut i = 0;
    while x < x1 {
        let color = colors[i % colors.len()];
        mesh.tri(
            Vec3::new(x, y, z),
            Vec3::new(x + 0.8, y, z),
            Vec3::new(x + 0.4, y, z - 0.9),
            color,
        );
        x += 1.1;
        i += 1;
    }
    // The rope it hangs from.
    mesh.quad_shaded(
        Vec3::new(x0, y, z + 0.02),
        Vec3::new(x1, y, z + 0.02),
        Vec3::new(x1, y, z + 0.12),
        Vec3::new(x0, y, z + 0.12),
        Rgb::hex(0xE8EAE2),
        1.0,
    );
}

/// The scenery beyond the bowl. `face` is the side the camera will see it from:
/// -1 for the stand at negative y, +1 for the one at positive y.
pub fn scenery(venue: Venue, teams: &[Team; 2], mesh: &mut Mesh, rand: &mut dyn FnMut() -> f32) {
    match venue.kind() {
        Kind::Practice => practice_scenery(mesh, rand),
        Kind::ClosedPractice => closed_practice(mesh),
        Kind::Home(k) => city_scenery(cities()[k.min(cities().len() - 1)].clone(), teams, mesh, rand),
        Kind::Underwater => underwater(mesh, rand),
        Kind::Outback => outback(mesh, rand),
        Kind::Beach => beach(mesh, rand),
        Kind::Championship => championship(teams, mesh, rand),
    }
}

fn practice_scenery(mesh: &mut Mesh, rand: &mut dyn FnMut() -> f32) {
    // A treeline and the practice shed: the field the game shipped with, with a
    // little more of a site around it than the bowl alone.
    for side in [-1.0f32, 1.0] {
        let mut x = -18.0;
        while x < 118.0 {
            let y = side * (34.0 + rand() * 6.0);
            pine(mesh, Vec3::new(x, y, 0.0), 5.0 + rand() * 4.0, rand);
            x += 9.0 + rand() * 6.0;
        }
    }
    mesh.box_shaded(
        Vec3::new(-26.0, 26.0, 2.4),
        Vec3::new(7.0, 5.0, 2.4),
        0.0,
        Rgb::hex(0x3C4652),
        1.0,
    );
    // And a line of trees beyond each end line, so the shot down the field has
    // a horizon rather than the back of the scoreboard. They start behind the
    // back of the end zone and step away from the field, never onto it: this
    // line used to walk from ten yards behind the left end zone twenty yards
    // into the field, and stood a row of pines on the paint.
    for end in [-1.0f32, 1.0] {
        for i in 0..9 {
            let x = if end < 0.0 { -18.0 - i as f32 * 5.0 } else { 118.0 + i as f32 * 5.0 };
            pine(mesh, Vec3::new(x, -26.0 + i as f32 * 6.5, 0.0), 6.0 + rand() * 3.0, rand);
        }
    }
}

/// A wall of tall windows: a sill below, a head above, and three courses of the
/// night outside between them, with a mullion on each bay and a transom on each
/// course line. The wall runs along x when `along_x`, and along y when not.
fn window_band(
    mesh: &mut Mesh,
    along_x: bool,
    at: f32,
    from: f32,
    to: f32,
    z_sill: f32,
    z_head: f32,
) {
    // The night outside, brighter than the wall it sits in, so the band reads
    // as glass from the far end of the building and not as a painted stripe.
    let night = [
        Rgb::hex(0x35485C),
        Rgb::hex(0x4E6E86),
        Rgb::hex(0x7496AE),
    ];
    let frame = Rgb::hex(0x1E242C);
    let p = |s: f32, z: f32| {
        if along_x {
            Vec3::new(s, at, z)
        } else {
            Vec3::new(at, s, z)
        }
    };
    let courses = night.len() as f32;
    // The glass, in courses: darkest at the floor, opening up toward the roof.
    for (i, tone) in night.iter().enumerate() {
        let za = z_sill + (z_head - z_sill) * i as f32 / courses;
        let zb = z_sill + (z_head - z_sill) * (i as f32 + 1.0) / courses;
        mesh.quad_shaded(p(from, za), p(to, za), p(to, zb), p(from, zb), *tone, 1.0);
    }
    let len = (to - from).abs();
    // A mullion on each bay, and a transom on each course line.
    let bays = (len / 9.0).round().max(1.0);
    for i in 0..=bays as usize {
        let s = from + (to - from) * i as f32 / bays;
        let half = if along_x {
            Vec3::new(0.3, 0.2, (z_head - z_sill) * 0.5)
        } else {
            Vec3::new(0.2, 0.3, (z_head - z_sill) * 0.5)
        };
        mesh.box_shaded(p(s, (z_sill + z_head) * 0.5), half, 0.0, frame, 1.0);
    }
    for i in 0..=night.len() {
        let z = z_sill + (z_head - z_sill) * i as f32 / courses;
        let half = if along_x {
            Vec3::new(len * 0.5, 0.2, 0.16)
        } else {
            Vec3::new(0.2, len * 0.5, 0.16)
        };
        mesh.box_shaded(p((from + to) * 0.5, z), half, 0.0, frame, 1.0);
    }
}

/// The practice with the roof on: four walls and a ceiling over the whole site,
/// nothing in the seats, and a band of big windows down both sides and through
/// both ends — the kind of glass a modern indoor stadium has, tall enough to
/// show the night rather than a slit of it. The lens sits under the roof, so the
/// ceiling is above the camera rather than in front of it.
fn closed_practice(mesh: &mut Mesh) {
    let steel = Rgb::hex(0x39424E);
    let wall = Rgb::hex(0x2A323C);
    let x0 = -30.0f32;
    let x1 = 130.0f32;
    let y = 34.0f32;
    let z_top = 46.0f32;
    // The ceiling, in panels, so it reads as a roof rather than a flat plate.
    let mut x = x0;
    while x < x1 {
        let tone = if ((x - x0) / 10.0) as i32 % 2 == 0 { steel } else { steel.scale(1.12) };
        mesh.quad_shaded(
            Vec3::new(x, -y, z_top),
            Vec3::new(x + 10.0, -y, z_top),
            Vec3::new(x + 10.0, y, z_top),
            Vec3::new(x, y, z_top),
            tone,
            1.0,
        );
        // A beam on the underside of each seam.
        mesh.box_shaded(
            Vec3::new(x, 0.0, z_top - 0.35),
            Vec3::new(0.3, y, 0.35),
            0.0,
            Rgb::hex(0x1E242C),
            1.0,
        );
        x += 10.0;
    }
    // Walls from the top of the bowl to the roof, on all four sides, with the
    // window band through the middle of them. The ends get the taller glass.
    let z_sill = 12.0f32;
    let z_head = 40.0f32;
    for side in [-1.0f32, 1.0] {
        window_band(mesh, true, side * y, x0, x1, z_sill, z_head);
        for (z0, z1) in [(9.0f32, z_sill), (z_head, z_top)] {
            mesh.quad_shaded(
                Vec3::new(x0, side * y, z0),
                Vec3::new(x1, side * y, z0),
                Vec3::new(x1, side * y, z1),
                Vec3::new(x0, side * y, z1),
                wall,
                1.0,
            );
        }
    }
    for (x, flip) in [(x0, 1.0f32), (x1, -1.0)] {
        window_band(mesh, false, x, -y, y, 6.0, z_head);
        for (z0, z1) in [(3.0f32, 6.0), (z_head, z_top)] {
            mesh.quad_shaded(
                Vec3::new(x, -y, z0),
                Vec3::new(x, y, z0),
                Vec3::new(x, y, z1),
                Vec3::new(x, -y, z1),
                wall.scale(if flip > 0.0 { 1.0 } else { 0.9 }),
                1.0,
            );
        }
    }
}

fn city_scenery(city: City, teams: &[Team; 2], mesh: &mut Mesh, rand: &mut dyn FnMut() -> f32) {
    if city.water {
        water(mesh, -30.0, 130.0, -78.0, -40.0, Rgb::hex(0x1E4E66));
    }
    // The far side: the city, seen over the stand.
    let mut x = -34.0;
    for i in 0..city.towers {
        let w = 3.0 + rand() * 4.0;
        let h = 9.0 + rand() * 15.0 + i as f32 * 1.4;
        let y = -(44.0 + rand() * 16.0);
        tower(
            mesh,
            Vec3::new(x, y, 0.0) + Vec3::new(0.0, 0.0, h * 0.5),
            Vec3::new(w, 3.0 + rand() * 2.0, h * 0.5),
            1.0,
            teams[0].primary.lerp(Rgb::hex(0x2A3440), 0.45),
            rand,
        );
        x += w * 2.0 + 2.0 + rand() * 4.0;
    }
    if city.spire {
        let at = Vec3::new(138.0, 12.0, 0.0);
        mesh.cylinder(at, at + Vec3::new(0.0, 0.0, 26.0), 1.3, 0.5, 6, Rgb::hex(0x3A4654));
        mesh.cylinder(
            at + Vec3::new(0.0, 0.0, 26.0),
            at + Vec3::new(0.0, 0.0, 31.0),
            0.5,
            0.0,
            6,
            teams[1].accent,
        );
    }
    let mut x = -30.0;
    for _ in 0..city.brick {
        let half = Vec3::new(3.0 + rand() * 2.5, 3.0, 1.8 + rand() * 1.6);
        brick_block(
            mesh,
            Vec3::new(x, 46.0 + rand() * 10.0, half.z),
            half,
            -1.0,
            Rgb::hex(0x6E3A2E).lerp(teams[1].primary, 0.25),
            rand,
        );
        x += half.x * 2.0 + 1.4;
    }
    for _ in 0..city.stacks {
        let px = -20.0 + rand() * 130.0;
        let h = 12.0 + rand() * 10.0;
        let base = Vec3::new(px, 60.0 + rand() * 8.0, 0.0);
        mesh.cylinder(base, base + Vec3::new(0.0, 0.0, h), 1.0, 0.55, 7, Rgb::hex(0x5A3A34));
        // A band of warning paint, so a stack reads as a stack at distance.
        mesh.cylinder(
            base + Vec3::new(0.0, 0.0, h * 0.72),
            base + Vec3::new(0.0, 0.0, h * 0.78),
            0.72,
            0.69,
            7,
            Rgb::hex(0xD8D2C0),
        );
    }
    // The end of the ground is what the shot down the field sees, so the city
    // gets a row there as well: the skyline is not only down the sides.
    for end in [-1.0f32, 1.0] {
        let base = if end < 0.0 { -34.0f32 } else { 128.0 };
        let count = (city.towers / 2).max(2);
        for i in 0..count {
            let w = 3.0 + rand() * 3.5;
            let h = 13.0 + rand() * 14.0;
            let y = -22.0 + i as f32 * (44.0 / count as f32) + rand() * 4.0;
            tower(
                mesh,
                Vec3::new(base + rand() * 12.0, y, h * 0.5),
                Vec3::new(w, 3.0, h * 0.5),
                if y > 0.0 { -1.0 } else { 1.0 },
                teams[1].primary.lerp(Rgb::hex(0x2A3440), 0.5),
                rand,
            );
        }
    }
    for _ in 0..city.palms {
        palm(
            mesh,
            Vec3::new(-24.0 + rand() * 140.0, 33.0 + rand() * 8.0, 0.0),
            5.0 + rand() * 3.5,
            rand,
        );
    }
    for _ in 0..city.pines {
        pine(
            mesh,
            Vec3::new(-24.0 + rand() * 140.0, 32.0 + rand() * 9.0, 0.0),
            5.0 + rand() * 4.5,
            rand,
        );
    }
}

fn underwater(mesh: &mut Mesh, rand: &mut dyn FnMut() -> f32) {
    // The water outside the glass: a wall of it on every side, dropping away.
    let deep = Rgb::hex(0x063D52);
    let deeper = Rgb::hex(0x03202E);
    for side in [-1.0f32, 1.0] {
        let y = side * 66.0;
        for band in 0..6 {
            let z0 = band as f32 * 5.0;
            let c = deep.lerp(deeper, band as f32 / 6.0);
            let (a, b) = if side < 0.0 { (y - side * 40.0, y) } else { (y, y + 40.0) };
            mesh.quad_shaded(
                Vec3::new(-60.0, a, z0),
                Vec3::new(170.0, a, z0),
                Vec3::new(170.0, b, z0 + 8.0),
                Vec3::new(-60.0, b, z0 + 8.0),
                c,
                1.0,
            );
        }
    }
    // Seabed, with a few rocks for the light to fall on.
    mesh.quad_shaded(
        Vec3::new(-60.0, -110.0, -6.0),
        Vec3::new(170.0, -110.0, -6.0),
        Vec3::new(170.0, 110.0, -6.0),
        Vec3::new(-60.0, 110.0, -6.0),
        Rgb::hex(0x2A3A34),
        1.0,
    );
    for _ in 0..14 {
        rock(
            mesh,
            Vec3::new(-40.0 + rand() * 180.0, -80.0 + rand() * 160.0, -6.0),
            2.0 + rand() * 4.0,
            Rgb::hex(0x3E4A42),
            rand,
        );
    }
    // The ends, so the shot down the field is water and glass, not a gap.
    for end in [-1.0f32, 1.0] {
        let x = if end < 0.0 { -70.0 } else { 160.0 };
        for band in 0..6 {
            let z0 = band as f32 * 5.0;
            let c = deep.lerp(deeper, band as f32 / 6.0);
            mesh.quad_shaded(
                Vec3::new(x, -70.0, z0),
                Vec3::new(x, 70.0, z0),
                Vec3::new(x - end * 40.0, 70.0, z0 + 8.0),
                Vec3::new(x - end * 40.0, -70.0, z0 + 8.0),
                c,
                1.0,
            );
        }
    }
    // The glass: a clear dome over the field. Pale panes in a light frame, held
    // up by ribs that follow the vault, so the sea above and the water beyond
    // read through the roof instead of being painted over by it.
    let glass = Rgb::hex(0xC8E6EE);
    let frame = Rgb::hex(0x8FB2BA);
    let eave_y = 32.0f32;
    let eave_z = 30.0f32;
    let crown_z = 54.0f32;
    let strips = 8;
    // The cross-section of the vault: y across the field, z up to the crown.
    let curve = |t: f32| {
        let y = -eave_y + 2.0 * eave_y * t;
        (y, eave_z + (crown_z - eave_z) * 4.0 * t * (1.0 - t))
    };
    let mut x = -24.0;
    let mut bay = 0;
    while x < 132.0 {
        let tone = if bay % 2 == 0 { glass } else { glass.scale(0.94) };
        for s in 0..strips {
            let (y0, z0) = curve(s as f32 / strips as f32);
            let (y1, z1) = curve((s + 1) as f32 / strips as f32);
            mesh.quad_shaded(
                Vec3::new(x, y0, z0),
                Vec3::new(x + 13.0, y0, z0),
                Vec3::new(x + 13.0, y1, z1),
                Vec3::new(x, y1, z1),
                tone,
                1.0,
            );
        }
        // A rib on the bay line, following the vault over the field.
        let mut prev = curve(0.0);
        for s in 1..=strips {
            let cur = curve(s as f32 / strips as f32);
            mesh.quad_shaded(
                Vec3::new(x - 0.22, prev.0, prev.1 + 0.06),
                Vec3::new(x + 0.22, prev.0, prev.1 + 0.06),
                Vec3::new(x + 0.22, cur.0, cur.1 + 0.06),
                Vec3::new(x - 0.22, cur.0, cur.1 + 0.06),
                frame,
                1.0,
            );
            prev = cur;
        }
        x += 13.0;
        bay += 1;
    }
    // The lights down the crown of the vault: clear glass over a lit field.
    mesh.quad_shaded(
        Vec3::new(-24.0, -0.7, crown_z - 0.2),
        Vec3::new(132.0, -0.7, crown_z - 0.2),
        Vec3::new(132.0, 0.7, crown_z - 0.2),
        Vec3::new(-24.0, 0.7, crown_z - 0.2),
        Rgb::hex(0xF4F0DA),
        1.0,
    );
    // Clear glass from the top of the bowl up to the eaves, with a mullion on
    // every bay so the wall still reads as built.
    for side in [-1.0f32, 1.0] {
        mesh.quad_shaded(
            Vec3::new(-24.0, side * eave_y, 9.0),
            Vec3::new(132.0, side * eave_y, 9.0),
            Vec3::new(132.0, side * eave_y, eave_z),
            Vec3::new(-24.0, side * eave_y, eave_z),
            glass.scale(0.9),
            1.0,
        );
        let mut mx = -24.0;
        while mx <= 132.0 {
            mesh.box_shaded(
                Vec3::new(mx, side * eave_y, (9.0 + eave_z) * 0.5),
                Vec3::new(0.18, 0.18, (eave_z - 9.0) * 0.5),
                0.0,
                frame,
                1.0,
            );
            mx += 13.0;
        }
    }
    // The ends fill the arch: a pane per strip, from the floor to the vault.
    for (x, tone) in [(-24.0f32, glass.scale(0.86)), (132.0, glass.scale(0.8))] {
        for s in 0..strips {
            let (y0, z0) = curve(s as f32 / strips as f32);
            let (y1, z1) = curve((s + 1) as f32 / strips as f32);
            mesh.quad_shaded(
                Vec3::new(x, y0, 3.0),
                Vec3::new(x, y1, 3.0),
                Vec3::new(x, y1, z1),
                Vec3::new(x, y0, z0),
                tone,
                1.0,
            );
        }
    }
}

fn outback(mesh: &mut Mesh, rand: &mut dyn FnMut() -> f32) {
    let dirt = Rgb::hex(0x8A4A32);
    // The dirt goes round the field, never over it: four bands outside the
    // paint, so the turf is still turf.
    for side in [-1.0f32, 1.0] {
        mesh.quad_shaded(
            Vec3::new(-60.0, side * 30.0, 0.008),
            Vec3::new(170.0, side * 30.0, 0.008),
            Vec3::new(170.0, side * 95.0, 0.008),
            Vec3::new(-60.0, side * 95.0, 0.008),
            dirt,
            1.0,
        );
    }
    for x0 in [-60.0f32, 120.0] {
        mesh.quad_shaded(
            Vec3::new(x0, -30.0, 0.008),
            Vec3::new(x0 + 50.0, -30.0, 0.008),
            Vec3::new(x0 + 50.0, 30.0, 0.008),
            Vec3::new(x0, 30.0, 0.008),
            dirt,
            1.0,
        );
    }
    // Mesas on the horizon: flat-topped, red, and further out than the eye.
    for _ in 0..7 {
        let w = 10.0 + rand() * 16.0;
        let h = 5.0 + rand() * 9.0;
        let at = Vec3::new(-50.0 + rand() * 200.0, if rand() < 0.5 { -1.0 } else { 1.0 } * (80.0 + rand() * 30.0), h * 0.5);
        mesh.box_shaded(at, Vec3::new(w, 6.0 + rand() * 6.0, h * 0.5), 0.0, Rgb::hex(0x9E5230), 1.0);
        mesh.box_shaded(
            at + Vec3::new(0.0, 0.0, h * 0.5 + 0.3),
            Vec3::new(w * 1.04, 6.6, 0.3),
            0.0,
            Rgb::hex(0xB86A3E),
            1.0,
        );
    }
    // The water tower that every ground out here has.
    let base = Vec3::new(136.0, 10.0, 0.0);
    for (dx, dy) in [(-1.6f32, -1.2f32), (1.6, -1.2), (-1.6, 1.2), (1.6, 1.2)] {
        mesh.cylinder(
            base + Vec3::new(dx, dy, 0.0),
            base + Vec3::new(dx * 0.75, dy * 0.75, 8.0),
            0.18,
            0.14,
            5,
            Rgb::hex(0x6E6A5E),
        );
    }
    mesh.cylinder(base + Vec3::new(0.0, 0.0, 7.2), base + Vec3::new(0.0, 0.0, 10.4), 2.6, 2.6, 9, Rgb::hex(0xA8A294));
    mesh.cylinder(base + Vec3::new(0.0, 0.0, 10.4), base + Vec3::new(0.0, 0.0, 11.2), 2.7, 0.4, 9, Rgb::hex(0x7E786C));
    // A windmill, its wheel still.
    let mill = Vec3::new(-14.0, -40.0, 0.0);
    mesh.cylinder(mill, mill + Vec3::new(0.0, 0.0, 9.0), 0.22, 0.16, 5, Rgb::hex(0x6E6A5E));
    let hub = mill + Vec3::new(0.0, 0.0, 9.0);
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let dir = Vec3::new(0.0, 0.0, 0.0);
        let _ = dir;
        mesh.tri(
            hub,
            hub + Vec3::new(a.cos() * 2.0, 0.0, a.sin() * 2.0),
            hub + Vec3::new((a + 0.35).cos() * 2.0, 0.0, (a + 0.35).sin() * 2.0),
            Rgb::hex(0x8E8A7E),
        );
    }
    for i in 0..5 {
        let w = 12.0 + rand() * 14.0;
        let h = 7.0 + rand() * 8.0;
        let x = if i % 2 == 0 { 170.0 } else { -55.0 };
        mesh.box_shaded(
            Vec3::new(x + rand() * 16.0, -20.0 + i as f32 * 10.0, h * 0.5),
            Vec3::new(w, 14.0, h * 0.5),
            0.0,
            Rgb::hex(0x9E5230),
            1.0,
        );
    }
    for _ in 0..26 {
        rock(
            mesh,
            Vec3::new(-30.0 + rand() * 160.0, if rand() < 0.5 { -1.0 } else { 1.0 } * (30.0 + rand() * 24.0), 0.0),
            0.5 + rand() * 1.6,
            Rgb::hex(0x7A4630),
            rand,
        );
    }
    for _ in 0..10 {
        // Scrub: a squat dark bush.
        rock(
            mesh,
            Vec3::new(-20.0 + rand() * 140.0, if rand() < 0.5 { -1.0 } else { 1.0 } * (28.0 + rand() * 8.0), 0.0),
            0.6 + rand() * 0.7,
            Rgb::hex(0x4A5232),
            rand,
        );
    }
}

fn beach(mesh: &mut Mesh, rand: &mut dyn FnMut() -> f32) {
    // Sand beyond the boards on the resort side; the sea on the other.
    mesh.quad_shaded(
        Vec3::new(-60.0, -120.0, 0.008),
        Vec3::new(170.0, -120.0, 0.008),
        Vec3::new(170.0, -34.0, 0.008),
        Vec3::new(-60.0, -34.0, 0.008),
        Rgb::hex(0x1C7C96),
        1.0,
    );
    water(mesh, -60.0, 170.0, -110.0, -40.0, Rgb::hex(0x1E8AA8));
    // Breakers: three white lines out from the beach.
    for (i, y) in [-44.0f32, -52.0, -64.0].iter().enumerate() {
        let w = 0.8 - i as f32 * 0.2;
        mesh.quad_shaded(
            Vec3::new(-60.0, *y, 0.03),
            Vec3::new(170.0, *y, 0.03),
            Vec3::new(170.0, *y + w, 0.03),
            Vec3::new(-60.0, *y + w, 0.03),
            Rgb::hex(0xDFF2F2),
            1.0,
        );
    }
    mesh.quad_shaded(
        Vec3::new(-60.0, 34.0, 0.008),
        Vec3::new(170.0, 34.0, 0.008),
        Vec3::new(170.0, 120.0, 0.008),
        Vec3::new(-60.0, 120.0, 0.008),
        Rgb::hex(0xD8C08A),
        1.0,
    );
    // The resort: three towers, balconies, and a lit sign.
    for i in 0..3 {
        let h = 16.0 + i as f32 * 6.0;
        let at = Vec3::new(10.0 + i as f32 * 34.0, 56.0, h * 0.5);
        mesh.box_shaded(at, Vec3::new(6.0, 4.0, h * 0.5), 0.0, Rgb::hex(0xE8E2D4), 1.0);
        for floor in 0..((h / 2.4) as usize).min(12) {
            let z = 1.6 + floor as f32 * 2.4;
            mesh.quad_shaded(
                Vec3::new(at.x - 6.2, 51.8, z),
                Vec3::new(at.x + 6.2, 51.8, z),
                Vec3::new(at.x + 6.2, 51.8, z + 0.5),
                Vec3::new(at.x - 6.2, 51.8, z + 0.5),
                Rgb::hex(0x3A6E7E),
                1.0,
            );
        }
        mesh.box_shaded(
            Vec3::new(at.x, 56.0, h + 0.8),
            Vec3::new(4.0, 0.3, 0.8),
            0.0,
            Rgb::hex(0xF2C14E),
            1.0,
        );
    }
    for _ in 0..9 {
        palm(
            mesh,
            Vec3::new(-24.0 + rand() * 140.0, 36.0 + rand() * 5.0, 0.0),
            6.0 + rand() * 4.0,
            rand,
        );
    }
    // A last block of the resort past the far end zone, and palms along the
    // near end, so the beach reads in the shot down the field as well.
    mesh.box_shaded(
        Vec3::new(142.0, 14.0, 9.0),
        Vec3::new(9.0, 12.0, 9.0),
        0.0,
        Rgb::hex(0xE8E2D4),
        1.0,
    );
    for i in 0..7 {
        mesh.quad_shaded(
            Vec3::new(132.8, -12.0 + i as f32 * 8.0, 2.0),
            Vec3::new(132.8, -8.0 + i as f32 * 8.0, 2.0),
            Vec3::new(132.8, -8.0 + i as f32 * 8.0, 8.0),
            Vec3::new(132.8, -12.0 + i as f32 * 8.0, 8.0),
            Rgb::hex(0x3A6E7E),
            1.0,
        );
    }
    for i in 0..5 {
        palm(mesh, Vec3::new(-24.0, -20.0 + i as f32 * 10.0, 0.0), 7.0 + rand() * 2.0, rand);
    }
    // Umbrellas along the sand.
    for i in 0..10 {
        let x = -14.0 + i as f32 * 13.0;
        let at = Vec3::new(x, 37.0 + rand() * 3.0, 0.0);
        mesh.cylinder(at, at + Vec3::new(0.0, 0.0, 2.4), 0.08, 0.06, 5, Rgb::hex(0xD8D2C0));
        let canopy = if i % 2 == 0 { Rgb::hex(0xE4572E) } else { Rgb::hex(0xE8E2D4) };
        mesh.cylinder(
            at + Vec3::new(0.0, 0.0, 2.4),
            at + Vec3::new(0.0, 0.0, 2.9),
            1.7,
            0.0,
            8,
            canopy,
        );
    }
}

fn championship(teams: &[Team; 2], mesh: &mut Mesh, rand: &mut dyn FnMut() -> f32) {
    let gold = Rgb::hex(0xE8C46A);
    // Bunting down both stands, in both clubs' colours.
    bunting(mesh, -14.0, 114.0, HALF_WIDTH + 5.2, 8.5, [teams[0].primary, gold, teams[1].primary]);
    bunting(mesh, -14.0, 114.0, -(HALF_WIDTH + 5.2), 8.5, [teams[1].primary, gold, teams[0].primary]);
    // A wall behind each end zone in that end's club colour, with its mark on
    // it: the whole point of the final is that both clubs are in the building.
    for (end, team) in [(-1.0f32, &teams[0]), (1.0f32, &teams[1])] {
        let x = if end < 0.0 { -20.0 } else { 120.0 };
        mesh.box_shaded(
            Vec3::new(x, 0.0, 6.0),
            Vec3::new(0.5, 30.0, 6.0),
            0.0,
            team.primary,
            1.0,
        );
        let face = x - end * 0.55;
        crate::broadcast::paint_word(
            mesh,
            &team.abbr.to_uppercase(),
            Vec3::new(face, 8.0, 8.6),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            0.8,
            team.accent,
        );
        // Flags either side of the wall, in the club's colours.
        for i in 0..8 {
            let y = -24.0 + i as f32 * 7.0;
            mesh.quad_shaded(
                Vec3::new(face, y, 2.0),
                Vec3::new(face, y + 4.0, 2.0),
                Vec3::new(face, y + 4.0, 8.0),
                Vec3::new(face, y, 8.0),
                if i % 2 == 0 { team.primary } else { team.accent },
                1.0,
            );
        }
    }
    // Confetti in the air over the field: a final should look like one.
    for _ in 0..70 {
        let at = Vec3::new(-10.0 + rand() * 120.0, -26.0 + rand() * 52.0, 6.0 + rand() * 16.0);
        let color = [teams[0].primary, teams[1].primary, gold][(rand() * 3.0) as usize % 3];
        mesh.quad_shaded(
            at,
            at + Vec3::new(0.35, 0.15, 0.0),
            at + Vec3::new(0.35, 0.15, -0.35),
            at + Vec3::new(0.0, 0.0, -0.35),
            color,
            1.0,
        );
    }
    // The trophy, painted on the fascia over the near side.
    crate::broadcast::paint_word(
        mesh,
        "CHAMPIONSHIP",
        Vec3::new(50.0, -HALF_WIDTH - 5.45, 9.3),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        0.5,
        gold,
    );
}

// ---------------------------------------------------------------------------
// Fish
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct FishFigure {
    pub radius: f32,
    pub height: f32,
    pub phase: f32,
    pub speed: f32,
    pub size: f32,
    pub color: Rgb,
}

/// A school for the dome. Fixed at build time, so the same fish are in the same
/// places on every replay.
pub fn school(seed: u32) -> Vec<FishFigure> {
    let mut seed = seed | 1;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / 16_777_215.0
    };
    let palette = [
        Rgb::hex(0xF2A65A),
        Rgb::hex(0xE8D26A),
        Rgb::hex(0x7AD8D8),
        Rgb::hex(0xD88AA8),
        Rgb::hex(0xB8C8D8),
    ];
    (0..34)
        .map(|i| FishFigure {
            radius: 34.0 + rand() * 30.0,
            height: 9.0 + rand() * 16.0,
            phase: rand() * std::f32::consts::TAU,
            speed: (0.10 + rand() * 0.14) * if i % 3 == 0 { -1.0 } else { 1.0 },
            size: 0.5 + rand() * 0.9,
            color: palette[(rand() * palette.len() as f32) as usize % palette.len()],
        })
        .collect()
}

/// The school, swimming: rebuilt every frame, because placing a mesh bakes it.
pub fn school_frame(fish: &[FishFigure], t: f32) -> Mesh {
    let mut mesh = Mesh::new();
    for f in fish {
        let a = f.phase + t * f.speed;
        let at = Vec3::new(50.0 + a.cos() * f.radius, a.sin() * f.radius, f.height);
        // Facing along the tangent of the circle.
        let dir = Vec3::new(-a.sin(), a.cos(), 0.0) * f.speed.signum();
        let side = Vec3::new(-dir.y, dir.x, 0.0);
        let s = f.size;
        let nose = at + dir * s;
        let tail = at - dir * s;
        // Body: two triangles, flat, so it reads from either side.
        mesh.tri(nose, at + side * s * 0.35, tail, f.color);
        mesh.tri(nose, tail, at - side * s * 0.35, f.color.scale(0.9));
        // Tail fin, flicking as it swims.
        let flick = (t * 4.0 + f.phase).sin() * 0.25 * s;
        mesh.tri(
            tail,
            tail - dir * s * 0.6 + side * (s * 0.45 + flick),
            tail - dir * s * 0.6 - side * (s * 0.45 - flick),
            f.color.lerp(Rgb::WHITE, 0.25),
        );
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use a7fl_core::team::league_teams;
    use crate::broadcast::Broadcast;

    #[test]
    fn every_ground_is_named_and_described() {
        let mut names = Vec::new();
        for i in 0..COUNT {
            let v = Venue::from_index(i);
            assert!(!v.name().is_empty(), "venue {i} has no name");
            assert!(!v.sub().is_empty(), "venue {i} has no subtitle");
            assert!(!v.blurb().is_empty(), "venue {i} has no blurb");
            assert!(!names.contains(&v.name()), "two grounds called {}", v.name());
            names.push(v.name());
        }
        assert_eq!(names.len(), COUNT);
    }

    #[test]
    fn the_practice_has_a_crowd_and_the_closed_practice_does_not() {
        let teams = league_teams();
        let open = Broadcast::with_venue(
            160,
            90,
            [teams[0].clone(), teams[1].clone()],
            Venue::from_index(0),
        );
        let shut = Broadcast::with_venue(
            160,
            90,
            [teams[0].clone(), teams[1].clone()],
            Venue::from_index(1),
        );
        assert!(Venue::from_index(0).has_crowd());
        assert!(!Venue::from_index(1).has_crowd());
        assert!(
            open.stand_triangles() > shut.stand_triangles(),
            "the empty ground has as many people in it as the full one: {} against {}",
            shut.stand_triangles(),
            open.stand_triangles()
        );
    }

    #[test]
    fn the_school_swims() {
        let fish = school(7);
        assert!(fish.len() > 10, "only {} fish", fish.len());
        let a = school_frame(&fish, 0.0);
        let b = school_frame(&fish, 1.0);
        assert!(!a.tris.is_empty());
        assert!(
            a.tris[0].a != b.tris[0].a,
            "the school is not moving between frames"
        );
    }

    /// The field belongs to the players. Everything a ground brings with it —
    /// trees, rocks, towers, water — sits outside the paint or above it, never
    /// on it. The final is the one exception: its confetti hangs over the field
    /// on purpose, so it is left out of the sweep.
    #[test]
    fn nothing_is_standing_on_the_field() {
        use a7fl_core::field::{BACK_OF_LEFT_ENDZONE, BACK_OF_RIGHT_ENDZONE, FIELD_HALF_WIDTH};
        let teams = league_teams();
        for i in 0..COUNT {
            let venue = Venue::from_index(i);
            if matches!(venue.kind(), Kind::Championship) {
                continue;
            }
            let b = Broadcast::with_venue(80, 45, [teams[0].clone(), teams[1].clone()], venue);
            for tri in &b.stands().tris {
                for p in [tri.a, tri.b, tri.c] {
                    // Low enough to be standing on the turf, rather than a roof
                    // or a light over it, and inside the paint.
                    let on_the_paint = p.z > 0.25
                        && p.z < 4.0
                        && p.x > BACK_OF_LEFT_ENDZONE + 0.5
                        && p.x < BACK_OF_RIGHT_ENDZONE - 0.5
                        && p.y.abs() < FIELD_HALF_WIDTH - 1.3;
                    assert!(
                        !on_the_paint,
                        "{} has something standing on the field at ({:.1}, {:.1}, {:.1})",
                        venue.name(),
                        p.x,
                        p.y,
                        p.z
                    );
                }
            }
        }
    }

    #[test]
    fn every_ground_builds_a_building() {
        let teams = league_teams();
        for i in 0..COUNT {
            let b = Broadcast::with_venue(
                80,
                45,
                [teams[0].clone(), teams[1].clone()],
                Venue::from_index(i),
            );
            assert!(
                b.stand_triangles() > 500,
                "{} has almost no building: {} triangles",
                Venue::from_index(i).name(),
                b.stand_triangles()
            );
        }
    }
}
