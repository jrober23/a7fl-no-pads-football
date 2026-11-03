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
            Kind::Practice => "A ground in the suburbs: rooflines and streetlights past the far end, and a crowd in the bowl.",
            Kind::ClosedPractice => "The same field with the roof down and nobody in the seats.",
            Kind::Home(k) => cities().get(k).map(|c| c.blurb).unwrap_or("A home ground."),
            Kind::Underwater => "A clear dome on the sand: coral heads, kelp standing in the current, and the fish swimming past.",
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
            Kind::Practice => [Rgb::hex(0x0A1A33), Rgb::hex(0x2A4C7C), Rgb::hex(0x8FB4D6)],
            Kind::ClosedPractice => [Rgb::hex(0x0B0F15), Rgb::hex(0x1A222D), Rgb::hex(0x3C4654)],
            Kind::Home(k) => cities()
                .get(k)
                .map(|c| c.sky)
                .unwrap_or([Rgb::hex(0x0A1428), Rgb::hex(0x20364E), Rgb::hex(0x7E8F96)]),
            Kind::Underwater => [Rgb::hex(0x02121C), Rgb::hex(0x0B5570), Rgb::hex(0x2FA8C0)],
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
            // The suburb is close enough to see: the houses keep their colour and
            // only the town on the horizon fades into the blue.
            Kind::Practice => {
                light.fog_density = 0.004;
            }
            Kind::Underwater => {
                light.ambient = 0.66;
                light.fog_density = 0.006;
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
    /// The building the city is known by, standing over the downtown.
    landmark: Landmark,
    sky: [Rgb; 3],
}

/// The one building a city is known by. Each is built to be recognised from the
/// far end of a football field: the pencil in Atlanta, the Empire State in New
/// York, the needle in Seattle, and so on down the league.
#[derive(Clone, Copy)]
enum Landmark {
    CustomHouse,
    Willis,
    Renaissance,
    Foshay,
    Pencil,
    Freedom,
    Reunion,
    TwinSpires,
    Empire,
    CityHallTower,
    Obelisk,
    Crown,
    PyramidHall,
    Butte,
    Needle,
    CashRegister,
}

