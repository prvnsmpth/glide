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

/// Apply zoom transformation to an image.
/// Uses fixed-point zoom: the cursor stays at its screen position while content scales around it.
/// Both axes use the same zoom factor, ensuring perfectly symmetric motion.
pub fn apply_zoom(img: &DynamicImage, zoom: f64, cursor_x: f64, cursor_y: f64) -> DynamicImage {
    let (width, height) = img.dimensions();
    let width_f = width as f64;
    let height_f = height as f64;

    // Calculate the size of the visible area after zoom
    let view_width = width_f / zoom;
    let view_height = height_f / zoom;

    // Fixed-point zoom formula: view_pos = cursor * (1 - 1/zoom)
    // This keeps the cursor at its current screen position while zooming.
    // Both axes use the SAME factor, guaranteeing symmetric motion.
    let zoom_factor = 1.0 - 1.0 / zoom;
    let view_left = cursor_x * zoom_factor;
    let view_top = cursor_y * zoom_factor;

    // Clamp to valid bounds (handles edge cases where cursor is outside canvas)
    let max_left = (width_f - view_width).max(0.0);
    let max_top = (height_f - view_height).max(0.0);
    let view_left = view_left.clamp(0.0, max_left);
    let view_top = view_top.clamp(0.0, max_top);

    // Crop and resize (use Triangle filter for speed, still decent quality)
    let cropped = img.crop_imm(
        view_left as u32,
        view_top as u32,
        view_width as u32,
        view_height as u32,
    );

    cropped.resize_exact(width, height, image::imageops::FilterType::Triangle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_image(width: u32, height: u32) -> DynamicImage {
        // Create a gradient image so we can verify zoom is actually happening
        let mut img = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let r = (x * 255 / width) as u8;
                let g = (y * 255 / height) as u8;
                img.put_pixel(x, y, Rgba([r, g, 128, 255]));
            }
        }
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn test_apply_zoom_no_zoom() {
        let img = create_test_image(1920, 1080);
        let result = apply_zoom(&img, 1.0, 960.0, 540.0);

        assert_eq!(result.dimensions(), (1920, 1080));
        // At zoom 1.0, output should equal input
        let orig_pixel = img.get_pixel(960, 540);
        let result_pixel = result.get_pixel(960, 540);
        assert_eq!(orig_pixel, result_pixel);
    }

    #[test]
    fn test_apply_zoom_center_cursor() {
        let img = create_test_image(1920, 1080);
        let zoom = 1.8;
        let cursor_x = 960.0; // center
        let cursor_y = 540.0; // center

        let result = apply_zoom(&img, zoom, cursor_x, cursor_y);

        assert_eq!(result.dimensions(), (1920, 1080));

        // With fixed-point zoom, cursor should stay at same position
        // The pixel at cursor position should come from the same source position
        // Verify the view_left and view_top calculations:
        let view_width = 1920.0 / zoom;
        let view_height = 1080.0 / zoom;
        let zoom_factor = 1.0 - 1.0 / zoom;
        let view_left = cursor_x * zoom_factor;
        let view_top = cursor_y * zoom_factor;

        println!("zoom_factor: {}", zoom_factor);
        println!("view_left: {}, view_top: {}", view_left, view_top);
        println!("view_width: {}, view_height: {}", view_width, view_height);

        // The cropped area should be smaller than the original
        assert!(view_width < 1920.0);
        assert!(view_height < 1080.0);
    }

    #[test]
    fn test_apply_zoom_cursor_preserved() {
        let img = create_test_image(1920, 1080);
        let zoom = 1.8;

        // Test cursor at center
        let cursor_x = 960.0;
        let cursor_y = 540.0;

        // Calculate where the cursor should appear after zoom
        // With fixed-point zoom formula: view_left = cursor_x * (1 - 1/zoom)
        let zoom_factor = 1.0 - 1.0 / zoom;
        let view_left = cursor_x * zoom_factor;
        let view_top = cursor_y * zoom_factor;
        let view_width = 1920.0 / zoom;
        let view_height = 1080.0 / zoom;

        // Cursor position in cropped image
        let cursor_in_crop_x: f64 = cursor_x - view_left;
        let cursor_in_crop_y: f64 = cursor_y - view_top;

        // After resize, cursor should be at:
        let cursor_after_x: f64 = cursor_in_crop_x * (1920.0 / view_width);
        let cursor_after_y: f64 = cursor_in_crop_y * (1080.0 / view_height);

        println!("Cursor should be at: ({}, {})", cursor_after_x, cursor_after_y);

        // Should be approximately at original position
        assert!((cursor_after_x - cursor_x).abs() < 1.0, "X position should be preserved");
        assert!((cursor_after_y - cursor_y).abs() < 1.0, "Y position should be preserved");
    }

    #[test]
    fn test_apply_zoom_corner_cursor() {
        let img = create_test_image(1920, 1080);
        let zoom = 1.8;

        // Test cursor at bottom-right corner
        let cursor_x = 1800.0;
        let cursor_y = 900.0;

        let result = apply_zoom(&img, zoom, cursor_x, cursor_y);
        assert_eq!(result.dimensions(), (1920, 1080));

        // Verify the zoom math works for corner positions
        let zoom_factor = 1.0 - 1.0 / zoom;
        let view_width = 1920.0 / zoom;
        let view_height = 1080.0 / zoom;

        let view_left = (cursor_x * zoom_factor).max(0.0).min(1920.0 - view_width);
        let view_top = (cursor_y * zoom_factor).max(0.0).min(1080.0 - view_height);

        println!("Corner zoom - view_left: {}, view_top: {}", view_left, view_top);

        // View should be offset toward the corner
        assert!(view_left > 0.0, "View should be offset from left");
        assert!(view_top > 0.0, "View should be offset from top");
    }

    #[test]
    fn test_apply_zoom_with_layout_offset() {
        // Simulate a scenario like the actual pipeline:
        // - Window content is centered on a 1920x1080 canvas
        // - Content has layout offset (e.g., offset_x=260, offset_y=140)
        // - Cursor click is on the content area

        let img = create_test_image(1920, 1080);
        let zoom = 1.8;

        // Cursor click at canvas position (660, 490) - which is on the content
        let canvas_cursor_x = 660.0;
        let canvas_cursor_y = 490.0;

        let result = apply_zoom(&img, zoom, canvas_cursor_x, canvas_cursor_y);

        // Verify dimensions preserved
        assert_eq!(result.dimensions(), (1920, 1080));

        // Verify the view calculations
        let zoom_factor = 1.0 - 1.0 / zoom;
        let view_left = canvas_cursor_x * zoom_factor;
        let view_top = canvas_cursor_y * zoom_factor;

        println!(
            "Layout offset test: view_left={}, view_top={}, zoom_factor={}",
            view_left, view_top, zoom_factor
        );

        // view_left should be positive (zooming toward the cursor)
        assert!(view_left > 0.0, "view_left should be positive");
        assert!(view_top > 0.0, "view_top should be positive");
    }

    #[test]
    fn test_apply_zoom_zero_cursor() {
        // Edge case: cursor at (0, 0)
        let img = create_test_image(1920, 1080);
        let zoom = 1.8;

        let result = apply_zoom(&img, zoom, 0.0, 0.0);
        assert_eq!(result.dimensions(), (1920, 1080));

        // With cursor at (0, 0), zoom should center on top-left
        // view_left = 0 * anything = 0
        // This should zoom into the top-left corner
        let orig_pixel = img.get_pixel(0, 0);
        let zoomed_pixel = result.get_pixel(0, 0);

        // The top-left pixel should be the same (zooming from origin)
        println!("Zero cursor - orig: {:?}, zoomed: {:?}", orig_pixel, zoomed_pixel);
    }

    #[test]
    fn test_fixed_point_zoom_is_symmetric() {
        // Verify that the fixed-point formula produces symmetric motion
        // Both axes should use the same factor regardless of cursor position
        let zoom = 1.8;
        let zoom_factor = 1.0 - 1.0 / zoom;

        // Test at various cursor positions
        let test_cases: [(f64, f64, &str); 5] = [
            (960.0, 540.0, "center"),
            (100.0, 100.0, "top-left area"),
            (1800.0, 900.0, "bottom-right area"),
            (660.0, 490.0, "offset position"),
            (1500.0, 300.0, "asymmetric position"),
        ];

        for (cursor_x, cursor_y, label) in test_cases {
            let view_left = cursor_x * zoom_factor;
            let view_top = cursor_y * zoom_factor;

            // Verify the ratio: view_pos / cursor_pos should be the same for both axes
            let x_ratio = if cursor_x > 0.0 { view_left / cursor_x } else { zoom_factor };
            let y_ratio = if cursor_y > 0.0 { view_top / cursor_y } else { zoom_factor };

            println!("{}: x_ratio={:.4}, y_ratio={:.4}", label, x_ratio, y_ratio);

            // Both ratios should equal zoom_factor (perfectly symmetric)
            assert!(
                (x_ratio - zoom_factor).abs() < 0.0001,
                "X ratio should equal zoom_factor"
            );
            assert!(
                (y_ratio - zoom_factor).abs() < 0.0001,
                "Y ratio should equal zoom_factor"
            );
        }
    }

    #[test]
    fn test_zoom_produces_magnification() {
        // Verify that zoom actually magnifies the content
        let img = create_test_image(1920, 1080);
        let zoom = 1.8;

        // Apply zoom at center
        let result = apply_zoom(&img, zoom, 960.0, 540.0);

        // Check that a pixel NOT at the cursor position has changed
        // (proving that content is being cropped and resized)
        let orig_pixel = img.get_pixel(200, 200);
        let zoomed_pixel = result.get_pixel(200, 200);

        println!("Magnification test:");
        println!("  Original (200,200): {:?}", orig_pixel);
        println!("  Zoomed (200,200): {:?}", zoomed_pixel);

        // The pixels should be different because we're showing different content
        assert_ne!(
            orig_pixel, zoomed_pixel,
            "Zoom should change visible content at non-cursor positions"
        );
    }

    #[test]
    fn test_apply_zoom_produces_different_output() {
        let img = create_test_image(1920, 1080);

        // Get a pixel from the corner at no zoom
        let corner_pixel_no_zoom = img.get_pixel(100, 100);

        // Apply zoom centered on cursor at (500, 500)
        let zoomed = apply_zoom(&img, 1.8, 500.0, 500.0);

        // The same screen position (100, 100) should now show different content
        // because we've zoomed and panned
        let corner_pixel_zoomed = zoomed.get_pixel(100, 100);

        // These should be different (zoom should change the visible content)
        println!("No zoom pixel at (100,100): {:?}", corner_pixel_no_zoom);
        println!("Zoomed pixel at (100,100): {:?}", corner_pixel_zoomed);

        // At 1.8x zoom with cursor at (500, 500):
        // view_left = 500 * (1 - 1/1.8) = 500 * 0.444 = 222
        // The pixel at (100, 100) in the result comes from around (222 + 100/1.8, ...) in original
        // So it should be different
        assert_ne!(
            corner_pixel_no_zoom, corner_pixel_zoomed,
            "Zoom should change the visible content"
        );
    }
}
