//! Decode recorded protocol bytes, not native Xlib structs. The variable-length
//! XKB key types precede the symbol maps; offsets follow the XkbSetMap wire layout:
//! https://xorg.freedesktop.org/archive/current/doc/kbproto/xkbproto.html

use crate::{key_from_code, rdev::UnicodeInfo, Event, EventType, Key};
use std::{convert::TryInto, io, time::SystemTime};
use x11::xlib;

pub(super) const CHANGE_KEYBOARD_MAPPING: u8 = 100;
pub(super) const XKB_SET_MAP: u16 = 9;
const KEYCODE_COUNT: usize = 256;
const UNICODE_KEYSYM: u32 = 0x01000000;
const ASCII_KEYSYMS: std::ops::RangeInclusive<u32> = 0x20..=0x7E;
const LATIN1_KEYSYMS: std::ops::RangeInclusive<u32> = 0xA0..=0xFF;
const SURROGATES: std::ops::RangeInclusive<u32> = 0xD800..=0xDFFF;
const XKB_KEY_TYPES: u16 = 1;
const XKB_KEY_SYMS: u16 = 2;
const REQUEST_LENGTH_BYTES: usize = 2;
const CORE_MAP_PADDING_BYTES: usize = 2;
const XKB_DEVICE_SPEC_BYTES: usize = 2;
const XKB_FLAGS_AND_BOUNDS_BYTES: usize = 5;
const XKB_REMAINING_HEADER_BYTES: usize = 20;
const XKB_SYMBOL_MAP_PREFIX_BYTES: usize = 6;
// Conventional XKB assignments: Alt = Mod1, Super = Mod4. Shift and Mod5
// (AltGr) can generate text. Custom modifier assignments are not inferred here.
const DEFAULT_SHORTCUT_MODIFIERS: u32 = xlib::ControlMask | xlib::Mod1Mask | xlib::Mod4Mask;

pub(super) struct Mapping {
    symbols: [u32; KEYCODE_COUNT],
    pressed_text: [Option<bool>; KEYCODE_COUNT],
    pub(super) xkb_opcode: u8,
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            symbols: [0; KEYCODE_COUNT],
            pressed_text: [None; KEYCODE_COUNT],
            xkb_opcode: 0,
        }
    }
}

impl Mapping {
    pub(super) fn update(&mut self, data: &[u8], swapped: bool) -> io::Result<()> {
        let mut pending = Self {
            symbols: self.symbols,
            pressed_text: self.pressed_text,
            xkb_opcode: self.xkb_opcode,
        };
        pending.read_request(data, swapped)?;
        self.symbols = pending.symbols;
        Ok(())
    }

    fn read_request(&mut self, data: &[u8], swapped: bool) -> io::Result<()> {
        let mut wire = Wire { data, swapped };
        let opcode = wire.byte()?;
        let detail = wire.byte()?;
        wire.skip(REQUEST_LENGTH_BYTES)?;
        if opcode == CHANGE_KEYBOARD_MAPPING {
            let first = wire.byte()?;
            let width = wire.byte()?;
            wire.skip(CORE_MAP_PADDING_BYTES)?;
            for index in usize::from(first)..usize::from(first) + usize::from(detail) {
                self.read_symbols(index, usize::from(width), &mut wire)?;
            }
        } else if opcode == self.xkb_opcode && u16::from(detail) == XKB_SET_MAP {
            self.update_xkb(&mut wire)?;
        }
        Ok(())
    }

    fn update_xkb(&mut self, wire: &mut Wire<'_>) -> io::Result<()> {
        wire.skip(XKB_DEVICE_SPEC_BYTES)?;
        let present = wire.short()?;
        wire.skip(XKB_FLAGS_AND_BOUNDS_BYTES)?;
        let types = wire.byte()?;
        let first = wire.byte()?;
        let count = wire.byte()?;
        wire.skip(XKB_REMAINING_HEADER_BYTES)?;
        if present & XKB_KEY_TYPES != 0 {
            skip_types(wire, types)?;
        }
        if present & XKB_KEY_SYMS == 0 {
            return Ok(());
        }
        for index in usize::from(first)..usize::from(first) + usize::from(count) {
            wire.skip(XKB_SYMBOL_MAP_PREFIX_BYTES)?;
            let count = usize::from(wire.short()?);
            self.read_symbols(index, count, wire)?;
        }
        Ok(())
    }

    fn read_symbols(&mut self, index: usize, count: usize, wire: &mut Wire<'_>) -> io::Result<()> {
        let mut symbol = if count == 0 { 0 } else { wire.long()? };
        // Distinct levels need normal modifier-aware conversion, not literal text.
        for _ in 1..count {
            if wire.long()? != symbol {
                symbol = 0;
            }
        }
        let target = self.symbols.get_mut(index).ok_or_else(invalid_mapping)?;
        *target = symbol;
        Ok(())
    }

