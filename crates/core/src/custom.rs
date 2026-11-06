//! What the player has made: their own clubs, and kits for the clubs that came
//! with the game.
//!
//! Everything here is a saved thing, not a setting: fifty slots for created
//! clubs, and a kit override per built-in club. The menu writes them and the
//! league reads them, through [`custom_clubs`] and the kit hook in
//! [`crate::team::club_kits`], so a club somebody made is available everywhere a
//! club is picked and a shirt somebody painted is worn everywhere a shirt is
//! drawn.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use crate::math::Rgb;
use crate::playbook::Book;
use crate::team::{Club, Division, JerseyPattern};

/// How many clubs the player can save. A slot is theirs until they clear it.
pub const TEAM_SLOTS: usize = 50;

/// The clubs a created team can wear in place of a colour: enough range to tell
/// two clubs apart, few enough to step through on a pad.
pub const PALETTE: [(&str, Rgb); 16] = [
    ("MAROON", Rgb::hex(0x6B1D2B)),
    ("RED", Rgb::hex(0xB4212C)),
    ("ORANGE", Rgb::hex(0xC25E1B)),
    ("AMBER", Rgb::hex(0xE0A32B)),
    ("GOLD", Rgb::hex(0xD7B54A)),
    ("LIME", Rgb::hex(0x9CCB3B)),
    ("FOREST", Rgb::hex(0x1B5E3A)),
    ("TEAL", Rgb::hex(0x0E6E6E)),
    ("SKY", Rgb::hex(0x3D8FC4)),
    ("NAVY", Rgb::hex(0x1B2A6B)),
    ("VIOLET", Rgb::hex(0x4A2472)),
    ("PLUM", Rgb::hex(0x6E2148)),
    ("SLATE", Rgb::hex(0x2B3038)),
    ("STONE", Rgb::hex(0x6B6257)),
    ("BONE", Rgb::hex(0xDCD6C8)),
    ("WHITE", Rgb::hex(0xF2F4F6)),
];

/// The patterns a jersey can be cut from.
pub const PATTERNS: [JerseyPattern; 5] = [
    JerseyPattern::Solid,
    JerseyPattern::ShoulderStripe,
    JerseyPattern::Sash,
    JerseyPattern::SidePanel,
    JerseyPattern::Hoops,
];

/// The colours of a club's two kits, in the order they were picked.
///
/// A built-in club's kit is built from its primary and accent, so an override
/// has to say all six colours rather than lean on the club's own: a player who
/// repaints the shirt should not find the shorts following it around.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KitEdit {
    pub home_jersey: Rgb,
    pub home_shorts: Rgb,
    pub home_accent: Rgb,
    pub away_jersey: Rgb,
    pub away_shorts: Rgb,
    pub away_accent: Rgb,
    pub pattern: JerseyPattern,
}

/// A club the player made, in one of the fifty slots.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomTeam {
    pub city: String,
    pub name: String,
    pub abbr: String,
    pub division: Division,
    pub primary: Rgb,
    pub secondary: Rgb,
    pub accent: Rgb,
    pub pattern: JerseyPattern,
    /// How good the men are, on the same scale a built club is built at.
    pub level: i32,
    pub white_home: bool,
    pub seed: u64,
    /// Which book it runs, picked the same way a built club's is.
    pub book: Book,
}

impl CustomTeam {
    /// A fresh club in a slot: a real seed, so its roster is its own and the
    /// same every time the slot is opened again.
    pub fn blank(slot: usize) -> CustomTeam {
        CustomTeam {
            city: "Springfield".to_string(),
            name: "Expansion".to_string(),
            abbr: "EXP".to_string(),
            division: Division::ALL[slot % Division::ALL.len()],
            primary: PALETTE[2].1,
            secondary: PALETTE[12].1,
            accent: PALETTE[4].1,
            pattern: JerseyPattern::Solid,
            level: 4,
            white_home: false,
            seed: 0xC0DE_0000 ^ (slot as u64).wrapping_mul(0x9E37_79B9),
            book: Book::Balanced,
        }
    }

