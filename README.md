# A7FL: No Pads Football

A playable American 7s Football League game written in Rust: a complete 7-on-7
rules engine, a simulated season-length game, and a software-rendered broadcast
package in the spirit of the ESPN NFL 2K5 presentation — but played under A7FL
rules, with players in A7FL kit (sleeveless jersey, shorts, bare arms and legs,
no helmets, no pads).

Everything is pure Rust with no GPU, no windowing system and no audio backend, so
it builds and runs headless (a cloud VM, a container, a Raspberry Pi) as well as on
a desktop. The only external crates are `image` and `gif`, both for writing the
rendered frames.

## Running it

```bash
cargo run -- teams          # the ten-team league and every roster
cargo run -- rules          # the A7FL rule set and the penalty catalog
cargo run -- sim --seed 7   # simulate a whole game, print the box score
cargo run -- play --seed 12 # call the plays yourself via a text menu
cargo run --release         # play a whole game live, in a window, on a gamepad
cargo run -- film --seed 4  # render broadcast stills + a highlight GIF into out/
```

`sim` and `play` accept `--home N` / `--away N` to pick teams by index
(`teams` prints them in order); `film` also takes `--out DIR`.

## Playing with a controller

`a7fl arcade` is the real-time client: plug in a controller (an 8BitDo works in
XInput, DirectInput or Switch mode) and run `cargo run --release`. It opens a
window (1280x720 by default, resizable) and draws the field with the same
software renderer as the broadcast package — no GPU, no game engine. Run it with
`--terminal` (or on a machine with no display) and the same game is drawn in ANSI
colour blocks straight into the terminal instead. With no controller attached the
keyboard drives it, so the game is always playable.

```
           the field, in ANSI blocks

 NEW 14 - 7 PAT   2ND 6:41   1ST & 10   BALL ON OWN 34
 DAGGER  ·  TRIPS RIGHT  ·  PAT shows COVER 2 ZONE
 TARGET  WR  Frankie Kelsey     4.2 yds  OPEN
 NEW ball   [STICK] MOVE   [B]/[X] TARGET   [A]/[RT] THROW   [START] QUIT
```

Controls:

| Input | On the call sheet | During the play |
| --- | --- | --- |
| Left stick | — | Move the quarterback (past the line of scrimmage he becomes a runner) |
| A / South | Snap the ball | Throw to the locked-on receiver |
| B / East | Next play | Next receiver |
| X / West | Previous play | Previous receiver |
| Y / North | Ask the coach | — |
| RT / ZR | — | Throw |
| Start | Quit | Quit |

| Keyboard | On the call sheet | During the play |
| --- | --- | --- |
| Arrows or WASD | — | Move the quarterback |
| Space or Enter | Snap the ball | Throw |
| X / Z | Next / previous play | Next / previous receiver |
| C | Ask the coach | — |
| Left Shift | — | Throw |
| Esc | Quit | Quit |

Fourth downs, conversions and throw-offs get their own prompt screens, drawn with
the same bitmap font as the broadcast overlay. In terminal mode the picture is
sized from `COLUMNS` and `LINES`, so it adapts to whatever terminal you run it in.

Controller support comes from `gilrs` and is a default-on Cargo feature. On Linux
the crate needs the udev headers (`sudo apt install libudev-dev`); build with
`--no-default-features` on a machine without them, and everything except the
controller still works (the keyboard and the window do not depend on it).

## What is implemented

### A7FL rules, not NFL rules

The rule set follows a7fl.com rather than the NFL rulebook:

- **7 on 7.** Quarterback, two down linemen and four eligibles against two edge
  rushers, two linebackers and three defensive backs.
- **4 × 15 minute quarters, running clock.** The clock stops only on penalties,
  change of possession and conversions (plus incompletions and out-of-bounds
  inside two minutes). 30 second play clock. 4 downs to gain 10.
- **No kicking of any kind.** There are no field goals, no punts and no kickoffs.
- **Throw-offs replace kickoffs.** The ball is *thrown* from the throwing team's
  own 35, with three gunners against one returner, a minimum of 40 air yards.
  Fair catch is legal, a touchback comes out to the 20, and a returner downed in
  his own end zone concedes a safety.
- **Fourth down becomes a throw-off** from the line of scrimmage instead of a punt.
- **Conversions are chosen, not kicked.** 1 point from the 5 or 2 points from the
  10, and the choice is locked in for the game.
- **Overtime** gives each side a possession from the 25 with two first downs
  allowed; two-point conversions are forced from the second overtime on.
