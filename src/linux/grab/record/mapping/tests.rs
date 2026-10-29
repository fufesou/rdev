use super::*;

#[test]
fn latin1_mapped_keys_are_text() {
    for (keycode, text) in [(202, 'é'), (248, 'ü'), (248, '~'), (202, ' ')] {
        let mut mapping = Mapping::default();
        let mut request = vec![CHANGE_KEYBOARD_MAPPING, 1, 3, 0, keycode, 1, 0, 0];
        request.extend_from_slice(&u32::from(text).to_ne_bytes());
        mapping.update(&request, false).unwrap();
        let mut wire_event = [0; 32];
        wire_event[0] = xlib::KeyPress as u8;
        wire_event[1] = keycode;
        let event = mapping.event(&wire_event, false).unwrap();
        let key = Key::Unknown(u32::from(keycode));
        assert_eq!(event.event_type, EventType::KeyPress(key));
        assert_eq!(event.unicode.unwrap().name, Some(text.to_string()));
    }
}

#[test]
fn unicode_mapped_function_key_is_text() {
    const F24_KEYCODE: u8 = 202;
    let mut mapping = Mapping::default();
    let mut request = vec![CHANGE_KEYBOARD_MAPPING, 1, 3, 0, F24_KEYCODE, 1, 0, 0];
    request.extend_from_slice(&(UNICODE_KEYSYM | u32::from('汉')).to_ne_bytes());
    mapping.update(&request, false).unwrap();
    let mut wire_event = [0; 32];
    wire_event[0] = xlib::KeyPress as u8;
    wire_event[1] = F24_KEYCODE;
    let event = mapping.event(&wire_event, false).unwrap();
    assert_eq!(event.event_type, EventType::KeyPress(Key::Unknown(202)));
    assert_eq!(event.unicode.unwrap().name.as_deref(), Some("汉"));

    wire_event[0] = xlib::KeyRelease as u8;
    let event = mapping.event(&wire_event, false).unwrap();
    assert_eq!(event.event_type, EventType::KeyRelease(Key::Unknown(202)));
    assert!(event.unicode.is_none());

    request[8..].copy_from_slice(&x11::keysym::XK_F24.to_ne_bytes());
    mapping.update(&request, false).unwrap();
    wire_event[0] = xlib::KeyPress as u8;
    assert_eq!(
        mapping.event(&wire_event, false).unwrap().event_type,
        EventType::KeyPress(Key::F24)
    );
}

#[test]
fn invalid_mapping_request_preserves_recorded_text() {
    const RECORD_UNIT_BYTES: usize = 4;
    let mut state = super::super::RecordState::default();
    let mut request = vec![CHANGE_KEYBOARD_MAPPING, 1, 3, 0, u8::MAX, 1, 0, 0];
    request.extend_from_slice(&u32::from('z').to_ne_bytes());
    state.mapping.update(&request, false).unwrap();
    // This request exceeds the keycode range after replacing the first symbol.
    request[1] = 2;
    request[2] = 4;
    request[8..].copy_from_slice(&u32::from('a').to_ne_bytes());
    request.extend_from_slice(&u32::from('b').to_ne_bytes());
    let mut raw: x11::xrecord::XRecordInterceptData = unsafe { std::mem::zeroed() };
    raw.category = x11::xrecord::XRecordFromClient;
    raw.data_len = (request.len() / RECORD_UNIT_BYTES) as _;
    raw.data = request.as_mut_ptr();
    unsafe { state.receive(&raw) }.unwrap();
    let mut wire_event = [0; 32];
    wire_event[0] = xlib::KeyPress as u8;
    wire_event[1] = u8::MAX;
    let event = state.mapping.event(&wire_event, false).unwrap();
    assert_eq!(event.unicode.unwrap().name.as_deref(), Some("z"));
}

#[test]
fn only_uniform_symbol_maps_are_text() {
    const F24_KEYCODE: u8 = 202;
    for second in [u32::from('A'), 0, u32::from('a')] {
        let mut mapping = Mapping::default();
        let mut request = vec![CHANGE_KEYBOARD_MAPPING, 1, 4, 0, F24_KEYCODE, 2, 0, 0];
        request.extend_from_slice(&u32::from('a').to_ne_bytes());
        request.extend_from_slice(&second.to_ne_bytes());
        mapping.update(&request, false).unwrap();
        let expected = if second == u32::from('a') {
            Key::Unknown(u32::from(F24_KEYCODE))
        } else {
            Key::F24
        };
        for modifiers in [xlib::ShiftMask, xlib::Mod5Mask] {
            let mut wire_event = [0; 32];
            wire_event[0] = xlib::KeyPress as u8;
            wire_event[1] = F24_KEYCODE;
            wire_event[28..30].copy_from_slice(&(modifiers as u16).to_ne_bytes());
            let event = mapping.event(&wire_event, false).unwrap();
            assert_eq!(event.event_type, EventType::KeyPress(expected));
        }
    }
}

#[test]
fn control_modified_function_key_preserves_press_release_pair() {
    const F24_KEYCODE: u8 = 202;
    let mut mapping = Mapping::default();
    let mut request = vec![CHANGE_KEYBOARD_MAPPING, 1, 3, 0, F24_KEYCODE, 1, 0, 0];
    request.extend_from_slice(&u32::from('a').to_ne_bytes());
    mapping.update(&request, false).unwrap();
    let text_key = Key::Unknown(u32::from(F24_KEYCODE));
    let mut wire_event = [0; 32];
    wire_event[1] = F24_KEYCODE;
    // Releasing Ctrl first must keep the shortcut pair; Auto-Type also keeps its pair.
    for (event_type, state, key) in [
        (xlib::KeyPress, xlib::ControlMask, Key::F24),
        (xlib::KeyRelease, xlib::ControlMask, Key::F24),
        (xlib::KeyPress, xlib::ControlMask, Key::F24),
        (xlib::KeyRelease, 0, Key::F24),
        (xlib::KeyPress, 0, text_key),
        (xlib::KeyRelease, xlib::ControlMask, text_key),
    ] {
        wire_event[0] = event_type as u8;
        wire_event[28..30].copy_from_slice(&(state as u16).to_ne_bytes());
        let event = mapping.event(&wire_event, false).unwrap();
        let expected = if event_type == xlib::KeyPress {
            EventType::KeyPress(key)
        } else {
            EventType::KeyRelease(key)
        };
        assert_eq!(event.event_type, expected, "modifier state: {state:#x}");
    }
}
