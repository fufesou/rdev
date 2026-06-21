use crate::keycodes::macos::{code_from_key, virtual_keycodes::*};
use crate::macos::common::CGEventSourceKeyState;
use crate::macos::keyboard::{
    unicode_from_code_with_keyboard_type, MODIFIER_STATE_ALT, MODIFIER_STATE_CAPS_LOCK,
    MODIFIER_STATE_COMMAND, MODIFIER_STATE_CONTROL, MODIFIER_STATE_NONE, MODIFIER_STATE_SHIFT,
};
use crate::rdev::{Button, EventType, RawKey, SimulateError};
use crate::MacKeyboardType;
use core_graphics::{
    event::{
        CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton,
        EventField, ScrollEventUnit,
    },
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::CGPoint,
    sys::CGEventSourceRef,
};
use foreign_types::ForeignType;
use std::cell::Cell;
use std::convert::TryInto;
use std::sync::atomic::{AtomicI64, Ordering};

static MOUSE_EXTRA_INFO: AtomicI64 = AtomicI64::new(0);
static KEYBOARD_EXTRA_INFO: AtomicI64 = AtomicI64::new(0);
type CGEventSourceKeyboardType = u32;
type ModifierKeyMask = u8;
// Apple defines these compatibility IDs in CarbonCore/Gestalt.h as
// gestaltThirdPartyANSIKbd/ISOKbd/JISKbd. They are the keyboard type IDs used
// by UCKeyTranslate and CGEventSourceSetKeyboardType, not HIToolbox Keyboards.h
// PhysicalKeyboardLayoutType FourCC values such as 'ANSI', 'ISO ', or 'JIS '.
const ANSI_KEYBOARD_TYPE: CGEventSourceKeyboardType = 40;
const ISO_KEYBOARD_TYPE: CGEventSourceKeyboardType = 41;
const JIS_KEYBOARD_TYPE: CGEventSourceKeyboardType = 42;
const MODIFIER_KEY_NONE: ModifierKeyMask = 0;
const MODIFIER_KEY_LEFT: ModifierKeyMask = 1 << 0;
const MODIFIER_KEY_RIGHT: ModifierKeyMask = 1 << 1;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceGetKeyboardType(source: CGEventSourceRef) -> CGEventSourceKeyboardType;
    fn CGEventSourceSetKeyboardType(
        source: CGEventSourceRef,
        keyboard_type: CGEventSourceKeyboardType,
    );
}

#[derive(Copy, Clone)]
struct KeyboardEventOptions {
    keydown: bool,
    keyboard_type: CGEventSourceKeyboardType,
}

#[derive(Copy, Clone)]
enum ModifierKey {
    Shift(ModifierSide),
    Alt(ModifierSide),
    CapsLock,
    Control(ModifierSide),
    Command(ModifierSide),
}

#[derive(Copy, Clone)]
enum ModifierSide {
    Left,
    Right,
}

fn keyboard_type_value(keyboard_type: MacKeyboardType) -> CGEventSourceKeyboardType {
    match keyboard_type {
        MacKeyboardType::Current => current_keyboard_type(),
        MacKeyboardType::Ansi => ANSI_KEYBOARD_TYPE,
        MacKeyboardType::Iso => ISO_KEYBOARD_TYPE,
        MacKeyboardType::Jis => JIS_KEYBOARD_TYPE,
        MacKeyboardType::Raw(keyboard_type) => keyboard_type,
    }
}

/// Sets the macOS user-data tag used for simulated mouse and wheel events.
///
/// Keyboard events use `set_keyboard_extra_info()` instead.
pub fn set_mouse_extra_info(extra: i64) {
    MOUSE_EXTRA_INFO.store(extra, Ordering::Relaxed);
}

/// Sets the macOS user-data tag used for simulated keyboard events.
///
/// This is separate from `set_mouse_extra_info()`.
pub fn set_keyboard_extra_info(extra: i64) {
    KEYBOARD_EXTRA_INFO.store(extra, Ordering::Relaxed);
}

