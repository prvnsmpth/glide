use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use std::sync::Arc;

// Output canvas dimensions
pub const OUTPUT_WIDTH: u32 = 1920;
pub const OUTPUT_HEIGHT: u32 = 1080;

// Corner radius for rounded corners
pub const CORNER_RADIUS: u32 = 12;

// Shadow settings
pub const SHADOW_OFFSET: i64 = 8;
pub const SHADOW_BLUR_RADIUS: u32 = 20;
pub const SHADOW_COLOR: Rgba<u8> = Rgba([0, 0, 0, 80]);

/// Background type for video processing
#[derive(Clone)]
pub enum Background {
    Color(Rgba<u8>),
    Image(Arc<RgbaImage>),
}

impl Background {
    /// Parse background from string: hex color (e.g., "#1a1a2e") or image path
    pub fn parse(input: Option<&str>) -> Result<Self> {
        match input {
            None => {
                // Default dark gray
                Ok(Background::Color(Rgba([26, 26, 46, 255])))
            }
            Some(s) => {
                // Check if it's a hex color
                let hex = s.trim_start_matches('#');
                if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                    Ok(Background::Color(Rgba([r, g, b, 255])))
                } else {
                    // Try to load as image
                    let img = image::open(s)
                        .with_context(|| format!("Failed to load background image: {}", s))?;
                    // Resize to output dimensions
                    let resized = img.resize_to_fill(
                        OUTPUT_WIDTH,
                        OUTPUT_HEIGHT,
                        image::imageops::FilterType::Lanczos3,
                    );
                    Ok(Background::Image(Arc::new(resized.to_rgba8())))
                }
            }
        }
    }

    /// Create a canvas with this background
    pub fn create_canvas(&self) -> RgbaImage {
        match self {
            Background::Color(color) => {
                RgbaImage::from_pixel(OUTPUT_WIDTH, OUTPUT_HEIGHT, *color)
            }
            Background::Image(img) => img.as_ref().clone(),
        }
    }
}

/// Layout info for placing content on canvas
pub struct ContentLayout {
    pub scale: f64,
    pub offset_x: u32,
    pub offset_y: u32,
    pub scaled_width: u32,
    pub scaled_height: u32,
}

impl ContentLayout {
    pub fn calculate(content_width: u32, content_height: u32) -> Self {
        // Calculate scale to fit content with padding (leave ~100px on each side)
        let max_content_width = OUTPUT_WIDTH - 200;
        let max_content_height = OUTPUT_HEIGHT - 200;

        let scale_x = max_content_width as f64 / content_width as f64;
        let scale_y = max_content_height as f64 / content_height as f64;
        let scale = scale_x.min(scale_y).min(1.0); // Don't upscale

        let scaled_width = (content_width as f64 * scale) as u32;
        let scaled_height = (content_height as f64 * scale) as u32;

        // Center on canvas
        let offset_x = (OUTPUT_WIDTH - scaled_width) / 2;
        let offset_y = (OUTPUT_HEIGHT - scaled_height) / 2;

        Self {
            scale,
            offset_x,
            offset_y,
            scaled_width,
            scaled_height,
        }
    }
}

/// Apply rounded corners to an RGBA image
pub fn apply_rounded_corners(img: &mut RgbaImage, radius: u32) {
    let width = img.width();
    let height = img.height();
    let radius = radius.min(width / 2).min(height / 2);

    for y in 0..height {
        for x in 0..width {
            let alpha = corner_alpha(x, y, width, height, radius);
            if alpha < 255 {
                let pixel = img.get_pixel_mut(x, y);
                // Multiply existing alpha by corner alpha
                let new_alpha = (pixel[3] as u32 * alpha as u32 / 255) as u8;
                pixel[3] = new_alpha;
            }
        }
    }
}

/// Calculate alpha value for a pixel based on corner rounding
fn corner_alpha(x: u32, y: u32, width: u32, height: u32, radius: u32) -> u8 {
    let radius_f = radius as f64;

    // Check each corner
    let corners = [
        (radius, radius),                          // top-left
        (width - radius - 1, radius),              // top-right
        (radius, height - radius - 1),             // bottom-left
        (width - radius - 1, height - radius - 1), // bottom-right
    ];

    for (cx, cy) in corners {
        // Check if pixel is in the corner region
        let in_corner_x =
            (x <= radius && cx == radius) || (x >= width - radius - 1 && cx == width - radius - 1);
        let in_corner_y = (y <= radius && cy == radius)
            || (y >= height - radius - 1 && cy == height - radius - 1);

        if in_corner_x && in_corner_y {
            let dx = x as f64 - cx as f64;
            let dy = y as f64 - cy as f64;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist > radius_f {
                return 0; // Outside corner
            } else if dist > radius_f - 1.5 {
                // Anti-aliasing at edge
                let alpha = (radius_f - dist + 0.5).clamp(0.0, 1.0);
                return (alpha * 255.0) as u8;
            }
        }
    }

    255 // Fully opaque
}

