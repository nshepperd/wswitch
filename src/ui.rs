//! The switcher panel: a borderless, non-activating HUD window whose contents
//! are plain Core Animation layers. All keyboard and mouse input is handled by
//! the event tap in main.rs, so the panel never needs to become key.

use crate::ffi::*;
use crate::msg;
use crate::windows::Window;
use std::collections::HashMap;
use std::ffi::c_void;

const RADIUS: f64 = 18.0; // panel corners
const PAD: f64 = 14.0; // panel edge to tiles
const GAP: f64 = 4.0; // between tiles
const TILE_PAD: f64 = 10.0; // tile edge to thumbnail
const TILE_RADIUS: f64 = 12.0;
const THUMB_RADIUS: f64 = 6.0;
const LABEL_GAP: f64 = 8.0;
const LABEL_H: f64 = 18.0;
const ICON: f64 = 18.0;
const FONT_SIZE: f64 = 12.0;
const MIN_TILE_W: f64 = 120.0;

struct Tile {
    frame: CGRect,
    thumb: Id,
    placeholder: Id,
}

pub struct Panel {
    panel: Id,
    /// Outer layer: carries the panel's hairline border.
    outer: Id,
    /// Tiles live here, in top-left-origin coordinates.
    root: Id,
    highlight: Id,
    hover: Id,
    /// Whether the system is in dark mode (decides text and border colors).
    dark: bool,
    tiles: Vec<Tile>,
    /// Panel frame in global top-left-origin (CoreGraphics) coordinates.
    frame: CGRect,
    pub thumb_height: f64,
    pub scale: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPathCreateWithRoundedRect(r: CGRect, rw: f64, rh: f64, t: *const c_void) -> *const c_void;
    fn CGPathRelease(p: *const c_void);
    fn CGContextAddPath(c: CGContextRef, p: *const c_void);
    fn CGContextSetRGBFillColor(c: CGContextRef, r: f64, g: f64, b: f64, a: f64);
    fn CGContextFillPath(c: CGContextRef);
}

#[repr(C)]
struct NSEdgeInsets {
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
}

fn color(r: f64, g: f64, b: f64, a: f64) -> CGColorRef {
    unsafe { CGColorCreateSRGB(r, g, b, a) }
}

fn set_color(layer: Id, setter: &str, c: CGColorRef) {
    match setter {
        "bg" => msg!((); layer, "setBackgroundColor:", c),
        "fg" => msg!((); layer, "setForegroundColor:", c),
        _ => msg!((); layer, "setBorderColor:", c),
    }
    unsafe { CGColorRelease(c) };
}

fn new_layer(cls: &std::ffi::CStr) -> Id {
    msg!(Id; class(cls), "layer")
}

fn no_animation<R>(f: impl FnOnce() -> R) -> R {
    let tx = class(c"CATransaction");
    msg!((); tx, "begin");
    msg!((); tx, "setDisableActions:", true);
    let r = f();
    msg!((); tx, "commit");
    r
}

/// A stretchable rounded-rect mask, used to round the blurred background.
/// (A layer corner radius doesn't reliably clip a behind-window blur.)
fn rounded_mask(radius: f64) -> Id {
    let scale = 3.0;
    let side = radius * 2.0 + 1.0;
    let px = (side * scale) as usize;
    unsafe {
        let cs = CGColorSpaceCreateDeviceRGB();
        let ctx = CGBitmapContextCreate(std::ptr::null_mut(), px, px, 8, 0, cs, 1 /* premultiplied last */);
        CGColorSpaceRelease(cs);
        let r = radius * scale;
        let path = CGPathCreateWithRoundedRect(CGRect::new(0.0, 0.0, px as f64, px as f64), r, r, std::ptr::null());
        CGContextAddPath(ctx, path);
        CGContextSetRGBFillColor(ctx, 0.0, 0.0, 0.0, 1.0);
        CGContextFillPath(ctx);
        CGPathRelease(path);
        let cg = CGBitmapContextCreateImage(ctx);
        CGContextRelease(ctx);
        let img = msg!(Id; class(c"NSImage"), "alloc");
        let img = msg!(Id; img, "initWithCGImage:size:", cg, CGSize { width: side, height: side });
        CGImageRelease(cg);
        msg!((); img, "setCapInsets:", NSEdgeInsets { top: radius, left: radius, bottom: radius, right: radius });
        msg!((); img, "setResizingMode:", 1isize /* stretch */);
        img
    }
}