#[allow(non_upper_case_globals)]
fn workaround_fn(event: CGEvent, keycode: CGKeyCode) -> CGEvent {
    match keycode {
        // https://github.com/rustdesk/rustdesk/issues/10126
        // https://stackoverflow.com/questions/74938870/sticky-fn-after-home-is-simulated-programmatically-macos
        // `kVK_F20` does not stick `CGEventFlags::CGEventFlagSecondaryFn`
        kVK_F1 | kVK_F2 | kVK_F3 | kVK_F4 | kVK_F5 | kVK_F6 | kVK_F7 | kVK_F8 | kVK_F9
        | kVK_F10 | kVK_F11 | kVK_F12 | kVK_F13 | kVK_F14 | kVK_F15 | kVK_F16 | kVK_F17
        | kVK_F18 | kVK_F19 | kVK_ANSI_KeypadClear | kVK_ForwardDelete | kVK_Home
        | kVK_End | kVK_PageDown | kVK_PageUp
        | 129 // Spotlight Search
        | 130 // Application
        | 131 // Launchpad
        | 144 // Brightness Up
        | 145 // Brightness Down
        => {
            let flags = event.get_flags();
            event.set_flags(flags & (!(CGEventFlags::CGEventFlagSecondaryFn)));
        }
        kVK_UpArrow | kVK_DownArrow | kVK_LeftArrow | kVK_RightArrow => {
            let flags = event.get_flags();
            event.set_flags(
                flags
                    & (!(CGEventFlags::CGEventFlagSecondaryFn
                        | CGEventFlags::CGEventFlagNumericPad)),
            );
        }
        kVK_Help => {
            let flags = event.get_flags();
            event.set_flags(
                flags
                    & (!(CGEventFlags::CGEventFlagSecondaryFn
                        | CGEventFlags::CGEventFlagHelp)),
            );
        }
        _ => {}
    }
    event
}

fn current_keyboard_type() -> CGEventSourceKeyboardType {
    unsafe { CGEventSourceGetKeyboardType(std::ptr::null_mut()) }
}

unsafe fn set_keyboard_source_type(
    source: &CGEventSource,
    keyboard_type: CGEventSourceKeyboardType,
) {
    // The source keeps this keyboard type for later events created from it.
    // Callers must ensure the source is not being mutated concurrently.
    CGEventSourceSetKeyboardType(source.as_ptr(), keyboard_type);
}

fn set_keyboard_event_type(event: &CGEvent, keyboard_type: CGEventSourceKeyboardType) {
    event.set_integer_value_field(
        EventField::KEYBOARD_EVENT_KEYBOARD_TYPE,
        keyboard_type as i64,
    );
}

fn keycode_from_key(key: crate::Key) -> Option<CGKeyCode> {
    match key {
        crate::Key::RawKey(RawKey::MacVirtualKeycode(keycode)) => Some(keycode as _),
        crate::Key::RawKey(_) => {
            // Only macOS virtual keycodes can be converted into CG key events.
            None
        }
        _ => code_from_key(key).map(|keycode| keycode as _),
    }
}

fn modifier_key_from_key(key: crate::Key) -> Option<ModifierKey> {
    match key {
        crate::Key::ShiftLeft => Some(ModifierKey::Shift(ModifierSide::Left)),
        crate::Key::ShiftRight => Some(ModifierKey::Shift(ModifierSide::Right)),
        crate::Key::Alt => Some(ModifierKey::Alt(ModifierSide::Left)),
        crate::Key::AltGr => Some(ModifierKey::Alt(ModifierSide::Right)),
        crate::Key::CapsLock => Some(ModifierKey::CapsLock),
        crate::Key::ControlLeft => Some(ModifierKey::Control(ModifierSide::Left)),
        crate::Key::ControlRight => Some(ModifierKey::Control(ModifierSide::Right)),
        crate::Key::MetaLeft => Some(ModifierKey::Command(ModifierSide::Left)),
        crate::Key::MetaRight => Some(ModifierKey::Command(ModifierSide::Right)),
        crate::Key::RawKey(RawKey::MacVirtualKeycode(keycode)) => match keycode {
            code if code == kVK_Shift => Some(ModifierKey::Shift(ModifierSide::Left)),
            code if code == kVK_RightShift => Some(ModifierKey::Shift(ModifierSide::Right)),
            code if code == kVK_Option => Some(ModifierKey::Alt(ModifierSide::Left)),
            code if code == kVK_RightOption => Some(ModifierKey::Alt(ModifierSide::Right)),
            code if code == kVK_CapsLock => Some(ModifierKey::CapsLock),
            code if code == kVK_Control => Some(ModifierKey::Control(ModifierSide::Left)),
            code if code == kVK_RightControl => Some(ModifierKey::Control(ModifierSide::Right)),
            code if code == kVK_Command => Some(ModifierKey::Command(ModifierSide::Left)),
            code if code == kVK_RightCommand => Some(ModifierKey::Command(ModifierSide::Right)),
            _ => None,
        },
        _ => None,
    }
}

fn modifier_key_mask(side: ModifierSide) -> ModifierKeyMask {
    match side {
        ModifierSide::Left => MODIFIER_KEY_LEFT,
        ModifierSide::Right => MODIFIER_KEY_RIGHT,
    }
}

