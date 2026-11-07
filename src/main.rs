//! wswitch: a small window switcher for macOS.
//!
//! Hold the modifier (Option by default) and press Tab to show every window
//! with a live preview; keep pressing Tab (Shift+Tab to go back) and release
//! the modifier to switch. Escape cancels, clicking a preview picks it.

#![allow(non_upper_case_globals)]

mod ffi;
mod ui;
mod windows;

use ffi::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Window;

const KEY_TAB: i64 = 48;
const KEY_RETURN: i64 = 36;
const KEY_ENTER: i64 = 76;
const KEY_ESCAPE: i64 = 53;
const KEY_LEFT: i64 = 123;
const KEY_RIGHT: i64 = 124;

/// Bumped every time the switcher opens or closes, so in-flight thumbnail
/// captures for a stale session stop early.
static SESSION: AtomicU64 = AtomicU64::new(0);

struct App {
    panel: ui::Panel,
    wins: Vec<Window>,
    sel: usize,
    visible: bool,
    thumbs: HashMap<CGWindowID, Image>,
    modifier: u64,
    tap: CFMachPortRef,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.borrow_mut().as_mut().map(f))
}

/// Moves a value to a closure that will run on the main thread anyway.
struct MainThread<T>(T);
unsafe impl<T> Send for MainThread<T> {}

impl App {
    /// Returns true to swallow the event.
    fn handle(&mut self, ty: u32, ev: CGEventRef) -> bool {
        let flags = unsafe { CGEventGetFlags(ev) };
        let held = flags & self.modifier != 0;
        match ty {
            kCGEventKeyDown => {
                let key = unsafe { CGEventGetIntegerValueField(ev, kCGKeyboardEventKeycode) };
                let shift = flags & kCGEventFlagMaskShift != 0;
                if !self.visible {
                    let others = (kCGEventFlagMaskCommand | kCGEventFlagMaskControl | kCGEventFlagMaskAlternate)
                        & !self.modifier;
                    if key == KEY_TAB && held && flags & others == 0 {
                        self.open(shift);
                        return true;
                    }
                    return false;
                }
                match key {
                    KEY_TAB if shift => self.step(-1),
                    KEY_TAB | KEY_RIGHT => self.step(1),
                    KEY_LEFT => self.step(-1),
                    KEY_ESCAPE => self.close(),
                    KEY_RETURN | KEY_ENTER => self.commit(),
                    _ => {}
                }
                true
            }
            kCGEventKeyUp => self.visible,
            kCGEventFlagsChanged => {
                if self.visible && !held {
                    self.commit();
                }
                false
            }
            kCGEventMouseMoved => {
                if self.visible {
                    // Hover is separate from the keyboard selection; it only
                    // decides what a click picks.
                    let p = unsafe { CGEventGetLocation(ev) };
                    self.panel.set_hover(self.panel.hit_test(p));
                }
                false
            }
            kCGEventLeftMouseDown | kCGEventRightMouseDown => {
                if !self.visible {
                    return false;
                }
                let p = unsafe { CGEventGetLocation(ev) };
                match self.panel.hit_test(p) {
                    Some(i) => {
                        self.sel = i;
                        self.commit();
                    }
                    None => self.close(),
                }
                true
            }
            _ => false,
        }
    }

    fn open(&mut self, backwards: bool) {
        self.wins = windows::list();
        if self.wins.is_empty() {
            return;
        }
        let n = self.wins.len();
        // Start on the *previous* window, unless the frontmost app has no
        // windows (e.g. Finder with only the desktop), in which case the
        // first window in the list isn't the one you're on.
        let ws = msg!(Id; class(c"NSWorkspace"), "sharedWorkspace");
        let front_pid = msg!(i32; msg!(Id; ws, "frontmostApplication"), "processIdentifier");
        self.sel = if backwards {
            n - 1
        } else if n > 1 && self.wins[0].pid == front_pid {
            1
        } else {
            0
        };

        self.visible = true;
        let session = SESSION.fetch_add(1, Ordering::SeqCst) + 1;
        self.panel.show(&self.wins, self.sel, &self.thumbs);

        let live: HashSet<CGWindowID> = self.wins.iter().map(|w| w.id).collect();
        self.thumbs.retain(|id, _| live.contains(id));

        // Without Screen Recording access, capturing just re-prompts; the
        // panel shows app icons instead.
        if !unsafe { CGPreflightScreenCaptureAccess() } {
            return;
        }
        // Capture fresh thumbnails off the main thread, nearest to the selection first.
        let mut jobs: Vec<(usize, CGWindowID)> = self.wins.iter().map(|w| w.id).enumerate().collect();
        let sel = self.sel;
        jobs.sort_by_key(|&(i, _)| i.abs_diff(sel));
        let max_h = (self.panel.thumb_height * self.panel.scale).ceil() as usize;
        std::thread::spawn(move || {
            for (i, wid) in jobs {
                if SESSION.load(Ordering::SeqCst) != session {
                    return;
                }
                if let Some(img) = windows::capture(wid, max_h) {
                    on_main(move || {
                        with_app(|a| {
                            if a.visible && SESSION.load(Ordering::SeqCst) == session {
                                a.panel.set_thumb(i, &img);
                            }
                            a.thumbs.insert(wid, img);
                        });
                    });
                }
            }
        });
    }

