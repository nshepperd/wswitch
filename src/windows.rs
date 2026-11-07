//! Finding windows, capturing their contents, and focusing them.

use crate::ffi::*;
use crate::msg;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::ptr::null_mut;

pub struct Window {
    pub id: CGWindowID,
    pub pid: i32,
    pub title: String,
    /// Width / height of the window frame.
    pub aspect: f64,
    /// Whether the window is currently visible (not minimized, hidden or elsewhere).
    pub on_screen: bool,
    /// Retained AXUIElement for the window, if Accessibility found it.
    ax: AXUIElementRef,
    /// Retained NSRunningApplication.
    app: Id,
}

impl Window {
    /// The owning application's icon (NSImage).
    pub fn icon(&self) -> Id {
        msg!(Id; self.app, "icon")
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            if !self.ax.is_null() {
                CFRelease(self.ax);
            }
        }
        msg!((); self.app, "release");
    }
}

struct AxWindow {
    el: AXUIElementRef, // retained
    title: String,
}

/// All switchable windows, roughly most-recently-used first: visible windows
/// in z-order, then minimized windows and windows of hidden apps.
pub fn list() -> Vec<Window> {
    let me = std::process::id() as i32;

    // Regular (Dock-visible) apps, keyed by pid.
    let mut apps: Vec<(i32, Id)> = Vec::new();
    let running = msg!(Id; msg!(Id; class(c"NSWorkspace"), "sharedWorkspace"), "runningApplications");
    for i in 0..msg!(usize; running, "count") {
        let app = msg!(Id; running, "objectAtIndex:", i);
        let pid = msg!(i32; app, "processIdentifier");
        if pid != me && msg!(isize; app, "activationPolicy") == 0 {
            apps.push((pid, app));
        }
    }

    // Ask each app for its windows over Accessibility. This is what lets us
    // skip invisible helper windows, and find minimized ones.
    let mut ax: HashMap<CGWindowID, AxWindow> = HashMap::new();
    let mut ax_ok: HashSet<i32> = HashSet::new();
    let mut ax_order: Vec<(i32, CGWindowID)> = Vec::new();
    for &(pid, _) in &apps {
        unsafe {
            let app_el = AXUIElementCreateApplication(pid);
            AXUIElementSetMessagingTimeout(app_el, 0.25);
            if let Some(arr) = ax_copy(app_el, "AXWindows") {
                ax_ok.insert(pid);
                for i in 0..CFArrayGetCount(arr) {
                    let el = CFArrayGetValueAtIndex(arr, i) as AXUIElementRef;
                    let mut wid: CGWindowID = 0;
                    if _AXUIElementGetWindow(el, &mut wid) != 0 || wid == 0 {
                        continue;
                    }
                    let title = ax_copy(el, "AXTitle")
                        .map(|t| {
                            let s = string_from(t);
                            CFRelease(t);
                            s
                        })
                        .unwrap_or_default();
                    CFRetain(el);
                    ax.insert(wid, AxWindow { el, title });
                    ax_order.push((pid, wid));
                }
                CFRelease(arr);
            }
            CFRelease(app_el);
        }
    }

    let app_for = |pid: i32| apps.iter().find(|a| a.0 == pid).map(|a| a.1);
    let mut out = Vec::new();

    // 1. On-screen windows, front to back.
    unsafe {
        let info = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            0,
        );
        if !info.is_null() {
            for i in 0..CFArrayGetCount(info) {
                let d = CFArrayGetValueAtIndex(info, i);
                let get = |k: CFStringRef| CFDictionaryGetValue(d, k);
                if cf_i64(get(kCGWindowLayer)) != Some(0) || cf_f64(get(kCGWindowAlpha)).unwrap_or(1.0) <= 0.0 {
                    continue;
                }
                let (Some(wid), Some(pid)) = (cf_i64(get(kCGWindowNumber)), cf_i64(get(kCGWindowOwnerPID))) else {
                    continue;
                };
                let (wid, pid) = (wid as CGWindowID, pid as i32);
                let Some(app) = app_for(pid) else { continue };
                let mut b = CGRect::default();
                CGRectMakeWithDictionaryRepresentation(get(kCGWindowBounds), &mut b);
                if b.size.width < 60.0 || b.size.height < 40.0 {
                    continue;
                }
                let axw = ax.remove(&wid);
                // If the app answered Accessibility but doesn't list this window,
                // it's an overlay or helper, not a real window.
                if axw.is_none() && ax_ok.contains(&pid) {
                    continue;
                }
                let (el, mut title) = axw.map(|a| (a.el, a.title)).unwrap_or((null_mut(), String::new()));
                if title.is_empty() {
                    title = string_from(get(kCGWindowName));
                }
                out.push(Window {
                    id: wid,
                    pid,
                    title,
                    aspect: b.size.width / b.size.height,
                    on_screen: true,
                    ax: el,
                    app: msg!(Id; app, "retain"),
                });
            }
            CFRelease(info);
        }
    }

    // 2. Minimized windows and windows of hidden apps.
    for (pid, wid) in ax_order {
        let Some(axw) = ax.remove(&wid) else { continue };
        let standard = ax_copy(axw.el, "AXSubrole").map(|s| unsafe {
            let ok = string_from(s) == "AXStandardWindow";
            CFRelease(s);
            ok
        });
        let minimized = ax_copy(axw.el, "AXMinimized").map(|v| unsafe {
            CFRelease(v);
            v == kCFBooleanTrue
        });
        let app = app_for(pid).unwrap();
        let hidden = msg!(bool; app, "isHidden");
        if standard == Some(true) && (minimized == Some(true) || hidden) {
            out.push(Window {
                id: wid,
                pid,
                title: axw.title,
                aspect: ax_aspect(axw.el).unwrap_or(1.6),
                on_screen: false,
                ax: axw.el,
                app: msg!(Id; app, "retain"),
            });
        } else {
            unsafe { CFRelease(axw.el) };
        }
    }
    // Anything left in `ax` is a window we decided not to show.
    for (_, a) in ax {
        unsafe { CFRelease(a.el) };
    }

    // Fall back to the app name for untitled windows.
    for w in &mut out {
        if w.title.is_empty() {
            w.title = string_from(msg!(Id; w.app, "localizedName"));
        }
    }
    out
}

