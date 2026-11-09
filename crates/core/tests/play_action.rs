//! The play action: the quarterback sells a give he never makes, and the defense
//! leans on it.

use nopads_core::math::Vec2;
use nopads_core::playbook::{playbook, 
    defensive_playbook, Assign, Formation, Handoff, OffRole, Play, PlayFamily,
};
use nopads_core::sim::{CoverageJob, Phase, PlaySim, SnapSetup, SNAP_DELAY};
use nopads_core::team::league_teams;

fn setup(seed: u64) -> SnapSetup {
    SnapSetup {
        los: 40.0,
        ball_y: 0.0,
        direction: 1,
        down: 1,
        to_go: 10.0,
        seed,
        offense_team: 0,
        defense_team: 1,
        is_try: false,
    }
}

/// What the designer draws when the pass is set to sell a give: the man in the
/// backfield comes across the quarterback's face and the routes go downfield.
fn play_action() -> Play {
    Play {
        name: "BANG 8",
        concept: "Play action",
        family: PlayFamily::PlayAction,
        formation: Formation::Pistol,
        drop: 5.0,
        handoff: Handoff::Inside,
        run_side: 0,
        flip: false,
        assignments: vec![
            (OffRole::Qb, Assign::PassBlock { slide: 0.0, max_depth: 5.0 }),
            (OffRole::Lg, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
            (OffRole::Rg, Assign::PassBlock { slide: 0.0, max_depth: -1.0 }),
            (
                OffRole::Back,
                Assign::Fake {
                    aim: Vec2::new(2.0, 1.0),
                    points: vec![Vec2::new(1.0, 3.0), Vec2::new(3.0, 9.0)],
                    depth: 9,
                },
            ),
            (OffRole::Slot, Assign::Route { points: vec![Vec2::new(13.0, 3.0)], depth: 13 }),
            (OffRole::WideL, Assign::Route { points: vec![Vec2::new(18.0, -3.0)], depth: 18 }),
            (OffRole::WideR, Assign::Route { points: vec![Vec2::new(12.0, -8.0)], depth: 12 }),
        ],
    }
}

/// The same routes with no fake: the quarterback turns and drops.
fn dropback() -> Play {
    let mut p = play_action();
    p.family = PlayFamily::DropbackPass;
    p
}

fn fresh(play: &Play, seed: u64) -> PlaySim<'static> {
    let teams = Box::leak(Box::new(league_teams()));
    let defs = Box::leak(Box::new(defensive_playbook()));
    PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(seed))
}

/// Snap the ball, then run the play `after` seconds past the snap. The snap is
/// its own moment -- every clock in the play is measured from it -- so the
/// seconds in these tests are seconds of the play, not of the sim.
fn ran(play: &Play, seed: u64, after: f32) -> (PlaySim<'static>, f32) {
    let mut sim = fresh(play, seed);
    sim.request_snap();
    let mut guard = 0;
    while sim.phase == Phase::PreSnap && sim.result.is_none() && guard < 600 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    let snapped = sim.t;
    let mut guard = 0;
    while sim.t < snapped + after && sim.result.is_none() && guard < 600 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    (sim, snapped)
}

fn back(sim: &PlaySim) -> usize {
    (0..7)
        .find(|&i| OffRole::ALL[sim.players[i].role_index] == OffRole::Back)
        .expect("a back")
}

/// A play action is a lie: the ball is shown to the man coming across and never
/// leaves the quarterback's hands.
#[test]
fn the_ball_is_shown_and_kept() {
    let play = play_action();
    let mut sim = fresh(&play, 7);
    sim.request_snap();
    let mut guard = 0;
    while sim.phase == Phase::PreSnap && sim.result.is_none() && guard < 600 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    let snapped = sim.t;
    let man = back(&sim);
    assert!(sim.carrier.is_none(), "the ball was given away on the snap");
    let mut frames = 0;
    while sim.t < snapped + 0.40 && sim.result.is_none() && frames < 200 {
        sim.step(1.0 / 60.0);
        frames += 1;
        assert_eq!(
            sim.ball.holder,
            Some(sim.qb),
            "the ball left the quarterback's hands on a play action"
        );
        assert_ne!(
            sim.carrier,
            Some(man),
            "the fake was given to the man it was sold to"
        );
    }
    assert!(frames > 10, "the play never got going: {frames} frames");
}

/// He goes to the man he is selling it to. The same play as a straight drop
/// does not: there is nobody to meet, and so he is further from him.
#[test]
fn the_sell_takes_him_to_the_man() {
    let (fake, _) = ran(&play_action(), 11, 0.40);
    let (straight, _) = ran(&dropback(), 11, 0.40);
    let gap = |sim: &PlaySim| sim.players[sim.qb].pos.dist(sim.players[back(sim)].pos);
    assert!(
        gap(&fake) < 2.2,
        "he never got to the man he was selling it to: {:.2} yd",
        gap(&fake)
    );
    assert!(
        gap(&fake) < gap(&straight) - 0.2,
        "the fake did not bring him to the give: {:.2} against {:.2} yd",
        gap(&fake),
        gap(&straight)
    );
}

