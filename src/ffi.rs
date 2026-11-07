//! Hand-written bindings to the handful of system APIs wswitch needs:
//! the Objective-C runtime, CoreFoundation, CoreGraphics, Accessibility,
//! libdispatch, and a few private SkyLight calls looked up at runtime.

#![allow(non_upper_case_globals, non_snake_case, dead_code)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::OnceLock;

pub type Id = *mut c_void;
pub type Sel = *const c_void;
pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFIndex = isize;
pub type CGImageRef = *mut c_void;
pub type CGColorRef = *mut c_void;
pub type CGContextRef = *mut c_void;
pub type CGColorSpaceRef = *mut c_void;
pub type CGEventRef = *mut c_void;
pub type CFMachPortRef = *mut c_void;
pub type CFRunLoopRef = *mut c_void;
pub type CFRunLoopSourceRef = *mut c_void;
pub type CFRunLoopTimerRef = *mut c_void;
pub type AXUIElementRef = *mut c_void;
pub type CGWindowID = u32;
pub type CGDirectDisplayID = u32;

pub const NIL: Id = std::ptr::null_mut();

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

impl CGRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        CGRect { origin: CGPoint { x, y }, size: CGSize { width: w, height: h } }
    }
    pub fn contains(&self, p: CGPoint) -> bool {
        p.x >= self.origin.x
            && p.y >= self.origin.y
            && p.x < self.origin.x + self.size.width
            && p.y < self.origin.y + self.size.height
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcessSerialNumber {
    pub high: u32,
    pub low: u32,
}

// ---------------------------------------------------------------------------
// Objective-C runtime
// ---------------------------------------------------------------------------

#[link(name = "objc")]
unsafe extern "C" {
    pub fn objc_getClass(name: *const c_char) -> Id;
    pub fn sel_registerName(name: *const c_char) -> Sel;
    pub fn objc_msgSend();
    pub fn objc_autoreleasePoolPush() -> *mut c_void;
    pub fn objc_autoreleasePoolPop(pool: *mut c_void);
}

// Linking these frameworks makes their classes available to objc_getClass.
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    pub fn NSApplicationLoad() -> bool;
}
#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

/// Send an Objective-C message. Argument types are inferred from the
/// expressions passed, so integer literals need explicit suffixes.
///
/// `msg!(ReturnType; receiver, "selector:with:", arg1, arg2)`
#[macro_export]
macro_rules! msg {
    ($ret:ty; $obj:expr, $sel:literal $(, $arg:expr)* $(,)?) => {{
        #[allow(unused_unsafe, clippy::missing_transmute_annotations)]
        unsafe {
            let f: unsafe extern "C" fn($crate::ffi::Id, $crate::ffi::Sel $(, $crate::msg!(@ty $arg))*) -> $ret =
                ::core::mem::transmute($crate::ffi::objc_msgSend as unsafe extern "C" fn());
            f($obj as $crate::ffi::Id, $crate::ffi::sel(concat!($sel, "\0")) $(, $arg)*)
        }
    }};
    (@ty $e:expr) => { _ };
}

pub fn sel(name_nul: &'static str) -> Sel {
    unsafe { sel_registerName(name_nul.as_ptr() as *const c_char) }
}

pub fn class(name: &CStr) -> Id {
    unsafe { objc_getClass(name.as_ptr()) }
}

/// Autoreleased NSString.
pub fn nsstring(s: &str) -> Id {
    let obj = msg!(Id; class(c"NSString"), "alloc");
    let obj = msg!(Id; obj, "initWithBytes:length:encoding:", s.as_ptr(), s.len(), 4usize);
    msg!(Id; obj, "autorelease")
}

