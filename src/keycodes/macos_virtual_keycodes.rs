// keycodes from #include <Carbon/Carbon.h>
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

#[cfg(not(target_os = "macos"))]
pub type CGKeyCode = u32;

#[cfg(target_os = "macos")]
pub type CGKeyCode = core_graphics::event::CGKeyCode;

pub const kVK_ANSI_A: CGKeyCode = 0;
pub const kVK_ANSI_S: CGKeyCode = 1;
pub const kVK_ANSI_D: CGKeyCode = 2;
pub const kVK_ANSI_F: CGKeyCode = 3;
pub const kVK_ANSI_H: CGKeyCode = 4;
pub const kVK_ANSI_G: CGKeyCode = 5;
pub const kVK_ANSI_Z: CGKeyCode = 6;
pub const kVK_ANSI_X: CGKeyCode = 7;
pub const kVK_ANSI_C: CGKeyCode = 8;
pub const kVK_ANSI_V: CGKeyCode = 9;
pub const kVK_ANSI_B: CGKeyCode = 11;
pub const kVK_ANSI_Q: CGKeyCode = 12;
pub const kVK_ANSI_W: CGKeyCode = 13;
pub const kVK_ANSI_E: CGKeyCode = 14;
pub const kVK_ANSI_R: CGKeyCode = 15;
pub const kVK_ANSI_Y: CGKeyCode = 16;
pub const kVK_ANSI_T: CGKeyCode = 17;
pub const kVK_ANSI_1: CGKeyCode = 18;
pub const kVK_ANSI_2: CGKeyCode = 19;
pub const kVK_ANSI_3: CGKeyCode = 20;
pub const kVK_ANSI_4: CGKeyCode = 21;
pub const kVK_ANSI_6: CGKeyCode = 22;
pub const kVK_ANSI_5: CGKeyCode = 23;
pub const kVK_ANSI_Equal: CGKeyCode = 24;
pub const kVK_ANSI_9: CGKeyCode = 25;
pub const kVK_ANSI_7: CGKeyCode = 26;
pub const kVK_ANSI_Minus: CGKeyCode = 27;
pub const kVK_ANSI_8: CGKeyCode = 28;
pub const kVK_ANSI_0: CGKeyCode = 29;
pub const kVK_ANSI_RightBracket: CGKeyCode = 30;
pub const kVK_ANSI_O: CGKeyCode = 31;
pub const kVK_ANSI_U: CGKeyCode = 32;
pub const kVK_ANSI_LeftBracket: CGKeyCode = 33;
pub const kVK_ANSI_I: CGKeyCode = 34;
pub const kVK_ANSI_P: CGKeyCode = 35;
pub const kVK_ANSI_L: CGKeyCode = 37;
pub const kVK_ANSI_J: CGKeyCode = 38;
pub const kVK_ANSI_Quote: CGKeyCode = 39;
pub const kVK_ANSI_K: CGKeyCode = 40;
pub const kVK_ANSI_Semicolon: CGKeyCode = 41;
pub const kVK_ANSI_Backslash: CGKeyCode = 42;
pub const kVK_ANSI_Comma: CGKeyCode = 43;
pub const kVK_ANSI_Slash: CGKeyCode = 44;
pub const kVK_ANSI_N: CGKeyCode = 45;
pub const kVK_ANSI_M: CGKeyCode = 46;
pub const kVK_ANSI_Period: CGKeyCode = 47;
pub const kVK_ANSI_Grave: CGKeyCode = 50;
pub const kVK_ANSI_KeypadDecimal: CGKeyCode = 65;
pub const kVK_ANSI_KeypadMultiply: CGKeyCode = 67;
pub const kVK_ANSI_KeypadPlus: CGKeyCode = 69;
pub const kVK_ANSI_KeypadClear: CGKeyCode = 71;
pub const kVK_ANSI_KeypadDivide: CGKeyCode = 75;
pub const kVK_ANSI_KeypadEnter: CGKeyCode = 76;
pub const kVK_ANSI_KeypadMinus: CGKeyCode = 78;
pub const kVK_ANSI_KeypadEquals: CGKeyCode = 81;
pub const kVK_ANSI_Keypad0: CGKeyCode = 82;
pub const kVK_ANSI_Keypad1: CGKeyCode = 83;
pub const kVK_ANSI_Keypad2: CGKeyCode = 84;
pub const kVK_ANSI_Keypad3: CGKeyCode = 85;
pub const kVK_ANSI_Keypad4: CGKeyCode = 86;
pub const kVK_ANSI_Keypad5: CGKeyCode = 87;
pub const kVK_ANSI_Keypad6: CGKeyCode = 88;
pub const kVK_ANSI_Keypad7: CGKeyCode = 89;
pub const kVK_ANSI_Keypad8: CGKeyCode = 91;
pub const kVK_ANSI_Keypad9: CGKeyCode = 92;