/// The men behind the front lean toward a fake, and that lean is the daylight
/// the throw is made into.
#[test]
fn the_sell_pulls_the_defense_in() {
    const END: f32 = 0.40;
    let play = play_action();
    let start = fresh(&play, 11);
    let at = start.players[start.qb].pos;
    let man = (7..14)
        .filter(|&i| !matches!(start.players[i].job, CoverageJob::Rush))
        .min_by(|&a, &b| {
            let da = start.players[a].pos.dist(at);
            let db = start.players[b].pos.dist(at);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("a man in coverage");
    let (fake, _) = ran(&play, 11, END);
    let (straight, _) = ran(&dropback(), 11, END);
    assert!(
        fake.players[man].pos.dist(at) < straight.players[man].pos.dist(at) - 0.5,
        "the defense did not lean on the fake: {:.2} against {:.2} yd from the ball",
        fake.players[man].pos.dist(at),
        straight.players[man].pos.dist(at)
    );
}

/// And it is still a pass: the ball is thrown when the sell is done.
#[test]
fn a_play_action_is_still_a_pass() {
    let (mut sim, _) = ran(&play_action(), 5, 0.0);
    let out = sim.run_to_completion();
    assert!(
        out.passer.is_some(),
        "nobody threw it on a play action: {:?}",
        out.outcome
    );
}

/// A play fake in the gun is sold from the pocket the quarterback snapped in: he
/// turns to the man, shows him the ball, and his feet stay where the play put
/// them. Walking after the runner is how a fake turns into a scramble.
#[test]
fn the_fake_is_sold_from_the_pocket_he_snapped_in() {
    let teams = league_teams();
    let def = defensive_playbook()[0].clone();
    let mut p = play_action();
    p.formation = Formation::Pistol;
    p.drop = 0.0;
    let mut sim = PlaySim::new(&teams[0], &teams[1], &p, &def, &setup(3));
    let qb = sim.qb;
    let los = 40.0f32;
    let y0 = sim.players[qb].pos.y;
    let snap = los - sim.players[qb].pos.x;
    assert!((snap - 3.2).abs() < 0.2, "he is not in the gun: {snap:.2} yards deep");
    let (mut deep, mut shallow, mut across, mut offered) = (snap, snap, 0.0f32, 0.0f32);
    while sim.t < SNAP_DELAY + 0.45 {
        sim.step(1.0 / 60.0);
        let q = sim.players[qb].pos;
        deep = deep.max(los - q.x);
        shallow = shallow.min(los - q.x);
        across = across.max((q.y - y0).abs());
        if sim.t > SNAP_DELAY {
            offered = offered.max(sim.players[qb].give);
        }
    }
    assert!(
        deep - snap < 0.5 && snap - shallow < 0.5,
        "he drifted selling it: {:.2} yards back, {:.2} forward",
        deep - snap,
        snap - shallow
    );
    assert!(across > 0.3, "he never turned to the man: {across:.2} yards across");
    assert!(offered > 0.5, "the ball was never shown: give {offered:.2}");
    // And once the sell is over he stays in the pocket rather than falling back
    // out of it on the momentum of the fake.
    while sim.t < SNAP_DELAY + 0.9 {
        sim.step(1.0 / 60.0);
    }
    let after = los - sim.players[qb].pos.x;
    assert!(
        (after - snap).abs() < 0.8,
        "he ended {:.2} yards from where he snapped after the fake",
        after - snap
    );
}

/// The league's own play actions sell the fake the same way a drawn one does:
/// from the pocket the quarterback snapped in, with the ball out, never walking
/// after the runner. Their drops are small -- a boot is a step, not a five-step
/// dropback -- so the pocket is the whole of what the defense is being sold.
#[test]
fn the_leagues_play_actions_sell_from_the_pocket_too() {
    let teams = league_teams();
    let def = defensive_playbook()[0].clone();
    let mut checked = 0;
    for p in playbook()
        .into_iter()
        .filter(|p| p.family == PlayFamily::PlayAction)
    {
        let mut sim = PlaySim::new(&teams[0], &teams[1], &p, &def, &setup(7));
        let qb = sim.qb;
        let los = 40.0f32;
        let snap = los - sim.players[qb].pos.x;
        let (mut deep, mut shallow, mut give) = (snap, snap, 0.0f32);
        while sim.t < SNAP_DELAY + 0.45 {
            sim.step(1.0 / 60.0);
            let q = sim.players[qb].pos;
            deep = deep.max(los - q.x);
            shallow = shallow.min(los - q.x);
            if sim.t > SNAP_DELAY {
                give = give.max(sim.players[qb].give);
            }
        }
        assert!(
            deep - snap < 0.5 && snap - shallow < 0.5,
            "{}: drifted {:.2} yards back and {:.2} forward selling the fake",
            p.name,
            deep - snap,
            snap - shallow
        );
        assert!(
            give > 0.5,
            "{}: the ball was never shown on the fake",
            p.name
        );
        checked += 1;
    }
    assert!(
        checked >= 4,
        "only {checked} play actions in the league book: that is not the check this test is"
    );
}