/// Copy an NSString / CFString into a Rust String.
pub fn string_from(ns: CFTypeRef) -> String {
    if ns.is_null() {
        return String::new();
    }
    let p = msg!(*const c_char; ns, "UTF8String");
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

pub struct AutoreleasePool(*mut c_void);

impl AutoreleasePool {
    pub fn new() -> Self {
        AutoreleasePool(unsafe { objc_autoreleasePoolPush() })
    }
}

impl Drop for AutoreleasePool {
    fn drop(&mut self) {
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}

// ---------------------------------------------------------------------------
// CoreFoundation
// ---------------------------------------------------------------------------

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFBooleanTrue: CFTypeRef;
    pub static kCFBooleanFalse: CFTypeRef;
    pub static kCFRunLoopCommonModes: CFStringRef;
    pub static kCFTypeDictionaryKeyCallBacks: c_void;
    pub static kCFTypeDictionaryValueCallBacks: c_void;

    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFArrayGetCount(a: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(a: CFArrayRef, i: CFIndex) -> CFTypeRef;
    pub fn CFDictionaryGetValue(d: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFDictionaryCreate(
        alloc: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        n: CFIndex,
        kcb: *const c_void,
        vcb: *const c_void,
    ) -> CFDictionaryRef;
    pub fn CFNumberGetValue(n: CFTypeRef, ty: CFIndex, out: *mut c_void) -> bool;
    pub fn CFStringCreateWithBytes(
        alloc: CFTypeRef,
        bytes: *const u8,
        len: CFIndex,
        enc: u32,
        external: bool,
    ) -> CFStringRef;
    pub fn CFRunLoopGetMain() -> CFRunLoopRef;
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, src: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopAddTimer(rl: CFRunLoopRef, t: CFRunLoopTimerRef, mode: CFStringRef);
    pub fn CFRunLoopTimerCreate(
        alloc: CFTypeRef,
        fire: f64,
        interval: f64,
        flags: u64,
        order: CFIndex,
        cb: extern "C" fn(CFRunLoopTimerRef, *mut c_void),
        ctx: *mut c_void,
    ) -> CFRunLoopTimerRef;
    pub fn CFRunLoopTimerInvalidate(t: CFRunLoopTimerRef);
    pub fn CFAbsoluteTimeGetCurrent() -> f64;
    pub fn CFMachPortCreateRunLoopSource(
        alloc: CFTypeRef,
        port: CFMachPortRef,
        order: CFIndex,
    ) -> CFRunLoopSourceRef;
}

pub const kCFNumberSInt32Type: CFIndex = 3;
pub const kCFNumberSInt64Type: CFIndex = 4;
pub const kCFNumberDoubleType: CFIndex = 13;

/// An owned CFString (released on drop). Usable anywhere an NSString is.
pub struct CFStr(pub CFStringRef);

impl CFStr {
    pub fn new(s: &str) -> Self {
        CFStr(unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as CFIndex, 0x0800_0100, false) })
    }
}

impl Drop for CFStr {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

pub fn cf_i64(n: CFTypeRef) -> Option<i64> {
    let mut v: i64 = 0;
    (!n.is_null() && unsafe { CFNumberGetValue(n, kCFNumberSInt64Type, &mut v as *mut _ as *mut c_void) }).then_some(v)
}

pub fn cf_f64(n: CFTypeRef) -> Option<f64> {
    let mut v: f64 = 0.0;
    (!n.is_null() && unsafe { CFNumberGetValue(n, kCFNumberDoubleType, &mut v as *mut _ as *mut c_void) }).then_some(v)
}

// ---------------------------------------------------------------------------
// CoreGraphics
// ---------------------------------------------------------------------------

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub static CGRectNull: CGRect;
    pub static kCGWindowNumber: CFStringRef;
    pub static kCGWindowLayer: CFStringRef;
    pub static kCGWindowOwnerPID: CFStringRef;
    pub static kCGWindowName: CFStringRef;
    pub static kCGWindowBounds: CFStringRef;
    pub static kCGWindowAlpha: CFStringRef;

    pub fn CGWindowListCopyWindowInfo(option: u32, relative_to: CGWindowID) -> CFArrayRef;
    pub fn CGWindowListCreateImage(bounds: CGRect, option: u32, wid: CGWindowID, image_opt: u32) -> CGImageRef;
    pub fn CGRectMakeWithDictionaryRepresentation(d: CFDictionaryRef, out: *mut CGRect) -> bool;
    pub fn CGPreflightScreenCaptureAccess() -> bool;
    pub fn CGRequestScreenCaptureAccess() -> bool;

    pub fn CGImageRetain(img: CGImageRef) -> CGImageRef;
    pub fn CGImageRelease(img: CGImageRef);
    pub fn CGImageGetWidth(img: CGImageRef) -> usize;
    pub fn CGImageGetHeight(img: CGImageRef) -> usize;
    pub fn CGColorSpaceCreateDeviceRGB() -> CGColorSpaceRef;
    pub fn CGColorSpaceRelease(cs: CGColorSpaceRef);
    pub fn CGBitmapContextCreate(
        data: *mut c_void,
        w: usize,
        h: usize,
        bpc: usize,
        bpr: usize,
        cs: CGColorSpaceRef,
        info: u32,
    ) -> CGContextRef;
    pub fn CGBitmapContextCreateImage(ctx: CGContextRef) -> CGImageRef;
    pub fn CGContextDrawImage(ctx: CGContextRef, r: CGRect, img: CGImageRef);
    pub fn CGContextSetInterpolationQuality(ctx: CGContextRef, q: i32);
    pub fn CGContextRelease(ctx: CGContextRef);
    pub fn CGColorCreateSRGB(r: f64, g: f64, b: f64, a: f64) -> CGColorRef;
    pub fn CGColorRelease(c: CGColorRef);

    pub fn CGMainDisplayID() -> CGDirectDisplayID;
    pub fn CGDisplayBounds(d: CGDirectDisplayID) -> CGRect;
    pub fn CGGetDisplaysWithPoint(p: CGPoint, max: u32, out: *mut CGDirectDisplayID, count: *mut u32) -> i32;

    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        cb: extern "C" fn(*mut c_void, u32, CGEventRef, *mut c_void) -> CGEventRef,
        info: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub fn CGEventGetIntegerValueField(e: CGEventRef, field: u32) -> i64;
    pub fn CGEventGetFlags(e: CGEventRef) -> u64;
    pub fn CGEventGetLocation(e: CGEventRef) -> CGPoint;
    pub fn CGEventCreate(src: *mut c_void) -> CGEventRef;
}

pub const kCGWindowListOptionOnScreenOnly: u32 = 1 << 0;
pub const kCGWindowListOptionIncludingWindow: u32 = 1 << 3;
pub const kCGWindowListExcludeDesktopElements: u32 = 1 << 4;
pub const kCGWindowImageBoundsIgnoreFraming: u32 = 1 << 0;
pub const kCGWindowImageNominalResolution: u32 = 1 << 4;

pub const kCGEventLeftMouseDown: u32 = 1;
pub const kCGEventRightMouseDown: u32 = 3;
pub const kCGEventMouseMoved: u32 = 5;
pub const kCGEventKeyDown: u32 = 10;
pub const kCGEventKeyUp: u32 = 11;
pub const kCGEventFlagsChanged: u32 = 12;
pub const kCGEventTapDisabledByTimeout: u32 = 0xFFFF_FFFE;
pub const kCGEventTapDisabledByUserInput: u32 = 0xFFFF_FFFF;
pub const kCGKeyboardEventKeycode: u32 = 9;

pub const kCGEventFlagMaskShift: u64 = 1 << 17;
pub const kCGEventFlagMaskControl: u64 = 1 << 18;
pub const kCGEventFlagMaskAlternate: u64 = 1 << 19;
pub const kCGEventFlagMaskCommand: u64 = 1 << 20;

/// A retained CGImage.
pub struct Image(pub CGImageRef);

unsafe impl Send for Image {}

impl Clone for Image {
    fn clone(&self) -> Self {
        Image(unsafe { CGImageRetain(self.0) })
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { CGImageRelease(self.0) }
    }
}

// ---------------------------------------------------------------------------
// Accessibility (HIServices) and legacy process APIs
// ---------------------------------------------------------------------------

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub static kAXTrustedCheckOptionPrompt: CFStringRef;

    pub fn AXIsProcessTrusted() -> bool;
    pub fn AXIsProcessTrustedWithOptions(opts: CFDictionaryRef) -> bool;
    pub fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(e: AXUIElementRef, attr: CFStringRef, out: *mut CFTypeRef) -> i32;
    pub fn AXUIElementSetAttributeValue(e: AXUIElementRef, attr: CFStringRef, v: CFTypeRef) -> i32;
    pub fn AXUIElementPerformAction(e: AXUIElementRef, action: CFStringRef) -> i32;
    pub fn AXUIElementSetMessagingTimeout(e: AXUIElementRef, secs: f32) -> i32;
    /// Private but long-stable: maps an AX window element to its CGWindowID.
    pub fn _AXUIElementGetWindow(e: AXUIElementRef, out: *mut CGWindowID) -> i32;
    pub fn GetProcessForPID(pid: i32, psn: *mut ProcessSerialNumber) -> i32;
}