fn ax_aspect(el: AXUIElementRef) -> Option<f64> {
    unsafe extern "C" {
        fn AXValueGetValue(v: CFTypeRef, ty: u32, out: *mut c_void) -> bool;
    }
    let v = ax_copy(el, "AXSize")?;
    let mut s = CGSize::default();
    let ok = unsafe { AXValueGetValue(v, 2 /* kAXValueCGSizeType */, &mut s as *mut _ as *mut c_void) };
    unsafe { CFRelease(v) };
    (ok && s.height > 0.0).then(|| s.width / s.height)
}

/// Capture a window's contents, scaled to at most `max_h` pixels tall.
/// Safe to call from a background thread.
pub fn capture(wid: CGWindowID, max_h: usize) -> Option<Image> {
    let sl = skylight();
    let mut img: CGImageRef = null_mut();
    unsafe {
        // The same private call alt-tab uses: faster, and works for minimized windows.
        if let (Some(cid), Some(cap)) = (sl.main_connection_id, sl.hw_capture_window_list) {
            const IGNORE_GLOBAL_CLIP_SHAPE: u32 = 1 << 11;
            const NOMINAL_RESOLUTION: u32 = 1 << 9;
            let arr = cap(cid(), &wid, 1, IGNORE_GLOBAL_CLIP_SHAPE | NOMINAL_RESOLUTION);
            if !arr.is_null() {
                if CFArrayGetCount(arr) > 0 {
                    img = CGImageRetain(CFArrayGetValueAtIndex(arr, 0) as CGImageRef);
                }
                CFRelease(arr);
            }
        }
        if img.is_null() {
            img = CGWindowListCreateImage(
                CGRectNull,
                kCGWindowListOptionIncludingWindow,
                wid,
                kCGWindowImageBoundsIgnoreFraming | kCGWindowImageNominalResolution,
            );
        }
    }
    if img.is_null() {
        return None;
    }
    let img = Image(img);
    let (w, h) = unsafe { (CGImageGetWidth(img.0), CGImageGetHeight(img.0)) };
    if w < 2 || h < 2 {
        return None;
    }
    if h <= max_h {
        return Some(img);
    }
    // Downscale so a screenful of cached thumbnails stays small in memory.
    let nh = max_h;
    let nw = ((w * nh) / h).max(1);
    unsafe {
        let cs = CGColorSpaceCreateDeviceRGB();
        // kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little
        let ctx = CGBitmapContextCreate(null_mut(), nw, nh, 8, 0, cs, 2 | (2 << 12));
        CGColorSpaceRelease(cs);
        if ctx.is_null() {
            return Some(img);
        }
        CGContextSetInterpolationQuality(ctx, 3 /* high */);
        CGContextDrawImage(ctx, CGRect::new(0.0, 0.0, nw as f64, nh as f64), img.0);
        let small = CGBitmapContextCreateImage(ctx);
        CGContextRelease(ctx);
        if small.is_null() { Some(img) } else { Some(Image(small)) }
    }
}

/// Bring a window to the front and give it keyboard focus.
pub fn focus(w: &Window) {
    let sl = skylight();
    unsafe {
        if !w.ax.is_null() {
            let attr = CFStr::new("AXMinimized");
            AXUIElementSetAttributeValue(w.ax, attr.0, kCFBooleanFalse);
        }
        if msg!(bool; w.app, "isHidden") {
            msg!(bool; w.app, "unhide");
        }

        let mut psn = ProcessSerialNumber::default();
        let have_psn = GetProcessForPID(w.pid, &mut psn) == 0;
        if let (true, Some(front), Some(post)) = (have_psn, sl.set_front_process, sl.post_event_record_to) {
            // Activate the app with this specific window as key, without
            // dragging all of its other windows forward.
            const USER_GENERATED: u32 = 0x200;
            front(&psn, w.id, USER_GENERATED);
            for kind in [1u8, 2u8] {
                let mut rec = [0u8; 0xf8];
                rec[0x04] = 0xf8;
                rec[0x08] = kind;
                rec[0x3a] = 0x10;
                rec[0x20..0x30].fill(0xff);
                rec[0x3c..0x40].copy_from_slice(&w.id.to_ne_bytes());
                post(&psn, rec.as_ptr());
            }
        } else {
            // Public fallback: NSApplicationActivateIgnoringOtherApps.
            msg!(bool; w.app, "activateWithOptions:", 2usize);
        }

        if !w.ax.is_null() {
            let raise = CFStr::new("AXRaise");
            AXUIElementPerformAction(w.ax, raise.0);
        }
    }
}