fn set_modifier_key_state(state: &Cell<ModifierKeyMask>, side: ModifierSide, keydown: bool) {
    let mask = modifier_key_mask(side);
    let current = state.get();
    if keydown {
        state.set(current | mask);
    } else {
        state.set(current & !mask);
    }
}

fn modifier_suppresses_unicode(modifier_state: u32) -> bool {
    modifier_state & (MODIFIER_STATE_COMMAND | MODIFIER_STATE_CONTROL) != 0
}

fn should_translate_key_unicode(key: crate::Key, modifier_state: u32) -> bool {
    modifier_key_from_key(key).is_none() && !modifier_suppresses_unicode(modifier_state)
}

fn new_keyboard_event(
    source: CGEventSource,
    keycode: CGKeyCode,
    options: KeyboardEventOptions,
) -> Result<CGEvent, ()> {
    let event = CGEvent::new_keyboard_event(source, keycode, options.keydown)?;
    set_keyboard_event_type(&event, options.keyboard_type);
    Ok(event)
}

fn keyboard_event_from_key(
    source: CGEventSource,
    key: crate::Key,
    options: KeyboardEventOptions,
    modifier_state: Option<u32>,
    dead_state: Option<&Cell<u32>>,
) -> Option<CGEvent> {
    let keycode = keycode_from_key(key)?;
    let event = new_keyboard_event(source, keycode, options).ok()?;
    if options.keydown {
        if let (Some(modifier_state), Some(dead_state)) = (modifier_state, dead_state) {
            if should_translate_key_unicode(key, modifier_state) {
                let mut dead_state_value = dead_state.get();
                let unicode = unsafe {
                    unicode_from_code_with_keyboard_type(
                        keycode as u32,
                        modifier_state,
                        options.keyboard_type,
                        &mut dead_state_value,
                    )
                };
                dead_state.set(dead_state_value);
                if let Some(unicode) = unicode {
                    if !unicode.unicode.is_empty() {
                        event.set_string_from_utf16_unchecked(&unicode.unicode);
                    }
                }
            } else if modifier_key_from_key(key).is_none()
                && modifier_suppresses_unicode(modifier_state)
            {
                dead_state.set(0);
            }
        }
        // KeyPress intentionally skips workaround_fn(); applying it here makes F11 fail.
        return Some(event);
    }
    Some(workaround_fn(event, keycode))
}

fn mouse_button_event(
    source: CGEventSource,
    button: Button,
    event_type: CGEventType,
) -> Option<CGEvent> {
    let point = unsafe { get_current_mouse_location()? };
    match button {
        Button::Left | Button::Right => CGEvent::new_mouse_event(
            source,
            event_type,
            point,
            CGMouseButton::Left, // ignored because we don't use OtherMouse EventType
        )
        .ok(),
        _ => None,
    }
}

fn mouse_move_event(source: CGEventSource, x: f64, y: f64) -> Option<CGEvent> {
    let point = CGPoint { x, y };
    CGEvent::new_mouse_event(source, CGEventType::MouseMoved, point, CGMouseButton::Left).ok()
}

fn wheel_event(source: CGEventSource, delta_x: i64, delta_y: i64) -> Option<CGEvent> {
    let wheel_count = 2;
    CGEvent::new_scroll_event(
        source,
        ScrollEventUnit::PIXEL,
        wheel_count,
        delta_y.try_into().ok()?,
        delta_x.try_into().ok()?,
        0,
    )
    .ok()
}

fn event_source_user_data(event_type: &EventType) -> i64 {
    match event_type {
        EventType::KeyPress(_) | EventType::KeyRelease(_) => {
            KEYBOARD_EXTRA_INFO.load(Ordering::Relaxed)
        }
        EventType::ButtonPress(_)
        | EventType::ButtonRelease(_)
        | EventType::MouseMove { .. }
        | EventType::Wheel { .. } => {
            // Wheel events are pointer events and intentionally use mouse extra info.
            MOUSE_EXTRA_INFO.load(Ordering::Relaxed)
        }
    }
}

fn set_event_source_user_data(event: &CGEvent, event_type: &EventType) {
    event.set_integer_value_field(
        EventField::EVENT_SOURCE_USER_DATA,
        event_source_user_data(event_type),
    );
}