pub const kVK_Return: CGKeyCode = 36;
pub const kVK_Tab: CGKeyCode = 48;
pub const kVK_Space: CGKeyCode = 49;
pub const kVK_Delete: CGKeyCode = 51;
pub const kVK_Escape: CGKeyCode = 53;
pub const kVK_Command: CGKeyCode = 55;
pub const kVK_Shift: CGKeyCode = 56;
pub const kVK_CapsLock: CGKeyCode = 57;
pub const kVK_Option: CGKeyCode = 58;
pub const kVK_Control: CGKeyCode = 59;
pub const kVK_RightCommand: CGKeyCode = 54;
pub const kVK_RightShift: CGKeyCode = 60;
pub const kVK_RightOption: CGKeyCode = 61;
pub const kVK_RightControl: CGKeyCode = 62;
pub const kVK_Function: CGKeyCode = 63;
pub const kVK_F17: CGKeyCode = 64;
pub const kVK_VolumeUp: CGKeyCode = 72;
pub const kVK_VolumeDown: CGKeyCode = 73;
pub const kVK_Mute: CGKeyCode = 74;
pub const kVK_F18: CGKeyCode = 79;
pub const kVK_F19: CGKeyCode = 80;
pub const kVK_F20: CGKeyCode = 90;
pub const kVK_F5: CGKeyCode = 96;
pub const kVK_F6: CGKeyCode = 97;
pub const kVK_F7: CGKeyCode = 98;
pub const kVK_F3: CGKeyCode = 99;
pub const kVK_F8: CGKeyCode = 100;
pub const kVK_F9: CGKeyCode = 101;
pub const kVK_F11: CGKeyCode = 103;
pub const kVK_F13: CGKeyCode = 105;
pub const kVK_F16: CGKeyCode = 106;
pub const kVK_F14: CGKeyCode = 107;
pub const kVK_F10: CGKeyCode = 109;
pub const kVK_F12: CGKeyCode = 111;
pub const kVK_F15: CGKeyCode = 113;
pub const kVK_Help: CGKeyCode = 114;
pub const kVK_Home: CGKeyCode = 115;
pub const kVK_PageUp: CGKeyCode = 116;
pub const kVK_ForwardDelete: CGKeyCode = 117;
pub const kVK_F4: CGKeyCode = 118;
pub const kVK_End: CGKeyCode = 119;
pub const kVK_F2: CGKeyCode = 120;
pub const kVK_PageDown: CGKeyCode = 121;
pub const kVK_F1: CGKeyCode = 122;
pub const kVK_LeftArrow: CGKeyCode = 123;
pub const kVK_RightArrow: CGKeyCode = 124;
pub const kVK_DownArrow: CGKeyCode = 125;
pub const kVK_UpArrow: CGKeyCode = 126;

pub const kVK_ISO_Section: CGKeyCode = 10;

pub const kVK_JIS_Yen: CGKeyCode = 93;
pub const kVK_JIS_Underscore: CGKeyCode = 94;
pub const kVK_JIS_KeypadComma: CGKeyCode = 95;
pub const kVK_JIS_Eisu: CGKeyCode = 102;
pub const kVK_JIS_Kana: CGKeyCode = 104;

pub const kVK_Context_Menu: CGKeyCode = 110;
pub const kVK_Unknown: CGKeyCode = 0xFFFF;