impl Panel {
    pub fn new() -> Panel {
        let rect = CGRect::new(0.0, 0.0, 400.0, 200.0);
        let panel = msg!(Id; class(c"NSPanel"), "alloc");
        let panel = msg!(Id; panel, "initWithContentRect:styleMask:backing:defer:",
            rect, 1usize << 7 /* borderless | nonactivating */, 2usize /* buffered */, false);
        msg!((); panel, "setLevel:", 101isize /* NSPopUpMenuWindowLevel */);
        msg!((); panel, "setOpaque:", false);
        msg!((); panel, "setBackgroundColor:", msg!(Id; class(c"NSColor"), "clearColor"));
        msg!((); panel, "setHasShadow:", true);
        msg!((); panel, "setIgnoresMouseEvents:", true);
        msg!((); panel, "setHidesOnDeactivate:", false);
        msg!((); panel, "setReleasedWhenClosed:", false);
        msg!((); panel, "setAnimationBehavior:", 2isize /* none */);
        // all Spaces | stationary | ignores cycle | over full-screen apps
        msg!((); panel, "setCollectionBehavior:", (1usize << 0) | (1 << 4) | (1 << 6) | (1 << 8));

        let effect = msg!(Id; class(c"NSVisualEffectView"), "alloc");
        let effect = msg!(Id; effect, "initWithFrame:", rect);
        msg!((); effect, "setMaterial:", 13isize /* HUD window */);
        msg!((); effect, "setBlendingMode:", 0isize /* behind window */);
        msg!((); effect, "setState:", 1isize /* always active */);
        msg!((); effect, "setMaskImage:", rounded_mask(RADIUS));
        msg!((); panel, "setContentView:", effect);

        // A layer-hosting view. AppKit resets the root layer's geometry, so
        // tiles go in a child layer with a top-left origin (like CG coordinates).
        let root = new_layer(c"CALayer");
        let content = new_layer(c"CALayer");
        msg!((); content, "setGeometryFlipped:", true);
        msg!((); root, "addSublayer:", content);
        msg!((); root, "setCornerRadius:", RADIUS);
        msg!((); root, "setBorderWidth:", 1.0f64);
        let host = msg!(Id; class(c"NSView"), "alloc");
        let host = msg!(Id; host, "initWithFrame:", rect);
        msg!((); host, "setLayer:", root);
        msg!((); host, "setWantsLayer:", true);
        msg!((); host, "setAutoresizingMask:", 18usize /* width | height */);
        msg!((); effect, "addSubview:", host);

        Panel {
            panel,
            outer: root,
            root: content,
            highlight: NIL,
            hover: NIL,
            dark: false,
            tiles: Vec::new(),
            frame: CGRect::default(),
            thumb_height: 160.0,
            scale: 2.0,
        }
    }

    pub fn show(&mut self, wins: &[Window], sel: usize, cache: &HashMap<CGWindowID, Image>) {
        let screen = screen_under_mouse();
        let (tiles, size) = layout(wins, screen.size, &mut self.thumb_height);

        self.frame = CGRect::new(
            (screen.origin.x + (screen.size.width - size.width) / 2.0).round(),
            (screen.origin.y + (screen.size.height - size.height) / 2.0).round(),
            size.width,
            size.height,
        );
        let main_h = unsafe { CGDisplayBounds(CGMainDisplayID()) }.size.height;
        let appkit = CGRect::new(
            self.frame.origin.x,
            main_h - self.frame.origin.y - size.height,
            size.width,
            size.height,
        );
        msg!((); self.panel, "setFrame:display:", appkit, false);
        self.scale = msg!(f64; self.panel, "backingScaleFactor");

        let appearance = msg!(Id; self.panel, "effectiveAppearance");
        self.dark = string_from(msg!(Id; appearance, "name")).contains("Dark");

        no_animation(|| {
            let edge = if self.dark { 1.0 } else { 0.0 };
            set_color(self.outer, "border", color(edge, edge, edge, 0.12));
            msg!((); self.root, "setFrame:", CGRect::new(0.0, 0.0, size.width, size.height));
            msg!((); self.root, "setSublayers:", NIL);

            // Keyboard selection (what releasing the modifier picks): a faint
            // accent tint. Mouse hover (what a click picks): a full accent ring,
            // shown only while the cursor is over a tile.
            self.highlight = accent_layer(0.20, 0.45);
            self.hover = accent_layer(0.0, 1.0);
            msg!((); self.hover, "setHidden:", true);
            msg!((); self.root, "addSublayer:", self.highlight);
            msg!((); self.root, "addSublayer:", self.hover);

            self.tiles = wins
                .iter()
                .zip(tiles)
                .map(|(w, (frame, thumb_rect))| self.build_tile(w, frame, thumb_rect, cache.get(&w.id)))
                .collect();
        });
        self.select(sel);
        msg!((); self.panel, "orderFrontRegardless");
        msg!((); self.panel, "invalidateShadow");
    }

