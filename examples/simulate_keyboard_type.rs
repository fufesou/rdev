#[cfg(target_os = "macos")]
use rdev::EventType;
#[cfg(target_os = "macos")]
use rdev::MacKeyboardType;
#[cfg(target_os = "macos")]
use std::{env, thread, time};

#[cfg(target_os = "macos")]
const EVENT_DELAY_MS: u64 = 20;

#[cfg(target_os = "macos")]
const FOCUS_DELAY_SECS: u64 = 2;
#[cfg(target_os = "macos")]
const SEND_CONFIRM_ARG: &str = "--send";

#[cfg(target_os = "macos")]
struct MacKeyboardSample {
    name: &'static str,
    keycodes: &'static [rdev::CGKeyCode],
}

#[cfg(target_os = "macos")]
enum SampleError {
    CreateVirtualInput,
    SendInput,
}

#[cfg(target_os = "macos")]
fn mac_key(keycode: rdev::CGKeyCode) -> rdev::Key {
    rdev::Key::RawKey(rdev::RawKey::MacVirtualKeycode(keycode))
}

#[cfg(target_os = "macos")]
fn send_with_input(input: &rdev::VirtualInput, event_type: &EventType) -> Result<(), SampleError> {
    let delay = time::Duration::from_millis(EVENT_DELAY_MS);
    input
        .simulate(event_type)
        .map_err(|_| SampleError::SendInput)?;
    thread::sleep(delay);
    Ok(())
}

#[cfg(target_os = "macos")]
fn send_key_sequence(
    input: &rdev::VirtualInput,
    keycodes: &[rdev::CGKeyCode],
) -> Result<(), SampleError> {
    let mut pressed = Vec::with_capacity(keycodes.len());
    for keycode in keycodes {
        if let Err(error) = send_with_input(input, &EventType::KeyPress(mac_key(*keycode))) {
            release_pressed_keys(input, &pressed);
            return Err(error);
        }
        pressed.push(*keycode);
    }
    let mut release_error = None;
    for keycode in pressed.iter().rev() {
        if let Err(error) = send_with_input(input, &EventType::KeyRelease(mac_key(*keycode))) {
            release_error = Some(error);
        }
    }
    if let Some(error) = release_error {
        return Err(error);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn release_pressed_keys(input: &rdev::VirtualInput, keycodes: &[rdev::CGKeyCode]) {
    for keycode in keycodes.iter().rev() {
        let _ = send_with_input(input, &EventType::KeyRelease(mac_key(*keycode)));
    }
}

#[cfg(target_os = "macos")]
fn send_keyboard_type_samples(
    keyboard_type_name: &str,
    keyboard_type: MacKeyboardType,
    samples: &[MacKeyboardSample],
) -> Result<(), SampleError> {
    let Ok(virtual_input) = rdev::VirtualInput::new(
        rdev::CGEventSourceStateID::HIDSystemState,
        rdev::CGEventTapLocation::HID,
    ) else {
        return Err(SampleError::CreateVirtualInput);
    };
    let virtual_input = virtual_input.with_keyboard_type(keyboard_type);

    println!("Sending {} keyboard samples:", keyboard_type_name);
    for sample in samples {
        println!("  {}", sample.name);
        send_key_sequence(&virtual_input, sample.keycodes)?;
        send_key_sequence(&virtual_input, &[rdev::kVK_Space])?;
    }
    send_key_sequence(&virtual_input, &[rdev::kVK_Return])
}

#[cfg(target_os = "macos")]
fn test_macos_keys() {
    if !env::args().any(|arg| arg == SEND_CONFIRM_ARG) {
        println!(
            "This example sends real key events to the focused application. Run with {} to continue.",
            SEND_CONFIRM_ARG
        );
        return;
    }

    let samples = [
        MacKeyboardSample {
            name: "Shift+2",
            keycodes: &[rdev::kVK_Shift, rdev::kVK_ANSI_2],
        },
        MacKeyboardSample {
            name: "ANSI backslash",
            keycodes: &[rdev::kVK_ANSI_Backslash],
        },
        MacKeyboardSample {
            name: "ISO section",
            keycodes: &[rdev::kVK_ISO_Section],
        },
        MacKeyboardSample {
            name: "JIS yen",
            keycodes: &[rdev::kVK_JIS_Yen],
        },
    ];

    println!(
        "Focus a text field. Sending ANSI, ISO, and JIS samples in {} seconds.",
        FOCUS_DELAY_SECS
    );
    println!("Each row uses: Shift+2, ANSI backslash, ISO section, JIS yen.");
    thread::sleep(time::Duration::from_secs(FOCUS_DELAY_SECS));

    for (keyboard_type_name, keyboard_type) in [
        ("ANSI", MacKeyboardType::Ansi),
        ("ISO", MacKeyboardType::Iso),
        ("JIS", MacKeyboardType::Jis),
    ] {
        if let Err(error) = send_keyboard_type_samples(keyboard_type_name, keyboard_type, &samples)
        {
            match error {
                SampleError::CreateVirtualInput => {
                    println!(
                        "Failed to create VirtualInput for {} keyboard samples",
                        keyboard_type_name
                    );
                }
                SampleError::SendInput => {
                    println!("Failed to send {} keyboard samples", keyboard_type_name);
                }
            }
            return;
        }
    }
}

#[cfg(target_os = "macos")]
fn main() {
    test_macos_keys();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("This example is only implemented for MacOS.");
}
