use crate::{
    keycodes::macos::virtual_keycodes::kVK_ANSI_A,
    macos::keyboard::Keyboard,
    rdev::{Event, EventType, Key, UnicodeInfo},
};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventRef, CGEventType, EventField};
use std::{ptr::null_mut, time::SystemTime};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventKeyboardGetUnicodeString(
        event: *const CGEventRef,
        max_length: usize,
        actual_length: *mut usize,
        unicode: *mut u16,
    );
}

pub(super) fn convert(
    event_type: CGEventType,
    event: &CGEvent,
    keyboard: &Keyboard,
) -> Option<Event> {
    let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
    let shortcuts = CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagCommand;
    if code != i64::from(kVK_ANSI_A)
        || event.get_integer_value_field(EventField::EVENT_SOURCE_UNIX_PROCESS_ID) == 0
        || event.get_flags().intersects(shortcuts)
    {
        return None;
    }
    let key = Key::Unknown(code as u32);
    let event_type = match event_type {
        CGEventType::KeyDown => EventType::KeyPress(key),
        CGEventType::KeyUp => EventType::KeyRelease(key),
        _ => return None,
    };
    let unicode = read_unicode(event)?;
    // Quartz also supplies Unicode for ordinary synthetic keys. Keep their key semantics
    // when the payload agrees with the layout, without advancing the live dead-key state.
    let physical = unsafe {
        keyboard
            .clone()
            .create_unicode_for_key(code as u32, event.get_flags())
    };
    if physical.is_some_and(|physical| physical.unicode == unicode) {
        return None;
    }
    let unicode = if matches!(event_type, EventType::KeyPress(_)) {
        Some(UnicodeInfo {
            name: String::from_utf16(&unicode).ok(),
            unicode,
            is_dead: false,
        })
    } else {
        None
    };
    Some(Event {
        event_type,
        time: SystemTime::now(),
        unicode,
        platform_code: code as u32,
        position_code: 0,
        usb_hid: 0,
        extra_data: event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA),
    })
}

fn read_unicode(event: &CGEvent) -> Option<Vec<u16>> {
    let mut length = 0;
    unsafe { CGEventKeyboardGetUnicodeString(&**event, 0, &mut length, null_mut()) };
    if length == 0 {
        return None;
    }
    let mut unicode = vec![0; length];
    unsafe { CGEventKeyboardGetUnicodeString(&**event, length, &mut length, unicode.as_mut_ptr()) };
    unicode.truncate(length);
    Some(unicode)
}