    fn build_tile(&self, w: &Window, frame: CGRect, thumb_rect: CGRect, cached: Option<&Image>) -> Tile {
        let icon = w.icon();

        let thumb = new_layer(c"CALayer");
        msg!((); thumb, "setFrame:", thumb_rect);
        msg!((); thumb, "setCornerRadius:", THUMB_RADIUS);
        msg!((); thumb, "setMasksToBounds:", true);
        msg!((); thumb, "setContentsGravity:", nsstring("resizeAspect"));
        if !w.on_screen {
            msg!((); thumb, "setOpacity:", 0.6f32);
        }
        msg!((); self.root, "addSublayer:", thumb);

        // Shown until (or instead of, without Screen Recording) a capture.
        let placeholder = new_layer(c"CALayer");
        let ph = (thumb_rect.size.height * 0.45).min(thumb_rect.size.width * 0.45).min(72.0);
        msg!((); placeholder, "setFrame:", CGRect::new(
            ((thumb_rect.size.width - ph) / 2.0).round(),
            ((thumb_rect.size.height - ph) / 2.0).round(),
            ph,
            ph,
        ));
        msg!((); placeholder, "setContents:", icon);
        msg!((); placeholder, "setContentsGravity:", nsstring("resizeAspect"));
        msg!((); placeholder, "setContentsScale:", self.scale);
        msg!((); thumb, "addSublayer:", placeholder);
        match cached {
            Some(img) => set_thumb_layers(thumb, placeholder, img),
            None => {
                let (ink, alpha) = if self.dark { (1.0, 0.08) } else { (0.0, 0.06) };
                set_color(thumb, "bg", color(ink, ink, ink, alpha))
            }
        }

        let label_y = frame.origin.y + TILE_PAD + self.thumb_height + LABEL_GAP;
        let x = frame.origin.x + TILE_PAD;

        let icon_layer = new_layer(c"CALayer");
        msg!((); icon_layer, "setFrame:", CGRect::new(x, label_y + (LABEL_H - ICON) / 2.0, ICON, ICON));
        msg!((); icon_layer, "setContents:", icon);
        msg!((); icon_layer, "setContentsGravity:", nsstring("resizeAspect"));
        msg!((); icon_layer, "setContentsScale:", self.scale);
        msg!((); self.root, "addSublayer:", icon_layer);

        let text = new_layer(c"CATextLayer");
        let tx = x + ICON + 6.0;
        msg!((); text, "setFrame:", CGRect::new(tx, label_y + 1.0, frame.origin.x + frame.size.width - TILE_PAD - tx, LABEL_H));
        msg!((); text, "setString:", nsstring(&w.title));
        let font = msg!(Id; class(c"NSFont"), "systemFontOfSize:weight:", FONT_SIZE, 0.23f64 /* medium */);
        msg!((); text, "setFont:", font);
        msg!((); text, "setFontSize:", FONT_SIZE);
        let (ink, alpha) = if self.dark { (1.0, 0.92) } else { (0.0, 0.85) };
        set_color(text, "fg", color(ink, ink, ink, alpha));
        msg!((); text, "setTruncationMode:", nsstring("end"));
        msg!((); text, "setContentsScale:", self.scale);
        msg!((); self.root, "addSublayer:", text);

        Tile { frame, thumb, placeholder }
    }

    pub fn set_hover(&self, i: Option<usize>) {
        no_animation(|| match i.and_then(|i| self.tiles.get(i)) {
            Some(t) => {
                msg!((); self.hover, "setFrame:", t.frame);
                msg!((); self.hover, "setHidden:", false);
            }
            None => msg!((); self.hover, "setHidden:", true),
        });
    }

    pub fn select(&self, i: usize) {
        if let Some(t) = self.tiles.get(i) {
            no_animation(|| msg!((); self.highlight, "setFrame:", t.frame));
        }
    }

    pub fn set_thumb(&self, i: usize, img: &Image) {
        if let Some(t) = self.tiles.get(i) {
            no_animation(|| set_thumb_layers(t.thumb, t.placeholder, img));
        }
    }

    pub fn hide(&mut self) {
        msg!((); self.panel, "orderOut:", NIL);
        no_animation(|| msg!((); self.root, "setSublayers:", NIL));
        self.tiles.clear();
    }

