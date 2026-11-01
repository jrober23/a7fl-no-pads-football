# A7FL: No Pads Football

A playable American 7s Football League game written in Rust: a complete 7-on-7
rules engine, a simulated season-length game, and a software-rendered broadcast
package in the spirit of the ESPN NFL 2K5 presentation — but played under A7FL
rules, with players in A7FL kit (sleeveless jersey, shorts, bare arms and legs,
no helmets, no pads).

Everything is pure Rust with no GPU and no audio backend, so
it builds and runs headless (a cloud VM, a container, a Raspberry Pi) as well as on
a desktop. The only external crates are `image` and `gif`, both for writing the
rendered frames.

## Download (Windows)

Grab `a7fl-v0.13.0-windows-x86_64.zip` from the
[releases page](https://github.com/jrober23/a7fl-no-pads-football/releases), unzip
it anywhere and run `a7fl.exe`. Switch the pad on before launching: the client
looks for a controller for three seconds, then falls back to the keyboard. Windows
10 or 11, no runtime or installer needed. The build is unsigned, so SmartScreen may
ask you to confirm first (More info → Run anyway).

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
colour blocks straight into the terminal instead. Add `--flip-y` if your pad
reports its stick's vertical axis the other way round. With no controller attached the
keyboard drives it, so the game is always playable.

```
           the field, with a button over every receiver

 NEW 14 - 7 PAT   2ND 6:41   1ST & 10   BALL ON OWN 34
 SLANT RIGHT - DROPBACK PASS - DAL shows COVER 2
 A KELSEY 4.2   B ROSS 1.4   X HALE 3.1   Y DIAZ 0.9
 NEW ball   MOVE   HOLD A/B/X/Y TO PASS   AIM WITH STICK   [START] PAUSE
```

Controls:

| Input | On the call sheet | During the play |
| --- | --- | --- |
| D-pad or left stick | Highlight a play: sideways moves through the set on screen, up and down moves between sets | Move the quarterback (past the line of scrimmage he becomes a runner); with a pass wound up, aim it |
| A / South | Snap the play that is highlighted | Hold to wind up, release to throw to A; once you are running, spin |
| B / East | — | Hold to wind up, release to throw to B; once you are running, hurdle |
| X / West | — | Throw to X; once you are running, dive |
| Y / North | Ask the coach | Throw to Y; once you are running, slide |
| RT / ZR | — | Change camera |
| Start | — | Pause: resume, camera, quit |

| Keyboard | On the call sheet | During the play |
| --- | --- | --- |
| Arrows or WASD | Highlight a play | Move the quarterback; aim the pass |
| Space or Enter | Snap the play that is highlighted | Hold to wind up, release to throw to A; once you are running, spin |
| X | — | Throw to B; once you are running, hurdle |
| Z | — | Throw to X; once you are running, dive |
| C | Ask the coach | Throw to Y; once you are running, slide |
| Left Shift | — | Change camera |
| Esc | — | Pause: resume, camera, quit |

### Calling plays

The call sheet is a grid of cards, one card per play, with the set you are picking
from on screen at once: eight plays in two rows of four. Each card is a small
diagram of the play it calls — the formation, every route, the run path, the blocks
and the line of scrimmage — drawn from the same data the simulation runs, so a card
can never advertise a play the game does not play. The highlighted card is ringed in
yellow and its name and concept are echoed along the bottom of the screen.

The d-pad (or the left stick, or the arrow keys) moves the highlight. Sideways walks
through the plays in the set on screen and wraps around it; up and down move between
sets, dropping off the bottom row into the top of the next set. Press A to snap the
one you are on, or Y to let the coach call it.

### Cameras

RT (Left Shift on the keyboard) cycles the shot while the ball is live. On a throw-off
it is X instead — the trigger is the throw itself there — or pick the shot from the
pause menu:

* **Broadcast** — the high sideline camera the broadcast package uses, panning with
  the ball.
* **Classic** — behind the ball, looking downfield at the defense, so the men you are
  throwing at are ahead of you rather than beside you.
* **Sky** — overhead, for seeing the whole thing at once.

The stick follows whichever of those is on screen, and they do not agree about which
axis runs up it. Broadcast looks across the field, so up the stick is across the field
and right is down it. Classic and Sky stand behind the ball and look down the field, so
there up the stick is up the field — toward the end zone you are attacking — and right
is the ball carrier's right hand, which means the mapping turns around with the teams
when they change ends. It used to be broadcast's mapping for all three, so in Classic
and Sky pushing up ran the man into the sideline.

Every one of them anchors on the ball: wherever it goes, that is what the picture is
about, and the aim lags a few frames behind it the way a heavy lens does instead of
snapping from spot to spot. That is what keeps a long run in the frame — the shot used
to be pinned to the line of scrimmage, so a runner who got past the defence walked out
of the corner of a picture still framing the snap. When a pass is in the air the shot
leans a quarter of the way toward where the ball is going, so the pan starts on the
release rather than at the catch.

Throw-offs have their own shot, because no fixed camera can hold both ends of a
forty-five-yard, three-and-a-half-second throw. It used to sit fifteen and a half yards up,
planted behind the thrower and aimed at the spot the ball was going to land: for the
whole flight you watched a dot travel toward a dot, and the men doing anything about it
were the small end of the picture. It sits at a runner's height now and goes with the
ball — downfield with the throw, then swinging behind the returner the moment he takes
it in, so the return is shot the way a return should be.

### Pausing

START (Esc on the keyboard) pauses the game:

* **RESUME** — back to it.
* **CAMERA** — cycles the same three shots, so the framing can be changed in the middle
  of a drive rather than only between plays.
* **QUIT GAME** — closes the game.

It works on the call sheet and on the fourth-down choice as well, and START no longer
closes the game anywhere: **QUIT GAME** is the only way out. It used to close on the
spot, which meant the camera setting — the one thing a player does want to change
mid-drive — was unreachable once the ball was snapped, and a stray press on the call
sheet ended the whole game.

### Passing

Every eligible receiver gets a button over his head the moment the ball is snapped,
colour-coded by how much room he has: green open, yellow tight, red covered. Hold
his button and the pass meter fills; the longer you hold, the harder the ball is
thrown. The stick bends the throw off him while the meter fills — that is how you
lead a receiver into grass or put it on his back shoulder — and a guide runs out
across the turf to a marker showing exactly where the ball will come down. Let go
and he throws it.

The meter is a range of passes, not one pass thrown harder. Tap the button and it is a
floater — slow, high, and short enough that it cannot be a bomb; hold it and it is a
rope that gets downfield in a hurry. The meter names what it has in it as it fills
(TOUCH, DRIVE, BOMB), and the ball is timed on the grass it actually has to cover, so
a swing pass to the flat crosses the field at the same speed as a comeback instead of
arriving like a fastball.

Power is also range: a tap drops the ball in front of the man and a full meter carries
it over his head. There is a notch on the meter at the weight that would just reach
him, so an underthrow is something you can see coming rather than something the game
does to you. The badge colours are live, the guide is drawn
from the same function that flies the ball, and an overthrown pass is incomplete —
there is no fifty-fifty for a receiver to win on a ball that lands ten yards from
him.

Fourth downs get their own prompt (go for it, or throw it off), and so do conversions
(one point from the 5, two from the 10). Both are lists you drive with the d-pad or
the stick — sideways or up and down, whichever way the list runs — with the
highlighted line in the call sheet's accent, and A takes the answer. Conversions are
then called and played like any other snap. In terminal mode the picture is sized
from `COLUMNS` and `LINES`, so it adapts to whatever terminal you run it in.

The flag itself does not stop the play. It goes down where the foul happened — a
yellow pennant standing on the turf, the call in the corner of the picture and a line
in the bar — and the snap runs out from there: the run still goes for what it goes
for, the throw is still thrown and can still be caught or picked. The walk-off comes
after the whistle, on its own card: what was called, who it was on, the yards, and
the ball and down it leaves behind. A flag on a play that scored is declined; the
points stand.

### Running with it

Once the ball is in your hands and there is no pass left to throw, the four face
buttons stop being receivers and become the runner's moves. On a run play that is
the moment the back takes the handoff. On a pass it is the moment the ball is caught,
or the moment the quarterback crosses the line of scrimmage — past the line a forward
pass is a penalty, so he has nothing left to do with it but run.

| Move | What it does | What it costs |
| --- | --- | --- |
| **Spin** (A) | Keeps his feet and changes the angle; a wrap-up is nearly half as likely to land while he is turning | Nothing but the beat it takes |
| **Hurdle** (B) | Leaves his feet over a defender; a tackle attempt goes under him and the defender is left on the ground | A fifth of his speed, and the stick stops steering him until he lands |
| **Dive** (X) | Lunges forward for the extra yard at half again his speed | The play is over where he lands |
| **Slide** (Y) | Gives himself up, the way a quarterback does rather than take the hit | The play is over, and he does not get the extra yard a dive would |

A move is a state, not a button press that does something on the frame it is pressed:
it runs for a fixed time, the tackle mathematics know what he is in the middle of, and
there is a short cooldown afterwards so mashing the buttons cannot chain moves into a
permanent immunity. The active move is named in the text bar while it runs.

### Throw-offs

The A7FL has no kickoffs: the ball is thrown from your own 35 and has to travel 40
yards in the air. It is yours to throw whenever your team is the one throwing it
off — which is right after you score — and yours to return whenever the other lot
throw it.

Throwing it off is three beats, the way a kickoff is. First the line: left and right
on the stick walks the landing marker across the field, and the bar under the picture
reads the aim back to you in yards. Then the arm: press and hold A and the meter fills
with the marker locked where you left it. Then the throw: let go, and the ball goes on
the weight you gave it. The notch on the meter is the 40-yard rule, and a throw short
of it is a Run Back, so the first thing you learn is that a soft throw is a penalty.
Past the notch it goes where you aim it, and full power comes down on the receiving
team's 20 — twenty yards short of the end zone, with eighty in front of the return.

None of that is on a clock: a throw-off waits for the man on the pad, and the game no
longer opens in the middle of one. A kickoff card sits in front of the first snap with
the matchup, who is throwing it off and what the pad does, and it waits for A. Back to
receive, the computer's throw is held a beat longer and the bar counts it in, so the
ball is never in the air before you have found it.

A throw that breaks the rule is still thrown. Short of the forty, or off the
sideline, the ball goes up where you aimed it and comes down where it was always
going to, and only then does the card tell you it was a Run Back — you watch the ball
leave your hand instead of the picture cutting away from it. A ball nobody catches is
live: it lies there while the returner and the three gunners get to it, and it is dead
when somebody falls on it, or down where it lies if nobody does. Only after that does
the next screen say what the throw-off was worth.

The aim used to run up and down the stick, which in this lens — behind the ball,
looking downfield — is towards the camera rather than across the field, so the one
thing a throw-off asks you to choose was the one thing the stick could not do.

It used to top out at seventy-four air yards, which put every hard throw into the
end zone for a touchback and turned the play into nothing but a ball in the air.
A touchback is still a rule: it just takes a fourth-down free throw from the
other side of midfield to get near an end zone now.

Back to receive, the ball is live and you are the returner: get under it, catch it,
run it back. Nobody teleports you underneath a throw-off any more — if you are not
where it comes down, it lands untouched and dies there. Once you have it the three
gunners are chasing you, so a return is a run rather than a dice roll. Throwing it
off, you become the gunner nearest the ball and can go down and make the tackle
yourself.

The return is also where the tackling work starts: the three-on-one geometry the
throw-off has always had is now something you can drive from either side of it.

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

### Defense

Defense is played out on the field, not decided in the dark:

- **Wrap tackles.** A tackle is not a switch. The defender has to close, get his
  arms around the man and go to the ground with him: the figure sinks, wraps and
  drives, the carrier goes over the top of the hit, and the ball is dead when the
  two of them are on the turf. The clock stops on contact, so the extra beat costs
  picture and not time played.
- **Sacks.** A quarterback still holding the ball in the pocket is a live target.
  The rush gets home, the hit is the same wrap, and the picture says `SACK`.
- **Tackles for loss.** A run stopped behind the line is read as a tackle for loss
  on the picture and on the card, with the yards lost spelled out.
- **Balls in the air.** A defender standing in the flight path is not scenery. He
  either knocks the ball down or takes it away, depending on how well he covers
  and how good his hands are. Height matters — nobody bats down a ball over his
  head — and it is one chance per throw, not a dice roll on every frame of it.

### Simulation

`a7fl_core::sim` runs one snap as a continuous 7-on-7 play at 60 Hz: dropback,
route running against man and zone coverage, pass rush and holding, quarterback
progression reads, throw power and accuracy, catch radius, wrap tackles, broken
tackles, fumbles, interceptions, defensive pass interference, sacks, safeties,
touchdowns and out-of-bounds. `a7fl_core::throwoff` runs the throw-off the same
way, phase by phase (presnap, flight, return, result), with fair catches, muffs,
touchbacks, return touchdowns and safeties.

A tackle, a sack and a return stopped in the open field all resolve the same way:
contact starts a wrap, the two men go to the ground, and the result is read when the
ball is dead. The client holds the shot through the hit and the landing before the
card comes up, so the next screen is about a play you watched rather than one that
happened underneath it.

Each snap is seeded, so a recorded play replays bit-for-bit. That is what lets the
renderer produce a frame for any moment of a play that was already simulated.

The AI has a play-calling personality per team, adjusts to down, distance and
score, decides fourth downs (go for it or throw it off), picks conversion values,
and manages timeouts and clock.

### Presentation

The renderer is a small software 3D pipeline (`a7fl_render`): z-buffered,
perspective-correct textured triangles, near-plane clipping, distance fog and soft
ground shadows, plus five camera setups — high sideline broadcast, behind the
quarterback, sky cam, low replay and end zone.

The lighting is deliberately cartoon: the diffuse term is quantised onto three
tones with a narrow blend at each terminator, a green bounce comes back up off the
turf, and an edge light wraps the silhouette. Broad soft steps read as a painted
figure at 640x360, where a full grey ramp reads as a low-poly model.

The turf is painted at sixteen pixels per yard — mown stripes with feathered mow
edges, yard lines and hashes, painted numbers, coaching boxes, end-zone wordmarks
and the midfield mark — and the ground pass samples it across the whole field.

Around it stands a stadium: a run-off apron, advertising boards, a bowl of seating
with a crowd in team colours, a stand behind each end zone, a roof over the far
side, a scoreboard carrying the league mark and four banks of lights, all placed
under the height of the broadcast lens so the building is in the picture without
ever getting between the camera and the ball.

The overlay is 2K5-flavoured: a score bug with possession highlight and timeout
pips, a down-and-distance card with play clock bar, the A7FL badge, a lower-third
ticker carrying the play-by-play line, big TOUCHDOWN / TURNOVER / SACK / TACKLE FOR
LOSS / PASS BATTED DOWN / FLAG banners, the word over the dead ball while the
players get up off the turf, and a full-screen post-game stats panel.

Players are built at runtime from their attributes and appearance (skin tone,
hair, beard, build) so the seven on the field match the roster. The body is a
skeleton of lofted, rounded cross-sections rather than a stack of boxes: a chest
that tapers into a waist over a seat, quadriceps that swell above the knee, calves
that fill a sock, deltoids that overhang the ribcage — and the jersey carries its
number, painted onto the cloth front and back, so the man wearing 23 is the man
wearing 23.

The figure animates the hits too. A tackle is a pose, not a state flag: the defender
reaches out and around the man with his chest over his toes, and the carrier tips
over the top of it, hips first, until he is flat on the turf. A finished contract is
not needed to see who hit whom.

Each figure is built at one of three detail levels chosen by its distance from the
lens, so a man on the far hash is not paying for eyes, a bicep and forty digit
patches that would each be a pixel and a half wide. Full detail costs about seven
hundred triangles; fourteen figures a frame is not something to spend freely.

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
  geometry is hand-built rather than scanned: figures are flat-shaded triangle
  bodies with painted-on faces, no motion capture, no skinning, no facial
  animation, no broadcast-quality commentary, and no licensed assets or marks. It
  is a full-figure, cel-shaded cartoon look, not a photograph of a football game.
- **No audio, franchise mode, drafting or online play.** `arcade` is one player
  against the CPU; the renderer writes images and ANSI blocks, it does not open a
  window.
- Teams and players are fictional, with names and colours invented so nothing
  real is reproduced.
- Rendering is CPU-bound: a 1280×720 still takes a fraction of a second, the full
  film package about half a minute.
