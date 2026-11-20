//! Small binding to VTE's stable GTK3 C ABI (Ubuntu 22.04: VTE 0.68).
//! The native terminal owns the PTY descriptor; output never crosses WebView/JS.
use glib::translate::*;
use gtk::{gdk, glib, pango, prelude::*};
use std::{
    cell::{Cell, RefCell},
    ffi::{c_char, c_void, CString},
    os::fd::{IntoRawFd, OwnedFd},
    ptr,
    rc::Rc,
};

#[link(name = "libvte-2.91.so.0", kind = "dylib", modifiers = "+verbatim")]
extern "C" {
    fn vte_terminal_new() -> *mut gtk::ffi::GtkWidget;
    fn vte_terminal_set_scrollback_lines(terminal: *mut c_void, lines: i64);
    fn vte_terminal_set_font(terminal: *mut c_void, font: *const pango::ffi::PangoFontDescription);
    fn vte_terminal_set_input_enabled(terminal: *mut c_void, enabled: i32);
    fn vte_terminal_set_cell_height_scale(terminal: *mut c_void, scale: f64);
    fn vte_terminal_set_colors(
        terminal: *mut c_void,
        fg: *const gdk::ffi::GdkRGBA,
        bg: *const gdk::ffi::GdkRGBA,
        palette: *const gdk::ffi::GdkRGBA,
        count: usize,
    );
    fn vte_terminal_set_bold_is_bright(terminal: *mut c_void, enabled: i32);
    fn vte_terminal_set_cursor_blink_mode(terminal: *mut c_void, mode: i32);
    fn vte_terminal_set_cursor_shape(terminal: *mut c_void, shape: i32);
    fn vte_terminal_set_allow_hyperlink(terminal: *mut c_void, allow: i32);
    fn vte_terminal_set_pty(terminal: *mut c_void, pty: *mut c_void);
    fn vte_pty_new_foreign_sync(
        fd: i32,
        cancellable: *mut c_void,
        error: *mut *mut glib::ffi::GError,
    ) -> *mut c_void;
    fn vte_terminal_feed(terminal: *mut c_void, text: *const u8, length: isize);
    fn vte_terminal_paste_text(terminal: *mut c_void, text: *const c_char);
    fn vte_terminal_copy_clipboard_format(terminal: *mut c_void, format: i32);
    fn vte_terminal_search_set_wrap_around(terminal: *mut c_void, wrap: i32);
    fn vte_terminal_search_set_regex(terminal: *mut c_void, regex: *mut c_void, flags: u32);
    fn vte_regex_new_for_search(
        pattern: *const c_char,
        len: isize,
        flags: u32,
        error: *mut *mut glib::ffi::GError,
    ) -> *mut c_void;
    fn vte_regex_unref(regex: *mut c_void);
    fn vte_terminal_search_find_next(terminal: *mut c_void) -> i32;
    fn vte_terminal_search_find_previous(terminal: *mut c_void) -> i32;
    fn vte_terminal_get_text_range(
        terminal: *mut c_void,
        start_row: i64,
        start_col: i64,
        end_row: i64,
        end_col: i64,
        selected: *mut c_void,
        data: *mut c_void,
        attrs: *mut c_void,
    ) -> *mut c_char;
    fn vte_terminal_get_cursor_position(terminal: *mut c_void, column: *mut i64, row: *mut i64);
    fn vte_terminal_feed_child(terminal: *mut c_void, data: *const u8, length: isize);
    #[cfg(feature = "native-probe")]
    fn vte_terminal_select_all(terminal: *mut c_void);
}

#[derive(Clone)]
pub struct Terminal {
    pub widget: gtk::Widget,
    alive: Rc<Cell<bool>>,
    search_query: Rc<RefCell<String>>,
}

impl Terminal {
    pub fn new() -> Self {
        unsafe {
            let widget: gtk::Widget = from_glib_none(vte_terminal_new());
            let alive = Rc::new(Cell::new(true));
            let on_destroy = alive.clone();
            widget.connect_destroy(move |_| on_destroy.set(false));
            let terminal = Self {
                widget,
                alive,
                search_query: Rc::new(RefCell::new(String::new())),
            };
            let raw = terminal.raw();
            vte_terminal_set_scrollback_lines(raw, 3000);
            let mut font = pango::FontDescription::from_string(
                "QZ Design Mono A, Noto Sans Mono CJK SC, DejaVu Sans Mono",
            );
            font.set_absolute_size(15.0 * f64::from(pango::SCALE));
            vte_terminal_set_font(raw, font.to_glib_none().0);
            vte_terminal_set_cell_height_scale(raw, 1.08);
            let rgba = |hex: &str| gdk::RGBA::parse(hex).unwrap();
            let fg = rgba("#d3d9e2");
            let bg = rgba("#17191d");
            // 让 ls 的蓝字绿底和白字蓝底都清晰可读。普通目录用粗体蓝，仍显示为亮蓝。
            vte_terminal_set_bold_is_bright(raw, 1);
            let palette: Vec<_> = [
                "#1e2126", "#e69a9a", "#e3ffd9", "#ddc28d", "#4072b2", "#c6a7d9", "#91c6cb",
                "#f4f7fb", "#a2aab7", "#f2aeaa", "#b7d6ac", "#eed6a1", "#b5d0f1", "#dbbde7",
                "#afe0db", "#ffffff",
            ]
            .iter()
            .map(|hex| *rgba(hex).to_glib_none().0)
            .collect();
            vte_terminal_set_colors(
                raw,
                fg.to_glib_none().0,
                bg.to_glib_none().0,
                palette.as_ptr(),
                palette.len(),
            );
            vte_terminal_set_cursor_blink_mode(raw, 2);
            vte_terminal_set_cursor_shape(raw, 1);
            vte_terminal_set_allow_hyperlink(raw, 0);
            vte_terminal_search_set_wrap_around(raw, 1);
            terminal.widget.set_hexpand(true);
            terminal.widget.set_vexpand(true);
            terminal
        }
    }
    fn raw(&self) -> *mut c_void {
        self.widget.as_ptr().cast()
    }
    pub fn attach(&self, fd: OwnedFd) -> Result<(), String> {
        if !self.alive.get() {
            return Err("终端已关闭".into());
        }
        unsafe {
            let mut error = ptr::null_mut();
            let pty = vte_pty_new_foreign_sync(fd.into_raw_fd(), ptr::null_mut(), &mut error);
            if pty.is_null() {
                return Err(take_error(error));
            }
            vte_terminal_set_pty(self.raw(), pty);
            glib::gobject_ffi::g_object_unref(pty.cast());
            Ok(())
        }
    }
    pub fn dispose(&self) {
        // Ubuntu 24's VTE 0.76 dereferences a null pty in the public setter.
        // Widget destruction releases the PTY through VTE's internal path.
        // No terminal API is queried after destruction.
        if self.alive.replace(false) {
            unsafe {
                self.widget.destroy();
            }
        }
    }
    pub fn disable_input(&self) {
        if self.alive.get() {
            unsafe {
                vte_terminal_set_input_enabled(self.raw(), 0);
            }
        }
    }
    pub fn message(&self, text: &str) {
        if !self.alive.get() {
            return;
        }
        unsafe {
            vte_terminal_feed(self.raw(), text.as_ptr(), text.len() as isize);
        }
    }
    pub fn insert_snippet(&self, text: &str) -> bool {
        if !self.alive.get() || ssh_core::validate_snippet_text(text).is_err() {
            return false;
        }
        self.paste(text);
        true
    }

