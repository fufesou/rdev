#![allow(clippy::upper_case_acronyms)]
use crate::keycodes::macos::code_from_key;
use crate::rdev::{EventType, Key, KeyboardState, UnicodeInfo};
use core_foundation::base::{CFRelease, OSStatus};
use core_foundation::string::UniChar;
use core_foundation_sys::data::CFDataGetBytePtr;
use core_graphics::event::CGEventFlags;
use std::convert::TryInto;
use std::ffi::c_void;
use std::os::raw::c_uint;

type TISInputSourceRef = *mut c_void;
type ModifierState = u32;
type UniCharCount = usize;

// UCKeyTranslate expects Carbon modifierKeyState values, encoded as
// cmdKey/shiftKey/alphaLock/optionKey/controlKey shifted right by 8.
pub(crate) const MODIFIER_STATE_NONE: ModifierState = 0;
pub(crate) const MODIFIER_STATE_COMMAND: ModifierState = 1;
pub(crate) const MODIFIER_STATE_SHIFT: ModifierState = 2;
pub(crate) const MODIFIER_STATE_CAPS_LOCK: ModifierState = 4;
pub(crate) const MODIFIER_STATE_ALT: ModifierState = 8;
pub(crate) const MODIFIER_STATE_CONTROL: ModifierState = 16;

type OptionBits = c_uint;
#[allow(non_upper_case_globals)]
const kUCKeyTranslateDeadKeysBit: OptionBits = 1 << 31;
#[allow(non_upper_case_globals)]
const kUCKeyActionDown: u16 = 0;

#[allow(non_upper_case_globals, dead_code)]
const NSEventModifierFlagCapsLock: u64 = 1 << 16;
#[allow(non_upper_case_globals)]
const NSEventModifierFlagShift: u64 = 1 << 17;
#[allow(non_upper_case_globals)]
const NSEventModifierFlagControl: u64 = 1 << 18;
#[allow(non_upper_case_globals)]
const NSEventModifierFlagOption: u64 = 1 << 19;
#[allow(non_upper_case_globals)]
const NSEventModifierFlagCommand: u64 = 1 << 20;
const BUF_LEN: usize = 4;

#[allow(non_upper_case_globals, dead_code)]
const cmdKeyBit: u32 = 8;
#[allow(non_upper_case_globals, dead_code)]
const shiftKeyBit: u32 = 9;
#[allow(non_upper_case_globals, dead_code)]
const alphaLockBit: u32 = 10;
#[allow(non_upper_case_globals, dead_code)]
const optionKeyBit: u32 = 11;
#[allow(non_upper_case_globals, dead_code)]
const controlKeyBit: u32 = 12;

#[allow(non_upper_case_globals, dead_code)]
const cmdKey: u32 = 1 << cmdKeyBit;
#[allow(non_upper_case_globals, dead_code)]
const shiftKey: u32 = 1 << shiftKeyBit;
#[allow(non_upper_case_globals, dead_code)]
const alphaLock: u32 = 1 << alphaLockBit;
#[allow(non_upper_case_globals, dead_code)]
const optionKey: u32 = 1 << optionKeyBit;
#[allow(non_upper_case_globals, dead_code)]
const controlKey: u32 = 1 << controlKeyBit;

#[cfg(target_os = "macos")]
lazy_static::lazy_static! {
    static ref QUEUE: dispatch::Queue = dispatch::Queue::main();
}

#[cfg(target_os = "macos")]
#[link(name = "Cocoa", kind = "framework")]
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> TISInputSourceRef;
    fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> TISInputSourceRef;
    // Actually return CFDataRef which is const here, but for coding convienence, return *mut c_void
    fn TISGetInputSourceProperty(source: TISInputSourceRef, property: *const c_void)
        -> *mut c_void;
    fn UCKeyTranslate(
        layout: *const u8,
        code: u16,
        key_action: u16,
        modifier_state: u32,
        keyboard_type: u32,
        key_translate_options: OptionBits,
        dead_key_state: *mut u32,
        max_length: UniCharCount,
        actual_length: *mut UniCharCount,
        unicode_string: *mut [UniChar; BUF_LEN],
    ) -> OSStatus;
    static kTISPropertyUnicodeKeyLayoutData: *mut c_void;
}