unsafe fn convert_native_with_source(
    event_type: &EventType,
    source: &CGEventSource,
    keyboard_type: CGEventSourceKeyboardType,
    modifier_state: Option<u32>,
    dead_state: Option<&Cell<u32>>,
) -> Option<CGEvent> {
    match event_type {
        EventType::KeyPress(_) | EventType::KeyRelease(_) => {
            // Set both the source and event field to the requested keyboard type:
            // event creation can read the source, while the field is explicit.
            unsafe {
                set_keyboard_source_type(source, keyboard_type);
            }
        }
        _ => {}
    }

    let event = match event_type {
        EventType::KeyPress(key) => keyboard_event_from_key(
            source.clone(),
            *key,
            KeyboardEventOptions {
                keydown: true,
                keyboard_type,
            },
            modifier_state,
            dead_state,
        ),
        EventType::KeyRelease(key) => keyboard_event_from_key(
            source.clone(),
            *key,
            KeyboardEventOptions {
                keydown: false,
                keyboard_type,
            },
            modifier_state,
            dead_state,
        ),
        EventType::ButtonPress(Button::Left) => {
            mouse_button_event(source.clone(), Button::Left, CGEventType::LeftMouseDown)
        }
        EventType::ButtonPress(Button::Right) => {
            mouse_button_event(source.clone(), Button::Right, CGEventType::RightMouseDown)
        }
        EventType::ButtonRelease(Button::Left) => {
            mouse_button_event(source.clone(), Button::Left, CGEventType::LeftMouseUp)
        }
        EventType::ButtonRelease(Button::Right) => {
            mouse_button_event(source.clone(), Button::Right, CGEventType::RightMouseUp)
        }
        EventType::ButtonPress(_) | EventType::ButtonRelease(_) => None,
        EventType::MouseMove { x, y } => mouse_move_event(source.clone(), *x, *y),
        EventType::Wheel { delta_x, delta_y } => wheel_event(source.clone(), *delta_x, *delta_y),
    }?;
    set_event_source_user_data(&event, event_type);
    Some(event)
}

unsafe fn convert_native(event_type: &EventType) -> Option<CGEvent> {
    // https://developer.apple.com/documentation/coregraphics/cgeventsourcestateid#:~:text=kCGEventSourceStatePrivate
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    convert_native_with_source(event_type, &source, current_keyboard_type(), None, None)
}

unsafe fn get_current_mouse_location() -> Option<CGPoint> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    let event = CGEvent::new(source).ok()?;
    Some(event.location())
}

pub fn simulate(event_type: &EventType) -> Result<(), SimulateError> {
    unsafe {
        if let Some(cg_event) = convert_native(event_type) {
            cg_event.post(CGEventTapLocation::HID);
            Ok(())
        } else {
            Err(SimulateError)
        }
    }
}

pub struct VirtualInput {
    source: CGEventSource,
    tap_loc: CGEventTapLocation,
    keyboard_type: MacKeyboardType,
    shift: Cell<ModifierKeyMask>,
    alt: Cell<ModifierKeyMask>,
    caps_lock: Cell<bool>,
    control: Cell<ModifierKeyMask>,
    command: Cell<ModifierKeyMask>,
    dead_state: Cell<u32>,
}

impl VirtualInput {
    pub fn new(state_id: CGEventSourceStateID, tap_loc: CGEventTapLocation) -> Result<Self, ()> {
        let source = CGEventSource::new(state_id)?;

        Ok(Self {
            source,
            tap_loc,
            keyboard_type: MacKeyboardType::Current,
            shift: Cell::new(MODIFIER_KEY_NONE),
            alt: Cell::new(MODIFIER_KEY_NONE),
            caps_lock: Cell::new(false),
            control: Cell::new(MODIFIER_KEY_NONE),
            command: Cell::new(MODIFIER_KEY_NONE),
            dead_state: Cell::new(0),
        })
    }

    /// Sets the hardware keyboard type used for physical keycode translation.
    ///
    /// This keeps using the active macOS input source/layout for character output.
    /// For key events, the underlying event source is updated when events are
    /// sent through this `VirtualInput`.
    /// The selected type is fixed for this `VirtualInput`; create another
    /// instance to use a different keyboard type for a separate input stream.
    /// `MacKeyboardType::Current` stores the selection, but `simulate()` resolves
    /// it through `keyboard_type_value()` for each event.
    /// Do not share the same `CGEventSource` with code that expects a different
    /// keyboard type.
    pub fn with_keyboard_type(mut self, keyboard_type: MacKeyboardType) -> Self {
        self.keyboard_type = keyboard_type;
        self
    }

    fn modifier_state(&self) -> u32 {
        let mut modifier_state = MODIFIER_STATE_NONE;
        if self.command.get() != MODIFIER_KEY_NONE {
            modifier_state |= MODIFIER_STATE_COMMAND;
        }
        if self.shift.get() != MODIFIER_KEY_NONE {
            modifier_state |= MODIFIER_STATE_SHIFT;
        }
        if self.caps_lock.get() {
            modifier_state |= MODIFIER_STATE_CAPS_LOCK;
        }
        if self.alt.get() != MODIFIER_KEY_NONE {
            modifier_state |= MODIFIER_STATE_ALT;
        }
        if self.control.get() != MODIFIER_KEY_NONE {
            modifier_state |= MODIFIER_STATE_CONTROL;
        }
        modifier_state
    }