    pub fn paste(&self, text: &str) {
        if !self.alive.get() {
            return;
        }
        if let Ok(text) = CString::new(text) {
            unsafe {
                vte_terminal_paste_text(self.raw(), text.as_ptr());
            }
        }
    }
    pub fn copy(&self) {
        if self.alive.get() {
            unsafe {
                vte_terminal_copy_clipboard_format(self.raw(), 1);
            }
        }
    }
    pub fn find(&self, text: &str, previous: bool) -> bool {
        if !self.alive.get() {
            return false;
        }
        unsafe {
            // VTE retains the compiled regex. Reuse it for next/previous so the
            // search position advances and repeated navigation allocates nothing.
            if self.search_query.borrow().as_str() != text {
                let mut escaped = String::with_capacity(text.len());
                for character in text.chars() {
                    if ".+*?()|[]{}^$\\".contains(character) {
                        escaped.push('\\');
                    }
                    escaped.push(character);
                }
                let Ok(pattern) = CString::new(escaped) else {
                    return false;
                };
                let mut error = ptr::null_mut();
                // PCRE2_MULTILINE, UTF and UCP: Unicode literal text, no user regex.
                let regex = if text.is_empty() {
                    ptr::null_mut()
                } else {
                    vte_regex_new_for_search(
                        pattern.as_ptr(),
                        -1,
                        0x0000_0400 | 0x0008_0000 | 0x0002_0000,
                        &mut error,
                    )
                };
                if !error.is_null() {
                    let _ = take_error(error);
                    return false;
                }
                vte_terminal_search_set_regex(self.raw(), regex, 0);
                if !regex.is_null() {
                    vte_regex_unref(regex);
                }
                *self.search_query.borrow_mut() = text.to_string();
            }
            if text.is_empty() {
                return true;
            }
            (if previous {
                vte_terminal_search_find_previous(self.raw())
            } else {
                vte_terminal_search_find_next(self.raw())
            }) != 0
        }
    }
    #[cfg(feature = "native-probe")]
    pub fn text(&self) -> String {
        self.recent_text(3100)
    }
    pub fn recent_text(&self, lines: i64) -> String {
        if !self.alive.get() {
            return String::new();
        }
        unsafe {
            let mut column = 0;
            let mut row = 0;
            vte_terminal_get_cursor_position(self.raw(), &mut column, &mut row);
            glib::GString::from_glib_full(vte_terminal_get_text_range(
                self.raw(),
                (row - lines).max(0),
                0,
                row,
                1000,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ))
            .to_string()
        }
    }
    #[cfg(feature = "native-probe")]
    pub fn screen_text(&self) -> String {
        if !self.alive.get() {
            return String::new();
        }
        unsafe {
            let mut column = 0;
            let mut row = 0;
            vte_terminal_get_cursor_position(self.raw(), &mut column, &mut row);
            glib::GString::from_glib_full(vte_terminal_get_text_range(
                self.raw(),
                (row - 3100).max(0),
                0,
                row + 1000,
                1000,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ))
            .to_string()
        }
    }
    #[cfg(feature = "native-probe")]
    pub fn select_all(&self) {
        if self.alive.get() {
            unsafe {
                vte_terminal_select_all(self.raw());
            }
        }
    }
    pub fn send_tab(&self) {
        self.send("\t");
    }

    pub(crate) fn send(&self, text: &str) {
        if !self.alive.get() {
            return;
        }
        unsafe {
            vte_terminal_feed_child(self.raw(), text.as_ptr(), text.len() as isize);
        }
    }
}

unsafe fn take_error(error: *mut glib::ffi::GError) -> String {
    if error.is_null() {
        "原生终端初始化失败".into()
    } else {
        let error: glib::Error = from_glib_full(error);
        error.to_string()
    }
}
