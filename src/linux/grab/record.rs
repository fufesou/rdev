//! Keep temporary mapping changes and grabbed keys in one RECORD stream; querying
//! the live keymap later can see KeePassXC's already-restored mapping instead.
//! RECORD connection, client-selection, and callback contracts:
//! https://xorg.freedesktop.org/archive/current/doc/libXtst/recordlib.html

mod mapping;

use super::{GrabEvent, GRAB_KEY_EVENT_SENDER};
use crate::GrabError;
use mapping::{Mapping, CHANGE_KEYBOARD_MAPPING, XKB_SET_MAP};
use std::{convert::TryInto, io, mem::zeroed, os::raw::c_char, ptr, slice};
use x11::{xlib, xrecord};

pub(super) fn is_supported(display: *mut xlib::Display) -> bool {
    let (mut opcode, mut event, mut error) = (0, 0, 0);
    unsafe {
        xlib::XQueryExtension(
            display,
            b"RECORD\0".as_ptr().cast(),
            &mut opcode,
            &mut event,
            &mut error,
        ) != 0
    }
}

pub(super) struct RecordedKeyboard {
    control: *mut xlib::Display,
    data: *mut xlib::Display,
    context: xrecord::XRecordContext,
    state: Box<RecordState>,
}

#[derive(Default)]
struct RecordState {
    mapping: Mapping,
    error: Option<io::Error>,
}

impl RecordedKeyboard {
    pub(super) unsafe fn new(display: *mut xlib::Display) -> Result<Self, GrabError> {
        let mut record = Self {
            control: ptr::null_mut(),
            data: ptr::null_mut(),
            context: 0,
            state: Box::default(),
        };
        record.control = xlib::XOpenDisplay(ptr::null());
        record.data = xlib::XOpenDisplay(ptr::null());
        if record.control.is_null() || record.data.is_null() {
            return Err(GrabError::MissingDisplayError);
        }
        record.requests().map_err(GrabError::IoError)?;
        record.events(display).map_err(GrabError::IoError)?;
        let state = &mut *record.state as *mut RecordState;
        let enabled = xrecord::XRecordEnableContextAsync(
            record.data,
            record.context,
            Some(receive),
            state.cast(),
        );
        if enabled == 0 {
            return Err(GrabError::IoError(record_error(
                "Cannot enable X11 keyboard recording",
            )));
        }
        Ok(record)
    }

    unsafe fn requests(&mut self) -> io::Result<()> {
        let (mut opcode, mut event, mut error) = (0, 0, 0);
        if xlib::XQueryExtension(
            self.control,
            b"XKEYBOARD\0".as_ptr().cast(),
            &mut opcode,
            &mut event,
            &mut error,
        ) == 0
        {
            return Err(record_error("XKEYBOARD extension is unavailable"));
        }
        self.state.mapping.xkb_opcode = opcode
            .try_into()
            .map_err(|_| record_error("Invalid XKEYBOARD opcode"))?;
        let mut range: xrecord::XRecordRange = zeroed();
        range.core_requests.first = CHANGE_KEYBOARD_MAPPING;
        range.core_requests.last = CHANGE_KEYBOARD_MAPPING;
        range.ext_requests.ext_major.first = self.state.mapping.xkb_opcode;
        range.ext_requests.ext_major.last = self.state.mapping.xkb_opcode;
        range.ext_requests.ext_minor.first = XKB_SET_MAP;
        range.ext_requests.ext_minor.last = XKB_SET_MAP;
        let mut clients = xrecord::XRecordAllClients;
        self.context = xrecord::XRecordCreateContext(
            self.control,
            0,
            &mut clients,
            1,
            &mut &mut range as *mut &mut _ as _,
            1,
        );
        if self.context == 0 {
            return Err(record_error("Cannot create X11 keyboard recording context"));
        }
        Ok(())
    }

    unsafe fn events(&self, display: *mut xlib::Display) -> io::Result<()> {
        // RECORD accepts a client's resource ID. Use one from the grab connection
        // to record delivered keys only for that client, not global device events.
        let mut client = xlib::XCreateSimpleWindow(
            display,
            xlib::XDefaultRootWindow(display),
            0,
            0,
            1,
            1,
            0,
            0,
            0,
        );
        xlib::XSync(display, xlib::False);
        let mut range: xrecord::XRecordRange = zeroed();
        range.delivered_events.first = xlib::KeyPress as u8;
        range.delivered_events.last = xlib::KeyRelease as u8;
        let registered = xrecord::XRecordRegisterClients(
            self.control,
            self.context,
            0,
            &mut client,
            1,
            &mut &mut range as *mut &mut _ as _,
            1,
        );
        // Registration on the control connection must finish while the resource exists.
        xlib::XSync(self.control, xlib::False);
        xlib::XDestroyWindow(display, client);
        if registered == 0 {
            return Err(record_error("Cannot record grabbed keyboard events"));
        }
        Ok(())
    }

    pub(super) fn fd(&self) -> i32 {
        unsafe { xlib::XConnectionNumber(self.data) }
    }

    pub(super) fn process(&mut self) -> Result<(), GrabError> {
        unsafe { xrecord::XRecordProcessReplies(self.data) };
        match self.state.error.take() {
            Some(error) => Err(GrabError::IoError(error)),
            None => Ok(()),
        }
    }
}