- **Mercy rule:** a seven-touchdown lead at the start of the fourth ends the game.

The penalty catalog covers 33 fouls (false start, offside, delay of game, holding,
block in the back, clipping, chop block, intentional grounding, both pass
interferences, defensive holding, illegal contact, roughing the passer, late hit,
targeting, horse collar, blindside block, unsportsmanlike conduct and the rest)
with the correct enforcement spot — basic spot, previous spot, spot of foul or end
of run — plus half-the-distance-to-the-goal limits, automatic first downs and loss
of down.

### Simulation

`a7fl_core::sim` runs one snap as a continuous 7-on-7 play at 60 Hz: dropback,
route running against man and zone coverage, pass rush and holding, quarterback
progression reads, throw power and accuracy, catch radius, wrap tackles, broken
tackles, fumbles, interceptions, defensive pass interference, sacks, safeties,
touchdowns and out-of-bounds. `a7fl_core::throwoff` runs the throw-off the same
way, phase by phase (presnap, flight, return, result), with fair catches, muffs,
touchbacks, return touchdowns and safeties.

Each snap is seeded, so a recorded play replays bit-for-bit. That is what lets the
renderer produce a frame for any moment of a play that was already simulated.

The AI has a play-calling personality per team, adjusts to down, distance and
score, decides fourth downs (go for it or throw it off), picks conversion values,
and manages timeouts and clock.

### Presentation

The renderer is a small software 3D pipeline (`a7fl_render`): z-buffered,
perspective-correct textured triangles, near-plane clipping, directional lighting,
distance fog, soft ground shadows, a painted turf texture with mow stripes, yard
numbers, hashes and end-zone paint, a stadium with ad boards and stepped
bleachers, and five camera setups — high sideline broadcast, behind the
quarterback, sky cam, low replay and end zone.

The overlay is 2K5-flavoured: a score bug with possession highlight and timeout
pips, a down-and-distance card with play clock bar, the A7FL badge, a lower-third
ticker carrying the play-by-play line, big TOUCHDOWN / TURNOVER / SACK / FLAG
banners, and a full-screen post-game stats panel.

Players are built from primitives at runtime from their attributes and appearance
(skin tone, hair, beard, build) so the seven on the field match the roster.

`film` renders six stills and a 44-frame GIF: a throw-off, a pre-snap look, a pass
in flight from the broadcast camera, a scoring play, a turnover, the final stats
panel and the highlight loop.

## Architecture

```
crates/core     a7fl_core   — no dependencies
  math.rs       vectors, colours, deterministic PCG random
  field.rs      canonical field geometry (100 yards + two 10-yard end zones, 37.5 wide)
  rules.rs      penalty catalog and enforcement
  team.rs       attributes, appearance, uniforms, the ten-team league
  playbook.rs   22 offensive plays, 6 formations, 9 defensive calls
  sim.rs        the 7-on-7 snap engine
  throwoff.rs   the throw-off engine
  stats.rs      box score, drives, per-player lines
  ai.rs         play calling and game management
  game.rs       quarters, clock, downs, conversions, overtime, mercy, play log

crates/render   a7fl_render — depends on `image` (PNG) and `gif`
  raster.rs     framebuffer, z-buffer, textured/flat/alpha triangles, bitmap font
  font.rs       5x7 bitmap font
  scene.rs      camera, projector, near clipping, mesh primitives, lighting, fog
  fieldtex.rs   the painted turf texture
  players.rs    the A7FL player meshes and poses
  broadcast.rs  camera work and frame assembly
  hud.rs        score bug, banners, ticker, stats panel
  capture.rs    PNG and GIF writers

crates/cli      the `a7fl` binary
```

The `a7fl_core` crate has no dependencies and no rendering code, so the rules and
simulation can be unit tested (39 tests) without touching the renderer.

## Honest limits

- **It is a stylised renderer, not 2K5.** The presentation vocabulary — camera
  work, score bug, ticker, banners, stats panel — is modelled on 2K5, but the
  geometry, animation and lighting are primitive: no motion capture, no facial
  animation, no broadcast-quality commentary, and no licensed assets or marks.
- **No audio, franchise mode, drafting or online play.** `arcade` is one player
  against the CPU; the renderer writes images and ANSI blocks, it does not open a
  window.
- Teams and players are fictional, with names and colours invented so nothing
  real is reproduced.
- Rendering is CPU-bound: a 1280×720 still takes a fraction of a second, the full
  film package about half a minute.
