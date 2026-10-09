# ZEROOS BOOT LOGO INTEGRATION REV1

## Executive Summary

| Attribute | Specification |
|---|---|
| **Authoritative Asset Path** | [`assets/zeroos_boot_logo.png`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/assets/zeroos_boot_logo.png) |
| **Source Image Resolution** | `1254 x 1254` pixels (1:1 square aspect ratio, RGBA 32-bit) |
| **Pre-Decoded Boot Asset** | [`assets/zeroos_boot_logo_160.raw`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/assets/zeroos_boot_logo_160.raw) |
| **Boot Target Dimensions** | `160 x 160` pixels (1:1 preserved aspect ratio) |
| **Memory Footprint** | `102,400 bytes` (100 KiB static read-only `.rodata` array, 0 runtime allocations) |
| **Framebuffer Interface** | Multiboot 1 Linear Framebuffer (`multiboot_info.flags & (1 << 12)`) |
| **Background Color** | `#0A0E17` (Clean dark blue-gray background) |
| **Renderer Integration** | `kernel/src/boot_logo.rs` invoked early in `kernel_main` ([`kernel/src/lib.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/lib.rs)) |
| **Headless / Serial Fallback** | Non-blocking, fail-safe serial logging over COM1 (`0x3F8`) |

---

## 1. Asset Provenance & Conversion Procedure

The official ZeroOS boot logo asset provided by the user (`ChatGPT Image Sep 20, 2026, 06_11_50 PM.png`) has been preserved unchanged in the repository at [`assets/zeroos_boot_logo.png`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/assets/zeroos_boot_logo.png).

### Deterministic Conversion Seam

To avoid runtime image decoding overhead and large heap allocations in the freestanding `no_std` Rust kernel, the original PNG asset is converted offline into a raw pixel array:

1. **Aspect Ratio & Resampling**: Scaled from 1254x1254 to 160x160 pixels using high-quality Lanczos anti-aliasing resampling, maintaining the exact 1:1 square aspect ratio.
2. **Color Format**: Extracted 32-bit RGBA raw byte sequence (`[R, G, B, A]`).
3. **Static Embedding**: Embedded into `kernel/src/boot_logo.rs` via Rust `include_bytes!("../../assets/zeroos_boot_logo_160.raw")`.

---

## 2. Bootloader & Framebuffer Architecture

### Multiboot 1 Video Handshake

1. **Header Flag Request**: Multiboot header in [`boot/boot.asm`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/boot/boot.asm) requests linear graphics mode (`MB_FLAGS = 0x00000007`, requesting 1024x768x32 linear VBE framebuffer).
2. **Boot Info Inspection**: Early in `kernel_main`, the kernel inspects `multiboot_info.flags & (1 << 12)`.
3. **Linear Framebuffer Mapping**: When linear framebuffer is present, physical VRAM base (`framebuffer_addr`), resolution (`width` x `height`), pitch, and color depth (`bpp`) are parsed from `multiboot_info`.

---

## 3. Centered Presentation & Alpha Blending

### Presentation Rules

- **Centering Calculation**:
  $$\text{start\_x} = \frac{\text{fb\_width} - \text{LOGO\_WIDTH}}{2}, \quad \text{start\_y} = \frac{\text{fb\_height} - \text{LOGO\_HEIGHT}}{2}$$
- **Background Fill**: The entire linear framebuffer screen is pre-filled with clean `#0A0E17` dark blue-gray.
- **Alpha Blending Formula**:
  For pixels with alpha $0 < A < 255$:
  $$C_{\text{blended}} = \frac{C_{\text{logo}} \times A + C_{\text{bg}} \times (255 - A)}{255}$$
  For $A = 255$, the logo pixel $C_{\text{logo}}$ is rendered directly. For $A = 0$, the background color $C_{\text{bg}}$ is preserved.

---

## 4. Headless & Fail-Safe Telemetry

In headless environments (`qemu-system-x86_64 -display none` or serial execution), framebuffer flag bit 12 is zero. The renderer logs diagnostic serial telemetry over COM1 (`0x3F8`):

```text
[BOOT_LOGO] Headless/Serial Mode Detected (No Framebuffer) - Boot Logo Skipped.
```

Startup proceeds immediately to GDT/IDT exception verification and kernel initialization without delay or error.
