//! Project Zero - Official Boot Logo Renderer
//!
//! Authoritative Asset: `assets/zeroos_boot_logo.png` (Converted to `assets/zeroos_boot_logo_256.raw`)
//! Rendered at startup when linear framebuffer graphics mode is available from Multiboot.

use crate::kprintln;
use crate::mm::inventory::MultibootInfo;

pub const LOGO_WIDTH: usize = 160;
pub const LOGO_HEIGHT: usize = 160;
pub const LOGO_PIXELS: &[u8; LOGO_WIDTH * LOGO_HEIGHT * 4] = include_bytes!("../../assets/zeroos_boot_logo_160.raw");

/// Clean background color for boot logo splash presentation (#0A0E17 dark blue-gray).
pub const BG_COLOR: (u8, u8, u8) = (0x0A, 0x0E, 0x17);

/// Attempts to render the official ZeroOS boot logo on the linear framebuffer.
pub fn render_boot_logo(multiboot_info: &MultibootInfo) {
    // Check if Multiboot flag bit 12 (0x1000) is set for framebuffer info
    if (multiboot_info.flags & (1 << 12)) == 0 {
        kprintln!("[BOOT_LOGO] Headless/Serial Mode Detected (No Framebuffer) - Boot Logo Skipped.");
        return;
    }

    let fb_phys = multiboot_info.framebuffer_addr;
    let fb_width = multiboot_info.framebuffer_width as usize;
    let fb_height = multiboot_info.framebuffer_height as usize;
    let fb_pitch = multiboot_info.framebuffer_pitch as usize;
    let fb_bpp = multiboot_info.framebuffer_bpp;

    if fb_phys == 0 || fb_width == 0 || fb_height == 0 || fb_pitch == 0 || fb_bpp != 32 {
        kprintln!(
            "[BOOT_LOGO] Unsupported or uninitialized framebuffer: {}x{} @ {:#X} (bpp={})",
            fb_width, fb_height, fb_phys, fb_bpp
        );
        return;
    }

    kprintln!(
        "[BOOT_LOGO] Linear Framebuffer Detected: {}x{} @ {:#X} (pitch={}, bpp={})",
        fb_width, fb_height, fb_phys, fb_pitch, fb_bpp
    );

    let start_x = if fb_width > LOGO_WIDTH { (fb_width - LOGO_WIDTH) / 2 } else { 0 };
    let start_y = if fb_height > LOGO_HEIGHT { (fb_height - LOGO_HEIGHT) / 2 } else { 0 };

    // Higher-half direct map or identity mapped physical pointer
    let fb_ptr = if fb_phys >= 0xFFFF_8000_0000_0000 {
        fb_phys as *mut u8
    } else {
        (0xFFFF_8000_0000_0000 + fb_phys) as *mut u8
    };

    // 1. Fill entire screen background with clean dark blue-gray (#0A0E17)
    let bg_pixel_val = ((BG_COLOR.0 as u32) << 16) | ((BG_COLOR.1 as u32) << 8) | (BG_COLOR.2 as u32);
    for y in 0..fb_height {
        let row_offset = y * fb_pitch;
        for x in 0..fb_width {
            let offset = row_offset + x * 4;
            unsafe {
                core::ptr::write_volatile(fb_ptr.add(offset) as *mut u32, bg_pixel_val);
            }
        }
    }

    // 2. Render centered boot logo with alpha blending
    for ly in 0..LOGO_HEIGHT {
        let sy = start_y + ly;
        if sy >= fb_height { break; }

        for lx in 0..LOGO_WIDTH {
            let sx = start_x + lx;
            if sx >= fb_width { break; }

            let asset_idx = (ly * LOGO_WIDTH + lx) * 4;
            let r = LOGO_PIXELS[asset_idx] as u32;
            let g = LOGO_PIXELS[asset_idx + 1] as u32;
            let b = LOGO_PIXELS[asset_idx + 2] as u32;
            let a = LOGO_PIXELS[asset_idx + 3] as u32;

            let (blended_r, blended_g, blended_b) = if a == 255 {
                (r, g, b)
            } else if a == 0 {
                (BG_COLOR.0 as u32, BG_COLOR.1 as u32, BG_COLOR.2 as u32)
            } else {
                let inv_a = 255 - a;
                let br = (r * a + (BG_COLOR.0 as u32) * inv_a) / 255;
                let bg = (g * a + (BG_COLOR.1 as u32) * inv_a) / 255;
                let bb = (b * a + (BG_COLOR.2 as u32) * inv_a) / 255;
                (br, bg, bb)
            };

            let pixel_val = (blended_r << 16) | (blended_g << 8) | blended_b;
            let offset = sy * fb_pitch + sx * 4;
            unsafe {
                core::ptr::write_volatile(fb_ptr.add(offset) as *mut u32, pixel_val);
            }
        }
    }

    kprintln!(
        "[BOOT_LOGO] Official ZeroOS Boot Logo Rendered Centered at ({}, {}) [{}x{}]",
        start_x, start_y, LOGO_WIDTH, LOGO_HEIGHT
    );
}