impl Drop for RecordedKeyboard {
    fn drop(&mut self) {
        unsafe {
            if self.context != 0 {
                xrecord::XRecordDisableContext(self.control, self.context);
                xrecord::XRecordFreeContext(self.control, self.context);
                xlib::XSync(self.control, xlib::False);
            }
            for display in [self.data, self.control] {
                if !display.is_null() {
                    xlib::XCloseDisplay(display);
                }
            }
        }
    }
}

impl RecordState {
    unsafe fn receive(&mut self, raw: &xrecord::XRecordInterceptData) -> io::Result<()> {
        if raw.category != xrecord::XRecordFromClient && raw.category != xrecord::XRecordFromServer
        {
            return Ok(());
        }
        const RECORD_UNIT_BYTES: usize = 4;
        let length = (raw.data_len as usize)
            .checked_mul(RECORD_UNIT_BYTES)
            .filter(|length| *length <= isize::MAX as usize)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Invalid keyboard record length")
            })?;
        let data = slice::from_raw_parts(raw.data, length);
        if raw.category == xrecord::XRecordFromClient {
            // RECORD sees requests before server validation; keep the previous map on error.
            if let Err(error) = self.mapping.update(data, raw.client_swapped != 0) {
                log::trace!("Discarding invalid X11 keyboard mapping request: {error}");
            }
            return Ok(());
        }
        let event = self.mapping.event(data, raw.client_swapped != 0)?;
        let sender = GRAB_KEY_EVENT_SENDER.lock().unwrap();
        let sender = sender
            .as_ref()
            .ok_or_else(|| record_error("Keyboard callback is unavailable"))?;
        sender
            .send(GrabEvent::KeyEvent(event))
            .map_err(|error| record_error(&error.to_string()))
    }
}

unsafe extern "C" fn receive(state: *mut c_char, raw: *mut xrecord::XRecordInterceptData) {
    let state = &mut *state.cast::<RecordState>();
    if state.error.is_none() {
        state.error = state.receive(&*raw).err();
    }
    xrecord::XRecordFreeData(raw);
}

fn record_error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc::channel,
        time::{Duration, Instant},
    };
    use x11::xtest;

    #[test]
    #[ignore = "Requires an isolated X11 server"]
    fn autotype_text_survives_immediate_mapping_reset() {
        unsafe { xlib::XInitThreads() };
        let grabber = super::super::KeyboardGrabber::create().unwrap();
        let (sender, receiver) = channel();
        *GRAB_KEY_EVENT_SENDER.lock().unwrap() = Some(sender);
        unsafe { super::super::KEYBOARD = crate::Keyboard::new() };
        let mut record = unsafe { RecordedKeyboard::new(grabber.display) }.unwrap();
        let text = "汉😀🦀".repeat(20);
        let expected = format!("{text}A");
        unsafe { produce(grabber.display, &text) };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut received = Vec::new();
        while received.len() < expected.encode_utf16().count() && Instant::now() < deadline {
            record.process().unwrap();
            for event in receiver.try_iter() {
                let GrabEvent::KeyEvent(event) = event else {
                    continue;
                };
                let Some(unicode) = event.unicode else {
                    continue;
                };
                received.extend(if unicode.unicode.is_empty() {
                    unicode.name.unwrap_or_default().encode_utf16().collect()
                } else {
                    unicode.unicode
                });
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(String::from_utf16(&received).unwrap(), expected);
    }

    unsafe fn produce(display: *mut xlib::Display, text: &str) {
        const AUTO_TYPE_KEYCODE: u32 = 248;
        const UNICODE_KEYSYM: u32 = 0x01000000;
        assert_eq!(
            xlib::XGrabKeyboard(
                display,
                xlib::XDefaultRootWindow(display),
                xlib::False,
                xlib::GrabModeAsync,
                xlib::GrabModeAsync,
                xlib::CurrentTime
            ),
            xlib::GrabSuccess
        );
        xlib::XSync(display, xlib::False);
        let producer = xlib::XOpenDisplay(ptr::null());
        assert!(!producer.is_null());
        for unit in text.encode_utf16() {
            let mut symbol = (UNICODE_KEYSYM | u32::from(unit)) as xlib::KeySym;
            xlib::XChangeKeyboardMapping(producer, AUTO_TYPE_KEYCODE as i32, 1, &mut symbol, 1);
            xtest::XTestFakeKeyEvent(producer, AUTO_TYPE_KEYCODE, xlib::True, xlib::CurrentTime);
            xtest::XTestFakeKeyEvent(producer, AUTO_TYPE_KEYCODE, xlib::False, xlib::CurrentTime);
            symbol = 0;
            xlib::XChangeKeyboardMapping(producer, AUTO_TYPE_KEYCODE as i32, 1, &mut symbol, 1);
            xlib::XSync(producer, xlib::False);
        }
        const SHIFT_KEYCODE: u32 = 50;
        const A_KEYCODE: u32 = 38;
        xtest::XTestFakeKeyEvent(producer, SHIFT_KEYCODE, xlib::True, xlib::CurrentTime);
        xtest::XTestFakeKeyEvent(producer, A_KEYCODE, xlib::True, xlib::CurrentTime);
        xtest::XTestFakeKeyEvent(producer, A_KEYCODE, xlib::False, xlib::CurrentTime);
        xtest::XTestFakeKeyEvent(producer, SHIFT_KEYCODE, xlib::False, xlib::CurrentTime);
        xlib::XSync(producer, xlib::False);
        xlib::XCloseDisplay(producer);
    }
}
