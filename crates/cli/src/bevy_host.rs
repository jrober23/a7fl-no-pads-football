//! A Bevy window around the game's own canvas.
//!
//! The game keeps drawing 640x360 frames with its software renderer; Bevy owns the
//! window, the swap chain and the event loop, and shows each frame as one texture
//! on one sprite. That is the whole port: nothing here knows about football, and
//! the renderer does not know a window exists.
//!
//! This thread is the main thread, because a window system insists on it, so the
//! game runs on a worker and the two talk over `nopads_platform::link`. See
//! `view_bevy.rs` for the game's end.

use std::time::Duration;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{MonitorSelection, PrimaryWindow, WindowMode};

use nopads_platform::link::Host;
use nopads_platform::pad::{Buttons, Input};

use crate::view::{CANVAS_H, CANVAS_W};

/// How long a frame pump waits for the game to post its next frame.
const PICKUP: Duration = Duration::from_millis(4);

#[derive(Resource)]
struct Seam {
    host: Host,
    image: Option<Handle<Image>>,
}

/// Open the window and present frames until the game finishes or the player
/// closes it.
pub fn run(host: Host, width: usize, height: usize) {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "7 on 7 No Pads Football".into(),
                        resolution: (width as u32, height as u32).into(),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                })
                // Hard pixels, not smoothed: the canvas is 640x360 and gets blown
                // up by a whole-number scale wherever the window allows it.
                .set(ImagePlugin::default_nearest()),
        )
        .insert_resource(Seam {
            host,
            image: None,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (pump, relay_input, relay_resize, close_when_done))
        .run();
}

/// One texture, one sprite, one camera.
fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut seam: ResMut<Seam>) {
    let canvas = Image::new(
        Extent3d {
            width: CANVAS_W as u32,
            height: CANVAS_H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0u8; CANVAS_W * CANVAS_H * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let handle = images.add(canvas);
    seam.image = Some(handle.clone());
    commands.spawn((Sprite::from_image(handle), Transform::default()));
    commands.spawn((
        Camera2d,
        // Fit the canvas in the window and letterbox the rest, which is what the
        // minifb blit does by hand. AutoMax caps the visible area at the canvas
        // size, so a wider window shows more black rather than more game.
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::AutoMax {
                max_width: CANVAS_W as f32,
                max_height: CANVAS_H as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// Take the game's newest frame and put it in the texture.
fn pump(mut images: ResMut<Assets<Image>>, seam: Res<Seam>) {
    let Some(shot) = seam.host.take(PICKUP) else {
        return;
    };
    let Some(handle) = seam.image.as_ref() else {
        return;
    };
    let Some(mut image) = images.get_mut(handle) else {
        return;
    };
    let Some(data) = image.data.as_mut() else {
        return;
    };
    if data.len() != shot.pixels.len() * 4 {
        return;
    }
    // A straight copy: the canvas is already 8-bit RGB and the texture is RGBA.
    for (dst, src) in data.chunks_exact_mut(4).zip(shot.pixels.iter()) {
        dst.copy_from_slice(&[src.r, src.g, src.b, 255]);
    }
}

/// Hand the pad to the game every frame.
///
/// Held buttons only: the game works out its own press edges when it polls, so a
/// button pressed and released between two game polls is still seen by neither
/// side's frame counter.
fn relay_input(keys: Res<ButtonInput<KeyCode>>, seam: Res<Seam>) {
    seam.host.post_input(Input {
        stick: stick(&keys),
        held: held(&keys),
        pressed: Buttons::default(),
    });
}

/// The same keyboard bindings the minifb window uses.
fn held(keys: &ButtonInput<KeyCode>) -> Buttons {
    let any = |list: &[KeyCode]| keys.any_pressed(list.iter().copied());
    Buttons {
        a: any(&[KeyCode::Space, KeyCode::Enter]),
        b: any(&[KeyCode::KeyX]),
        x: any(&[KeyCode::KeyZ]),
        y: any(&[KeyCode::KeyC]),
        throw: any(&[KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        select: any(&[KeyCode::Tab, KeyCode::Backspace]),
        preview: any(&[KeyCode::KeyR]),
        dummy: any(&[KeyCode::KeyQ]),
        shoulder: any(&[KeyCode::KeyE]),
        truck: any(&[KeyCode::KeyT]),
        stop: any(&[KeyCode::KeyG]),
        juke_l: any(&[KeyCode::KeyF]),
        juke_r: any(&[KeyCode::KeyH]),
        start: any(&[KeyCode::Escape]),
    }
}

/// Arrows and WASD, up the screen positive, as the pad reports it.
fn stick(keys: &ButtonInput<KeyCode>) -> nopads_core::math::Vec2 {
    let axis = |plus: &[KeyCode], minus: &[KeyCode]| {
        let p = plus.iter().any(|k| keys.pressed(*k)) as i32;
        let m = minus.iter().any(|k| keys.pressed(*k)) as i32;
        (p - m) as f32
    };
    let x = axis(
        &[KeyCode::ArrowRight, KeyCode::KeyD],
        &[KeyCode::ArrowLeft, KeyCode::KeyA],
    );
    let y = axis(
        &[KeyCode::ArrowUp, KeyCode::KeyW],
        &[KeyCode::ArrowDown, KeyCode::KeyS],
    );
    nopads_core::math::Vec2::new(x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0))
}

/// Apply a size the settings screen asked for.
fn relay_resize(mut windows: Query<&mut Window, With<PrimaryWindow>>, seam: Res<Seam>) {
    let Some(size) = seam.host.take_resize() else {
        return;
    };
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.resolution.set(size.width as f32, size.height as f32);
    window.mode = if size.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
}

/// The game is over, so the window should go with it.
fn close_when_done(seam: Res<Seam>, mut exit: MessageWriter<AppExit>) {
    if seam.host.closed() {
        exit.write(AppExit::Success);
    }
}