    /// A three-letter mark from the name, unique against the clubs that exist.
    pub fn mark(&self, taken: &[String]) -> String {
        let letters: String = self
            .name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_uppercase();
        let base: String = letters.chars().take(3).collect();
        let base = if base.len() == 3 { base } else { "EXP".to_string() };
        if !taken.iter().any(|t| *t == base) {
            return base;
        }
        for n in 1..100u8 {
            let try_me = format!("{}{}", &base[..2], n % 10);
            if !taken.iter().any(|t| *t == try_me) {
                return try_me;
            }
        }
        format!("{}X", &base[..2])
    }
}

fn teams() -> &'static Mutex<Vec<Option<CustomTeam>>> {
    static SLOTS: OnceLock<Mutex<Vec<Option<CustomTeam>>>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(vec![None; TEAM_SLOTS]))
}

fn kits() -> &'static Mutex<BTreeMap<String, KitEdit>> {
    static KITS: OnceLock<Mutex<BTreeMap<String, KitEdit>>> = OnceLock::new();
    KITS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Every saved club, in slot order.
pub fn saved_teams() -> Vec<Option<CustomTeam>> {
    teams()
        .lock()
        .map(|t| t.clone())
        .unwrap_or_else(|_| vec![None; TEAM_SLOTS])
}

pub fn team(slot: usize) -> Option<CustomTeam> {
    teams().lock().ok().and_then(|t| t.get(slot).cloned().flatten())
}

/// Save a club into a slot, or clear the slot with `None`. The mark is settled
/// here, against every other club in the league, so two created clubs cannot
/// answer to the same three letters.
pub fn set_team(slot: usize, team: Option<CustomTeam>) {
    let Ok(mut slots) = teams().lock() else { return };
    if slot >= slots.len() {
        return;
    }
    match team {
        None => slots[slot] = None,
        Some(mut t) => {
            let mut taken: Vec<String> = crate::team::built_in_clubs()
                .iter()
                .map(|c| c.abbr.to_string())
                .chain(
                    slots
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != slot)
                        .filter_map(|(_, s)| s.as_ref().map(|s| s.abbr.clone())),
                )
                .collect();
            taken.dedup();
            t.abbr = t.mark(&taken);
            slots[slot] = Some(t);
        }
    }
}

/// The kit a club has been repainted into, if it has.
pub fn kit(abbr: &str) -> Option<KitEdit> {
    kits().lock().ok().and_then(|k| k.get(abbr).copied())
}

pub fn set_kit(abbr: &str, edit: Option<KitEdit>) {
    let Ok(mut k) = kits().lock() else { return };
    match edit {
        Some(e) => {
            k.insert(abbr.to_string(), e);
        }
        None => {
            k.remove(abbr);
        }
    }
}

pub fn kit_edits() -> Vec<(String, KitEdit)> {
    kits()
        .lock()
        .map(|k| k.iter().map(|(a, e)| (a.clone(), *e)).collect())
        .unwrap_or_default()
}

/// The created clubs, as clubs: leaked strings, because a club is `Copy`-shaped
/// everywhere the league handles one and a table of fifty is small.
pub fn custom_clubs() -> Vec<Club> {
    saved_teams()
        .into_iter()
        .flatten()
        .map(|t| Club {
            city: Box::leak(t.city.into_boxed_str()),
            name: Box::leak(t.name.into_boxed_str()),
            abbr: Box::leak(t.abbr.into_boxed_str()),
            book: t.book,
            division: t.division,
            primary: t.primary,
            secondary: t.secondary,
            accent: t.accent,
            pattern: t.pattern,
            seed: t.seed,
            level: t.level,
            white_home: t.white_home,
        })
        .collect()
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn color(text: &str) -> Option<Rgb> {
    let t = text.trim().trim_start_matches('#');
    if t.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(t, 16).ok()?;
    Some(Rgb::hex(v))
}

fn pattern(text: &str) -> Option<JerseyPattern> {
    match text.trim() {
        "Solid" => Some(JerseyPattern::Solid),
        "ShoulderStripe" => Some(JerseyPattern::ShoulderStripe),
        "Sash" => Some(JerseyPattern::Sash),
        "SidePanel" => Some(JerseyPattern::SidePanel),
        "Hoops" => Some(JerseyPattern::Hoops),
        _ => None,
    }
}

fn division(text: &str) -> Option<Division> {
    Division::ALL
        .iter()
        .copied()
        .find(|d| d.label().eq_ignore_ascii_case(text.trim()))
}

/// The book's name in the save file, and back again.
pub fn book_name(book: Book) -> &'static str {
    match book {
        Book::WestCoast => "WestCoast",
        Book::AirRaid => "AirRaid",
        Book::Vertical => "Vertical",
        Book::Spread => "Spread",
        Book::Power => "Power",
        Book::Zone => "Zone",
        Book::Balanced => "Balanced",
        Book::Custom => "Custom",
    }
}

