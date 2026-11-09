# 7 on 7 No Pads Football

A playable 7 on 7 no-pads football game written in Rust: a complete 7 on 7 rules engine,
a simulated season-length game, and a software-rendered broadcast package in the
spirit of the ESPN NFL 2K5 presentation. Players wear no-pads kit — sleeveless
jersey, shorts, bare arms and legs, no helmets, no pads.

Everything is pure Rust with no GPU and no audio backend (the default build; there
is an optional Bevy window, described below), so
it builds and runs headless (a cloud VM, a container, a Raspberry Pi) as well as on
a desktop. The only external crates are `image` and `gif`, both for writing the
rendered frames.

## Download (Windows)

Grab `7on7-v0.25.111-windows-x86_64.zip` from the
[releases page](https://github.com/jrober23/a7fl-no-pads-football/releases), unzip
it anywhere and run `7on7.exe`. Switch the pad on before launching: the client
looks for a controller for three seconds, then falls back to the keyboard. Windows
10 or 11, no runtime or installer needed. The build is unsigned, so SmartScreen may
ask you to confirm first (More info → Run anyway).

The game is built as a windowed program, so double-clicking `7on7.exe` opens the
game and nothing else — there is no Command Prompt window in front of it. The text
commands still print into the terminal they are run from (`7on7 sim`, `7on7 rules`,
`7on7 teams`, `7on7 play`), and started from Explorer with nobody to print to they
simply run. A build made with `--features console` carries its own console instead,
which is what to use if the borrowed one is not enough for the way you launch it.

## Running it

```bash
cargo run -- teams          # the thirty-two-club league and every roster
cargo run -- rules          # the rule set and the penalty catalog
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
run from the cap to the wristband. A couple of men a side are dealt a balaclava in
club colour — one or two out of the seven, not the whole team — built as a second
sheet over the skull with an aperture cut for the brows and eyes: cloth over the
nose, mouth, jaw and neck, hair and beard underneath it. Everybody else plays
bare-headed, with his hair and beard built. Tights are dealt out the same way, per
man and stable across replays.

The silhouette is deliberately the one on the league's character sheets: shoulders
and arms carrying the width, a waist you can see, a small head, and a stance with
the feet wider than the hips. The kit carries the number front and back, trim at
the armhole, the shorts hem and the sock cuff, gloves, and — depending on the man
— a headband, a knee sleeve, tights and a beard. Skin tone, hair, build and height all
come off the roster, so the same seven never look like seven copies.

The full figure is about 5,100 triangles and the close-up build about 8,400;
further out they are built at a lower level, because eyes and a bicep are worth
nothing at a pixel and a half.

## Playing with a controller

`7on7 arcade` is the real-time client: plug in a controller (an 8BitDo works in
XInput, DirectInput or Switch mode) and run `cargo run --release`. It opens a
window (1280x720 by default, resizable) and draws the field with the same
software renderer as the broadcast package — no GPU, no game engine. Run it with
`--terminal` (or on a machine with no display) and the same game is drawn in ANSI
colour blocks straight into the terminal instead.

Windows are pluggable. Built with `--features bevy_view`, the client can put that
same canvas in a Bevy window rather than the minifb one:

```sh
cargo run --release --features bevy_view -- arcade --bevy
```

The game keeps its own renderer and its own blocking loop; Bevy takes the window,
the event loop and the swap chain, and the two meet over a channel that carries one
frame one way and the pad the other. Nothing about the picture changes, so a
controller still works and every keyboard binding is the same. This is off by
default and needs a GPU, which is exactly the property the default build is built
around not needing, and the release download is built the default way, so `--bevy`
there is not compiled in: it is a build-from-source option. With no display it says
so and falls back to the terminal. From the game's end the two windows are the same
machine -- the same canvas, the same key bindings, the same press edges out of
`Buttons::edges` -- so nothing above the window knows which one it has.

### The look

The picture is treated before it is handed over, in three passes that cost about
6.5 ms a frame at the canvas size here. The glow is deliberately kept to a hint —
at a stronger setting the middle of the field washed out and the whole picture read
as fog rather than weather:

- **Soft edges.** Hard polygon edges stepping a whole pixel are the loudest
  early-3D tell. Brightness edges are found and blended across, more on a real
  edge and less inside texture detail.
- **Glow.** The brightest parts of the picture bleed a little — the white of the
  lines in sun, the score bars, lit jerseys. The bright pass is quarter-sized
  before it is blurred, so it costs a fraction of the frame.
- **Grade.** A gentle vignette, so the eye sits on the middle of the play.

The overlay is drawn afterwards, never through it, so a score bug stays crisp with
soft edges of its own rather than going through the same wash as the grass.
Chrome is built from a painted panel — a shadow, a lit top shading to a dark
bottom, a hairline edge and a highlight — rather than a flat rectangle, and type
above one pixel is drawn with soft outlines instead of a staircase of whole
pixels while its strokes stay solid.

The figures are built to the club art sheets: a shoulder span deliberately past
human, deltoids as the widest thing on the body, a bicep that swells and a forearm
that flares under the elbow, heavier quads and calves, a sleeve that ends on a band
of trim, and gloved fists. `cargo run --release -p nopads_render --example sheet`
draws one man from four sides at close detail into `artifacts/sheet.png`; on the
field a player is forty pixels tall, so that sheet is the only way to work on one
and know whether it got better. Add `--flip-y` if your pad
reports its stick's vertical axis the other way round. With no controller attached the
keyboard drives it, so the game is always playable.

```
           the field, with a button over every receiver

 NEW 14 - 7 PAT   2ND 6:41   1ST & 10   BALL ON OWN 34
 SLANT RIGHT - DROPBACK PASS - DAL shows COVER 2
 A KELSEY 4.2   B ROSS 1.4   X HALE 3.1   Y DIAZ 0.9
```

That page under the picture is the terminal's; the window draws the field and
nothing over it. Every button is on the **CONTROLS** page instead -- open it from
**SETTINGS**, or from **CONTROLS** in any pause menu -- where it is laid out on
four pages with the moves the old bar never had room for.

Controls (all of them are on the **CONTROLS** page, under SETTINGS, and on the
same page from every pause menu):

| Input | On the call sheet | At the line and during the play |
| --- | --- | --- |
| D-pad or left stick | Up and down move between the formations; right (or A) goes into the plays of one, and left comes back out | Move the quarterback (past the line of scrimmage he becomes a runner); the ball itself goes to the man whose button you press |
| A / South | Go into the plays, then call the highlighted one | Snap the ball; then hold to wind up and release to throw to A, tap to pump fake, or spin once you are running |
| B / East | Back out to the formations | Hold to wind up, release to throw to B; once you are running, hurdle |
| X / West | — | Open the audible over the field; once the ball is live, throw to X, or dive as a runner |
| Y / North | Flip the call to the other side of the formation | Flip the call; once the ball is live, throw to Y, or stiff-arm as a runner (slide as a quarterback) |
| Select / Minus | — | Change camera |
| LT / ZL | — | Hold for the play preview: the shot lifts over the route tree until you let go; in the play editor it cycles the handoff, and on a pass turns the play into a play action |
| LB / L1 | — | Dummy count at the line: bark the snap count and try to draw a defender across; in the play editor it cycles a pass's drop |
| RT / ZR | — | Throw the throw-off |
| RB / R1 | — | On defense, check to another call: three at a time, over the field |
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
| Right stick up / down, T / G | Hit stick: a high hit over the nameplate or a low hit underneath. Either goes for the ball; the high one is the one that can be flagged |
| Select / Minus, Tab | Change camera |
| Start, Esc | Pause |

### The start menu

Bare `7on7` opens on the splash — the **7ON7** mark, the name and a football above
it — and a press of any face button, START or SELECT goes on to the start menu:
**GAME MODES**, **ROSTER**, **CUSTOMIZATION**, **SETTINGS**, **QUIT**.
The mark is drawn from the same geometry the club badges are, not loaded from a
file, so it scales with the interface grid. Coming back to the front end from a
game does not show the splash again. The stick moves, A selects, START leaves. `7on7 arcade --home 3 --away 7`
skips the front end and goes straight to kickoff with the clubs named, which is what
the film and timing paths use.

### Game modes

**GAME MODES** is everything you can play: **EXHIBITION** is one game picked in a
minute, **SEASON** is a whole league year, and with them are the three sessions that
are drills rather than games — no clock, nothing on the scoreboard, and you leave
when you have had enough.

The exhibition and the three drills ask for the same two clubs and the same side of the ball the exhibition
screen does — home team, away team, which end you take — so a drill is played with
the club you meant to practise with. **PRACTICE** goes on to ask which book to run,
the same picker the exhibition screen uses, so a book drawn on the shelf is what the
snap is played from. The tutorial and the gauntlet do not ask: the tutorial teaches
off the club's own plays, and a return drill runs none. The pause menu inside them
is the game's: same resume, same controls page, same lens.

**TUTORIAL** opens a second screen: **OFFENSE**, **DEFENSE** or **BOTH**. The
offense session is one control at a time, lined up on the ten. It asks for four
things in order — **snap it**, **throw it**, **run it**, **score it** — and it does
not move on until the snap did the thing it asked for: a run does not finish the
throw, and a play stopped at the one does not finish the score. The defense session
is three: **call it**, **stop it**, **cover it** — call a defense off the sheet,
make a tackle inside ten yards, and break up or intercept a pass. **BOTH** runs the
offense and then the defense. When the steps are done it tells you where the rest
of the controls live, which is the controls page on the pause menu.

A defender under a thrown ball shows the play he has decided on: **two hands up
for the interception, one hand up for the swat**, whether the pad is on him or the
AI is playing him. It is the same read a passer gets in real life, and it is set when
he commits, so a defender who goes for the pick and misses still looked like a man
going for the pick.

**ROSTER** on the start screen holds every club's depth chart and roster.
**CUSTOMIZATION** holds the create modes:

- **CREATE A TEAM** — fifty save slots. Pick an empty one and set the city, the
  team name, the division, the primary and accent colours, the jersey pattern and
  the attributes. The mark is cut from the name and kept unique, and every created
  club is dealt a fresh roster of its own men, so it never fields another club's
  players. A created club turns up in every picker the built clubs do.
- **EDIT A UNIFORM** — repaint any club's home and away kits, colour by colour,
  with a pattern and a restore row. The kit is worn wherever that club plays.
- **DESIGN A PLAYBOOK** — a shelf of up to six books, each one named. Start one
  from a blank sheet or from a copy of any book the league runs, then change
  anything. Every spot carries its job and Y changes which job stands there, so
  any of the seven can line up anywhere the rules allow, with the line rule and
  the quarterback checked while you draw. A play is drawn with the stick: A puts
  the pen down, every yard you walk the path picks up a waypoint, and A lifts it
  again. On a run the shoulder button picks which man takes the ball on the snap --
  the quarterback, the back, the tight end or either wide man -- and he is drawn
  with it at his feet and his path in white. Every other man with a path is marked
  **PITCH**: those are the men coming round behind the ball, and in the game they
  are the four face buttons, so pressing one pitches back to him and not pressing
  any of them leaves the ball with the man who has it. A man with no path blocks.
  That is the whole read option: point the ball at the quarterback, draw the back
  and the tight end coming round, and decide at the mesh -- a pitch into traffic
  can come out of the catcher's hands. The ball is handed over by the two men
  meeting, not by a button: the quarterback runs his own drawn path — his start,
  and the line he takes to the mesh — and gives it up as soon as he and the man
  the play was drawn for get to each other. The give goes the runner's way: he
  takes the runner's inside first, so a back sweeping left is handed the ball
  from his right and the ball travels left with him — he steps round rather than
  hand it back across the runner, against the run. He turns to the man
  as they meet and holds the ball out to him with both hands, so the give is
  offered before it is taken -- and the exchange waits for the ball to be out,
  so a handoff is made at arm's length rather than thrown at him from three
  yards. The man the play was drawn for keeps running it: a run path is the
  ground the card draws, so an outside run drawn left starts going left instead
  of forward-left, and with the stick at rest a sweep goes on round the corner
  by itself. The stick takes him back the moment it moves. Draw the quarterback a path that
  crosses the runner's and that crossing is where the handoff happens; leave him
  no path and he stands and waits for the runner to come to him. Whoever has it
  can give it on while he is still behind the line, so a back who takes the first
  one can pitch again to the man trailing him: that second pitch is the triple
  option. Past the line the option is gone and the buttons are the runner's moves
  again. The buttons are never the handoff: a man already carrying it is not on
  one, and the ones that are up are the pitch.

  What kind of give it is, the designer sets, and it changes the play: **FORWARD**
  hands it in front of the runner, **INSIDE** hands it back and across the
  quarterback's face and is the plain exchange every play starts as, **OUTSIDE** is a toss that can be made with the runner
  already level with the ball, **FAKE** is sold and kept — the runner runs his
  path as a decoy and the quarterback keeps it, so a run becomes a bootleg and a
  pass becomes a play action — and **OPTION** is the read give: the quarterback
  keeps it at the mesh and the man the play was drawn for runs his path as the
  pitch, so whoever is behind the ball is on a face button until the line is
  crossed and the decision is the player's. An option needs both paths drawn,
  the quarterback's keep and the pitch man's route, and while he has the ball the
  line above the buttons names the end he is reading, on the side the ball is
  going, so the read is a man rather than a guess. On a pass the same screen sets the drop: **3 STEP**,
  **5 STEP** or **7 STEP**, and the routes have that many steps' worth of time to
  get open. Left trigger cycles the handoff, left bumper the drop, and the line
  under the play's name says which one is set. On a pass the same trigger is the
  play action: the quarterback turns and shows the ball to the man coming across
  his face, then pulls it back and drops, and the men behind the front lean
  toward the fake while it is being sold.

  A play is a thing to watch as well as a thing to draw: press the right trigger
  in the editor, or on a play in the list, and it opens a session of its own — a
  play test, not a practice drill. That one play is the play, every snap, with no
  clock, no downs and no call sheet: what is being watched is the drawing, not a
  session of football. The card after each snap says how many yards it made, [A]
  runs it again and [B] goes back to the list it came from, on the play he was
  drawing.

  The same trigger works on a defensive call, in the coverage editor or on the
  call in the list, and opens the same kind of session on the other side of the
  ball: the call is the call every snap, the machine's offense snaps a play at
  it, and the card says how many yards it allowed. [A] runs it again and [B] puts
  him back in the calls of the front he drew it on. The computer runs the same play the same way: it reads the end on the
  play side once, hands off if he stays wide, keeps it if he crashes down, and
  gives it on to the trailer when they come for the keeper. The defense is drawn
  the same way: a front of X's, then a call whose
  coverage you draw defender by defender, each drop a path on the same grid. A
  The formations of a book are listed by name with the formation itself drawn
  beside the list — the men as circles (X's on defense) on the yard grid with a
  job against each one — so a name and the shape it stands in are read at once,
  and a new formation is named as it is made, before a man has been moved. A on
  a formation opens a drop-down: **EDIT FORMATION** and, on offense, **EDIT
  PLAYS** (on defense, **EDIT CALLS**), with rename and delete on it too, so the
  shape and the plays under it are two roads out of the same name rather than one
  screen behind the other. A book holds two to five formations a side, each with
  one to nine plays, and two to five fronts, each with one to nine calls. Y
  renames a book, a play or a call from the screen that lists it. Once a book is legal it
  stands at the top of the book picker ahead of the league's, marked **READY** and
  taken with A, so it can be run in exhibition, in a season and in practice, and a
  created club can carry it. A book still being drawn is listed too, marked **NEEDS
  WORK** with the one rule it is missing under it, so it never disappears from the
  picker while it is unfinished. On
  defense at the line, tap **LT** to press the corners or back them off.

What you make — the clubs, the kits and the playbook — is kept in `custom.txt`
beside the settings and read back at start-up.

**PRACTICE** asks the same question the tutorial does — offense, defense or both —
and then which book to run and how hard the other lot should play, and gives you the
same session without the list: your call from your book, no clock, no downs, no
turnover to lose, and the ball back on the same spot for the next snap. It is where
a play gets learned without a game being spent on it. There is no toss in front of a
practice session: the side of the ball is the choice, so there is nothing for a coin
to decide. The toss is the gauntlet's, where it settles who brings the first return
back.

**THREE-ON-ONE GAUNTLET** is the drill a throw-off already is — one man with the
ball and three gunners coming for him, nobody else on the field — played back and
forth between the two clubs. There is no book to pick in it, because a return drill
runs no plays: it asks for the two clubs, the side, the difficulty and the toss, and
that is the whole card. Both ends of it are yours: you bring your club's returns
back, and you throw the other club's off, so nothing in the drill is played by
nobody. The toss decides who brings the first one back; then the clubs alternate,
five returns each player's side, one club's card against the other's. A return is scored by the ground it made: thirty yards is a point, sixty
is two, the house is six. If the five apiece leave a tie — or a lead of one —
sudden death: more returns in pairs, and it takes a lead of two to end it.

**ROSTER** is the league's paperwork, and it is for every club rather than only
the one you are about to play: pick a club off the thirty-two and the screen
carries its **depth chart** and its **roster**. The depth chart is the eight
positions down the left with the men who play them, best first, the way the game
picks its seven — the starter in white, the man behind him greyed. The roster is
every man on the side beside it, number, name, position and rating, sixteen to a
page and the stick pages through the rest. The club's overall is on its card in the
grid, so a side can be read before its roster is opened.

### Exhibition

The league is thirty-two clubs, eight in each of four divisions — North, South, East and
West — and the picker rolls them like a carousel: the badge in the middle, the city and the
nickname underneath it, the clubs either side waiting their turn. Up and down change
division, left and right roll through the eight clubs in it, A takes the card you are
on.

Both sides are yours to pick, and the screens say which is which: **HOME TEAM** and
then **AWAY TEAM**. The second can come from any division — a divisional game is
still there if you want it, it is just no longer the only game in the menu.
After the last club in a division there is a **RANDOM** card: take it and the
division you were looking at picks the club for you, never the one already in the
game. Both screens carry one, so a player who does not want to choose can roll
twice and be done.

The club you take is yours for the whole game, both ways; the scoreboard carries
`YOU:` against it, so there is never a snap where you are guessing which shirt you
are in.

Then the ground: twenty-two of them, each with a picture of the field you will be
looking at, so the stadium is picked before anything else happens on it. Pick it and
you are standing in it. The arrows walk the grid one picture at a time — up is up —
and stop at the top and bottom of it rather than wrapping round to the other end.

Then the game itself, on one card: **QUARTERS** and **DIFFICULTY**, and then
**KICK OFF**. Both are about the game you are about to play, so they are asked here
rather than buried in the settings screen, which is where the machine's defaults
live. The club's name is over the coin, not over a menu.

Then the toss. Call heads or tails and watch the coin come down. Win it and you
choose whether to throw the opening throw-off or receive it; lose it and the computer
takes the ball. Either way the next thing you see is the throw-off, not another
screen.

The cities are real; every nickname, mark and colour set is invented for this game,
so nothing on a jersey belongs to a club somebody already owns.

### Season

**SEASON** sits under GAME MODES and plays a whole league year rather than one game.
Pick a club, then the book it runs — the league's or one off your own shelf — and
then the length, anything from four weeks to the full thirty-one —
and the season is scheduled round robin, so every club plays every week and no two
clubs meet twice until the league has been round. The top two in each division make
an eight-club bracket, seeded by record, running quarterfinal, semifinal and
championship; the final is played at the 7-on-7 Championship ground. The season
screen carries the week, your next fixture, your division's table and the playoff
picture, and left and right flip it to the bracket. **PLAY MY GAME** drops you into
the fixture, **SIM MY GAME** hands that week to the simulation, and every other game
in the league — playoffs included — is simulated as the weeks go by.

### Settings

Four things, all of them changed without leaving the screen: the default camera
(CLASSIC, BROADCAST or SKY), the lens, the window resolution (960x540, 1280x720,
1600x900, 1920x1080) and whether the game runs in a window or borderless fullscreen.
Left and right walk a value and A cycles it. The game opens on **1920x1080**: the
canvas is 640x360, so 1080p is exactly three screen pixels to one canvas pixel, with
no resampling anywhere in the chain. A smaller desktop steps down to 1600x900 or
1280x720, which are whole-number steps too. Changes apply as you make them and are written
to `%APPDATA%\7on7\settings.cfg` on Windows (`~/.config/7on7/settings.cfg`
elsewhere), so the next launch opens the way you left it. Delete that file to go back
to the defaults; a hand-edited or half-written file falls back to them as well.

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
formations. Y flips the call to the other side of the design, and the cards show it
flipped, routes and all. Calling it that way does not put anybody down in a new
place: the two outside receivers keep their sidelines and trade jobs, so the deep
route that belonged to one man is run by the man across from him. The insides -- the
tight end and the back -- walk across to their mirrored spots, and the two men on
the line do not move at all.

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

A throw-off is three gunners against one returner, as the rules say. All four face
the ball: the throwing side looks downfield at the returner, and the returner looks
back at the men coming for him. On normal and hard the coverage is taught to
contain: the nearest gunner goes and gets him while the other two take a lane either
side, a step ahead, and close only once the chase arrives — so the ground he is
given is up the middle, where the help is, and not the corner. On easy they come at
him in a line instead, which is the difficulty where the return is the reward. The thrower is
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

The right stick is the hit stick on defense. Push it up as the tackle lands and he
goes over the nameplate; push it down and he goes underneath it. Either one is a
tackle thrown for the ball rather than for the man alone, and how much of it he comes
up with is his strength and his tackling added together: the strongest tackler in the
league is worth about three times the softest one, and the high hit is the bigger of
the two. It is also the one the flag is watching for, because this is a no-pads
league and a tackle above the nameplate is unnecessary roughness. Leave the stick
alone and the tackle is the ordinary wrap-up. The line at the bottom of the picture
reads `HIGH HIT` or `LOW HIT` while the stick is held, so you can see what the hit is
before the ball comes out.

### Replay

The pause screen can run the last snap back. START, then **REPLAY**: the tape of
the play you just ran is in your hands, and it opens playing.

| Input | In the replay |
| --- | --- |
| A | Play it; press it again to stop it where it is |
| RB / LB | Run it forward fast, or back fast |
| Y / X | Step one frame forward, or one frame back |
| Right stick up / down | Zoom in and out |
| Right stick left / right | Orbit the camera round the play |
| Left stick | Pan the camera over the field |
| B or START | Back to the game |

The picture is the same broadcast the game is played in — the shot is only off its
rail this time, so a player can stand where the camera cannot and look at a play
from the other side of it. The tape is the snap you were in when you paused, from
the snap to the whistle: your throws, your cuts and your stick work are all in
it, because it is the frames that were played rather than a re-run of them. A
kick is on the same tape from the moment it is caught, so REPLAY appears on a
return once the ball is in the returner's hands.

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
* **Sky** — overhead, straight down the field: the lens stands behind the ball in
  the ball's own lane instead of off to one side of it, so it looks upfield rather
  than across the ground.

The stick follows whichever of those is on screen, and they do not agree about which
axis runs up it. Broadcast looks across the field, so up the stick is across the field
and right is down it. Classic and Sky stand behind the ball and look down the field, so
there up the stick is up the field — toward the end zone you are attacking — and right
is the ball carrier's right hand, which means the mapping turns around with the teams
when they change ends. It used to be broadcast's mapping for all three, so in Classic
and Sky pushing up ran the man into the sideline.

The lens sits on top of all of it. The zoom and tilt tuned in the settings, or at
the pause screen, are applied to every camera — Classic, Broadcast, Sky, the
throw-off shot and the replay — so the shot the player set is the shot the whole
game is played in, rather than one framing at the line and another on the kickoff.

Every one of them anchors on the ball: wherever it goes, that is what the picture is
about, and the aim lags a few frames behind it the way a heavy lens does instead of
snapping from spot to spot. That is what keeps a long run in the frame — the shot used
to be pinned to the line of scrimmage, so a runner who got past the defense walked out
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
* **LENS** — the lens screen from the settings, on the frozen game: left and right
  zoom, up and down tilt, X resets, A returns to the menu. The picture behind it is
  the game you are paused in, not a formation, so what you see is what you get.
* **QUIT GAME** — closes the game.

The lens is saved when the game closes, so a shot tuned at the half is still there
the next time it opens.

### The final whistle

The FINAL card is a choice, not a dead end: **A** plays the game again — the same two
clubs, on the same ground, under the same clock and difficulty, from zero — and **B**
goes back to the front end to pick a different game. It used to show the box score
and stop there, with nothing to press and no way back into the program.

It works on the call sheet and on the fourth-down choice as well, and START no longer
closes the game anywhere. **LEAVE GAME** asks which game you mean: *BACK TO THE MENU*
puts down this one and returns to the front end to pick two clubs again, and *QUIT THE
GAME* closes the program. It used to be one choice only, and it closed the window on
the spot, so a player who wanted another game had to start the program again.

### At the line

The line of scrimmage is a place to work now, not a menu to get through.

Holding LT at the line is the play preview: the shot lifts straight up over the
whole route tree — it rises into the sky shot, and every frame of the rise is square
to the field rather than tilted across it, because only the height and the distance
travel and the axis never leaves the ball's lane — and the routes are drawn on the
grass, heavier, with a bigger bar at the end of each one.

The line of scrimmage is painted blue and the first-down marker yellow: the line you
have to reach is the bright one, and the line the ball is already on is not. Let go and the picture drops back behind the ball and the drawing goes
with it — the call is not left lying open for the defense to read.

The line is live otherwise. You may walk a defender anywhere before the snap,
including across it; a hand on an offensive player while you are across is
encroachment and is whistled on the spot. Standing across when the ball is hiked is
offside, five yards, and the same goes for an offensive man who is past the line
when it goes.

LB is the dummy count. The quarterback barks the count and a man either bites or he
does not, and the flag follows the man who moved rather than the side the count was
aimed at: a defender drawn across the ball is encroachment, five yards on the
defense, and a count that catches your own line leaning is a false start on you,
five yards the other way. Either way the down is replayed and the ball never goes.
A defense that sits still has heard the count for nothing, which is the risk: the
count is a gamble with both teams' nerves.

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

A flip is a jog now, not a slide. Y turns the men inside who have to change sides,
they run to their mirrored spots at a pace a person runs, and they square their
shoulders to the line when they get there. Nobody outside moves a step: the two wide
men keep their sidelines and take each other's assignment, and the men on the ball
do not move at all.

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
permanent immunity.

### Scoreboard, replays and the picture

The scoreboard sits in the top-left corner the way a broadcast's does — both clubs,
their colours, the score, timeouts, quarter, clock and down — and it gets out of the
way when the ball is hiked, coming back once the play is dead and the next one is
set. Classic, the shot from behind the ball, is tilted down a little further than it
used to be, so the running back behind the quarterback is in the frame instead of
under it.

Any snap that gains fifteen yards or more, scores, comes back out of an
interception, or carries any flag at all gets a replay before the card comes up, with
`REPLAY` in the corner: A skips it. The replay is the play as it was played — the
frames it was drawn from are kept for it — so the throws, moves and stick work in it
are the ones you put in.

A flagged snap is replayed at the flag instead. The tape is cut to the spot of the
foul and labelled `THE FLAG`, because on a big play with a call on it the call is
what the second look is for: the pennant stands where the infraction happened in the
same picture, and the play that made the replay worth running is the background.
Every flag gets that second look, whether or not the play was worth one on its own.

A man who goes out of bounds stays on the grass side of the advertising boards. The
boards stand on the bleacher line rather than on the paint, with a run-off apron
between them and the white sideline, so the sideline is still the field's edge and
the barrier is the base of the stand: one straight line the length of the ground.
The picture used to let a man run through the boards and into the front row, which
read as crossing the bleachers rather than stepping out at the paint.

There are no kickoffs: the ball is thrown from your own 35 and has to travel 40
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
live: it lies there while the returner and the three gunners get to it, and whoever is
closest when they have all had two seconds is on it — the receiving side, or the
throwing side if the receiving side never got there. It is not whistled dead with
nobody touching it. Only after that does the next screen say what the throw-off was
worth.

The aim used to run up and down the stick, which in this lens — behind the ball,
looking downfield — is towards the camera rather than across the field, so the one
thing a throw-off asks you to choose was the one thing the stick could not do.

It used to top out at seventy-four air yards, which put every hard throw into the
end zone for a touchback and turned the play into nothing but a ball in the air.
A touchback is still a rule: it just takes a fourth-down free throw from the
other side of midfield to get near an end zone now.

Back to receive, the ball is live and you are the returner: get under it, catch it,
run it back. Nobody teleports you underneath a throw-off any more, but a man who is
not touching the stick is standing under the ball rather than frozen at his mark, so
his feet take him to it and the catch is his. A ball that comes down more than three
yards from him is nobody's catch: it is on the turf and live, and the men chasing it
sort out who falls on it. The thirty-second clock is on the return, from the catch,
not on the throw-off, so a throw that took its time can no longer have the return
whistled dead the instant it is caught. Once you have it the three
gunners are chasing you, so a return is a run rather than a dice roll. Throwing it
off, you become the gunner nearest the ball and can go down and make the tackle
yourself.

A return is a run, and a run has the runner's buttons: **[A] spin, [B] hurdle,
[X] dive, [Y] stiff-arm**, the same four a back gets, with a dive ending the return
where he goes down. The man who picks off a pass and the man who falls on a fumble
get them too. A return used to be the one carry in the game where the face buttons
did nothing and the only way past the last man was to outrun him.

On the cover, a dive that gets there and does not hold on puts the gunner on the
turf — and he stays there for a beat before he is back on his feet and chasing
again, and he is not steered while he is down. He used to be left on the ground for
the rest of the return and still driven by the pad, which is a body in a diving pose
running after the ball. A return that goes twenty-five yards or more is a big play
like a long snap, so it is flagged for the highlight package.

The return is also where the tackling work starts: the three-on-one geometry the
throw-off has always had is now something you can drive from either side of it.

Controller support comes from `gilrs` and is a default-on Cargo feature. On Linux
the crate needs the udev headers (`sudo apt install libudev-dev`); build with
`--no-default-features` on a machine without them, and everything except the
controller still works (the keyboard and the window do not depend on it).

### Difficulty, the lens and the crowd

The setup card carries three levels. **Easy** is the game as it shipped: the
other sideline calls a sensible mix and plays what is in front of it. **Normal**
throws on early downs and pushes the ball downfield instead of grinding for the
sticks, and is quicker to go for it on fourth down. **Hard** adds a defense that
hunts the ball: it presses and plays man on third down, and it goes for the strip,
so a hard side takes the ball away more often, on the ground and in the air. The
level is remembered in `settings.cfg`, so the card opens on the last one played.

**Lens** opens a live preview: left and right zoom, up and down tilt, X resets. It is
the real broadcast camera on a real formation rather than a diagram, and the two
numbers are saved and applied to every camera mode.

The crowd is built man by man now, and it moves: every body bobs on its own phase,
and a few put their arms up on the cheer. The bleachers also sit five yards further
back from the advertising boards, so a man running out of bounds is not standing in
the front row any more. Field numbers stand with their feet on the sideline they belong
to and their tops toward midfield, reading left to right from that stand. The two
sets are therefore half a turn apart, which is how a real field is painted.

Replays start at the snap rather than at the line. When the tape runs out the picture
holds on the last frame, then sinks to black before the call sheet comes back. While
the route tree is up for the pre-snap preview, the scoreboard gets out of the way so
the whole call can be seen.

## What is implemented

### No-pads rules, not NFL rules

The rule set follows the 7 on 7 no-pads rulebook rather than the NFL's:

- **7 on 7.** Quarterback, two down linemen and four eligibles against two edge
  rushers, two linebackers and three defensive backs.
- **4 × 15 minute quarters, running clock.** The clock stops only on penalties,
  change of possession and conversions (plus incompletions and out-of-bounds
  inside two minutes). 30 second play clock. 4 downs to gain 10.
- **No kicking of any kind.** There are no field goals, no punts and no kickoffs.
- **Throw-offs replace kickoffs.** The ball is *thrown* from the throwing team's
  own 35, with three gunners against one returner, a minimum of 40 air yards.
  Fair catch is legal, a touchback comes out to the 20, and a returner downed in
  his own end zone concedes a safety. A returner alone under the ball always fields
  it: only a catch made in contact can be muffed.
- **Fourth down becomes a throw-off** from the line of scrimmage instead of a punt.
- **The line blocks the man in front of it.** Each lineman takes the closest rusher
  to him, hands out in front — which is a wider block than a body standing in the
  way — and the hand on the side the rusher goes to follows him up to 45 degrees,
  with the body turning after it. That is the pocket: a wall that bends with the man
  in front of it. On a run the hands come up and the defender is driven back upfield.
- **Laterals are live.** A pass thrown backwards that hits the turf is not an
  incompletion: it is a loose ball, and either side can come up with it.
- **A fumble does not stop the play.** The ball comes out, it rolls, both teams go
  after it, and whoever recovers it is a ball carrier: he runs with it, with every
  runner's move, until he is tackled. Only then is the ball dead, and a defender
  who recovers can take it back for six.
- **A backed-up computer throws it off.** On Easy and Normal it takes the throw-off
  from anywhere on its own side of the field rather than snap a long fourth down.
  Hard will go for it instead when it is behind late enough that it has to have the
  ball, and otherwise decides on down and distance.
- **The preview button works on defense too.** Hold it at the line and the picture
  lifts over your own call: man cover gets a line to his man, zones get the ground
  they own, and the rushers get their track.
- **Conversions are chosen, not kicked.** 1 point from the 5 or 2 points from the
  10, and the choice is locked in for the game.
- **Overtime** gives each side a possession from the 25 with two first downs
  allowed; two-point conversions are forced from the second overtime on.
- **Mercy rule:** five touchdowns before the end of the first quarter ends the game,
  and seven touchdowns from the second quarter on. The lead is checked the moment it
  is taken, not at the next change of quarters.

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
- **The hit stick.** The right stick behind a tackle turns a wrap-up into a hit for
  the ball. Up is over the nameplate, down is underneath; either adds to the chance of
  knocking it loose, in proportion to the tackler's strength and tackling, and the
  high hit adds more and carries the roughness flag with it.
- **Sacks.** A quarterback still holding the ball in the pocket is a live target.
  The rush gets home, the hit is the same wrap, and the picture says `SACK`.
- **Tackles for loss.** A run stopped behind the line is read as a tackle for loss
  on the picture and on the card, with the yards lost spelled out.
- **Returns are worth running.** The chase has to turn and run and the man with the
  ball does not, so an interception comes back ten or fifteen yards before anybody
  can lay a hand on it, instead of dying three yards past the catch.
- **The line works inside out.** On Normal and Hard every blocker takes his own
  rusher, starting with the man nearest the ball, so two linemen never double up
  while a third walks through the gap they left. Easy still blocks the nearest body.
- **Balls in the air.** A defender standing in the flight path is not scenery. He
  either knocks the ball down or takes it away, depending on how well he covers
  and how good his hands are. Height matters — nobody bats down a ball over his
  head — and it is one chance per throw, not a dice roll on every frame of it.

### The grounds

Twenty-two of them, chosen before the coin toss: you pick the ground you want to
play on before you call the coin.

The two practice fields are the ones the game shipped with: **the practice field**,
open air with a full house, and **the closed practice**, the same bowl with the roof
on and the stands empty. The lens sits under the roof, so the shot still sees the
field rather than the top of the building, and both long walls and both ends of it
are given over to tall windows, so the building reads as glass with a night behind
it rather than as a sealed box.

The practice field is the league's ground in the suburbs. A street runs behind each
sideline and behind each end — asphalt, a kerb down each edge and a dashed centre
line — with two rows of houses on the far side of it, each one fronted by a
driveway, streetlights on the kerb, back-yard pines over the roofs, and grass
running out past the houses to a blue horizon, with the town itself low on the sky
beyond. The shot down the field ends in lit windows, rooflines and ground rather
than in a black band. None of it is inside the boards: the paint still belongs to
the players.

The thirty-two clubs each have their own park, and each one is its city: Boston's
harbour and brick rowhouses, Chicago's stacks and steel, Miami's palms and pastel
hotels, New York's wall of windows, Denver's pines under the mountains, and so on.

Every one of those cities puts its own building over the rooftops, the one it is
known by: the pencil in Atlanta, the Empire State in New York, the needle in Seattle,
the ball on the stick in Dallas, the obelisk in Washington, the twin spires in
Nashville, the drums of the Renaissance Center in Detroit, the barrel on the roof in
Denver, and a butte standing in for Phoenix. Past the end of the field there is a
downtown to go with them: a street grid with asphalt, a kerb down each edge and a
dashed centre line, and the low and mid-rise blocks that fill the land between the
streets, lit floor by floor, standing between the ground and the towers. The skyline,
the sky and the fog change with the ground; the field, the stand and the scoreboard
stay the league's.

Then the four the league takes the game to:

- **The Deep** — a ground on the floor of the ocean, under a clear glass dome. The
  glass is only its frame — ribs over the vault, a rail at every course, mullions
  between them — so the sea is seen through it rather than painted over by it. Out
  on the sand there are coral heads in the colours a reef comes in, forests of kelp
  standing in the current, dunes and ripples the water has left, and a school of
  fish crossing past the glass, moving frame by frame the way the crowd does. The
  water is dark at the sand and turns to daylight towards the surface, and the field
  sits on it all, lit.
- **The Back of Beyond** — a field cut into red earth, with mesas, boulders, a water
  tower and a windmill, and nothing else for miles.
- **The Strand** — the ocean down one sideline, a hotel down the other, palms and
  umbrellas over the sand. The sand runs from the boards up the hotel side and round
  both ends, so the beach is in the shot down the field as well: the far end zone has
  palms, umbrellas and a lit resort block set back on the sand behind it, and the sea
  starts out past the grandstand with a strip of wet sand between it and the boards.
  It used to stop sixteen yards short of the boards on the hotel side and at the end
  of the resort, so the shot down the field ran out onto nothing.
- **The Championship** — the final. Both clubs are painted into both end zones in
  their own colours, with bunting down both stands and the confetti already falling.

The ground has its own screen, which opens on the home club's own park: a grid of
pictures, twelve grounds to a page, each one a still of the ground taken from the
lens the game is played through. The arrows move the cursor one picture at a time —
left and right along a row, up and down a row at a time — and the ground under the
cursor is named underneath, with its own description. Nothing moves on its own and
nothing is picked for you: the stadium you land on is the stadium you play in.

### Ratings and the clock

The picker shows what a club is worth before it is picked: **OVR** for the whole
side, **OFF** for the seven who take the snap and **DEF** for the seven who defend
it. Each is the average of the starters' player ratings — the best man at every
position, counted the way the formation counts him — so a club is rated on the men
who will actually be on the field and not on its bench. The three best players on
the roster are named under the number with their positions and their own ratings,
which is where the team number comes from.

The setup card carries the quarter length as well: 4, 6, 8, 10, 12 or 15 minute
quarters, remembered in `settings.cfg` and used for every quarter of this game,
including the clock that runs out at the end of regulation. Fifteen is still the
league's default, and overtime is unaffected. A game started from the command line
takes its clock from the same file.

### Simulation

`nopads_core::sim` runs one snap as a continuous 7 on 7 play at 60 Hz: dropback,
route running against man and zone coverage, pass rush and holding, quarterback
progression reads, throw power and accuracy, catch radius, wrap tackles, broken
tackles, fumbles, interceptions, defensive pass interference, sacks, safeties,
touchdowns and out-of-bounds. `nopads_core::throwoff` runs the throw-off the same
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

The renderer is a small software 3D pipeline (`nopads_render`): z-buffered,
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

Around it stands a stadium: a run-off apron, advertising boards, one grandstand with
a crowd in team colours down the far sideline, a roof over it, a scoreboard carrying
the home club's mark and four banks of lights, all placed under the height of the
broadcast lens so the building is in the picture without ever getting between the
camera and the ball. One grandstand and not two, the way a school field is built: the
near sideline is boards and open air, so the lens has a clear line to the ball, and
there is no stand behind either end zone, so the end-zone and overhead shots look
past the paint at the ground beyond it. Everything a ground brings with it, from the
treeline to the water tower, stands outside the boards or above the field. The field
is the players'.

The overlay is 2K5-flavoured: a score bug with possession highlight and timeout
pips, a down-and-distance card with play clock bar, the league badge, a lower-third
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
crates/core     nopads_core   — no dependencies
  math.rs       vectors, colours, deterministic PCG random
  field.rs      canonical field geometry (100 yards + two 10-yard end zones, 37.5 wide)
  rules.rs      penalty catalog and enforcement
  team.rs       attributes, appearance, uniforms, the sixteen-club league
  playbook.rs   22 offensive plays, 6 formations, 9 defensive calls in 3 fronts
  sim.rs        the 7 on 7 snap engine
  throwoff.rs   the throw-off engine
  stats.rs      box score, drives, per-player lines
  ai.rs         play calling and game management
  game.rs       quarters, clock, downs, conversions, overtime, mercy, play log

crates/render   nopads_render — depends on `image` (PNG) and `gif`
  raster.rs     framebuffer, z-buffer, textured/flat/alpha triangles, bitmap font
  font.rs       5x7 bitmap font
  scene.rs      camera, projector, near clipping, mesh primitives, lighting, fog
  fieldtex.rs   the painted turf texture
  players.rs    the 7 on 7 player meshes and poses
  broadcast.rs  camera work and frame assembly
  hud.rs        score bug, banners, ticker, stats panel
  capture.rs    PNG and GIF writers

crates/cli      the `7on7` binary
```

The `nopads_core` crate has no dependencies and no rendering code, so the rules and
simulation can be unit tested (125 tests) without touching the renderer.

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
  against the CPU, drawn either in a window or as ANSI blocks in a terminal; the
  renderer itself writes images and never needs a display.
- Teams and players are fictional, with names and colours invented so nothing
  real is reproduced.
- Rendering is CPU-bound: a 1280×720 still takes a fraction of a second, the full
  film package about half a minute.