    fn update_modifier_state(&self, event_type: &EventType) {
        match event_type {
            EventType::KeyPress(key) => self.update_key_modifier_state(*key, true),
            EventType::KeyRelease(key) => self.update_key_modifier_state(*key, false),
            _ => {}
        }
    }

    fn update_key_modifier_state(&self, key: crate::Key, keydown: bool) {
        let Some(modifier_key) = modifier_key_from_key(key) else {
            return;
        };
        match modifier_key {
            ModifierKey::Shift(side) => set_modifier_key_state(&self.shift, side, keydown),
            ModifierKey::Alt(side) => set_modifier_key_state(&self.alt, side, keydown),
            ModifierKey::Control(side) => set_modifier_key_state(&self.control, side, keydown),
            ModifierKey::Command(side) => set_modifier_key_state(&self.command, side, keydown),
            ModifierKey::CapsLock if keydown => self.caps_lock.set(!self.caps_lock.get()),
            ModifierKey::CapsLock => {}
        }
    }

    /// Sends one event and updates this input stream's modifier/dead-key state.
    ///
    /// Call this from the main thread for keyboard events. Unicode translation
    /// reads the active macOS input source through Carbon TIS APIs.
    /// Printable key presses set the event Unicode string from `UCKeyTranslate`
    /// output, so they are not keycode-only events.
    ///
    /// Do not call this concurrently on the same `VirtualInput`; keyboard events
    /// update the underlying `CGEventSource` keyboard type before creating the
    /// event.
    ///
    /// Caps Lock is tracked inside this input stream and toggles on key press.
    /// Callers that need matching Unicode output must route Caps Lock through
    /// the same `VirtualInput`; real system Caps Lock changes are not
    /// synchronized. Dead-key state is carried between key presses until
    /// `UCKeyTranslate` clears it, and is reset when Unicode translation is
    /// suppressed for Command/Control shortcuts. Use a separate `VirtualInput`
    /// for an independent keyboard stream.
    ///
    /// Modifier state is updated only after an event is created and posted
    /// successfully. Dead-key state is updated during event creation because
    /// `UCKeyTranslate` reads and writes that state.
    /// `KeyRelease(CapsLock)` does not change the tracked Caps Lock state.
    pub fn simulate(&self, event_type: &EventType) -> Result<(), SimulateError> {
        unsafe {
            let keyboard_type = keyboard_type_value(self.keyboard_type);
            if let Some(cg_event) = convert_native_with_source(
                event_type,
                &self.source,
                keyboard_type,
                Some(self.modifier_state()),
                Some(&self.dead_state),
            ) {
                cg_event.post(self.tap_loc);
                self.update_modifier_state(event_type);
                Ok(())
            } else {
                Err(SimulateError)
            }
        }
    }