fn parse_book(text: &str) -> Option<Book> {
    Book::ALL
        .iter()
        .copied()
        .find(|b| book_name(*b).eq_ignore_ascii_case(text.trim()))
}

/// The whole store as text: one line a club, one line a kit.
pub fn text() -> String {
    let mut out = String::from(
        "# 7 on 7 created clubs and repainted kits. One line a club, one line a kit.\n",
    );
    for (slot, team) in saved_teams().into_iter().enumerate() {
        let Some(t) = team else { continue };
        // Ten fields, then the three names joined up: a nickname with a space
        // in it would otherwise split the line.
        out.push_str(&format!(
            "team {} {} {} {} {} {} {} {} {} {} ",
            slot,
            t.division.label(),
            t.level,
            t.white_home as u8,
            t.abbr,
            hex(t.primary),
            hex(t.secondary),
            hex(t.accent),
            pattern_name(t.pattern),
            book_name(t.book),
        ));
        out.push_str(&format!(
            "{}|{}|{}\n",
            t.city.replace(' ', "_"),
            t.name.replace(' ', "_"),
            t.abbr
        ));
    }
    out.push_str(&crate::design::text());
    for (abbr, e) in kit_edits() {
        out.push_str(&format!(
            "kit {} {} {} {} {} {} {} {}\n",
            abbr,
            pattern_name(e.pattern),
            hex(e.home_jersey),
            hex(e.home_shorts),
            hex(e.home_accent),
            hex(e.away_jersey),
            hex(e.away_shorts),
            hex(e.away_accent),
        ));
    }
    out
}

pub fn pattern_name(p: JerseyPattern) -> &'static str {
    match p {
        JerseyPattern::Solid => "Solid",
        JerseyPattern::ShoulderStripe => "ShoulderStripe",
        JerseyPattern::Sash => "Sash",
        JerseyPattern::SidePanel => "SidePanel",
        JerseyPattern::Hoops => "Hoops",
    }
}

