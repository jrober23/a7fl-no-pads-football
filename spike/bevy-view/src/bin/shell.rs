//! The spike itself: open a Bevy window and put the game's own canvas in it.
//!
//! One still frame, rendered by the game's software renderer from a simulated
//! game, uploaded as a single Bevy texture and shown through a 2D camera that
//! letterboxes it the way the minifb blit does. The keyboard is read through Bevy
//! and printed as the game's own `Input` shape, which is the part a real port has
//! to get right before anything else is worth moving.
//!
//! It cannot run in this VM: there is no Vulkan driver and no display. It builds,
//! and its pure parts are tested. See README.md.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use bevy_view_spike::{frame_to_rgba, poll, Buttons, Input};
use nopads_core::game::{GameState, PlayRecord};
use nopads_core::team::league_teams;
use nopads_render::broadcast::{Broadcast, CameraMode};
use nopads_render::hud;
use nopads_render::raster::Frame;

/// The canvas, rendered once, waiting to become a texture.
#[derive(Resource)]
struct Shot {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
}

/// The keyboard state carried between polls, so presses can be told from holds.
#[derive(Resource, Default)]
struct Pad {
    prev: Buttons,
    last: Option<Input>,
}

fn main() {
    let frame = broadcast_still();
    let (w, h) = (frame.w as u32, frame.h as u32);
    let rgba = frame_to_rgba(&frame);

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "7 on 7 No Pads Football — Bevy spike".into(),
                        resolution: (1280u32, 720u32).into(),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                })
                // The canvas is 640x360 and must be blown up with hard pixels, not
                // smoothed: same rule the minifb blit follows.
                .set(ImagePlugin::default_nearest()),
        )
        .insert_resource(Shot { rgba, w, h })
        .init_resource::<Pad>()
        .add_systems(Startup, present)
        .add_systems(Update, read_pad)
        .run();
}

/// A real frame from a real simulated game, through the real renderer.
///
/// The point is to prove the seam carries an actual picture and not a test
/// pattern: this is `Broadcast`, the same code `7on7 film` uses.
fn broadcast_still() -> Frame {
    let teams = league_teams();
    let mut game = GameState::new(teams[0].clone(), teams[1].clone(), 7);
    game.simulate_game();
    let pair = [game.teams[0].clone(), game.teams[1].clone()];
    let mut broadcast = Broadcast::new(bevy_view_spike::CANVAS_W, bevy_view_spike::CANVAS_H, pair);

    let scrimmage = game
        .log
        .iter()
        .find(|r| matches!(r, PlayRecord::Scrimmage(_)))
        .cloned();
    match scrimmage {
        Some(rec) => broadcast.frame_at(&game, &rec, 1.2, CameraMode::Broadcast),
        None => {
            let mut frame = Frame::new(bevy_view_spike::CANVAS_W, bevy_view_spike::CANVAS_H);
            hud::draw_stats_panel(&mut frame, &game, "FINAL");
            frame
        }
    }
}

/// Put the canvas on screen: one texture, one sprite, one camera.
fn present(mut commands: Commands, mut images: ResMut<Assets<Image>>, shot: Res<Shot>) {
    let image = Image::new(
        Extent3d {
            width: shot.w,
            height: shot.h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        shot.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let handle = images.add(image);
    commands.spawn((
        Sprite::from_image(handle),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    commands.spawn((
        Camera2d,
        // Fit the canvas inside the window and letterbox the rest, which is what
        // the minifb blit does by hand today. Projection::default_2d() is the
        // "fill the viewport" default; AutoMax caps the visible area at the canvas
        // size, so a wider window shows more black rather than more picture.
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::AutoMax {
                max_width: shot.w as f32,
                max_height: shot.h as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// Read the keyboard through Bevy and log the buttons that went down, in the
/// game's own terms. A real port hands this straight to `arcade.rs`.
fn read_pad(keys: Res<ButtonInput<KeyCode>>, mut pad: ResMut<Pad>) {
    if keys.get_just_pressed().next().is_none() && keys.get_just_released().next().is_none() {
        return;
    }
    let input = poll(&keys, pad.prev);
    pad.prev = input.held;
    let pressed = input.pressed;
    let any = pressed.a
        || pressed.b
        || pressed.x
        || pressed.y
        || pressed.throw
        || pressed.select
        || pressed.preview
        || pressed.dummy
        || pressed.shoulder
        || pressed.start;
    if any {
        info!(
            "a={} b={} x={} y={} throw={} select={} stick={:?}",
            pressed.a,
            pressed.b,
            pressed.x,
            pressed.y,
            pressed.throw,
            pressed.select,
            (input.stick.x, input.stick.y)
        );
    }
    pad.last = Some(input);
}
