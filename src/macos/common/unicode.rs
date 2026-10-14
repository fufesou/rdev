use crate::{
    keycodes::macos::virtual_keycodes::kVK_ANSI_A,
    rdev::{Event, EventType, Key, UnicodeInfo},
};
use core_foundation::{
    base::{CFAllocatorRef, CFType, CFTypeRef, TCFType},
    data::{CFData, CFDataRef},
};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventRef, CGEventType, EventField};
use std::{ptr, time::SystemTime};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventKeyboardGetUnicodeString(
        event: CFTypeRef,
        max_length: usize,
        actual_length: *mut usize,
        unicode: *mut u16,
    );
    fn CGEventKeyboardSetUnicodeString(event: CFTypeRef, length: usize, unicode: *const u16);
    fn CGEventCreateCopy(event: CFTypeRef) -> CFTypeRef;
    fn CGEventCreateData(allocator: CFAllocatorRef, event: CFTypeRef) -> CFDataRef;
}

pub(super) fn convert(
    event_type: CGEventType,
    event: &CGEvent,
) -> Result<Option<Event>, &'static str> {
    let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
    let shortcuts = CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagCommand;
    if code != i64::from(kVK_ANSI_A)
        || event.get_integer_value_field(EventField::EVENT_SOURCE_UNIX_PROCESS_ID) == 0
        || event.get_flags().intersects(shortcuts)
    {
        return Ok(None);
    }
    let key = Key::Unknown(code as u32);
    let event_type = match event_type {
        CGEventType::KeyDown => EventType::KeyPress(key),
        CGEventType::KeyUp => EventType::KeyRelease(key),
        _ => return Ok(None),
    };
    let Some(unicode) = read_unicode(event) else {
        return Ok(None);
    };
    if !has_explicit_unicode(event, &unicode)? {
        return Ok(None);
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
    Ok(Some(Event {
        event_type,
        time: SystemTime::now(),
        unicode,
        platform_code: code as u32,
        position_code: 0,
        usb_hid: 0,
        extra_data: event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA),
    }))
}

fn has_explicit_unicode(event: &CGEvent, unicode: &[u16]) -> Result<bool, &'static str> {
    let event = (&**event as *const CGEventRef).cast();
    let copy = unsafe { CGEventCreateCopy(event) };
    if copy.is_null() {
        return Err("Cannot copy macOS keyboard event");
    }
    let copy = unsafe { CFType::wrap_under_create_rule(copy) };
    // The getter also returns layout-derived text; the setter explicitly overrides it:
    // https://developer.apple.com/documentation/coregraphics/cgevent/keyboardsetunicodestring(stringlength:unicodestring:)
    // Setting it on a copy changes layout-derived data but preserves an explicit
    // override in the tested CoreGraphics implementation. This is an observed
    // distinction, not a documented serialization guarantee; recheck both ordinary
    // synthetic A and explicit Unicode when validating another macOS release.
    // Compare opaque data, never private byte offsets:
    // https://developer.apple.com/documentation/coregraphics/cgeventcreatedata
    unsafe {
        CGEventKeyboardSetUnicodeString(copy.as_CFTypeRef(), unicode.len(), unicode.as_ptr())
    };
    Ok(event_data(event)? == event_data(copy.as_CFTypeRef())?)
}

fn event_data(event: CFTypeRef) -> Result<CFData, &'static str> {
    let data = unsafe { CGEventCreateData(ptr::null(), event) };
    if data.is_null() {
        return Err("Cannot serialize macOS keyboard event");
    }
    Ok(unsafe { CFData::wrap_under_create_rule(data) })
}

fn read_unicode(event: &CGEvent) -> Option<Vec<u16>> {
    let event = (&**event as *const CGEventRef).cast();
    let mut length = 0;
    unsafe { CGEventKeyboardGetUnicodeString(event, 0, &mut length, ptr::null_mut()) };
    if length == 0 {
        return None;
    }
    let mut unicode = vec![0; length];
    unsafe { CGEventKeyboardGetUnicodeString(event, length, &mut length, unicode.as_mut_ptr()) };
    unicode.truncate(length);
    Some(unicode)
}
