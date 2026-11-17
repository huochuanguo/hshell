//! Scoped synthetic GDK events, through GTK dispatch without global keyboard state.
use gtk::{gdk, glib::translate::*, prelude::*};

pub fn key(widget: &gtk::Widget, keyval: u32, state: gdk::ModifierType) -> Result<(), String> {
    let window = widget.window().ok_or("Key target is unrealized")?;
    let seat = widget.display().default_seat().ok_or("No input seat")?;
    let keymap = gdk::Keymap::for_display(&widget.display()).ok_or("No keymap")?;
    let mapping = keymap
        .entries_for_keyval(keyval)
        .into_iter()
        .next()
        .ok_or("Key is unmapped")?;
    for kind in [gdk::EventType::KeyPress, gdk::EventType::KeyRelease] {
        let mut event = gdk::Event::new(kind);
        event.set_device(seat.keyboard().as_ref());
        unsafe {
            let pointer: *mut gdk::ffi::GdkEvent = event.to_glib_none_mut().0;
            let raw = &mut (*pointer).key;
            raw.window = window.to_glib_full();
            raw.send_event = 1;
            raw.time = 0;
            raw.keyval = keyval;
            raw.state = state.into_glib();
            raw.hardware_keycode = mapping.keycode() as u16;
            raw.group = mapping.group() as u8;
        }
        gtk::main_do_event(&mut event);
    }
    Ok(())
}
pub fn scroll(widget: &gtk::Widget, down: bool) -> Result<(), String> {
    let window = widget.window().ok_or("Scroll target is unrealized")?;
    let seat = widget.display().default_seat().ok_or("No input seat")?;
    let mut event = gdk::Event::new(gdk::EventType::Scroll);
    event.set_device(seat.pointer().as_ref());
    unsafe {
        let pointer: *mut gdk::ffi::GdkEvent = event.to_glib_none_mut().0;
        let raw = &mut (*pointer).scroll;
        raw.window = window.to_glib_full();
        raw.send_event = 1;
        raw.time = 0;
        raw.x = 20.0;
        raw.y = 20.0;
        raw.direction = if down {
            gdk::ffi::GDK_SCROLL_DOWN
        } else {
            gdk::ffi::GDK_SCROLL_UP
        };
    }
    gtk::main_do_event(&mut event);
    Ok(())
}