/// Copy an AX attribute; the result (if any) is owned by the caller.
pub fn ax_copy(e: AXUIElementRef, attr: &str) -> Option<CFTypeRef> {
    let a = CFStr::new(attr);
    let mut out: CFTypeRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(e, a.0, &mut out) };
    (err == 0 && !out.is_null()).then_some(out)
}

// ---------------------------------------------------------------------------
// libdispatch
// ---------------------------------------------------------------------------

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, ctx: *mut c_void, work: extern "C" fn(*mut c_void));
}

/// Run `f` on the main thread, on a later run loop turn.
pub fn on_main<F: FnOnce() + Send + 'static>(f: F) {
    extern "C" fn trampoline<F: FnOnce()>(ctx: *mut c_void) {
        let f = unsafe { Box::from_raw(ctx as *mut F) };
        let _pool = AutoreleasePool::new();
        f();
    }
    let ctx = Box::into_raw(Box::new(f)) as *mut c_void;
    unsafe { dispatch_async_f(&_dispatch_main_q, ctx, trampoline::<F>) }
}

// ---------------------------------------------------------------------------
// Private SkyLight calls, resolved lazily so a missing symbol degrades
// gracefully instead of failing to launch.
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

pub struct SkyLight {
    pub main_connection_id: Option<unsafe extern "C" fn() -> i32>,
    pub hw_capture_window_list: Option<unsafe extern "C" fn(i32, *const CGWindowID, i32, u32) -> CFArrayRef>,
    pub set_front_process: Option<unsafe extern "C" fn(*const ProcessSerialNumber, CGWindowID, u32) -> i32>,
    pub post_event_record_to: Option<unsafe extern "C" fn(*const ProcessSerialNumber, *const u8) -> i32>,
    pub set_symbolic_hotkey_enabled: Option<unsafe extern "C" fn(i32, bool) -> i32>,
}

pub fn skylight() -> &'static SkyLight {
    static SL: OnceLock<SkyLight> = OnceLock::new();
    SL.get_or_init(|| unsafe {
        let path = c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight";
        let h = dlopen(path.as_ptr(), 1 /* RTLD_LAZY */);
        let sym = |name: &str| -> *mut c_void {
            if h.is_null() {
                return std::ptr::null_mut();
            }
            let n = CString::new(name).unwrap();
            dlsym(h, n.as_ptr())
        };
        macro_rules! load {
            ($name:literal) => {{
                let p = sym($name);
                if p.is_null() { None } else { Some(std::mem::transmute(p)) }
            }};
        }
        SkyLight {
            main_connection_id: load!("SLSMainConnectionID"),
            hw_capture_window_list: load!("SLSHWCaptureWindowList"),
            set_front_process: load!("_SLPSSetFrontProcessWithOptions"),
            post_event_record_to: load!("SLPSPostEventRecordTo"),
            set_symbolic_hotkey_enabled: load!("SLSSetSymbolicHotKeyEnabled"),
        }
    })
}