    pub(super) fn event(&mut self, data: &[u8], swapped: bool) -> io::Result<Event> {
        const EVENT_STATE_OFFSET: usize = 28;
        const EVENT_TYPE_AND_DETAIL_BYTES: usize = 2;
        const SEND_EVENT_FLAG: u8 = 0x80;
        let mut wire = Wire { data, swapped };
        let event_type = wire.byte()? & !SEND_EVENT_FLAG;
        let code = u32::from(wire.byte()?);
        wire.skip(EVENT_STATE_OFFSET - EVENT_TYPE_AND_DETAIL_BYTES)?;
        let state = u32::from(wire.short()?);
        let is_press = i32::from(event_type) == xlib::KeyPress;
        let key = key_from_code(code);
        let symbol = self.symbols[code as usize];
        let unicode = if is_text_key(key) {
            unicode(symbol)
        } else {
            None
        };
        let text = unicode.is_some()
            && (matches!(key, Key::Unknown(_)) || state & DEFAULT_SHORTCUT_MODIFIERS == 0);
        // Modifiers can change before release. Refresh on each press so a release
        // missed while ungrabbed cannot leave a stale classification.
        let text = if is_press {
            self.pressed_text[code as usize] = Some(text);
            text
        } else {
            self.pressed_text[code as usize].take().unwrap_or(text)
        };
        if !text {
            return Ok(super::super::convert_event(code, is_press, state));
        }
        let key = Key::Unknown(code);
        Ok(Event {
            event_type: if is_press {
                EventType::KeyPress(key)
            } else {
                EventType::KeyRelease(key)
            },
            time: SystemTime::now(),
            unicode: if is_press { unicode } else { None },
            platform_code: symbol,
            position_code: code,
            usb_hid: 0,
        })
    }
}

fn is_text_key(key: Key) -> bool {
    // Auto-Type can reuse unassigned function-key slots, including F24.
    matches!(
        key,
        Key::Unknown(_)
            | Key::F1
            | Key::F2
            | Key::F3
            | Key::F4
            | Key::F5
            | Key::F6
            | Key::F7
            | Key::F8
            | Key::F9
            | Key::F10
            | Key::F11
            | Key::F12
            | Key::F13
            | Key::F14
            | Key::F15
            | Key::F16
            | Key::F17
            | Key::F18
            | Key::F19
            | Key::F20
            | Key::F21
            | Key::F22
            | Key::F23
            | Key::F24
    )
}

fn unicode(symbol: u32) -> Option<UnicodeInfo> {
    let code = if ASCII_KEYSYMS.contains(&symbol) || LATIN1_KEYSYMS.contains(&symbol) {
        symbol
    } else {
        symbol.checked_sub(UNICODE_KEYSYM)?
    };
    // Injected UTF-16 halves must reach the consumer intact so it can join pairs.
    let unicode = if SURROGATES.contains(&code) {
        vec![code as u16]
    } else {
        char::from_u32(code)?.to_string().encode_utf16().collect()
    };
    Some(UnicodeInfo {
        name: String::from_utf16(&unicode).ok(),
        unicode,
        is_dead: false,
    })
}

fn skip_types(wire: &mut Wire<'_>, count: u8) -> io::Result<()> {
    const TYPE_PREFIX: usize = 5;
    const TYPE_ENTRY_BYTES: usize = 4;
    const TYPE_PADDING_BYTES: usize = 1;
    for _ in 0..count {
        wire.skip(TYPE_PREFIX)?;
        let entries = usize::from(wire.byte()?);
        let preserve = usize::from(wire.byte()? != 0);
        wire.skip(TYPE_PADDING_BYTES + entries * TYPE_ENTRY_BYTES * (1 + preserve))?;
    }
    Ok(())
}

struct Wire<'a> {
    data: &'a [u8],
    swapped: bool,
}

impl Wire<'_> {
    fn skip(&mut self, count: usize) -> io::Result<()> {
        self.data = self.data.get(count..).ok_or_else(invalid_mapping)?;
        Ok(())
    }

    fn take<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let bytes = self.data.get(..N).ok_or_else(invalid_mapping)?;
        let value = bytes.try_into().map_err(|_| invalid_mapping())?;
        self.skip(N)?;
        Ok(value)
    }

    fn byte(&mut self) -> io::Result<u8> {
        Ok(self.take::<1>()?[0])
    }

    fn short(&mut self) -> io::Result<u16> {
        let value = u16::from_ne_bytes(self.take()?);
        Ok(if self.swapped {
            value.swap_bytes()
        } else {
            value
        })
    }

    fn long(&mut self) -> io::Result<u32> {
        let value = u32::from_ne_bytes(self.take()?);
        Ok(if self.swapped {
            value.swap_bytes()
        } else {
            value
        })
    }
}

fn invalid_mapping() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid X11 keyboard mapping record",
    )
}

#[cfg(test)]
mod tests;