    // keycode is defined in rdev::macos::virtual_keycodes
    pub fn get_key_state(state_id: CGEventSourceStateID, keycode: CGKeyCode) -> bool {
        unsafe { CGEventSourceKeyState(state_id, keycode) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::string::UniChar;
    use serial_test::serial;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventKeyboardGetUnicodeString(
            event: core_graphics::sys::CGEventRef,
            maxStringLength: usize,
            actualStringLength: *mut usize,
            unicodeString: *mut UniChar,
        );
    }

    // Tests using this guard mutate global extra-info state and must be `#[serial]`.
    struct ExtraInfoGuard {
        mouse: i64,
        keyboard: i64,
    }

    impl ExtraInfoGuard {
        fn new(mouse: i64, keyboard: i64) -> Self {
            let guard = Self {
                mouse: MOUSE_EXTRA_INFO.load(Ordering::Relaxed),
                keyboard: KEYBOARD_EXTRA_INFO.load(Ordering::Relaxed),
            };
            set_mouse_extra_info(mouse);
            set_keyboard_extra_info(keyboard);
            guard
        }
    }

    impl Drop for ExtraInfoGuard {
        fn drop(&mut self) {
            set_mouse_extra_info(self.mouse);
            set_keyboard_extra_info(self.keyboard);
        }
    }

    const TEST_MOUSE_EXTRA_INFO: i64 = 11;
    const TEST_KEYBOARD_EXTRA_INFO: i64 = 22;
    const TEST_MOUSE_X: f64 = 1.0;
    const TEST_MOUSE_Y: f64 = 2.0;
    const RAW_KEYBOARD_TYPE: CGEventSourceKeyboardType = 91;
    const TEST_UNICODE_BUFFER_LEN: usize = 8;
    const TEST_PENDING_DEAD_STATE: u32 = 1;
    fn event_unicode_string(event: &CGEvent) -> Vec<u16> {
        let mut unicode = [0_u16; TEST_UNICODE_BUFFER_LEN];
        let mut length = 0;
        unsafe {
            CGEventKeyboardGetUnicodeString(
                event.as_ptr(),
                TEST_UNICODE_BUFFER_LEN,
                &mut length,
                unicode.as_mut_ptr(),
            );
        }
        assert!(length <= TEST_UNICODE_BUFFER_LEN);
        unicode[..length].to_vec()
    }

    fn key_press_event(input: &VirtualInput, keycode: CGKeyCode) -> CGEvent {
        let dead_state = Cell::new(0);
        unsafe {
            convert_native_with_source(
                &EventType::KeyPress(crate::Key::RawKey(RawKey::MacVirtualKeycode(keycode))),
                &input.source,
                keyboard_type_value(input.keyboard_type),
                Some(input.modifier_state()),
                Some(&dead_state),
            )
            .unwrap()
        }
    }

    fn virtual_input_event(input: &VirtualInput, event_type: &EventType) -> CGEvent {
        unsafe {
            convert_native_with_source(
                event_type,
                &input.source,
                keyboard_type_value(input.keyboard_type),
                Some(input.modifier_state()),
                Some(&input.dead_state),
            )
            .unwrap()
        }
    }

    #[test]
    #[serial]
    fn caps_lock_uses_alpha_lock_modifier_state() {
        let input =
            VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session).unwrap();

        input.update_modifier_state(&EventType::KeyPress(crate::Key::CapsLock));

        assert_eq!(MODIFIER_STATE_CAPS_LOCK, input.modifier_state());
    }

    #[test]
    #[serial]
    fn virtual_input_current_keyboard_type_uses_system_keyboard_type() {
        let input = VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session)
            .unwrap()
            .with_keyboard_type(MacKeyboardType::Current);

        let expected_keyboard_type = current_keyboard_type();
        let keyboard_type = keyboard_type_value(input.keyboard_type);
        let dead_state = Cell::new(0);
        let event = unsafe {
            convert_native_with_source(
                &EventType::KeyPress(crate::Key::RawKey(RawKey::MacVirtualKeycode(kVK_ANSI_2))),
                &input.source,
                keyboard_type,
                Some(input.modifier_state()),
                Some(&dead_state),
            )
            .unwrap()
        };

