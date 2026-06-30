// src/dock.rs — macOS Dock icon for `tauri dev` (raw objc runtime, no crate).
//
// `tauri dev` runs the raw cargo binary — no `.app` bundle — so macOS shows the
// generic executable icon; the bundled `.icns` only applies to `tauri build`
// output. Setting NSApplication's `applicationIconImage` covers the dev case
// (and is a visual no-op in a bundle, where the same artwork already is the
// icon). Hand-declared externs, same approach as hid.rs/watcher.rs — must run
// on the main thread (the Tauri setup hook does).

use std::os::raw::{c_char, c_void};

type Id = *mut c_void;
type Sel = *const c_void;

extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
}

// The baked full-bleed squircle (level-meter mark) at 256. `applicationIconImage`
// is drawn as-is (macOS does not round programmatically-set icons), so the dev Dock
// needs the rounding baked into the pixels here. The bundled `.icns` (icon.icns) is
// the same baked squircle — macOS only auto-draws the rounded enclosure for icons in
// the Icon Composer `.icon`/Assets.car format, not a plain Tauri `.icns`, so a
// square-cornered `.icns` would show as a hard tile. dock.png = icons/png/icon-256.png
// from the design handoff (see the App-icon note in CLAUDE.md).
static ICON_PNG: &[u8] = include_bytes!("../icons/dock.png");

pub fn set_dock_icon() {
    unsafe {
        let nsdata = objc_getClass(c"NSData".as_ptr());
        let nsimage = objc_getClass(c"NSImage".as_ptr());
        let nsapp = objc_getClass(c"NSApplication".as_ptr());
        if nsdata.is_null() || nsimage.is_null() || nsapp.is_null() {
            return;
        }
        // objc_msgSend is intentionally untyped — cast per call signature
        // (the same pattern the objc crate's msg_send! macro generates).
        let send0: unsafe extern "C" fn(Id, Sel) -> Id =
            std::mem::transmute(objc_msgSend as *const c_void);
        let send1: unsafe extern "C" fn(Id, Sel, Id) -> Id =
            std::mem::transmute(objc_msgSend as *const c_void);
        let send_bytes: unsafe extern "C" fn(Id, Sel, *const c_void, usize) -> Id =
            std::mem::transmute(objc_msgSend as *const c_void);

        // [NSData dataWithBytes:ICON_PNG length:len] (autoreleased)
        let data = send_bytes(
            nsdata,
            sel_registerName(c"dataWithBytes:length:".as_ptr()),
            ICON_PNG.as_ptr() as *const c_void,
            ICON_PNG.len(),
        );
        if data.is_null() {
            return;
        }
        // [[NSImage alloc] initWithData:data]
        let img = send1(
            send0(nsimage, sel_registerName(c"alloc".as_ptr())),
            sel_registerName(c"initWithData:".as_ptr()),
            data,
        );
        if img.is_null() {
            return;
        }
        // [[NSApplication sharedApplication] setApplicationIconImage:img]
        let app = send0(nsapp, sel_registerName(c"sharedApplication".as_ptr()));
        let _ = send1(
            app,
            sel_registerName(c"setApplicationIconImage:".as_ptr()),
            img,
        );
    }
}
