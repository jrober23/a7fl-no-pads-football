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

Grab `a7fl-v0.18.6-windows-x86_64.zip` from the
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

## The players

Every figure is lofted rather than assembled out of boxes. The torso is a
seven-band loft with a chest, ribs and a waist; the arm is one smooth surface from
the deltoid through a bicep swell, an elbow pinch and a forearm flare into a
gloved fist; the legs are lofted the same way with a quadriceps sweep, a knee and
a calf that swells and tapers into the sock. The jersey goes over the top of the
shoulder and is cut inside the deltoid, so the sleeve cap is cloth and the arm is
bare beneath it — except on the men dealt a long white undershirt, whose sleeves
run from the cap to the wristband. Every head wears a balaclava in club colour,
built as a second sheet over the skull with an aperture cut for the brows and
eyes: cloth over the nose, mouth, jaw and neck, hair and beard underneath it.
Tights are dealt out the same way, per man and stable across replays.

The silhouette is deliberately the one on the A7FL character sheets: shoulders
and arms carrying the width, a waist you can see, a small head, and a stance with
the feet wider than the hips. The kit carries the number front and back, trim at
the armhole, the shorts hem and the sock cuff, gloves, and — depending on the man
— a headband, a knee sleeve, tights and a beard. Skin tone, hair, build and height all
come off the roster, so the same seven never look like seven copies.

The full figure is about 5,100 triangles and the close-up build about 8,400;
further out they are built at a lower level, because eyes and a bicep are worth
nothing at a pixel and a half.

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
 NEW ball   MOVE   HOLD A/B/X/Y TO PASS   TAP TO PUMP   [START] PAUSE