fn cities() -> Vec<City> {
    fn hex(v: u32) -> Rgb {
        Rgb::hex(v)
    }
    vec![
        // North
        City { suffix: "THE HARBOUR", landmark: Landmark::CustomHouse, blurb: "Brick rowhouses, a harbour full of masts, and the wind off the water.", towers: 3, brick: 6, palms: 0, pines: 0, stacks: 1, water: true, spire: false, sky: [hex(0x0A1428), hex(0x243A4E), hex(0x8A9AA0)] },
        City { suffix: "FOUNDRY YARD", landmark: Landmark::Willis, blurb: "Steel over the old works: stacks, gantries and a train that never stops.", towers: 2, brick: 5, palms: 0, pines: 0, stacks: 4, water: false, spire: false, sky: [hex(0x141018), hex(0x3A2A2A), hex(0xB08A6A)] },
        City { suffix: "ASSEMBLY GROUNDS", landmark: Landmark::Renaissance, blurb: "A factory town's field: brick, chimneys and the plant running nights.", towers: 2, brick: 6, palms: 0, pines: 0, stacks: 3, water: true, spire: false, sky: [hex(0x101820), hex(0x2E3C46), hex(0x93A3A6)] },
        City { suffix: "NORTHWIND FIELD", landmark: Landmark::Foshay, blurb: "Glass towers joined by skybridges, and pines standing in the cold.", towers: 4, brick: 1, palms: 0, pines: 8, stacks: 0, water: true, spire: false, sky: [hex(0x081C2E), hex(0x1E3C56), hex(0xAFC4CE)] },
        // South
        City { suffix: "FIREFLY PARK", landmark: Landmark::Pencil, blurb: "A warm night, glass towers, and the trees full of light.", towers: 4, brick: 2, palms: 0, pines: 6, stacks: 0, water: false, spire: false, sky: [hex(0x1A1030), hex(0x3E2A5E), hex(0xC79A8A)] },
        City { suffix: "RIPTIDE SHORES", landmark: Landmark::Freedom, blurb: "Pastel hotels, palms over the walk, and the ocean past the boards.", towers: 3, brick: 1, palms: 10, pines: 0, stacks: 0, water: true, spire: false, sky: [hex(0x123C5E), hex(0x3E8FB0), hex(0xF6E9C9)] },
        City { suffix: "RUSTLER RANCH", landmark: Landmark::Reunion, blurb: "Stockyards and derricks: low brick, tall steel, and a lot of sky.", towers: 3, brick: 5, palms: 0, pines: 0, stacks: 2, water: false, spire: false, sky: [hex(0x2E1A2E), hex(0x6E3E36), hex(0xE0A878)] },
        City { suffix: "BRASS HALL", landmark: Landmark::TwinSpires, blurb: "Neon down the street, a lit spire over it, and music in the walls.", towers: 3, brick: 4, palms: 0, pines: 0, stacks: 1, water: true, spire: true, sky: [hex(0x181026), hex(0x3A2448), hex(0xC98E6A)] },
        // East
        City { suffix: "BEACON HEIGHTS", landmark: Landmark::Empire, blurb: "The city at night: a wall of windows and a light on top of it.", towers: 6, brick: 2, palms: 0, pines: 0, stacks: 0, water: true, spire: true, sky: [hex(0x080E22), hex(0x1A2A4E), hex(0x8E9AB6)] },
        City { suffix: "ANVIL WORKS", landmark: Landmark::CityHallTower, blurb: "Brick, iron and a foundry glow: the oldest ground in the league.", towers: 2, brick: 6, palms: 0, pines: 0, stacks: 3, water: true, spire: false, sky: [hex(0x0E1218), hex(0x2A2E34), hex(0x9AA0A6)] },
        City { suffix: "MONUMENT GROUNDS", landmark: Landmark::Obelisk, blurb: "Marble and glass, a lit obelisk, and the capital behind the stand.", towers: 3, brick: 2, palms: 0, pines: 4, stacks: 0, water: true, spire: true, sky: [hex(0x0A1626), hex(0x22364E), hex(0xB6BCC2)] },
        City { suffix: "AVIATOR FIELD", landmark: Landmark::Crown, blurb: "Hangars and a control tower, with the city lights on the horizon.", towers: 3, brick: 3, palms: 0, pines: 5, stacks: 1, water: false, spire: false, sky: [hex(0x0C1A2E), hex(0x24405C), hex(0xA6B4BC)] },
        // West
        City { suffix: "COMET BOWL", landmark: Landmark::PyramidHall, blurb: "Palms, hills and a city that runs all the way to the ridge.", towers: 5, brick: 1, palms: 8, pines: 0, stacks: 0, water: false, spire: false, sky: [hex(0x140E2A), hex(0x4A2E5E), hex(0xE0A070)] },
        City { suffix: "SANDSTORM MESA", landmark: Landmark::Butte, blurb: "The desert at dusk: mesas, cactus and one bright strip of sky.", towers: 1, brick: 1, palms: 2, pines: 0, stacks: 0, water: false, spire: false, sky: [hex(0x2A1A38), hex(0x8A4A46), hex(0xE8B478)] },
        City { suffix: "RAINMAKER FIELD", landmark: Landmark::Needle, blurb: "Pines, water and glass: the rain comes in off the sound.", towers: 4, brick: 1, palms: 0, pines: 9, stacks: 0, water: true, spire: false, sky: [hex(0x0C1A20), hex(0x223C44), hex(0x8AA4A8)] },
        City { suffix: "ALTITUDE PARK", landmark: Landmark::CashRegister, blurb: "The mountains stand over the stand, and the ball carries up here.", towers: 2, brick: 1, palms: 0, pines: 8, stacks: 0, water: false, spire: false, sky: [hex(0x0A1A30), hex(0x244260), hex(0xC8D2DC)] },
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
    // A course per floor on the face the end-zone lens sees, so downtown is lit
    // on the side the camera comes down the field on.
    let x_face = at.x - (half.x + 0.04);
    let floors = ((half.z * 2.0 / 2.6) as usize).clamp(1, 14);
    for floor in 0..floors {
        let z = 1.4 + floor as f32 * 2.6;
        if z + 1.4 > half.z * 2.0 - 0.8 {
            break;
        }
        let tone = if rand() < 0.5 {
            Rgb::hex(0xE8C27E)
        } else {
            Rgb::hex(0x222A34)
        };
        mesh.box_shaded(
            Vec3::new(x_face, at.y, z + 0.7),
            Vec3::new(0.05, half.y * 0.88, 0.6),
            0.0,
            tone,
            1.0,
        );
    }
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

/// A two-storey house: siding, a pitched roof with a gable at either end, a
/// chimney, lit windows on the face turned toward the field, and a porch light
/// by the door. `face` points from the house at the field, along one axis.
fn house(
    mesh: &mut Mesh,
    at: Vec3,
    half: Vec3,
    face: Vec3,
    wall: Rgb,
    roof: Rgb,
    rand: &mut dyn FnMut() -> f32,
) {
    mesh.box_shaded(at, half, 0.0, wall, 1.0);
    // A foundation course, so the house sits on its lawn rather than in it.
    mesh.box_shaded(
        Vec3::new(at.x, at.y, 0.3),
        Vec3::new(half.x * 1.02, half.y * 1.02, 0.3),
        0.0,
        wall.scale(0.78),
        1.0,
    );
    // The ridge runs across the face, so the gable ends show from the field.
    let along_x = face.y.abs() > 0.5;
    let face_sign = if along_x { face.y } else { face.x };
    let long = if along_x { half.x } else { half.y };
    let short = if along_x { half.y } else { half.x };
    let p = |a: f32, b: f32, z: f32| {
        if along_x {
            Vec3::new(at.x + a, at.y + b, z)
        } else {
            Vec3::new(at.x + b, at.y + a, z)
        }
    };
    let eave = at.z + half.z;
    let ridge = eave + short * 0.72;
    let out = short * 1.14;
    let over = long * 1.06;
    // Both pitches wound the same way, so neither disappears when the face
    // turned the other way.
    mesh.quad_shaded(
        p(-over, out, eave),
        p(over, out, eave),
        p(over, 0.0, ridge),
        p(-over, 0.0, ridge),
        roof,
        1.0,
    );
    mesh.quad_shaded(
        p(over, -out, eave),
        p(-over, -out, eave),
        p(-over, 0.0, ridge),
        p(over, 0.0, ridge),
        roof.scale(0.92),
        1.0,
    );
    for a in [-long, long] {
        mesh.tri(p(a, -short, eave), p(a, short, eave), p(a, 0.0, ridge), wall.scale(0.94));
    }
    if rand() < 0.6 {
        let a = long * 0.45 * if rand() < 0.5 { -1.0 } else { 1.0 };
        let bottom = eave - 0.8;
        let top = ridge + 0.4;
        mesh.box_shaded(
            p(a, short * 0.3, (bottom + top) * 0.5),
            Vec3::new(0.45, 0.45, (top - bottom) * 0.5),
            0.0,
            Rgb::hex(0x5E4034),
            1.0,
        );
    }
    // Lit windows on the face turned toward the field: two floors, three bays.
    // They carry the house at night, so they are generous and warm.
    let w_off = face_sign * (short + 0.06);
    for floor in 0..2 {
        for col in 0..3 {
            let a = -long * 0.58 + (col as f32 + 0.5) * (long * 1.16 / 3.0);
            let z0 = at.z + half.z * (0.3 + floor as f32 * 0.9);
            let lit = rand() < 0.66;
            let color = if lit { Rgb::hex(0xFCDE9E) } else { Rgb::hex(0x1A2230) };
            mesh.quad_shaded(
                p(a - 0.58, w_off, z0),
                p(a + 0.58, w_off, z0),
                p(a + 0.58, w_off, z0 + 0.95),
                p(a - 0.58, w_off, z0 + 0.95),
                color,
                1.0,
            );
        }
    }
    // The porch light, and the door under it.
    if rand() < 0.7 {
        let a = long * 0.16;
        let door_z = at.z - half.z * 0.45;
        mesh.quad_shaded(
            p(a - 0.55, w_off, door_z),
            p(a + 0.55, w_off, door_z),
            p(a + 0.55, w_off, door_z + 1.9),
            p(a - 0.55, w_off, door_z + 1.9),
            Rgb::hex(0x2A2420),
            1.0,
        );
        mesh.quad_shaded(
            p(a + 0.7, w_off, door_z + 1.7),
            p(a + 1.1, w_off, door_z + 1.7),
            p(a + 1.1, w_off, door_z + 2.1),
            p(a + 0.7, w_off, door_z + 2.1),
            Rgb::hex(0xFFE9B8),
            1.0,
        );
    }
}

/// A streetlight /// A streetlight: a pole, an arm over the kerb, and a warm lamp on the end.
fn streetlight(mesh: &mut Mesh, at: Vec3, arm: Vec3) {
    let top = at + Vec3::new(0.0, 0.0, 8.0);
    mesh.cylinder(at, top, 0.18, 0.13, 6, Rgb::hex(0x2A313A));
    let head = top + arm;
    mesh.cylinder(top, head, 0.12, 0.1, 5, Rgb::hex(0x2A313A));
    mesh.box_shaded(head, Vec3::new(0.55, 0.32, 0.16), 0.0, Rgb::hex(0xF6DEA2), 1.0);
}

/// A straight street: asphalt, a kerb down each edge, and a dashed centre line
/// down the middle. `along_x` runs it along the field, otherwise across it.
fn road(mesh: &mut Mesh, along_x: bool, fixed: f32, from: f32, to: f32, half_width: f32) {
    let p = |s: f32, b: f32, z: f32| {
        if along_x {
            Vec3::new(s, fixed + b, z)
        } else {
            Vec3::new(fixed + b, s, z)
        }
    };
    mesh.quad_shaded(
        p(from, -half_width, 0.006),
        p(to, -half_width, 0.006),
        p(to, half_width, 0.006),
        p(from, half_width, 0.006),
        Rgb::hex(0x24282E),
        1.0,
    );
    // Kerbs, so the street has an edge instead of fading into the lawn.
    for edge in [-1.0f32, 1.0] {
        mesh.quad_shaded(
            p(from, edge * half_width, 0.007),
            p(to, edge * half_width, 0.007),
            p(to, edge * (half_width + 0.8), 0.007),
            p(from, edge * (half_width + 0.8), 0.007),
            Rgb::hex(0x5E6266),
            1.0,
        );
    }
    // A dashed centre line.
    let mut s = from;
    while s < to {
        let e = (s + 3.0).min(to);
        mesh.quad_shaded(
            p(s, -0.18, 0.008),
            p(e, -0.18, 0.008),
            p(e, 0.18, 0.008),
            p(s, 0.18, 0.008),
            Rgb::hex(0xB4A98E),
            1.0,
        );
        s += 7.0;
    }
}

fn practice_scenery(mesh: &mut Mesh, rand: &mut dyn FnMut() -> f32) {
    // The ground sits in the suburbs: a street behind each sideline and behind
    // each end, houses beyond them with the lights on, streetlights on the kerb,
    // and the city away off past the rooftops. All of it is outside the boards.
    let walls = [
        Rgb::hex(0x8C867C),
        Rgb::hex(0x7A7E82),
        Rgb::hex(0x93806A),
        Rgb::hex(0x6E7278),
        Rgb::hex(0x8A7A66),
        Rgb::hex(0x6E5A4A),
    ];
    let roofs = [
        Rgb::hex(0x6E5C4C),
        Rgb::hex(0x5A6068),
        Rgb::hex(0x75604C),
        Rgb::hex(0x4A525A),
    ];
    let pick = |rand: &mut dyn FnMut() -> f32, n: usize| (rand() * n as f32) as usize % n;

    // The ground the neighbourhood stands on: grass out past the houses in every
    // direction, so the shot over the rooflines lands on a suburb and not on the
    // sky. It is laid outside the boards, in four bands around the field.
    let lawn = Rgb::hex(0x24331F);
    for (x0, x1, y0, y1) in [
        (-520.0, 600.0, 26.5, 520.0),
        (-520.0, 600.0, -520.0, -26.5),
        (-520.0, -14.5, -26.5, 26.5),
        (114.5, 600.0, -26.5, 26.5),
    ] {
        mesh.quad_shaded(
            Vec3::new(x0, y0, 0.002),
            Vec3::new(x1, y0, 0.002),
            Vec3::new(x1, y1, 0.002),
            Vec3::new(x0, y1, 0.002),
            lawn,
            1.0,
        );
    }

    // The two side streets, and the houses along them. Everything on the +y side
    // sits outside the broadcast lens, which stands at y = 32.
    for side in [-1.0f32, 1.0] {
        road(mesh, true, side * 41.5, -90.0, 200.0, 3.5);
        let mut x = -48.0;
        while x < 158.0 {
            let w = 4.0 + rand() * 2.4;
            let d = 3.2 + rand() * 1.4;
            let h = 2.9 + rand() * 0.7;
            let at = Vec3::new(x + w, side * (51.0 + rand() * 9.0), h);
            let wall = walls[pick(rand, walls.len())];
            let roof = roofs[pick(rand, roofs.len())];
            house(mesh, at, Vec3::new(w, d, h), Vec3::new(0.0, -side, 0.0), wall, roof, rand);
            // A driveway from the kerb up to the front of the house.
            let near = at.y - side * d;
            mesh.quad_shaded(
                Vec3::new(at.x - 1.3, near, 0.009),
                Vec3::new(at.x + 1.3, near, 0.009),
                Vec3::new(at.x + 1.3, side * 45.0, 0.009),
                Vec3::new(at.x - 1.3, side * 45.0, 0.009),
                Rgb::hex(0x6A6A66),
                1.0,
            );
            x += w * 2.0 + 4.0 + rand() * 4.0;
        }
        let mut lx = -44.0;
        while lx < 158.0 {
            streetlight(mesh, Vec3::new(lx, side * 37.5, 0.0), Vec3::new(0.0, -side * 1.8, 0.0));
            lx += 21.0;
        }
        // Back-yard pines behind the houses, for the skyline over the roofs.
        let mut tx = -44.0;
        while tx < 156.0 {
            pine(
                mesh,
                Vec3::new(tx + rand() * 5.0, side * (62.0 + rand() * 12.0), 0.0),
                6.0 + rand() * 4.0,
                rand,
            );
            tx += 11.0 + rand() * 9.0;
        }
    }

    // The two end streets: houses looking back down the field, a second row
    // behind the first, and the rooftops of the neighbourhood past both. The
    // left row keeps the middle clear for the scoreboard.
    for (face, line) in [(1.0f32, -26.0f32), (-1.0, 126.0)] {
        road(mesh, false, line + face * 5.5, -90.0, 90.0, 3.5);
        for row in 0..2 {
            let back = row as f32 * 16.0;
            let mut y = -46.0;
            while y < 54.0 {
                let w = 3.8 + rand() * 2.2;
                let d = 3.0 + rand() * 1.4;
                let h = 2.4 + rand() * 0.5;
                if line < 0.0 && row == 0 && y < 14.0 && y + w * 2.0 > -14.0 {
                    // In front of the scoreboard, which gets the lot to itself.
                    y += 8.0;
                    continue;
                }
                let at = Vec3::new(
                    line - face * (d + 2.0 + rand() * 6.0 + back),
                    y + w,
                    h,
                );
                let wall = walls[pick(rand, walls.len())];
                let roof = roofs[pick(rand, roofs.len())];
                house(mesh, at, Vec3::new(d, w, h), Vec3::new(face, 0.0, 0.0), wall, roof, rand);
                y += w * 2.0 + 5.0 + rand() * 5.0;
            }
        }
        for i in 0..6 {
            streetlight(
                mesh,
                Vec3::new(line + face, -40.0 + i as f32 * 16.0, 0.0),
                Vec3::new(face * 1.8, 0.0, 0.0),
            );
        }
        // A treeline past the end houses, so the streets end in trees.
        for i in 0..9 {
            pine(
                mesh,
                Vec3::new(
                    line - face * (28.0 + rand() * 16.0),
                    -48.0 + i as f32 * 12.0 + rand() * 4.0,
                    0.0,
                ),
                6.0 + rand() * 4.0,
                rand,
            );
        }
        // And the neighbourhood beyond the trees: a few low rooftops with a
        // window or two lit, so the street has a town behind it rather than a
        // void — far enough off to be a horizon, not a wall of buildings.
        for i in 0..4 {
            let w = 8.0 + rand() * 8.0;
            let d = 5.0 + rand() * 4.0;
            let h = 4.5 + rand() * 4.0;
            let x = line - face * (96.0 + rand() * 34.0);
            let y = -62.0 + i as f32 * 32.0 + rand() * 10.0;
            mesh.box_shaded(
                Vec3::new(x, y, h * 0.5),
                Vec3::new(d, w, h * 0.5),
                0.0,
                Rgb::hex(0x3E444C),
                1.0,
            );
            let xf = x + face * (d + 0.05);
            let floors = (h / 3.0) as usize;
            for f in 0..floors {
                for c in 0..3 {
                    if rand() < 0.5 {
                        continue;
                    }
                    let z = 1.6 + f as f32 * 3.0;
                    let yy = y - w * 0.62 + (c as f32 + 0.5) * w * 0.62;
                    mesh.quad_shaded(
                        Vec3::new(xf, yy - 0.6, z),
                        Vec3::new(xf, yy + 0.6, z),
                        Vec3::new(xf, yy + 0.6, z + 0.9),
                        Vec3::new(xf, yy - 0.6, z + 0.9),
                        Rgb::hex(0xD8BC86),
                        1.0,
                    );
                }
            }
        }
    }

    // The practice shed.    // The practice shed.
    mesh.box_shaded(
        Vec3::new(-26.0, 26.0, 2.4),
        Vec3::new(7.0, 5.0, 2.4),
        0.0,
        Rgb::hex(0x3C4652),
        1.0,
    );

    // The city itself, low on the sky past the far rooftops: this is the
    // suburban part of somewhere, and the somewhere shows at night.
    for i in 0..6 {
        let w = 5.0 + rand() * 5.0;
        let h = 18.0 + rand() * 16.0 + i as f32 * 2.0;
        tower(
            mesh,
            Vec3::new(-30.0 + i as f32 * 28.0 + rand() * 8.0, -150.0 - rand() * 40.0, h * 0.5),
            Vec3::new(w, 4.0, h * 0.5),
            1.0,
            Rgb::hex(0x2A3440),
            rand,
        );
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

/// A low block with window courses: the downtown between the towers, the kind
/// that fills a city's streets rather than standing on the skyline alone.
fn mid_rise(mesh: &mut Mesh, at: Vec3, half: Vec3, face: f32, color: Rgb, rand: &mut dyn FnMut() -> f32) {
    mesh.box_shaded(at, half, 0.0, color, 1.0);
    mesh.box_shaded(
        Vec3::new(at.x, at.y, at.z + half.z + 0.16),
        Vec3::new(half.x * 1.04, half.y * 1.04, 0.16),
        0.0,
        color.scale(0.7),
        1.0,
    );
    let rows = ((half.z * 2.0 / 2.3) as usize).max(1);
    for row in 0..rows {
        let z = 1.5 + row as f32 * 2.3;
        if z + 0.8 > half.z * 2.0 {
            break;
        }
        let tone = if rand() < 0.55 {
            Rgb::hex(0xE8C27E)
        } else {
            Rgb::hex(0x232A34)
        };
        mesh.box_shaded(
            Vec3::new(at.x - (half.x + 0.04), at.y, z + 0.7),
            Vec3::new(0.05, half.y * 0.88, 0.6),
            0.0,
            tone,
            1.0,
        );
    }
    let y_face = at.y - face * (half.y + 0.04);
    for row in 0..rows {
        let z = 1.5 + row as f32 * 2.3;
        if z + 0.8 > half.z * 2.0 {
            break;
        }
        let lit = rand() < 0.55;
        let tone = if lit { Rgb::hex(0xE8C27E) } else { Rgb::hex(0x232A34) };
        mesh.quad_shaded(
            Vec3::new(at.x - half.x + 0.5, y_face, z),
            Vec3::new(at.x + half.x - 0.5, y_face, z),
            Vec3::new(at.x + half.x - 0.5, y_face, z + 0.85),
            Vec3::new(at.x - half.x + 0.5, y_face, z + 0.85),
            tone,
            1.0,
        );
    }
}

/// A landmark's own ground: the yard it stands on, and the two colours it is lit
/// in. The builders below are deliberately made of a handful of boxes and drums,
/// because at the distance the lens sees them, that is all a silhouette needs.
struct Site<'a, 'b> {
    mesh: &'a mut Mesh,
    rand: &'b mut dyn FnMut() -> f32,
    at: Vec3,
    /// Everything the builders below do is in yards at full size; this scales
    /// the whole landmark to what fits in the frame at the far end of the field.
    k: f32,
    c: Rgb,
    a: Rgb,
}

impl Site<'_, '_> {
    fn block(&mut self, dx: f32, dy: f32, z0: f32, hw: f32, hd: f32, h: f32, tone: f32) {
        let k = self.k;
        self.mesh.box_shaded(
            Vec3::new(
                self.at.x + dx * k,
                self.at.y + dy * k,
                self.at.z + (z0 + h * 0.5) * k,
            ),
            Vec3::new(hw * k, hd * k, h * 0.5 * k),
            0.0,
            self.c.scale(tone),
            1.0,
        );
    }

    /// A shaft or a drum: a cylinder with as few sides as it takes to read.
    fn taper(&mut self, dx: f32, dy: f32, z0: f32, h: f32, r0: f32, r1: f32, sides: usize, color: Rgb) {
        let k = self.k;
        let base = Vec3::new(self.at.x + dx * k, self.at.y + dy * k, self.at.z + z0 * k);
        self.mesh.cylinder(
            base,
            base + Vec3::new(0.0, 0.0, h * k),
            r0 * k,
            r1 * k,
            sides,
            color,
        );
    }

    /// A lit band: a clock face, a cornice, a crown of lights.
    fn band(&mut self, dx: f32, dy: f32, z: f32, hw: f32, h: f32, color: Rgb) {
        let k = self.k;
        self.mesh.box_shaded(
            Vec3::new(
                self.at.x + dx * k,
                self.at.y + dy * k,
                self.at.z + (z + h * 0.5) * k,
            ),
            Vec3::new(hw * k, 0.06 * k, h * 0.5 * k),
            0.0,
            color,
            1.0,
        );
    }

    /// Courses of windows, on the face turned toward the field and on the face
    /// turned toward the end-zone lens: a downtown seen from the far end of the
    /// field is all side wall.
    fn windows(&mut self, dx: f32, dy: f32, hw: f32, hd: f32, z0: f32, z1: f32) {
        let k = self.k;
        let lit = Rgb::hex(0xF2D08A);
        let dark = Rgb::hex(0x2A3440);
        let mut z = z0 + 1.0;
        while z + 0.9 < z1 {
            let mut x = -hw + 0.9;
            while x < hw - 0.6 {
                let tone = if (self.rand)() < 0.55 { lit } else { dark };
                self.mesh.box_shaded(
                    Vec3::new(
                        self.at.x + (dx + x) * k,
                        self.at.y + (dy + hd) * k,
                        self.at.z + (z + 0.4) * k,
                    ),
                    Vec3::new(0.5 * k, 0.06 * k, 0.4 * k),
                    0.0,
                    tone,
                    1.0,
                );
                self.mesh.box_shaded(
                    Vec3::new(
                        self.at.x + (dx - hw) * k,
                        self.at.y + (dy + x) * k,
                        self.at.z + (z + 0.4) * k,
                    ),
                    Vec3::new(0.06 * k, 0.5 * k, 0.4 * k),
                    0.0,
                    tone,
                    1.0,
                );
                x += 2.4;
            }
            z += 2.8;
        }
    }
}

/// Build the building a city is known by.
fn landmark(
    mesh: &mut Mesh,
    kind: Landmark,
    at: Vec3,
    k: f32,
    c: Rgb,
    a: Rgb,
    rand: &mut dyn FnMut() -> f32,
) {
    let mut site = Site { mesh, rand, at, k, c, a };
    match kind {
        // Boston: the Custom House tower, a clock over the harbour and a lantern
        // on top of it.
        Landmark::CustomHouse => {
            site.block(0.0, 0.0, 0.0, 5.0, 5.0, 10.0, 0.9);
            site.block(0.0, 0.0, 10.0, 3.0, 3.0, 32.0, 1.0);
            site.windows(0.0, 0.0, 3.0, 3.0, 15.0, 40.0);
            site.band(0.0, 3.05, 34.0, 2.4, 2.0, Rgb::hex(0xF4E7C2));
            site.taper(0.0, 0.0, 42.0, 5.0, 2.6, 1.8, 8, Rgb::hex(0x7E705A));
            site.taper(0.0, 0.0, 47.0, 4.0, 1.2, 0.15, 6, a);
        }
        // Chicago: the two antennas on the tall grey slab.
        Landmark::Willis => {
            site.block(0.0, 0.0, 0.0, 5.0, 5.0, 20.0, 0.92);
            site.block(0.0, 0.0, 20.0, 4.0, 4.0, 22.0, 1.0);
            site.block(0.0, 0.0, 42.0, 3.0, 3.0, 18.0, 0.86);
            site.windows(0.0, 0.0, 3.6, 3.6, 6.0, 56.0);
            site.taper(-1.5, 0.0, 60.0, 12.0, 0.3, 0.12, 5, Rgb::hex(0x9AA6B2));
            site.taper(1.5, 0.0, 60.0, 12.0, 0.3, 0.12, 5, Rgb::hex(0x9AA6B2));
        }
        // Detroit: the five drums of the Renaissance Center.
        Landmark::Renaissance => {
            site.block(0.0, 0.0, 0.0, 9.0, 6.0, 12.0, 0.88);
            for i in 0..5 {
                let dx = -7.0 + i as f32 * 3.5;
                let h = 26.0 + (1.0 - (i as f32 - 2.0).abs() * 0.35) * 10.0;
                site.taper(dx, 0.0, 12.0, h, 1.5, 1.5, 10, c.scale(0.9 + i as f32 * 0.03));
                site.taper(dx, 0.0, 12.0 + h - 1.0, 1.2, 1.65, 1.6, 10, a);
            }
        }
        // Minneapolis: the Foshay's obelisk on a slab.
        Landmark::Foshay => {
            site.block(0.0, 0.0, 0.0, 4.0, 4.0, 30.0, 0.94);
            site.windows(0.0, 0.0, 4.0, 4.0, 6.0, 28.0);
            site.band(0.0, 4.05, 28.0, 4.2, 1.2, a);
            site.taper(0.0, 0.0, 30.0, 12.0, 2.6, 0.5, 4, c);
            site.taper(0.0, 0.0, 42.0, 6.0, 0.25, 0.1, 4, Rgb::hex(0xB0BCC8));
        }
        // Atlanta: the pencil, a shaft with a lit pyramid on the end of it.
        Landmark::Pencil => {
            site.block(0.0, 0.0, 0.0, 5.0, 5.0, 16.0, 0.9);
            site.block(0.0, 0.0, 16.0, 2.0, 2.0, 46.0, 1.0);
            site.windows(0.0, 0.0, 2.0, 2.0, 18.0, 60.0);
            site.taper(0.0, 0.0, 62.0, 8.0, 2.9, 0.1, 4, a);
            site.taper(0.0, 0.0, 70.0, 4.0, 0.14, 0.05, 4, Rgb::hex(0xD8E0E8));
        }
        // Miami: the Freedom Tower, a dome over an arcade.
        Landmark::Freedom => {
            site.block(0.0, 0.0, 0.0, 6.0, 5.0, 12.0, 0.92);
            site.block(0.0, 0.0, 12.0, 3.4, 3.0, 16.0, 1.0);
            site.windows(0.0, 0.0, 3.4, 3.0, 13.0, 26.0);
            site.taper(0.0, 0.0, 28.0, 6.0, 3.0, 1.2, 10, Rgb::hex(0xE6D8C4));
            site.taper(0.0, 0.0, 34.0, 3.0, 0.8, 0.3, 8, Rgb::hex(0xF0E4CC));
            site.taper(0.0, 0.0, 36.0, 6.0, 0.3, 0.06, 6, a);
        }
        // Dallas: the ball on the stick.
        Landmark::Reunion => {
            site.taper(0.0, 0.0, 0.0, 34.0, 1.6, 1.2, 8, c.scale(0.95));
            for i in 0..4 {
                site.taper(0.0, 0.0, 21.0 + i as f32 * 4.0, 1.2, 1.9, 1.9, 8, a);
            }
            site.taper(0.0, 0.0, 34.0, 6.0, 5.2, 5.2, 12, c.scale(1.05));
            for i in 0..3 {
                site.taper(0.0, 0.0, 35.6 + i as f32 * 1.6, 0.9, 5.4, 5.4, 12, a);
            }
            site.taper(0.0, 0.0, 40.0, 18.0, 0.3, 0.08, 5, Rgb::hex(0xC8D2DC));
        }
        // Nashville: the two spires over the slab — the ones everybody in town
        // calls the bat.
        Landmark::TwinSpires => {
            site.block(0.0, 0.0, 0.0, 6.0, 3.0, 26.0, 0.94);
            site.windows(0.0, 0.0, 6.0, 3.0, 5.0, 25.0);
            site.block(0.0, 0.0, 26.0, 5.0, 2.4, 6.0, 0.8);
            for side in [-1.0f32, 1.0] {
                site.taper(side * 4.0, 0.0, 26.0, 2.0, 1.2, 1.1, 8, a);
                site.taper(side * 4.0, 0.0, 28.0, 20.0, 0.9, 0.12, 6, c.scale(1.1));
            }
        }
        // New York: the Empire State, stepped, with the mast on the top.
        Landmark::Empire => {
            site.block(0.0, 0.0, 0.0, 8.0, 5.0, 10.0, 0.9);
            site.block(0.0, 0.0, 10.0, 6.0, 4.0, 12.0, 0.96);
            site.block(0.0, 0.0, 22.0, 3.6, 2.8, 20.0, 1.0);
            site.block(0.0, 0.0, 42.0, 2.6, 2.0, 8.0, 0.94);
            site.block(0.0, 0.0, 50.0, 1.8, 1.5, 8.0, 0.9);
            site.windows(0.0, 0.0, 3.2, 2.4, 24.0, 52.0);
            site.band(0.0, 2.85, 44.0, 2.8, 1.0, a);
            site.taper(0.0, 0.0, 58.0, 4.0, 1.3, 0.9, 8, Rgb::hex(0x9AA6B6));
            site.taper(0.0, 0.0, 62.0, 14.0, 0.3, 0.08, 5, a);
        }
        // Philadelphia: the tower of City Hall, clock and all, under the statue.
        Landmark::CityHallTower => {
            site.block(0.0, 0.0, 0.0, 7.0, 7.0, 14.0, 0.92);
            site.block(0.0, 0.0, 14.0, 3.0, 3.0, 26.0, 1.0);
            site.windows(0.0, 0.0, 3.0, 3.0, 16.0, 38.0);
            site.band(0.0, 3.05, 33.0, 2.4, 2.4, Rgb::hex(0xF0E6C8));
            site.taper(0.0, 0.0, 40.0, 3.0, 2.2, 1.8, 8, Rgb::hex(0x7E8A94));
            site.taper(0.0, 0.0, 43.0, 4.0, 0.6, 0.45, 5, Rgb::hex(0xC8B47E));
        }
        // Washington: the obelisk.
        Landmark::Obelisk => {
            site.taper(0.0, 0.0, 0.0, 3.0, 3.4, 3.2, 4, c.scale(0.94));
            site.taper(0.0, 0.0, 3.0, 48.0, 2.4, 1.4, 4, Rgb::hex(0xDCD8CE));
            site.taper(0.0, 0.0, 51.0, 5.0, 1.4, 0.05, 4, Rgb::hex(0xE8E4DA));
        }
        // Charlotte: the tower with the crown of lights on it.
        Landmark::Crown => {
            site.block(0.0, 0.0, 0.0, 5.0, 4.0, 12.0, 0.9);
            site.block(0.0, 0.0, 12.0, 4.0, 3.2, 32.0, 1.0);
            site.windows(0.0, 0.0, 4.0, 3.2, 14.0, 42.0);
            site.taper(0.0, 0.0, 44.0, 4.0, 4.4, 3.0, 8, c.scale(1.08));
            site.taper(0.0, 0.0, 48.0, 8.0, 3.0, 0.1, 8, a);
        }
        // Los Angeles: City Hall, a stepped tower with a pyramid on it.
        Landmark::PyramidHall => {
            site.block(0.0, 0.0, 0.0, 6.0, 5.0, 22.0, 0.94);
            site.windows(0.0, 0.0, 6.0, 5.0, 6.0, 20.0);
            site.band(0.0, 5.05, 20.0, 6.2, 1.2, Rgb::hex(0xE8DCC0));
            site.block(0.0, 0.0, 22.0, 3.6, 3.0, 18.0, 1.0);
            site.taper(0.0, 0.0, 40.0, 8.0, 3.4, 0.1, 4, a);
        }
        // Phoenix: the butte the city is named around, no tower at all.
        Landmark::Butte => {
            site.block(0.0, 0.0, -1.0, 12.0, 9.0, 8.0, 0.95);
            site.block(2.0, 1.0, 7.0, 8.0, 6.0, 8.0, 1.0);
            site.block(3.0, 1.0, 15.0, 5.0, 4.0, 4.0, 0.92);
            site.block(-8.0, -6.0, -1.0, 4.0, 3.0, 3.0, 0.9);
        }
        // Seattle: the needle.
        Landmark::Needle => {
            site.block(0.0, 0.0, 0.0, 3.0, 3.0, 6.0, 0.9);
            site.taper(0.0, 0.0, 6.0, 30.0, 1.5, 1.1, 8, c.scale(0.95));
            site.taper(0.0, 0.0, 36.0, 2.0, 2.4, 5.6, 12, c.scale(1.05));
            site.taper(0.0, 0.0, 38.0, 1.4, 5.6, 4.6, 12, c.scale(1.1));
            site.taper(0.0, 0.0, 39.4, 3.0, 3.6, 2.0, 12, c);
            site.taper(0.0, 0.0, 36.2, 0.8, 5.8, 5.6, 12, a);
            site.taper(0.0, 0.0, 42.4, 12.0, 0.22, 0.05, 5, Rgb::hex(0xC8D2DC));
        }
        // Denver: the building with the barrel on the roof.
        Landmark::CashRegister => {
            site.block(0.0, 0.0, 0.0, 5.0, 4.0, 34.0, 0.94);
            site.windows(0.0, 0.0, 5.0, 4.0, 6.0, 32.0);
            site.taper(0.0, 0.0, 34.0, 4.0, 5.0, 4.6, 8, c.scale(1.05));
            site.taper(0.0, 0.0, 38.0, 3.0, 4.0, 3.0, 8, c);
            site.taper(0.0, 0.0, 41.0, 2.2, 2.6, 1.6, 8, c.scale(0.96));
            site.taper(0.0, 0.0, 43.2, 8.0, 0.2, 0.05, 5, a);
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
        let base = if end < 0.0 { -34.0f32 } else { 119.0 };
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
    // The ground the downtown stands on, so the streets and the blocks past the
    // end of the field are not floating in the dark.
    mesh.quad_shaded(
        Vec3::new(118.0, -150.0, -0.08),
        Vec3::new(340.0, -150.0, -0.08),
        Vec3::new(340.0, 150.0, -0.08),
        Vec3::new(118.0, 150.0, -0.08),
        Rgb::hex(0x333941),
        1.0,
    );
    // A street grid past the far end: three streets one way, three the other,
    // with the downtown built along them.
    let low = teams[0].primary.lerp(Rgb::hex(0x2A3240), 0.82);
    for i in 0..3 {
        road(mesh, false, 132.0 + i as f32 * 54.0, -140.0, 140.0, 7.0);
    }
    for j in 0..3 {
        road(mesh, true, -78.0 + j as f32 * 52.0, 120.0, 340.0, 7.0);
    }
    for i in 0..3 {
        for j in 0..3 {
            let half = Vec3::new(
                4.0 + rand() * 4.0,
                3.6 + rand() * 3.0,
                3.0 + rand() * 7.0,
            );
            // In the middle of a block, so no building stands in a street.
            let x = 132.0 + i as f32 * 54.0 + 27.0 + (rand() - 0.5) * 6.0;
            let y = -78.0 + j as f32 * 52.0 + 26.0 + (rand() - 0.5) * 8.0;
            mid_rise(
                mesh,
                Vec3::new(x, y, half.z),
                half,
                if y < 0.0 { 1.0 } else { -1.0 },
                low.scale(0.85 + rand() * 0.4),
                rand,
            );
        }
    }
    // And a row of lower blocks along the far stand, so the skyline over the
    // roof is a city with streets in it rather than towers standing in a field.
    for _ in 0..7 {
        let half = Vec3::new(4.0 + rand() * 3.5, 3.2 + rand() * 2.0, 4.0 + rand() * 5.0);
        mid_rise(
            mesh,
            Vec3::new(-30.0 + rand() * 150.0, -(33.0 + rand() * 3.0), half.z),
            half,
            1.0,
            low.scale(0.8 + rand() * 0.5),
            rand,
        );
    }
    // The building the city is known by: past the far end and just off the
    // middle, where the lens down the field and the lens across it both find it.
    landmark(
        mesh,
        city.landmark,
        Vec3::new(200.0, -14.0, 0.0),
        0.55,
        teams[0].primary.lerp(Rgb::hex(0x404A58), 0.55),
        teams[1].accent,
        rand,
    );
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

/// A head of coral: a few stubby arms leaning out of the sand with a bulb on
/// each, so it reads as something growing there rather than as a rock.
fn coral(mesh: &mut Mesh, at: Vec3, size: f32, color: Rgb, rand: &mut dyn FnMut() -> f32) {
    let arms = 3 + (rand() * 3.0) as i32;
    for i in 0..arms {
        let a = i as f32 / arms as f32 * std::f32::consts::TAU + rand() * 0.7;
        let lean = 0.3 + rand() * 0.7;
        let base = at + Vec3::new(a.cos() * size * 0.4, a.sin() * size * 0.4, 0.0);
        let tip = base
            + Vec3::new(
                a.cos() * size * lean,
                a.sin() * size * lean,
                size * (1.0 + rand() * 1.1),
            );
        mesh.cylinder(base, tip, size * 0.17, size * 0.11, 5, color);
        mesh.box_shaded(
            tip,
            Vec3::new(size * 0.24, size * 0.24, size * 0.18),
            a,
            color.lerp(Rgb::WHITE, 0.2),
            1.0,
        );
    }
}

/// A stalk of kelp: blades off one holdfast, tall enough to lean in the water.
fn kelp(mesh: &mut Mesh, at: Vec3, height: f32, color: Rgb, rand: &mut dyn FnMut() -> f32) {
    let fronds = 3 + (rand() * 4.0) as i32;
    for _ in 0..fronds {
        let a = rand() * std::f32::consts::TAU;
        let lean = 0.5 + rand() * 1.2;
        let base = at + Vec3::new(a.cos() * 0.5, a.sin() * 0.5, 0.0);
        let mid = base + Vec3::new(a.cos() * lean, a.sin() * lean, height * 0.55);
        let tip = mid + Vec3::new(a.cos() * lean * 0.7, a.sin() * lean * 0.7, height * 0.45);
        let tone = color.scale(0.85 + rand() * 0.3);
        mesh.cylinder(base, mid, 0.34, 0.2, 4, tone);
        mesh.cylinder(mid, tip, 0.2, 0.03, 4, tone.lerp(Rgb::WHITE, 0.14));
    }
}

fn underwater(mesh: &mut Mesh, rand: &mut dyn FnMut() -> f32) {
    let floor = -2.0f32;
    // The water outside the glass: a wall of it on every side, banded from the
    // dark at the sand up to the daylight at the surface, so the ground reads as
    // sitting on the floor of something deep.
    let shallow = Rgb::hex(0x33B4CC);
    let deep = Rgb::hex(0x04222F);
    let top_z = 84.0;
    let band = |i: usize| -> (f32, f32, Rgb) {
        let step = (top_z - floor) / 10.0;
        (
            floor + i as f32 * step,
            floor + (i + 1) as f32 * step,
            deep.lerp(shallow, i as f32 / 9.0),
        )
    };
    for side in [-1.0f32, 1.0] {
        for i in 0..10 {
            let (z0, z1, c) = band(i);
            mesh.quad_shaded(
                Vec3::new(-300.0, side * 120.0, z0),
                Vec3::new(420.0, side * 120.0, z0),
                Vec3::new(420.0, side * 120.0, z1),
                Vec3::new(-300.0, side * 120.0, z1),
                c,
                1.0,
            );
        }
    }
    for end in [-1.0f32, 1.0] {
        let x = if end < 0.0 { -170.0 } else { 290.0 };
        for i in 0..10 {
            let (z0, z1, c) = band(i);
            mesh.quad_shaded(
                Vec3::new(x, -130.0, z0),
                Vec3::new(x, 130.0, z0),
                Vec3::new(x, 130.0, z1),
                Vec3::new(x, -130.0, z1),
                c,
                1.0,
            );
        }
    }
    // The sand the whole ground stands on, out past everything.
    mesh.quad_shaded(
        Vec3::new(-280.0, -280.0, floor),
        Vec3::new(400.0, -280.0, floor),
        Vec3::new(400.0, 280.0, floor),
        Vec3::new(-280.0, 280.0, floor),
        Rgb::hex(0x8A7C52),
        1.0,
    );
    // Dunes under the current, and the ripples they leave.
    for _ in 0..26 {
        let w = 8.0 + rand() * 24.0;
        let h = 1.0 + rand() * 2.4;
        let at = Vec3::new(
            -140.0 + rand() * 340.0,
            if rand() < 0.5 { -1.0 } else { 1.0 } * (34.0 + rand() * 70.0),
            floor + h * 0.3,
        );
        mesh.box_shaded(
            at,
            Vec3::new(w, w * (0.5 + rand() * 0.7), h * 0.4),
            rand() * std::f32::consts::TAU,
            Rgb::hex(0x9C8C60),
            1.0,
        );
    }
    for _ in 0..40 {
        let y = if rand() < 0.5 { -1.0 } else { 1.0 } * (34.0 + rand() * 80.0);
        let x = -160.0 + rand() * 360.0;
        let len = 6.0 + rand() * 18.0;
        mesh.quad_shaded(
            Vec3::new(x, y, floor + 0.02),
            Vec3::new(x + len, y, floor + 0.02),
            Vec3::new(x + len, y + 0.9, floor + 0.02),
            Vec3::new(x, y + 0.9, floor + 0.02),
            Rgb::hex(0xA89670),
            1.0,
        );
    }
    // The reef: coral heads along both sides and off both ends, in the colours
    // a reef actually comes in.
    let corals = [
        0xE8808C, 0xF2A65A, 0xB98AD8, 0x6FD8C8, 0xE8C86A, 0xD8736A, 0xE8A0C0,
    ];
    for _ in 0..46 {
        let side = if rand() < 0.5 { -1.0f32 } else { 1.0 };
        let at = Vec3::new(-150.0 + rand() * 340.0, side * (36.0 + rand() * 40.0), floor);
        let color = Rgb::hex(corals[(rand() * corals.len() as f32) as usize % corals.len()]);
        coral(mesh, at, 6.0 + rand() * 7.0, color, rand);
    }
    for _ in 0..18 {
        let end = if rand() < 0.5 { -1.0f32 } else { 1.0 };
        let x = if end < 0.0 { -130.0 + rand() * 70.0 } else { 162.0 + rand() * 90.0 };
        let at = Vec3::new(x, -90.0 + rand() * 180.0, floor);
        let color = Rgb::hex(corals[(rand() * corals.len() as f32) as usize % corals.len()]);
        coral(mesh, at, 6.0 + rand() * 7.0, color, rand);
    }
    // Kelp standing off the sand, and a few rocks between the heads.
    for _ in 0..44 {
        let side = if rand() < 0.5 { -1.0f32 } else { 1.0 };
        let at = Vec3::new(-150.0 + rand() * 340.0, side * (36.0 + rand() * 44.0), floor);
        kelp(mesh, at, 18.0 + rand() * 24.0, Rgb::hex(0x2E7A46), rand);
    }
    for _ in 0..12 {
        let at = Vec3::new(
            -110.0 + rand() * 300.0,
            if rand() < 0.5 { -1.0 } else { 1.0 } * (36.0 + rand() * 44.0),
            floor,
        );
        rock(mesh, at, 1.2 + rand() * 2.6, Rgb::hex(0x3C4E52), rand);
    }
    // The glass is its frame, and nothing else: ribs over the vault, a rail at
    // every course and mullions between them, so the reef and the fish are seen
    // through the roof instead of being painted over by a pane.
    let glazing = Rgb::hex(0xCFEDF2);
    let eave_y = 32.0f32;
    let eave_z = 30.0f32;
    let crown_z = 54.0f32;
    let strips = 10;
    let curve = |t: f32| {
        let y = -eave_y + 2.0 * eave_y * t;
        (y, eave_z + (crown_z - eave_z) * 4.0 * t * (1.0 - t))
    };
    let mut x = -24.0;
    while x <= 132.0 {
        let mut prev = curve(0.0);
        for s in 1..=strips {
            let cur = curve(s as f32 / strips as f32);
            mesh.quad_shaded(
                Vec3::new(x - 0.24, prev.0, prev.1),
                Vec3::new(x + 0.24, prev.0, prev.1),
                Vec3::new(x + 0.24, cur.0, cur.1),
                Vec3::new(x - 0.24, cur.0, cur.1),
                glazing,
                1.0,
            );
            prev = cur;
        }
        x += 12.0;
    }
    // A rail along the crown, with the lights set in it.
    mesh.quad_shaded(
        Vec3::new(-24.0, -0.35, crown_z),
        Vec3::new(132.0, -0.35, crown_z),
        Vec3::new(132.0, 0.35, crown_z),
        Vec3::new(-24.0, 0.35, crown_z),
        Rgb::hex(0xF4F0DA),
        1.0,
    );
    // The eaves: a beam round the top of the bowl, where the walls meet the
    // vault.
    for side in [-1.0f32, 1.0] {
        mesh.box_shaded(
            Vec3::new(54.0, side * eave_y, eave_z),
            Vec3::new(78.0, 0.42, 0.42),
            0.0,
            glazing,
            1.0,
        );
    }
    for end in [-24.0f32, 132.0] {
        mesh.box_shaded(
            Vec3::new(end, 0.0, eave_z),
            Vec3::new(0.42, eave_y, 0.42),
            0.0,
            glazing,
            1.0,
        );
    }
    // The walls: a course rail at each height, and a mullion between them, so
    // the seam of the building still reads against the water.
    for side in [-1.0f32, 1.0] {
        for &z in &[11.0f32, 18.0, 25.0] {
            mesh.quad_shaded(
                Vec3::new(-24.0, side * (eave_y - 0.2), z),
                Vec3::new(132.0, side * (eave_y - 0.2), z),
                Vec3::new(132.0, side * (eave_y - 0.2), z + 0.32),
                Vec3::new(-24.0, side * (eave_y - 0.2), z + 0.32),
                glazing,
                1.0,
            );
        }
        let mut mx = -24.0;
        while mx <= 132.0 {
            mesh.box_shaded(
                Vec3::new(mx, side * (eave_y - 0.2), 20.0),
                Vec3::new(0.16, 0.16, 10.0),
                0.0,
                glazing,
                1.0,
            );
            mx += 12.0;
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
    (0..56)
        .map(|i| FishFigure {
            radius: 34.0 + rand() * 30.0,
            height: 13.0 + rand() * 18.0,
            phase: rand() * std::f32::consts::TAU,
            speed: (0.10 + rand() * 0.14) * if i % 3 == 0 { -1.0 } else { 1.0 },
            size: 1.1 + rand() * 2.0,
            color: palette[(rand() * palette.len() as f32) as usize % palette.len()],
        })
        .collect()
}

/// The school, swimming: rebuilt every frame, because placing a mesh bakes it.
pub fn school_frame(fish: &[FishFigure], t: f32) -> Mesh {
    let mut mesh = Mesh::new();
    for f in fish {
        let a = f.phase + t * f.speed;
        let at = Vec3::new(50.0 + a.cos() * f.radius * 1.7, a.sin() * f.radius, f.height);
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