/// Draw a shadow on the canvas
pub fn draw_shadow(canvas: &mut RgbaImage, x: i64, y: i64, width: u32, height: u32, radius: u32) {
    let shadow_x = x + SHADOW_OFFSET;
    let shadow_y = y + SHADOW_OFFSET;

    // Draw multiple layers for blur effect
    for blur_layer in 0..SHADOW_BLUR_RADIUS {
        let expand = blur_layer as i64;
        let layer_alpha = SHADOW_COLOR[3] as u32 * (SHADOW_BLUR_RADIUS - blur_layer) as u32
            / (SHADOW_BLUR_RADIUS * SHADOW_BLUR_RADIUS) as u32;

        if layer_alpha == 0 {
            continue;
        }

        let sx = (shadow_x - expand).max(0) as u32;
        let sy = (shadow_y - expand).max(0) as u32;
        let sw = (width as i64 + expand * 2).min(canvas.width() as i64 - sx as i64) as u32;
        let sh = (height as i64 + expand * 2).min(canvas.height() as i64 - sy as i64) as u32;

        for py in sy..sy + sh {
            for px in sx..sx + sw {
                if px >= canvas.width() || py >= canvas.height() {
                    continue;
                }

                // Check if inside rounded rectangle
                let local_x = px as i64 - shadow_x + expand;
                let local_y = py as i64 - shadow_y + expand;
                let layer_width = width + 2 * expand as u32;
                let layer_height = height + 2 * expand as u32;

                if is_inside_rounded_rect(local_x, local_y, layer_width, layer_height, radius + expand as u32)
                {
                    let pixel = canvas.get_pixel_mut(px, py);
                    // Blend shadow with existing pixel
                    let alpha = layer_alpha as u8;
                    pixel[0] = blend_channel(pixel[0], SHADOW_COLOR[0], alpha);
                    pixel[1] = blend_channel(pixel[1], SHADOW_COLOR[1], alpha);
                    pixel[2] = blend_channel(pixel[2], SHADOW_COLOR[2], alpha);
                }
            }
        }
    }
}

fn is_inside_rounded_rect(x: i64, y: i64, width: u32, height: u32, radius: u32) -> bool {
    if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
        return false;
    }

    let x = x as u32;
    let y = y as u32;
    let radius_f = radius as f64;

    // Check corners
    let corners = [
        (radius, radius),
        (width - radius - 1, radius),
        (radius, height - radius - 1),
        (width - radius - 1, height - radius - 1),
    ];

    for (cx, cy) in corners {
        let in_corner_x =
            (x <= radius && cx == radius) || (x >= width - radius - 1 && cx == width - radius - 1);
        let in_corner_y = (y <= radius && cy == radius)
            || (y >= height - radius - 1 && cy == height - radius - 1);

        if in_corner_x && in_corner_y {
            let dx = x as f64 - cx as f64;
            let dy = y as f64 - cy as f64;
            if dx * dx + dy * dy > radius_f * radius_f {
                return false;
            }
        }
    }

    true
}

/// Blend a single color channel with alpha
pub fn blend_channel(bg: u8, fg: u8, alpha: u8) -> u8 {
    let bg = bg as u32;
    let fg = fg as u32;
    let alpha = alpha as u32;
    ((bg * (255 - alpha) + fg * alpha) / 255) as u8
}

/// Apply zoom transformation to an image
pub fn apply_zoom(img: &DynamicImage, zoom: f64, cursor_x: f64, cursor_y: f64) -> DynamicImage {
    let (width, height) = img.dimensions();
    let width_f = width as f64;
    let height_f = height as f64;

    // Calculate the size of the visible area after zoom
    let view_width = width_f / zoom;
    let view_height = height_f / zoom;

    // Center the view on the cursor position, but clamp to image bounds
    let view_left = (cursor_x - view_width / 2.0)
        .max(0.0)
        .min(width_f - view_width);
    let view_top = (cursor_y - view_height / 2.0)
        .max(0.0)
        .min(height_f - view_height);

    // Crop and resize (use Triangle filter for speed, still decent quality)
    let cropped = img.crop_imm(
        view_left as u32,
        view_top as u32,
        view_width as u32,
        view_height as u32,
    );

    cropped.resize_exact(width, height, image::imageops::FilterType::Triangle)
}