```

Controls:

| Input | On the call sheet | At the line and during the play |
| --- | --- | --- |
| D-pad or left stick | Up and down move between the formations; right (or A) goes into the plays of one, and left comes back out | Move the quarterback (past the line of scrimmage he becomes a runner); the ball itself goes to the man whose button you press |
| A / South | Go into the plays, then call the highlighted one | Snap the ball; then hold to wind up and release to throw to A, tap to pump fake, or spin once you are running |
| B / East | Back out to the formations | Hold to wind up, release to throw to B; once you are running, hurdle |
| X / West | — | Open the audible over the field; once the ball is live, throw to X, or dive as a runner |
| Y / North | Flip the call to the other side of the formation | Flip the call; once the ball is live, throw to Y, or stiff-arm as a runner (slide as a quarterback) |
| Select / Minus | — | Change camera |
| LT / ZL | — | Hold for the play preview: the shot lifts over the route tree until you let go |
| LB / L1 | — | Dummy count at the line: bark the snap count and try to draw a defender across |
| RB / ZR | — | On defense, check to another call: three at a time, over the field |
| Start | — | Pause: resume, camera, quit |

| Keyboard | On the call sheet | At the line and during the play |
| --- | --- | --- |
| Arrows or WASD | Up and down move between formations; right goes into the plays, left comes back out | Move the quarterback; the ball goes to the man whose button you press |
| Space or Enter | Go into the plays, then call the highlighted one | Snap the ball; then hold to wind up and release to throw to A, or spin |
| X | Back out to the formations | Open the audible; then throw to B, or hurdle as a runner |
| Z | — | Flip the call; then throw to X, or dive as a runner |
| C | — | Throw to Y; once you are running, stiff-arm (slide as a quarterback) |
| Tab or Backspace | — | Change camera |
| R | — | Hold for the play preview |
| Q | — | Dummy count at the line |
| E | — | Defensive check at the line |
| Left Shift | — | Throw the throw-off |
| Esc | — | Pause: resume, camera, quit |

On defense the same pad does different work:

| Input | Playing defense |
| --- | --- |
| D-pad or left stick | Move the defender you are on, before the snap and after it |
| A / South, Space or Enter | Tackle: go and wrap up the ball carrier |
| B / East, X | Switch to the next defender, nearest the ball first |
| X / West, Z | Dive tackle: a lunging hit from a half-yard further out. With the ball in the air the same button is the bat-down |
| Y / North, C | Intercept: go and take the ball away |
| Select / Minus, Tab | Change camera |
| Start, Esc | Pause |

### Choosing your team

The first screen is the kickoff card, and it asks which of the two teams is yours:
push the stick to swap the highlight, A to kick off. The team you pick is the one you
run for the rest of the game, both ways; the other is the computer's, and its calls
are its own. Every screen that carries the score carries `YOU: NEW` (or whichever
team you took), so there is never a snap where you are guessing which shirt you are
in. Pick the throwing side and you throw the opening throw-off and then defend it;
pick the receiving side and you run it back and take the field on offense.

### Calling plays

The call sheet is a file, not a list. The formations run down the left — Pistol,
Bone, Trips, Doubles, Spread — with the personnel for each under its name, and the
right of the screen holds the cards for the formation you are on. Each card is a
small diagram of the play it calls: the formation, every route, the run path, the
blocks and the line of scrimmage, drawn from the same data the simulation runs, so a
card can never advertise a play the game does not play.

The first choice is the formation, which is what turns twenty-four designs into
something you can read rather than a wall of cards. Up and down move between
formations, A (or right) goes into the plays of the one you are on, the stick moves
between those plays, and A calls the one you are on. B goes back out to the
formations. Y flips the call to the other side of the formation, and the cards show
it flipped, routes and all. Calling it that way does not put anybody down in a new
place: the men who have to change sides run across the formation to their new spots,
and the two men on the line stay where they were, because a flip moves the skill
players and leaves the guards alone.

The sheet always opens in the formation column, so the first push of the stick is a
formational choice. It used to open wherever the previous call had left the cursor,
which meant that after the first snap the stick was already inside the plays and the
formations could not be reached again without finding B.

### At the line

The ball does not snap itself. You are on the line for as long as you like, looking
at the defense, and A starts the play: the clock does not run and the formation does
not drift while you look. X opens the audible, which is the same layered sheet drawn
over the field — any play in the book, any formation, and Y to flip it to the other
side. A calls it and the team re-lines up. B leaves the play that was already called.

Before the snap the called play is drawn on the grass: every route in the play,
from where the man is standing to where it ends, so the throw can be read against the
defense rather than off a card. You also know which man is yours — a cyan ring sits
under the player on the pad, with a caret and a name over his head, in every phase of
every play: the quarterback at the line, the man with the ball, the defender you
switched to, the returner under a throw-off.

### Playing defense

The game is not offense-only. When the other team has the ball you call the defense,
and its sheet is layered the way the offensive one is. The **fronts** run down the
left — Wide 9, Tight 5, Over 3 — each with the technique numbers the two edges are
playing and what the line is doing with the run (contain, squeeze or spill), and the
cards to the right are the calls in that front. Every card is a diagram of the
defense it calls: the seven aligned from the same front the snap will use, the
rushers' arrows at the quarterback, man lines to the receivers they have, and the
ground each zone defender owns. The lines under the picture spell it out — front,
coverage, rush count, and a job for all seven, from `EDG 9 CONTAIN` to `SAF DEEP
MIDDLE`.

Up and down pick the front, A or right goes into its calls, left comes back out, and
A takes the call. Then you take one of the seven yourself: B walks the pad around the
defense, nearest the ball first, and the stick moves whoever you are on.

A throw-off is three gunners against one returner, as the rules say. The thrower is
not a coverage man: once the ball is away he leaves the picture, and the pad takes one
of the three chasing it. B walks it round them, the way it walks round the seven on a
scrimmage snap.

An interception does not end the play. The man who took it has the ball and the
field in front of him, and the men who threw it are the ones chasing: the three
nearest turn and run him down, and he can be caught, run out of bounds at the
sideline, or take it all the way back for six. If you made the pick yourself, the pad
stays with you and you run it back.

A is the wrap-up. Get your man within reach of the carrier and press it and he gets
the first shot at the tackle, with a step more reach than the coordinator's defenders.
X is the dive: he throws himself at the carrier from further out, gets his hands out
further still, and holds on less — the button for catching a man from behind, and the
one that leaves him on the turf when it misses. Y goes and takes the ball away. With
the ball in the air X is the bat-down instead: he dives at it and knocks it down. X
and Y only work while the ball is up and your man is close enough to touch it, and he
gets one look per throw.

### The ball in the air

A pass is not over when the flight ends. A ball nobody catches drops the last of the
way to the turf, and the whistle, the result and the next snap wait for it to land
instead of cutting in while it is still up at catch height. That is true of a ball
that was batted down as well, and of a throw that was picked off — except that a pick
is not dead at all, because the man who has it is running the other way.

The man who goes to the ground keeps the ball. It used to be let go at the moment
of the tackle, which read as the tackler standing over the pile holding it; it now
stays in the carrier's hands all the way down.

A dead ball is not a cut either. The word over the picture — INCOMPLETE, PASS BATTED
DOWN, INTERCEPTION — used to be stamped on the frame the play ended; it now settles in
over half a second with the sentence underneath it a beat later, while the men who
were running coast to a stop instead of freezing mid-stride.

An interception is not a dead ball at all, and now the picture says so. The moment it
is caught the play turns round: the shot holds for seven tenths of a second while the
camera swings to face the other goal, and then the pad is on the man who took it. The
stick follows the picture rather than the snap, so up the screen is up the field again
and he can run it all the way back for six. Pass it to the other team and you watch him
go, with the shot turned round the same way.

### Cameras

SELECT (Tab or Backspace on the keyboard) cycles the shot, at the line and while the
ball is live. On a throw-off it does the same while you are covering or returning;
while you are throwing, the trigger is the throw itself, so pick the shot from the
pause menu:

* **Classic** — the default, and the first shot the trigger reaches: behind the ball,
  looking downfield at the defense, so the men you are throwing at are ahead of you
  rather than beside you.
* **Broadcast** — the high sideline camera the broadcast package uses, panning with
  the ball.
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

### At the line

The line of scrimmage is a place to work now, not a menu to get through.

Holding LT at the line is the play preview: the shot lifts over the whole route
tree and the routes are drawn heavier, with a bigger bar at the end of each one.
Let go and the picture drops back behind the ball. It is the same drawing that is
always on the grass before the snap, only readable at a glance.

LB is the dummy count. The quarterback barks the count and the men on the ball
either bite or they do not: a defender who jumps is five yards and the down is
replayed, and a defense that sits still has heard the count for nothing.

X opens the audible as a card over the picture, three plays at a time, and the
stick pages through the rest. The field stays on screen behind it, which matters,
because the whole reason to audible is the look the defense is showing. The check
stays inside the formation you are in: it changes the play, not the shape of the
huddle. On defense RB does the same thing with three calls out of the front that is
on the field, and the offense is not re-lined by a check -- the defense is.

On defense you walk the man on the pad before the snap, and there is no line he
cannot cross. The flag is for contact: put a hand on an offensive player while you
are across the line and it is on the ground before the snap, encroachment, five
yards, replay the down. Leaning into the neutral zone, working along the line, or
standing a yard into the backfield is not a foul, and neither is being across when
the ball goes. Once the ball is live a defender goes wherever he likes, because the
man with the ball is not going to wait.

A flip is a jog now, not a slide. Y turns the men who have to change sides, they
run to their mirrored spots at a pace a person runs, and they square their
shoulders to the line when they get there. The men on the ball do not move.

A run has no power meter: the meter weighs a throw, and a running play has no throw
in it. The face buttons are the carrier's moves from the snap, with nothing to wind
up.

### Passing

Every eligible receiver gets a button over his head the moment the ball is snapped,
colour-coded by how much room he has: green open, yellow tight, red covered. Press
his button and the ball goes to him. The sim leads him, so a throw at a crossing
route is aimed where he will be rather than where he is, and the stick has nothing
to do with where it lands: you pick the man, and the ball goes to the man. Hold the
button and the meter fills; let go and he throws it at whatever weight the meter got
to, with a guide across the turf to a marker showing exactly where it comes down.

A tap is not a throw. Press and release inside a sixth of a second and the
quarterback pumps the arm instead: the ball stays in his hand, and the defenders in
front of him who had already committed take a step they cannot get back. Tap twice
and he pumps twice. To throw, hold the button long enough for the meter to start.

The meter is a range of passes, not one pass thrown harder. A short hold is a floater
— slow, high, and short enough that it cannot be a bomb; hold it all the way and it is
a rope that gets downfield in a hurry. The meter names what it has in it as it fills
(TOUCH, DRIVE, BOMB), and the ball is timed on the grass it actually has to cover, so
a swing pass to the flat crosses the field at the same speed as a comeback instead of
arriving like a fastball.

Power is also range: a short hold drops the ball in front of the man and a full meter carries
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
| **Slide** (Y, quarterback only) | Gives himself up, the way a quarterback does rather than take the hit | The play is over, and he does not get the extra yard a dive would |
| **Stiff arm** (Y, runner) | Puts the free hand — the one not carrying the ball — into the chest of the first man who comes at him and shoves him off his line | The runner keeps his feet, his stride and the ball; the next man still gets a shot at him |

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

The lighting is cartoon without being flat: the diffuse term is quantised onto
three tones with a soft blend at each terminator, a green bounce comes back up off
the turf, and an edge light wraps the silhouette. Every corner of every triangle
carries its own normal, so shading is interpolated across the face instead of being
painted on it — a twelve-sided arm is lit as a round arm, not as twelve plates with
a step between each. That is the difference between a model that reads as faceted
and one that reads as carved.

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
that fill a sock, deltoids that overhang the ribcage.

The figure is built to a sixth-generation console look rather than to a low-poly
budget: twenty-two sides around the torso and twelve around a limb, six bands from
waist to shoulder so the pectorals sit over the ribs, cloth broken into soft folds
by the shading instead of reading as one smooth tube, baggy shorts with trim at the
hem, two stripes on each sock, a cleat built as a sole, an upper and a toe box, and
a face with a brow, eyes, a nose and a mouth under the hair. Limbs, torso and head
take their normals from the surface they were built from, so the light runs around
them; the only place a facet is left is the silhouette.

**The number is on the shirt.** Every patch of paint is laid against the same torso
profile the cloth was lofted from, at the depth of the surface under that patch and
with the cloth's own normal, so the digits wrap around the chest, lean with it when
he runs, and are lit as the shirt under them is. Painted on one flat plane — which
is what a jersey with a number floating beside the man looks like — they detach the
moment the figure leans at all. The digits are cut on a five-by-seven grid with bold
strokes rather than one-cell lines, which is what makes a numeral read as a numeral
instead of a scatter of squares.

The figure animates the hits too. A tackle is a pose, not a state flag: the defender
reaches out and around the man with his chest over his toes, and the carrier tips
over the top of it, hips first, until he is flat on the turf. A finished contract is
not needed to see who hit whom.

Each figure is built at one of three detail levels chosen by its distance from the
lens, so a man on the far hash is not paying for eyes, a bicep and a fistful of
patches of paint that would each be a pixel and a half wide. Full detail costs
about two and a half thousand triangles, up from seven hundred; fourteen figures a
frame is not something to spend freely, and this is the one place in the renderer
where the polygons were worth it.

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
  playbook.rs   22 offensive plays, 6 formations, 9 defensive calls in 3 fronts
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
  geometry is hand-built rather than scanned: figures are smooth-shaded triangle
  bodies with painted-on faces, no motion capture, no skinning, no facial
  animation, no texture maps on the body, no broadcast-quality commentary, and no
  licensed assets or marks. It is a full-figure, cel-shaded cartoon look, not a
  photograph of a football game: a yard from the lens you can still see the facets
  on a cheek, and closing that gap takes texture maps, not more polygons.
- **No audio, franchise mode, drafting or online play.** `arcade` is one player
  against the CPU; the renderer writes images and ANSI blocks, it does not open a
  window.
- Teams and players are fictional, with names and colours invented so nothing
  real is reproduced.
- Rendering is CPU-bound: a 1280×720 still takes a fraction of a second, the full
  film package about half a minute.