        assert_eq!(
            expected_keyboard_type as i64,
            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYBOARD_TYPE)
        );
    }

    #[test]
    #[serial]
    fn keyboard_event_uses_requested_keyboard_type_with_shift() {
        let input = VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session)
            .unwrap()
            .with_keyboard_type(MacKeyboardType::Jis);
        input.update_modifier_state(&EventType::KeyPress(crate::Key::ShiftLeft));
        let dead_state = Cell::new(0);
        let event = unsafe {
            convert_native_with_source(
                &EventType::KeyPress(crate::Key::RawKey(RawKey::MacVirtualKeycode(kVK_ANSI_2))),
                &input.source,
                keyboard_type_value(input.keyboard_type),
                Some(input.modifier_state()),
                Some(&dead_state),
            )
            .unwrap()
        };

        assert_eq!(
            JIS_KEYBOARD_TYPE as i64,
            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYBOARD_TYPE)
        );
        assert_eq!(MODIFIER_STATE_SHIFT, input.modifier_state());
    }

    #[test]
    #[serial]
    fn command_modifier_does_not_set_printable_unicode_string() {
        let input = VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session)
            .unwrap()
            .with_keyboard_type(MacKeyboardType::Ansi);
        input.update_modifier_state(&EventType::KeyPress(crate::Key::MetaLeft));

        let event = key_press_event(&input, kVK_ANSI_A);

        assert!(event_unicode_string(&event).is_empty());
    }

    #[test]
    #[serial]
    fn control_modifier_does_not_set_printable_unicode_string() {
        let input = VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session)
            .unwrap()
            .with_keyboard_type(MacKeyboardType::Ansi);
        input.update_modifier_state(&EventType::KeyPress(crate::Key::ControlLeft));

        let event = key_press_event(&input, kVK_ANSI_A);

        assert!(event_unicode_string(&event).is_empty());
    }

    #[test]
    #[serial]
    fn raw_shift_key_updates_modifier_state_for_keyboard_type_event() {
        let input = VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session)
            .unwrap()
            .with_keyboard_type(MacKeyboardType::Jis);
        input.update_modifier_state(&EventType::KeyPress(crate::Key::RawKey(
            RawKey::MacVirtualKeycode(kVK_Shift),
        )));
        let dead_state = Cell::new(0);
        let event = unsafe {
            convert_native_with_source(
                &EventType::KeyPress(crate::Key::RawKey(RawKey::MacVirtualKeycode(kVK_ANSI_2))),
                &input.source,
                keyboard_type_value(input.keyboard_type),
                Some(input.modifier_state()),
                Some(&dead_state),
            )
            .unwrap()
        };

        assert_eq!(
            JIS_KEYBOARD_TYPE as i64,
            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYBOARD_TYPE)
        );
        assert_eq!(MODIFIER_STATE_SHIFT, input.modifier_state());
    }

    #[test]
    #[serial]
    fn alt_gr_key_updates_modifier_state() {
        let input =
            VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session).unwrap();

        input.update_modifier_state(&EventType::KeyPress(crate::Key::AltGr));
        assert_eq!(MODIFIER_STATE_ALT, input.modifier_state());

        input.update_modifier_state(&EventType::KeyRelease(crate::Key::AltGr));
        assert_eq!(MODIFIER_STATE_NONE, input.modifier_state());
    }

    #[test]
    #[serial]
    fn raw_right_option_key_updates_modifier_state() {
        let input =
            VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session).unwrap();

        input.update_modifier_state(&EventType::KeyPress(crate::Key::RawKey(
            RawKey::MacVirtualKeycode(kVK_RightOption),
        )));
        assert_eq!(MODIFIER_STATE_ALT, input.modifier_state());

        input.update_modifier_state(&EventType::KeyRelease(crate::Key::RawKey(
            RawKey::MacVirtualKeycode(kVK_RightOption),
        )));
        assert_eq!(MODIFIER_STATE_NONE, input.modifier_state());
    }

    fn assert_key_press_dead_state(
        input: &VirtualInput,
        event_type: EventType,
        expected_dead_state: u32,
    ) {
        input.dead_state.set(TEST_PENDING_DEAD_STATE);
        unsafe {
            convert_native_with_source(
                &event_type,
                &input.source,
                keyboard_type_value(input.keyboard_type),
                Some(input.modifier_state()),
                Some(&input.dead_state),
            )
            .unwrap();
        }

        assert_eq!(expected_dead_state, input.dead_state.get());
    }

    #[test]
    #[serial]
    fn command_suppressed_key_press_clears_pending_dead_state() {
        let input =
            VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session).unwrap();

        input.update_modifier_state(&EventType::KeyPress(crate::Key::MetaLeft));
        assert_key_press_dead_state(&input, EventType::KeyPress(crate::Key::KeyA), 0);
    }

    #[test]
    #[serial]
    fn modifier_key_press_preserves_pending_dead_state() {
        let input =
            VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session).unwrap();

        assert_key_press_dead_state(
            &input,
            EventType::KeyPress(crate::Key::ShiftLeft),
            TEST_PENDING_DEAD_STATE,
        );
    }

    #[test]
    #[serial]
    fn command_modified_modifier_key_press_preserves_pending_dead_state() {
        let input =
            VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session).unwrap();

        input.update_modifier_state(&EventType::KeyPress(crate::Key::MetaLeft));
        assert_key_press_dead_state(
            &input,
            EventType::KeyPress(crate::Key::ShiftLeft),
            TEST_PENDING_DEAD_STATE,
        );
    }

    fn assert_paired_modifier_state(
        input: &VirtualInput,
        key_pair: (crate::Key, crate::Key),
        modifier_state: u32,
    ) {
        let (left_key, right_key) = key_pair;
        input.update_modifier_state(&EventType::KeyPress(left_key));
        input.update_modifier_state(&EventType::KeyPress(right_key));
        input.update_modifier_state(&EventType::KeyRelease(left_key));
        assert_eq!(modifier_state, input.modifier_state());

        input.update_modifier_state(&EventType::KeyRelease(right_key));
        assert_eq!(MODIFIER_STATE_NONE, input.modifier_state());
    }

    #[test]
    #[serial]
    fn paired_modifier_keys_stay_active_until_both_are_released() {
        let input =
            VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session).unwrap();

        assert_paired_modifier_state(
            &input,
            (crate::Key::ShiftLeft, crate::Key::ShiftRight),
            MODIFIER_STATE_SHIFT,
        );
        assert_paired_modifier_state(
            &input,
            (crate::Key::Alt, crate::Key::AltGr),
            MODIFIER_STATE_ALT,
        );
        assert_paired_modifier_state(
            &input,
            (
                crate::Key::RawKey(RawKey::MacVirtualKeycode(kVK_Option)),
                crate::Key::RawKey(RawKey::MacVirtualKeycode(kVK_RightOption)),
            ),
            MODIFIER_STATE_ALT,
        );
        assert_paired_modifier_state(
            &input,
            (crate::Key::ControlLeft, crate::Key::ControlRight),
            MODIFIER_STATE_CONTROL,
        );
        assert_paired_modifier_state(
            &input,
            (crate::Key::MetaLeft, crate::Key::MetaRight),
            MODIFIER_STATE_COMMAND,
        );
    }

    #[test]
    fn modifier_keys_do_not_request_unicode_translation() {
        assert!(!should_translate_key_unicode(
            crate::Key::ShiftLeft,
            MODIFIER_STATE_NONE
        ));
        assert!(!should_translate_key_unicode(
            crate::Key::RawKey(RawKey::MacVirtualKeycode(kVK_RightOption)),
            MODIFIER_STATE_NONE
        ));
        assert!(!should_translate_key_unicode(
            crate::Key::KeyA,
            MODIFIER_STATE_COMMAND
        ));
        assert!(should_translate_key_unicode(
            crate::Key::KeyA,
            MODIFIER_STATE_SHIFT
        ));
    }

    #[test]
    #[serial]
    fn keyboard_event_conversion_matches_raw_keyboard_type() {
        let input = VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session)
            .unwrap()
            .with_keyboard_type(MacKeyboardType::Raw(RAW_KEYBOARD_TYPE));

        let event = key_press_event(&input, kVK_ANSI_2);

        assert_eq!(
            RAW_KEYBOARD_TYPE as i64,
            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYBOARD_TYPE)
        );
    }

    #[test]
    #[serial]
    fn event_source_user_data_matches_event_kind() {
        let _guard = ExtraInfoGuard::new(TEST_MOUSE_EXTRA_INFO, TEST_KEYBOARD_EXTRA_INFO);

        assert_eq!(
            TEST_KEYBOARD_EXTRA_INFO,
            event_source_user_data(&EventType::KeyPress(crate::Key::KeyA))
        );
        assert_eq!(
            TEST_MOUSE_EXTRA_INFO,
            event_source_user_data(&EventType::MouseMove {
                x: TEST_MOUSE_X,
                y: TEST_MOUSE_Y
            })
        );
    }

    #[test]
    #[serial]
    fn virtual_input_event_source_user_data_matches_event_kind() {
        let _guard = ExtraInfoGuard::new(TEST_MOUSE_EXTRA_INFO, TEST_KEYBOARD_EXTRA_INFO);
        let input =
            VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session).unwrap();

        let keyboard_event = virtual_input_event(&input, &EventType::KeyPress(crate::Key::KeyA));
        assert_eq!(
            TEST_KEYBOARD_EXTRA_INFO,
            keyboard_event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA)
        );

        let mouse_event = virtual_input_event(
            &input,
            &EventType::MouseMove {
                x: TEST_MOUSE_X,
                y: TEST_MOUSE_Y,
            },
        );
        assert_eq!(
            TEST_MOUSE_EXTRA_INFO,
            mouse_event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA)
        );
    }

    #[test]
    #[serial]
    fn virtual_input_keyboard_type_is_used_for_key_events() {
        let input = VirtualInput::new(CGEventSourceStateID::Private, CGEventTapLocation::Session)
            .unwrap()
            .with_keyboard_type(MacKeyboardType::Jis);

        let dead_state = Cell::new(0);
        let event = unsafe {
            convert_native_with_source(
                &EventType::KeyPress(crate::Key::RawKey(RawKey::MacVirtualKeycode(kVK_ANSI_2))),
                &input.source,
                keyboard_type_value(input.keyboard_type),
                Some(input.modifier_state()),
                Some(&dead_state),
            )
            .unwrap()
        };

        assert_eq!(
            JIS_KEYBOARD_TYPE as i64,
            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYBOARD_TYPE)
        );
    }

    #[test]
    #[serial]
    fn mac_keyboard_type_maps_to_raw_keyboard_type() {
        assert_eq!(
            current_keyboard_type(),
            keyboard_type_value(MacKeyboardType::Current)
        );
        assert_eq!(
            ANSI_KEYBOARD_TYPE,
            keyboard_type_value(MacKeyboardType::Ansi)
        );
        assert_eq!(ISO_KEYBOARD_TYPE, keyboard_type_value(MacKeyboardType::Iso));
        assert_eq!(JIS_KEYBOARD_TYPE, keyboard_type_value(MacKeyboardType::Jis));
        assert_eq!(
            RAW_KEYBOARD_TYPE,
            keyboard_type_value(MacKeyboardType::Raw(RAW_KEYBOARD_TYPE))
        );
    }
}
