# Bevy shell spike

A feasibility spike, not part of the game. It answers one question: if we switch
the 7 on 7 client from `minifb` to Bevy, what does that actually touch?

## What this is

`crates/cli/src/view.rs` already hides the platform behind a `View` enum with a
window arm and a terminal arm. The rest of the client — 71 call sites across
`arcade.rs`, `menu.rs` and `main.rs` — only ever calls `show`, `page`, `poll`,
`reset`, `apply` and `describe` on it. That enum is the only place an engine can be
bolted on without touching the game, so this crate is a second window arm:

- the game's own software renderer produces the same 640×360 canvas,
- Bevy owns the window, the swap chain, the event loop and the scaling,
- the canvas is one `Image` on one `Sprite` under one `Camera2d`, with nearest
  sampling so the pixels stay hard,
- the keyboard comes back through `ButtonInput<KeyCode>` mapped onto the same
  bindings and the same `Input { stick, held, pressed }` the game already polls.

It is deliberately *not* a port of the renderer. See "The other kind of port".

## What it proves

- Bevy 0.20.1 builds in this environment with `default-features = false` and the
  feature list in `Cargo.toml` — **2 m 50 s, 150 crates compiled**, no system headers
  beyond a compiler (x11 loads `libX11` at runtime; Wayland, audio and gamepad
  features are off because this VM has no Wayland headers, no ALSA and no libudev).
- The seam is one file. `frame_to_rgba`, the key map and the press/hold edge logic
  are pure functions with tests, so the platform-independent half of the shell is
  verified here even though the window is not.
- The canvas conversion is exact: the game's `Rgb` is already 8-bit, so the
  texture is a straight copy and nothing about the picture changes.

## What it does not prove

**It cannot run here.** This VM has no display and no Vulkan ICD
(`/usr/share/vulkan/icd.d` is empty, and `mesa-vulkan-drivers` is not installable
without root), so `wgpu` finds no adapter and Bevy panics before it opens a
window. Nothing below has been watched on a screen:

- whether the sprite letterboxes exactly as `WindowView::blit` does,
- whether the frame pacing and input latency feel the same,
- whether the window survives a resize / fullscreen toggle.

## The cost, measured

| | minifb today | Bevy shell |
|---|---|---|
| crates | 125 locked | ~150 compiled (412 locked) |
| clean debug build | seconds | 2 m 50 s (measured) |
| new dependencies for the game | none | `bevy` + transitive |
| GPU required | no | yes (Vulkan/Metal/DX12) |
| terminal fallback | works | unaffected, `View::Term` still there |
| code to write | — | one `View` arm, ~300 lines |

## The other kind of port

"Switch to Bevy" can also mean letting Bevy draw the game instead of the software
renderer. That is a different project, and the numbers say so:

| part | lines | what Bevy replaces it with |
|---|---|---|
| `venues.rs`, `players.rs`, `broadcast.rs`, `scene.rs`, `raster.rs`, `fieldtex.rs`, `head.rs` | 9,065 | meshes, materials, cameras, lights |
| `hud.rs`, `playsheet.rs`, `defsheet.rs`, `passing.rs`, `font.rs` | 1,811 | `bevy_ui`, `bevy_text` |
| `arcade.rs`, `menu.rs` control flow | 4,723 | states, schedules, systems |

The renderer is not a thin layer over a triangle pusher: it is hand-built
procedural geometry for stadiums, stands, crowds, uniforms and human limbs, and
the broadcast package — camera cuts, replays, the film room — is written against
it. Porting it to Bevy materials is a graphics rewrite that would change how the
game looks at the same time as how it is built, which is the opposite of what the
last few releases were for.

The control flow is the second wall. The client is a blocking script: 15 `loop {}`
blocks in `arcade.rs`, 11 in `menu.rs`, each one polling the pad and painting
inside the loop. Bevy is a scheduler over data. Moving that script onto states and
systems is not a mechanical edit; it is a rewrite of the client's structure.

`pad.rs` and `settings.rs` live inside the binary crate, so a real port would first
move them to a shared crate — that part is easy and worth doing whatever happens.

## How to run it, on a machine that has a GPU

```sh
cd spike/bevy-view
cargo run --bin shell
```

## Verdict

Bevy makes a clean window/input/presentation shell and the seam is genuinely small.
It does not make a better renderer for this game, and it would cost the one
property the README opens with: a game that builds and runs with no GPU at all.