    fn step(&mut self, delta: isize) {
        let n = self.wins.len() as isize;
        if n == 0 {
            return;
        }
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
        self.panel.select(self.sel);
    }

    fn commit(&mut self) {
        let target = (self.sel < self.wins.len()).then(|| self.wins.swap_remove(self.sel));
        self.close();
        if let Some(w) = target {
            // Focus after the event tap callback returns, so a slow app
            // can't stall keyboard input.
            let w = MainThread(w);
            on_main(move || {
                let w = w;
                windows::focus(&w.0);
            });
        }
    }

    fn close(&mut self) {
        self.visible = false;
        SESSION.fetch_add(1, Ordering::SeqCst);
        self.panel.hide();
        self.wins.clear();
    }
}

extern "C" fn tap_callback(_proxy: *mut c_void, ty: u32, ev: CGEventRef, _info: *mut c_void) -> CGEventRef {
    let _pool = AutoreleasePool::new();
    if ty == kCGEventTapDisabledByTimeout || ty == kCGEventTapDisabledByUserInput {
        with_app(|a| unsafe { CGEventTapEnable(a.tap, true) });
        return ev;
    }
    if with_app(|a| a.handle(ty, ev)).unwrap_or(false) { null_mut() } else { ev }
}

fn install_event_tap() -> bool {
    let mask = [
        kCGEventKeyDown,
        kCGEventKeyUp,
        kCGEventFlagsChanged,
        kCGEventLeftMouseDown,
        kCGEventRightMouseDown,
        kCGEventMouseMoved,
    ]
    .iter()
    .fold(0u64, |m, t| m | (1 << t));
    unsafe {
        // Session tap, inserted at the head, able to swallow events.
        let tap = CGEventTapCreate(1, 0, 0, mask, tap_callback, null_mut());
        if tap.is_null() {
            return false;
        }
        let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), src, kCFRunLoopCommonModes);
        CFRelease(src);
        CGEventTapEnable(tap, true);
        with_app(|a| a.tap = tap);
    }
    true
}

/// The event tap needs Accessibility access; retry until it's granted.
extern "C" fn retry_tap(timer: CFRunLoopTimerRef, _: *mut c_void) {
    if unsafe { AXIsProcessTrusted() } && install_event_tap() {
        unsafe { CFRunLoopTimerInvalidate(timer) };
    }
}

fn request_permissions() {
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let vals = [kCFBooleanTrue];
        let opts = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        AXIsProcessTrustedWithOptions(opts);
        CFRelease(opts);
        if !CGPreflightScreenCaptureAccess() {
            CGRequestScreenCaptureAccess();
        }
    }
}

// --- Using Command+Tab ------------------------------------------------------
// The Dock handles ⌘⇥ before any event tap sees it, so when Command is the
// modifier we switch the system shortcut off while running (as alt-tab does)
// and switch it back on when we exit.

const HOTKEY_CMD_TAB: i32 = 1;
const HOTKEY_CMD_SHIFT_TAB: i32 = 2;

fn set_system_cmd_tab(enabled: bool) {
    if let Some(f) = skylight().set_symbolic_hotkey_enabled {
        unsafe {
            f(HOTKEY_CMD_TAB, enabled);
            f(HOTKEY_CMD_SHIFT_TAB, enabled);
        }
    }
}

extern "C" fn restore_cmd_tab() {
    set_system_cmd_tab(true);
}

