//! PNG and animated GIF output for the broadcast package.

use std::fs::File;
use std::path::Path;

use crate::raster::Frame;

pub fn write_png(path: impl AsRef<Path>, frame: &Frame) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let buf = frame.to_rgb8();
    image::save_buffer(path, &buf, frame.w as u32, frame.h as u32, image::ColorType::Rgb8)
        .map_err(|e| format!("png write failed: {e}"))
}

/// Write a GIF. Frames are quantised by the `gif` crate's built-in encoder.
pub fn write_gif(path: impl AsRef<Path>, frames: &[Frame], delay_ms: u16) -> Result<(), String> {
    if frames.is_empty() {
        return Err("no frames to encode".to_string());
    }
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let w = frames[0].w as u16;
    let h = frames[0].h as u16;
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = gif::Encoder::new(std::io::BufWriter::new(file), w, h, &[]).map_err(|e| e.to_string())?;
    encoder.set_repeat(gif::Repeat::Infinite).map_err(|e| e.to_string())?;
    for f in frames {
        let mut pix = f.to_rgb8();
        let mut gif_frame = gif::Frame::from_rgb_speed(w, h, &mut pix, 10);
        gif_frame.delay = (delay_ms / 10).max(2);
        encoder.write_frame(&gif_frame).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Encode the frames as an APNG-friendly PNG sequence folder.
pub fn write_sequence(dir: impl AsRef<Path>, frames: &[Frame], prefix: &str) -> Result<Vec<String>, String> {
    let dir = dir.as_ref();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        let p = dir.join(format!("{prefix}_{i:03}.png"));
        write_png(&p, f)?;
        paths.push(p.display().to_string());
    }
    Ok(paths)
}