    /// Which tile, if any, is under a point in global CG coordinates.
    pub fn hit_test(&self, p: CGPoint) -> Option<usize> {
        let local = CGPoint { x: p.x - self.frame.origin.x, y: p.y - self.frame.origin.y };
        self.tiles.iter().position(|t| t.frame.contains(local))
    }
}

/// A rounded tile-sized layer in the system accent color.
fn accent_layer(fill_alpha: f64, ring_alpha: f64) -> Id {
    let layer = new_layer(c"CALayer");
    msg!((); layer, "setCornerRadius:", TILE_RADIUS);
    msg!((); layer, "setBorderWidth:", 2.5f64);
    let accent = msg!(Id; class(c"NSColor"), "controlAccentColor");
    let ring = msg!(Id; accent, "colorWithAlphaComponent:", ring_alpha);
    let fill = msg!(Id; accent, "colorWithAlphaComponent:", fill_alpha);
    msg!((); layer, "setBorderColor:", msg!(CGColorRef; ring, "CGColor"));
    msg!((); layer, "setBackgroundColor:", msg!(CGColorRef; fill, "CGColor"));
    layer
}

fn set_thumb_layers(thumb: Id, placeholder: Id, img: &Image) {
    msg!((); thumb, "setContents:", img.0);
    msg!((); thumb, "setBackgroundColor:", std::ptr::null_mut::<c_void>());
    msg!((); placeholder, "setHidden:", true);
}

fn screen_under_mouse() -> CGRect {
    unsafe {
        let ev = CGEventCreate(std::ptr::null_mut());
        let p = CGEventGetLocation(ev);
        CFRelease(ev);
        let mut id: CGDirectDisplayID = 0;
        let mut n = 0u32;
        if CGGetDisplaysWithPoint(p, 1, &mut id, &mut n) != 0 || n == 0 {
            id = CGMainDisplayID();
        }
        CGDisplayBounds(id)
    }
}

/// Flow tiles into centered rows. Returns each tile's frame and thumbnail
/// rect (panel-local, top-left origin) and the panel size. Shrinks the
/// thumbnail height until everything fits on screen.
fn layout(wins: &[Window], screen: CGSize, th: &mut f64) -> (Vec<(CGRect, CGRect)>, CGSize) {
    let max_w = (screen.width * 0.9 - PAD * 2.0).max(MIN_TILE_W);
    let max_h = screen.height * 0.85 - PAD * 2.0;
    *th = (screen.height / 5.5).clamp(90.0, 200.0);

    loop {
        let t = *th;
        let tile_h = TILE_PAD + t + LABEL_GAP + LABEL_H + TILE_PAD;
        let widths: Vec<f64> = wins
            .iter()
            .map(|w| (t * w.aspect.clamp(0.6, 2.0) + TILE_PAD * 2.0).max(MIN_TILE_W).min(max_w))
            .collect();

        let mut rows: Vec<(usize, usize, f64)> = Vec::new(); // start, end, width
        let mut start = 0;
        let mut row_w = 0.0;
        for (i, &w) in widths.iter().enumerate() {
            let add = if i == start { w } else { GAP + w };
            if i > start && row_w + add > max_w {
                rows.push((start, i, row_w));
                start = i;
                row_w = w;
            } else {
                row_w += add;
            }
        }
        rows.push((start, widths.len(), row_w));

        let content_h = rows.len() as f64 * tile_h + (rows.len() as f64 - 1.0) * GAP;
        if content_h > max_h && t > 50.0 {
            *th = (t * 0.85).floor();
            continue;
        }

        let content_w = rows.iter().map(|r| r.2).fold(0.0, f64::max);
        let mut out = Vec::with_capacity(wins.len());
        for (ri, &(s, e, rw)) in rows.iter().enumerate() {
            let mut x = PAD + ((content_w - rw) / 2.0).round();
            let y = PAD + ri as f64 * (tile_h + GAP);
            for i in s..e {
                let frame = CGRect::new(x, y, widths[i], tile_h);
                // Fit the window's aspect ratio into the thumbnail box.
                let box_w = widths[i] - TILE_PAD * 2.0;
                let a = wins[i].aspect.max(0.1);
                let (tw, tht) = if box_w / t > a { (t * a, t) } else { (box_w, box_w / a) };
                let thumb = CGRect::new(
                    (x + TILE_PAD + (box_w - tw) / 2.0).round(),
                    (y + TILE_PAD + (t - tht) / 2.0).round(),
                    tw.round(),
                    tht.round(),
                );
                out.push((frame, thumb));
                x += widths[i] + GAP;
            }
        }
        return (out, CGSize { width: content_w + PAD * 2.0, height: content_h + PAD * 2.0 });
    }
}