extern "C" fn restore_cmd_tab_and_exit(sig: i32) {
    set_system_cmd_tab(true);
    unsafe { _exit(128 + sig) };
}

unsafe extern "C" {
    fn atexit(f: extern "C" fn()) -> i32;
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    fn _exit(code: i32) -> !;
}

// --- Menu bar ---------------------------------------------------------------

fn install_status_item(modifier_symbol: &str) {
    let bar = msg!(Id; class(c"NSStatusBar"), "systemStatusBar");
    let item = msg!(Id; bar, "statusItemWithLength:", -1.0f64 /* variable */);
    msg!(Id; item, "retain");
    let button = msg!(Id; item, "button");
    let image = msg!(Id; class(c"NSImage"), "imageWithSystemSymbolName:accessibilityDescription:",
        nsstring("rectangle.on.rectangle"), nsstring("wswitch"));
    if image.is_null() {
        msg!((); button, "setTitle:", nsstring("⇥"));
    } else {
        msg!((); image, "setTemplate:", true);
        msg!((); button, "setImage:", image);
    }

    let menu = msg!(Id; msg!(Id; class(c"NSMenu"), "alloc"), "initWithTitle:", nsstring("wswitch"));
    msg!((); menu, "setAutoenablesItems:", false);
    let add = |title: &str, action: Sel, key: &str| -> Id {
        let mi = msg!(Id; class(c"NSMenuItem"), "alloc");
        let mi = msg!(Id; mi, "initWithTitle:action:keyEquivalent:", nsstring(title), action, nsstring(key));
        msg!((); menu, "addItem:", mi);
        mi
    };
    let info = add(&format!("Switch windows with {modifier_symbol}⇥"), std::ptr::null(), "");
    msg!((); info, "setEnabled:", false);
    msg!((); menu, "addItem:", msg!(Id; class(c"NSMenuItem"), "separatorItem"));
    let quit = add("Quit wswitch", sel("terminate:\0"), "q");
    msg!((); quit, "setTarget:", msg!(Id; class(c"NSApplication"), "sharedApplication"));
    msg!((); item, "setMenu:", menu);
}

fn main() {
    let _pool = AutoreleasePool::new();

    // `wswitch --list` prints what the switcher would show, for debugging.
    if std::env::args().any(|a| a == "--list") {
        for w in windows::list() {
            let state = if w.on_screen { "" } else { " (minimized/hidden)" };
            println!("{:>6}  pid {:<6} {:.2}  {}{state}", w.id, w.pid, w.aspect, w.title);
        }
        return;
    }
    let nsapp = msg!(Id; class(c"NSApplication"), "sharedApplication");
    msg!(bool; nsapp, "setActivationPolicy:", 1isize /* accessory: no Dock icon */);

    // Settings live in user defaults, e.g.
    //   defaults write io.github.wswitch Modifier command
    let defaults = msg!(Id; class(c"NSUserDefaults"), "standardUserDefaults");
    let modifier_name = string_from(msg!(Id; defaults, "stringForKey:", nsstring("Modifier")));
    let (modifier, symbol) = match modifier_name.to_lowercase().as_str() {
        "command" | "cmd" => (kCGEventFlagMaskCommand, "⌘"),
        "control" | "ctrl" => (kCGEventFlagMaskControl, "⌃"),
        _ => (kCGEventFlagMaskAlternate, "⌥"),
    };
    if modifier == kCGEventFlagMaskCommand {
        set_system_cmd_tab(false);
        unsafe {
            atexit(restore_cmd_tab);
            for sig in [1, 2, 3, 6, 15] {
                // SIGHUP, SIGINT, SIGQUIT, SIGABRT, SIGTERM
                signal(sig, restore_cmd_tab_and_exit);
            }
        }
    }

    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            panel: ui::Panel::new(),
            wins: Vec::new(),
            sel: 0,
            visible: false,
            thumbs: HashMap::new(),
            modifier,
            tap: null_mut(),
        })
    });

    if !msg!(bool; defaults, "boolForKey:", nsstring("HideMenuBarIcon")) {
        install_status_item(symbol);
    }

    request_permissions();
    if !install_event_tap() {
        unsafe {
            let timer = CFRunLoopTimerCreate(
                std::ptr::null(),
                CFAbsoluteTimeGetCurrent() + 1.0,
                1.0,
                0,
                0,
                retry_tap,
                null_mut(),
            );
            CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopCommonModes);
        }
    }

    msg!((); nsapp, "run");
}
