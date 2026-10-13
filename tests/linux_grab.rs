#![cfg(target_os = "linux")]

use rdev::{EventType, Key};
use std::{ptr, sync::mpsc, time::Duration};
use x11::xlib;

#[test]
#[ignore = "Requires an isolated X11 server with RECORD disabled"]
fn ordinary_keys_work_without_record() {
    unsafe { xlib::XInitThreads() };
    let display = unsafe { xlib::XOpenDisplay(ptr::null()) };
    assert!(!display.is_null());
    let (mut opcode, mut event, mut error) = (0, 0, 0);
    assert_eq!(
        unsafe {
            xlib::XQueryExtension(
                display,
                b"RECORD\0".as_ptr().cast(),
                &mut opcode,
                &mut event,
                &mut error,
            )
        },
        0
    );
    let (sender, receiver) = mpsc::channel();
    rdev::start_grab_listen(move |event| {
        sender.send(event.event_type).unwrap();
        None
    })
    .unwrap();
    rdev::enable_grab();
    for (kind, expected) in [
        (xlib::KeyPress, EventType::KeyPress(Key::KeyA)),
        (xlib::KeyRelease, EventType::KeyRelease(Key::KeyA)),
    ] {
        unsafe { send_key(display, kind) };
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            expected
        );
    }
    rdev::exit_grab_listen();
    unsafe { xlib::XCloseDisplay(display) };
}

unsafe fn send_key(display: *mut xlib::Display, kind: i32) {
    const A_KEYCODE: u32 = 38;
    let root = xlib::XDefaultRootWindow(display);
    let mut event: xlib::XEvent = std::mem::zeroed();
    event.key.type_ = kind;
    event.key.display = display;
    event.key.window = root;
    event.key.root = root;
    event.key.keycode = A_KEYCODE;
    event.key.same_screen = xlib::True;
    let mask = if kind == xlib::KeyPress {
        xlib::KeyPressMask
    } else {
        xlib::KeyReleaseMask
    };
    assert_ne!(
        xlib::XSendEvent(display, root, xlib::False, mask, &mut event),
        0
    );
    xlib::XSync(display, xlib::False);
}