/// Translates a macOS virtual keycode through the active Unicode keyboard layout.
///
/// The `keyboard_type` should be a CoreGraphics keyboard type ID suitable for
/// `UCKeyTranslate`, not a Carbon physical-layout FourCC.
///
/// `dead_state` is read and updated by `UCKeyTranslate`. Pass `0` to start a
/// fresh translation stream, and reuse the same value for consecutive
/// translations on the same logical keyboard stream.
///
/// # Safety
/// This must be called on the main thread because it calls Carbon/TIS APIs that
/// read the active keyboard layout from the current application state. Dispatch
/// to the main queue first when translating from another thread.
pub(crate) unsafe fn unicode_from_code_with_keyboard_type(
    code: u32,
    modifier_state: ModifierState,
    keyboard_type: u32,
    dead_state: &mut u32,
) -> Option<UnicodeInfo> {
    let code = code.try_into().ok()?;
    unsafe {
        let mut keyboard = TISCopyCurrentKeyboardInputSource();
        let mut layout = std::ptr::null_mut();
        if !keyboard.is_null() {
            layout = TISGetInputSourceProperty(keyboard, kTISPropertyUnicodeKeyLayoutData);
        }
        if layout.is_null() {
            if !keyboard.is_null() {
                CFRelease(keyboard);
            }
            // https://github.com/microsoft/vscode/issues/23833
            keyboard = TISCopyCurrentKeyboardLayoutInputSource();
            if !keyboard.is_null() {
                layout = TISGetInputSourceProperty(keyboard, kTISPropertyUnicodeKeyLayoutData);
            }
        }
        if layout.is_null() {
            if !keyboard.is_null() {
                CFRelease(keyboard);
            }
            keyboard = TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
            if !keyboard.is_null() {
                layout = TISGetInputSourceProperty(keyboard, kTISPropertyUnicodeKeyLayoutData);
            }
        }
        if layout.is_null() {
            if !keyboard.is_null() {
                CFRelease(keyboard);
            }
            return None;
        }
        let layout_ptr = CFDataGetBytePtr(layout as _);
        if layout_ptr.is_null() {
            if !keyboard.is_null() {
                CFRelease(keyboard);
            }
            return None;
        }

        let mut buff = [0_u16; BUF_LEN];
        let mut length = 0;
        let _retval = UCKeyTranslate(
            layout_ptr,
            code,
            kUCKeyActionDown,
            modifier_state,
            keyboard_type,
            kUCKeyTranslateDeadKeysBit,
            dead_state,
            BUF_LEN,
            &mut length,
            &mut buff,
        );
        if !keyboard.is_null() {
            CFRelease(keyboard);
        }
        if length == 0 {
            return if *dead_state != 0 {
                Some(UnicodeInfo {
                    name: None,
                    unicode: Vec::new(),
                    is_dead: true,
                })
            } else {
                None
            };
        }

        if length == 1 {
            match String::from_utf16(&buff[..length]) {
                Ok(s) => {
                    if let Some(c) = s.chars().next() {
                        if ('\u{1}'..='\u{1f}').contains(&c) {
                            return None;
                        }
                    }
                }
                Err(_) => {}
            }
        }

        let unicode = buff[..length].to_vec();
        Some(UnicodeInfo {
            name: String::from_utf16(&unicode).ok(),
            unicode,
            is_dead: false,
        })
    }
}

pub struct Keyboard {
    is_main_thread: bool,
    dead_state: u32,
    shift: bool,
    alt: bool, // options
    caps_lock: bool,
}

impl Keyboard {
    pub fn new() -> Option<Keyboard> {
        Some(Keyboard {
            is_main_thread: true,
            dead_state: 0,
            shift: false,
            alt: false,
            caps_lock: false,
        })
    }

    pub fn set_is_main_thread(&mut self, b: bool) {
        self.is_main_thread = b;
    }

    fn modifier_state(&self) -> ModifierState {
        let mut modifier_state = MODIFIER_STATE_NONE;
        if self.shift {
            modifier_state |= MODIFIER_STATE_SHIFT;
        }
        if self.caps_lock {
            modifier_state |= MODIFIER_STATE_CAPS_LOCK;
        }
        if self.alt {
            modifier_state |= MODIFIER_STATE_ALT;
        }
        modifier_state
    }

    #[allow(dead_code)]
    #[inline]
    pub(crate) unsafe fn create_unicode_for_key(
        &mut self,
        code: u32,
        flags: CGEventFlags,
    ) -> Option<UnicodeInfo> {
        let flags_bits = flags.bits();
        if flags_bits & NSEventModifierFlagCommand != 0
            || flags_bits & NSEventModifierFlagControl != 0
        {
            return None;
        }

        let modifier_state = flags_to_state(flags_bits);

        if self.is_main_thread {
            self.unicode_from_code(code, modifier_state)
        } else {
            QUEUE.exec_sync(move || self.unicode_from_code(code, modifier_state))
        }
    }

    #[inline]
    unsafe fn unicode_from_code(
        &mut self,
        code: u32,
        modifier_state: ModifierState,
    ) -> Option<UnicodeInfo> {
        unsafe {
            unicode_from_code_with_keyboard_type(
                code,
                modifier_state,
                super::common::LMGetKbdType() as u32,
                &mut self.dead_state,
            )
        }
    }

    pub fn is_dead(&self) -> bool {
        self.dead_state != 0
    }
}

impl KeyboardState for Keyboard {
    fn add(&mut self, event_type: &EventType) -> Option<UnicodeInfo> {
        match event_type {
            EventType::KeyPress(key) => match key {
                Key::ShiftLeft | Key::ShiftRight => {
                    self.shift = true;
                    None
                }
                Key::Alt | Key::AltGr => {
                    self.alt = true;
                    None
                }
                Key::CapsLock => {
                    self.caps_lock = !self.caps_lock;
                    None
                }
                key => {
                    let code = code_from_key(*key)?;
                    unsafe { self.unicode_from_code(code.into(), self.modifier_state()) }
                }
            },
            EventType::KeyRelease(key) => match key {
                Key::ShiftLeft | Key::ShiftRight => {
                    self.shift = false;
                    None
                }
                Key::Alt | Key::AltGr => {
                    self.alt = false;
                    None
                }
                _ => None,
            },
            _ => None,
        }
    }
}

#[allow(clippy::identity_op)]
pub unsafe fn flags_to_state(flags: u64) -> ModifierState {
    let has_alt = flags & NSEventModifierFlagOption;
    let has_caps_lock = flags & NSEventModifierFlagCapsLock;
    let has_control = flags & NSEventModifierFlagControl;
    let has_shift = flags & NSEventModifierFlagShift;
    let has_meta = flags & NSEventModifierFlagCommand;
    let mut modifier = 0;
    if has_alt != 0 {
        modifier |= optionKey;
    }
    if has_caps_lock != 0 {
        modifier |= alphaLock;
    }
    if has_control != 0 {
        modifier |= controlKey
    }
    if has_shift != 0 {
        modifier |= shiftKey;
    }
    if has_meta != 0 {
        modifier |= cmdKey;
    }
    (modifier >> 8) & 0xFF
}