/// Read the store. Anything unreadable is skipped: a hand-edited file must never
/// stop the game starting, the same rule the settings file lives by.
pub fn parse(text: &str) {
    crate::design::parse(text);
    let Ok(mut slots) = teams().lock() else { return };
    let Ok(mut kits) = kits().lock() else { return };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("team") => {
                let (Some(slot), Some(div), Some(level), Some(home_white)) =
                    (parts.next(), parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let (Some(abbr), Some(primary), Some(second), Some(accent)) =
                    (parts.next(), parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let (Some(pat), Some(book), Some(names)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let mut names = names.split('|');
                let (Some(city), Some(name)) = (names.next(), names.next()) else {
                    continue;
                };
                let (Some(slot), Some(div), Some(level)) = (
                    slot.parse::<usize>().ok(),
                    division(div),
                    level.parse::<i32>().ok(),
                ) else {
                    continue;
                };
                let (Some(primary), Some(second), Some(accent)) =
                    (color(primary), color(second), color(accent))
                else {
                    continue;
                };
                let (Some(pat), Some(book)) = (pattern(pat), parse_book(book)) else {
                    continue;
                };
                if slot >= TEAM_SLOTS {
                    continue;
                }
                slots[slot] = Some(CustomTeam {
                    city: city.replace('_', " "),
                    name: name.replace('_', " "),
                    abbr: abbr.to_string(),
                    division: div,
                    primary,
                    secondary: second,
                    accent,
                    pattern: pat,
                    level: level.clamp(-8, 12),
                    white_home: home_white != "0",
                    seed: 0xC0DE_0000 ^ (slot as u64).wrapping_mul(0x9E37_79B9),
                    book,
                });
            }
            Some("kit") => {
                let (Some(abbr), Some(pat)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let mut colors = Vec::new();
                for p in parts {
                    match color(p) {
                        Some(c) => colors.push(c),
                        None => break,
                    }
                }
                let (Some(pat), [h_j, h_s, h_a, a_j, a_s, a_a]) =
                    (pattern(pat), colors.as_slice())
                else {
                    continue;
                };
                kits.insert(
                    abbr.to_string(),
                    KitEdit {
                        home_jersey: *h_j,
                        home_shorts: *h_s,
                        home_accent: *h_a,
                        away_jersey: *a_j,
                        away_shorts: *a_s,
                        away_accent: *a_a,
                        pattern: pat,
                    },
                );
            }
            _ => {}
        }
    }
}

/// Forget everything, for the tests and for a fresh game that wants the league
/// back the way it shipped.
pub fn clear() {
    if let Ok(mut slots) = teams().lock() {
        *slots = vec![None; TEAM_SLOTS];
    }
    if let Ok(mut k) = kits().lock() {
        k.clear();
    }
    crate::design::clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// The store is one saved thing, so the tests take it one at a time: four
    /// tests clearing the same table in parallel is four tests reading each
    /// other's clubs.
    fn store() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_created_club_keeps_its_slot_and_its_men() {
        let _store = store();
        clear();
        let mut t = CustomTeam::blank(7);
        t.city = "Fresno".to_string();
        t.name = "Vinegaroons".to_string();
        set_team(7, Some(t.clone()));
        let back = team(7).expect("the club was not saved");
        assert_eq!(back.city, "Fresno");
        assert_eq!(back.abbr, "VIN");
        // The roster is built from the slot's seed, so it is the club's own men
        // and the same men every time it is opened.
        let a = custom_clubs()[0].roster();
        let b = custom_clubs()[0].roster();
        assert_eq!(a.len(), b.len());
        assert_eq!(a[0].last, b[0].last);
        clear();
    }

    #[test]
    fn two_created_clubs_do_not_share_a_mark() {
        let _store = store();
        clear();
        let mut a = CustomTeam::blank(0);
        a.name = "Wranglers".to_string();
        let mut b = CustomTeam::blank(1);
        b.name = "Wranglers".to_string();
        set_team(0, Some(a));
        set_team(1, Some(b));
        let marks: Vec<String> = saved_teams()
            .into_iter()
            .flatten()
            .map(|t| t.abbr)
            .collect();
        assert_eq!(marks.len(), 2);
        assert_ne!(marks[0], marks[1], "two clubs answer to one mark");
        clear();
    }

    #[test]
    fn the_store_survives_being_written_out_and_read_back() {
        let _store = store();
        clear();
        let mut t = CustomTeam::blank(3);
        t.city = "Bakersfield".to_string();
        t.name = "Dust".to_string();
        t.division = Division::West;
        t.level = 9;
        set_team(3, Some(t));
        let edit = KitEdit {
            home_jersey: PALETTE[1].1,
            home_shorts: PALETTE[12].1,
            home_accent: PALETTE[4].1,
            away_jersey: PALETTE[15].1,
            away_shorts: PALETTE[7].1,
            away_accent: PALETTE[3].1,
            pattern: JerseyPattern::Sash,
        };
        set_kit("BOS", Some(edit));

        let written = text();
        clear();
        assert!(custom_clubs().is_empty());
        parse(&written);

        let back = team(3).expect("the club came back empty");
        assert_eq!(back.city, "Bakersfield");
        assert_eq!(back.name, "Dust");
        assert_eq!(back.division, Division::West);
        assert_eq!(back.level, 9);
        assert_eq!(kit("BOS"), Some(edit));
        clear();
    }

    #[test]
    fn a_repainted_kit_is_the_kit_the_club_wears() {
        let _store = store();
        clear();
        let boston = crate::team::built_in_clubs()
            .into_iter()
            .find(|c| c.abbr == "BOS")
            .expect("Boston");
        let before = crate::team::club_kits(&boston).0.jersey;
        let edit = KitEdit {
            home_jersey: PALETTE[1].1,
            home_shorts: PALETTE[12].1,
            home_accent: PALETTE[4].1,
            away_jersey: PALETTE[15].1,
            away_shorts: PALETTE[7].1,
            away_accent: PALETTE[3].1,
            pattern: JerseyPattern::Hoops,
        };
        set_kit("BOS", Some(edit));
        let after = crate::team::club_kits(&boston).0;
        assert_ne!(before, after.jersey, "the shirt did not change");
        assert_eq!(after.jersey, PALETTE[1].1);
        assert_eq!(after.pattern, JerseyPattern::Hoops);
        clear();
        assert_eq!(crate::team::club_kits(&boston).0.jersey, before);
    }
}
