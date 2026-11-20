use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use evdev::{AttributeSet, KeyCode as EvdevKeyCode};

const EXTRA_FORWARDED_MOUSE_BUTTONS: [EvdevKeyCode; 3] = [
    EvdevKeyCode::BTN_FORWARD,
    EvdevKeyCode::BTN_BACK,
    EvdevKeyCode::BTN_TASK,
];

/// Converts a ClickForge mouse button to its Linux `evdev` button code.
pub fn mouse_button_to_evdev(mouse_button: MouseButton) -> EvdevKeyCode {
    match mouse_button {
        MouseButton::Left => EvdevKeyCode::BTN_LEFT,
        MouseButton::Right => EvdevKeyCode::BTN_RIGHT,
        MouseButton::Middle => EvdevKeyCode::BTN_MIDDLE,
        MouseButton::Side => EvdevKeyCode::BTN_SIDE,
        MouseButton::Extra => EvdevKeyCode::BTN_EXTRA,
    }
}

/// Converts a Linux `evdev` button code back into a ClickForge mouse button.
pub fn evdev_to_mouse_button(evdev_code: EvdevKeyCode) -> Option<MouseButton> {
    let mouse_button = match evdev_code {
        EvdevKeyCode::BTN_LEFT => MouseButton::Left,
        EvdevKeyCode::BTN_RIGHT => MouseButton::Right,
        EvdevKeyCode::BTN_MIDDLE => MouseButton::Middle,
        EvdevKeyCode::BTN_SIDE => MouseButton::Side,
        EvdevKeyCode::BTN_EXTRA => MouseButton::Extra,
        _ => return None,
    };
    Some(mouse_button)
}

/// Converts a ClickForge keyboard key code into its Linux `evdev` key code.
pub fn key_code_to_evdev(key_code: KeyCode) -> EvdevKeyCode {
    if let Some(letter_or_digit_code) = map_alphanumeric_and_function_to_evdev(key_code) {
        return letter_or_digit_code;
    }
    map_control_punctuation_and_numpad_to_evdev(key_code)
}

/// Converts a Linux `evdev` key code back into a ClickForge keyboard key code.
pub fn evdev_to_key_code(evdev_code: EvdevKeyCode) -> Option<KeyCode> {
    if let Some(letter_or_digit_key) = map_evdev_to_alphanumeric_and_function(evdev_code) {
        return Some(letter_or_digit_key);
    }
    map_evdev_to_control_punctuation_and_numpad(evdev_code)
}

/// Converts a unified ClickForge input trigger into its Linux `evdev` key or button code.
pub fn trigger_to_evdev(input_trigger: InputTrigger) -> EvdevKeyCode {
    match input_trigger {
        InputTrigger::Mouse(mouse_button) => mouse_button_to_evdev(mouse_button),
        InputTrigger::Key(key_code) => key_code_to_evdev(key_code),
    }
}

/// Converts a Linux `evdev` key or mouse button code into a unified ClickForge input trigger.
pub fn evdev_to_trigger(evdev_code: EvdevKeyCode) -> Option<InputTrigger> {
    if let Some(mouse_button) = evdev_to_mouse_button(evdev_code) {
        return Some(InputTrigger::Mouse(mouse_button));
    }
    evdev_to_key_code(evdev_code).map(InputTrigger::Key)
}

/// Builds the complete `evdev` key and button capability set for the virtual `uinput` device.
pub fn all_virtual_device_keys() -> AttributeSet<EvdevKeyCode> {
    let mut supported_codes = AttributeSet::<EvdevKeyCode>::new();
    for mouse_button in MouseButton::all_buttons() {
        supported_codes.insert(mouse_button_to_evdev(*mouse_button));
    }
    for extra_button in EXTRA_FORWARDED_MOUSE_BUTTONS {
        supported_codes.insert(extra_button);
    }
    for key_code in KeyCode::all_keys() {
        supported_codes.insert(key_code_to_evdev(*key_code));
    }
    supported_codes
}

fn map_alphanumeric_and_function_to_evdev(key_code: KeyCode) -> Option<EvdevKeyCode> {
    let evdev_code = match key_code {
        KeyCode::A => EvdevKeyCode::KEY_A,
        KeyCode::B => EvdevKeyCode::KEY_B,
        KeyCode::C => EvdevKeyCode::KEY_C,
        KeyCode::D => EvdevKeyCode::KEY_D,
        KeyCode::E => EvdevKeyCode::KEY_E,
        KeyCode::F => EvdevKeyCode::KEY_F,
        KeyCode::G => EvdevKeyCode::KEY_G,
        KeyCode::H => EvdevKeyCode::KEY_H,
        KeyCode::I => EvdevKeyCode::KEY_I,
        KeyCode::J => EvdevKeyCode::KEY_J,
        KeyCode::K => EvdevKeyCode::KEY_K,
        KeyCode::L => EvdevKeyCode::KEY_L,
        KeyCode::M => EvdevKeyCode::KEY_M,
        KeyCode::N => EvdevKeyCode::KEY_N,
        KeyCode::O => EvdevKeyCode::KEY_O,
        KeyCode::P => EvdevKeyCode::KEY_P,
        KeyCode::Q => EvdevKeyCode::KEY_Q,
        KeyCode::R => EvdevKeyCode::KEY_R,
        KeyCode::S => EvdevKeyCode::KEY_S,
        KeyCode::T => EvdevKeyCode::KEY_T,
        KeyCode::U => EvdevKeyCode::KEY_U,
        KeyCode::V => EvdevKeyCode::KEY_V,
        KeyCode::W => EvdevKeyCode::KEY_W,
        KeyCode::X => EvdevKeyCode::KEY_X,
        KeyCode::Y => EvdevKeyCode::KEY_Y,
        KeyCode::Z => EvdevKeyCode::KEY_Z,
        KeyCode::Digit0 => EvdevKeyCode::KEY_0,
        KeyCode::Digit1 => EvdevKeyCode::KEY_1,
        KeyCode::Digit2 => EvdevKeyCode::KEY_2,
        KeyCode::Digit3 => EvdevKeyCode::KEY_3,
        KeyCode::Digit4 => EvdevKeyCode::KEY_4,
        KeyCode::Digit5 => EvdevKeyCode::KEY_5,
        KeyCode::Digit6 => EvdevKeyCode::KEY_6,
        KeyCode::Digit7 => EvdevKeyCode::KEY_7,
        KeyCode::Digit8 => EvdevKeyCode::KEY_8,
        KeyCode::Digit9 => EvdevKeyCode::KEY_9,
        KeyCode::F1 => EvdevKeyCode::KEY_F1,
        KeyCode::F2 => EvdevKeyCode::KEY_F2,
        KeyCode::F3 => EvdevKeyCode::KEY_F3,
        KeyCode::F4 => EvdevKeyCode::KEY_F4,
        KeyCode::F5 => EvdevKeyCode::KEY_F5,
        KeyCode::F6 => EvdevKeyCode::KEY_F6,
        KeyCode::F7 => EvdevKeyCode::KEY_F7,
        KeyCode::F8 => EvdevKeyCode::KEY_F8,
        KeyCode::F9 => EvdevKeyCode::KEY_F9,
        KeyCode::F10 => EvdevKeyCode::KEY_F10,
        KeyCode::F11 => EvdevKeyCode::KEY_F11,
        KeyCode::F12 => EvdevKeyCode::KEY_F12,
        _ => return None,
    };
    Some(evdev_code)
}

fn map_control_punctuation_and_numpad_to_evdev(key_code: KeyCode) -> EvdevKeyCode {
    match key_code {
        KeyCode::LeftShift => EvdevKeyCode::KEY_LEFTSHIFT,
        KeyCode::RightShift => EvdevKeyCode::KEY_RIGHTSHIFT,
        KeyCode::LeftCtrl => EvdevKeyCode::KEY_LEFTCTRL,
        KeyCode::RightCtrl => EvdevKeyCode::KEY_RIGHTCTRL,
        KeyCode::LeftAlt => EvdevKeyCode::KEY_LEFTALT,
        KeyCode::RightAlt => EvdevKeyCode::KEY_RIGHTALT,
        KeyCode::LeftSuper => EvdevKeyCode::KEY_LEFTMETA,
        KeyCode::RightSuper => EvdevKeyCode::KEY_RIGHTMETA,
        KeyCode::CapsLock => EvdevKeyCode::KEY_CAPSLOCK,
        KeyCode::Up => EvdevKeyCode::KEY_UP,
        KeyCode::Down => EvdevKeyCode::KEY_DOWN,
        KeyCode::Left => EvdevKeyCode::KEY_LEFT,
        KeyCode::Right => EvdevKeyCode::KEY_RIGHT,
        KeyCode::Home => EvdevKeyCode::KEY_HOME,
        KeyCode::End => EvdevKeyCode::KEY_END,
        KeyCode::PageUp => EvdevKeyCode::KEY_PAGEUP,
        KeyCode::PageDown => EvdevKeyCode::KEY_PAGEDOWN,
        KeyCode::Insert => EvdevKeyCode::KEY_INSERT,
        KeyCode::Space => EvdevKeyCode::KEY_SPACE,
        KeyCode::Enter => EvdevKeyCode::KEY_ENTER,
        KeyCode::Tab => EvdevKeyCode::KEY_TAB,
        KeyCode::Escape => EvdevKeyCode::KEY_ESC,
        KeyCode::Backspace => EvdevKeyCode::KEY_BACKSPACE,
        KeyCode::Delete => EvdevKeyCode::KEY_DELETE,
        KeyCode::Pause => EvdevKeyCode::KEY_PAUSE,
        KeyCode::ScrollLock => EvdevKeyCode::KEY_SCROLLLOCK,
        KeyCode::PrintScreen => EvdevKeyCode::KEY_SYSRQ,
        KeyCode::Minus => EvdevKeyCode::KEY_MINUS,
        KeyCode::Equal => EvdevKeyCode::KEY_EQUAL,
        KeyCode::LeftBracket => EvdevKeyCode::KEY_LEFTBRACE,
        KeyCode::RightBracket => EvdevKeyCode::KEY_RIGHTBRACE,
        KeyCode::Backslash => EvdevKeyCode::KEY_BACKSLASH,
        KeyCode::Semicolon => EvdevKeyCode::KEY_SEMICOLON,
        KeyCode::Apostrophe => EvdevKeyCode::KEY_APOSTROPHE,
        KeyCode::Grave => EvdevKeyCode::KEY_GRAVE,
        KeyCode::Comma => EvdevKeyCode::KEY_COMMA,
        KeyCode::Period => EvdevKeyCode::KEY_DOT,
        KeyCode::Slash => EvdevKeyCode::KEY_SLASH,
        KeyCode::NumLock => EvdevKeyCode::KEY_NUMLOCK,
        KeyCode::Numpad0 => EvdevKeyCode::KEY_KP0,
        KeyCode::Numpad1 => EvdevKeyCode::KEY_KP1,
        KeyCode::Numpad2 => EvdevKeyCode::KEY_KP2,
        KeyCode::Numpad3 => EvdevKeyCode::KEY_KP3,
        KeyCode::Numpad4 => EvdevKeyCode::KEY_KP4,
        KeyCode::Numpad5 => EvdevKeyCode::KEY_KP5,
        KeyCode::Numpad6 => EvdevKeyCode::KEY_KP6,
        KeyCode::Numpad7 => EvdevKeyCode::KEY_KP7,
        KeyCode::Numpad8 => EvdevKeyCode::KEY_KP8,
        KeyCode::Numpad9 => EvdevKeyCode::KEY_KP9,
        KeyCode::NumpadAdd => EvdevKeyCode::KEY_KPPLUS,
        KeyCode::NumpadSubtract => EvdevKeyCode::KEY_KPMINUS,
        KeyCode::NumpadMultiply => EvdevKeyCode::KEY_KPASTERISK,
        KeyCode::NumpadDivide => EvdevKeyCode::KEY_KPSLASH,
        KeyCode::NumpadDecimal => EvdevKeyCode::KEY_KPDOT,
        KeyCode::NumpadEnter => EvdevKeyCode::KEY_KPENTER,
        // All alphanumeric and function keys are resolved by map_alphanumeric_and_function_to_evdev.
        KeyCode::A
        | KeyCode::B
        | KeyCode::C
        | KeyCode::D
        | KeyCode::E
        | KeyCode::F
        | KeyCode::G
        | KeyCode::H
        | KeyCode::I
        | KeyCode::J
        | KeyCode::K
        | KeyCode::L
        | KeyCode::M
        | KeyCode::N
        | KeyCode::O
        | KeyCode::P
        | KeyCode::Q
        | KeyCode::R
        | KeyCode::S
        | KeyCode::T
        | KeyCode::U
        | KeyCode::V
        | KeyCode::W
        | KeyCode::X
        | KeyCode::Y
        | KeyCode::Z
        | KeyCode::Digit0
        | KeyCode::Digit1
        | KeyCode::Digit2
        | KeyCode::Digit3
        | KeyCode::Digit4
        | KeyCode::Digit5
        | KeyCode::Digit6
        | KeyCode::Digit7
        | KeyCode::Digit8
        | KeyCode::Digit9
        | KeyCode::F1
        | KeyCode::F2
        | KeyCode::F3
        | KeyCode::F4
        | KeyCode::F5
        | KeyCode::F6
        | KeyCode::F7
        | KeyCode::F8
        | KeyCode::F9
        | KeyCode::F10
        | KeyCode::F11
        | KeyCode::F12 => EvdevKeyCode::KEY_RESERVED,
    }
}

fn map_evdev_to_alphanumeric_and_function(evdev_code: EvdevKeyCode) -> Option<KeyCode> {
    let key_code = match evdev_code {
        EvdevKeyCode::KEY_A => KeyCode::A,
        EvdevKeyCode::KEY_B => KeyCode::B,
        EvdevKeyCode::KEY_C => KeyCode::C,
        EvdevKeyCode::KEY_D => KeyCode::D,
        EvdevKeyCode::KEY_E => KeyCode::E,
        EvdevKeyCode::KEY_F => KeyCode::F,
        EvdevKeyCode::KEY_G => KeyCode::G,
        EvdevKeyCode::KEY_H => KeyCode::H,
        EvdevKeyCode::KEY_I => KeyCode::I,
        EvdevKeyCode::KEY_J => KeyCode::J,
        EvdevKeyCode::KEY_K => KeyCode::K,
        EvdevKeyCode::KEY_L => KeyCode::L,
        EvdevKeyCode::KEY_M => KeyCode::M,
        EvdevKeyCode::KEY_N => KeyCode::N,
        EvdevKeyCode::KEY_O => KeyCode::O,
        EvdevKeyCode::KEY_P => KeyCode::P,
        EvdevKeyCode::KEY_Q => KeyCode::Q,
        EvdevKeyCode::KEY_R => KeyCode::R,
        EvdevKeyCode::KEY_S => KeyCode::S,
        EvdevKeyCode::KEY_T => KeyCode::T,
        EvdevKeyCode::KEY_U => KeyCode::U,
        EvdevKeyCode::KEY_V => KeyCode::V,
        EvdevKeyCode::KEY_W => KeyCode::W,
        EvdevKeyCode::KEY_X => KeyCode::X,
        EvdevKeyCode::KEY_Y => KeyCode::Y,
        EvdevKeyCode::KEY_Z => KeyCode::Z,
        EvdevKeyCode::KEY_0 => KeyCode::Digit0,
        EvdevKeyCode::KEY_1 => KeyCode::Digit1,
        EvdevKeyCode::KEY_2 => KeyCode::Digit2,
        EvdevKeyCode::KEY_3 => KeyCode::Digit3,
        EvdevKeyCode::KEY_4 => KeyCode::Digit4,
        EvdevKeyCode::KEY_5 => KeyCode::Digit5,
        EvdevKeyCode::KEY_6 => KeyCode::Digit6,
        EvdevKeyCode::KEY_7 => KeyCode::Digit7,
        EvdevKeyCode::KEY_8 => KeyCode::Digit8,
        EvdevKeyCode::KEY_9 => KeyCode::Digit9,
        EvdevKeyCode::KEY_F1 => KeyCode::F1,
        EvdevKeyCode::KEY_F2 => KeyCode::F2,
        EvdevKeyCode::KEY_F3 => KeyCode::F3,
        EvdevKeyCode::KEY_F4 => KeyCode::F4,
        EvdevKeyCode::KEY_F5 => KeyCode::F5,
        EvdevKeyCode::KEY_F6 => KeyCode::F6,
        EvdevKeyCode::KEY_F7 => KeyCode::F7,
        EvdevKeyCode::KEY_F8 => KeyCode::F8,
        EvdevKeyCode::KEY_F9 => KeyCode::F9,
        EvdevKeyCode::KEY_F10 => KeyCode::F10,
        EvdevKeyCode::KEY_F11 => KeyCode::F11,
        EvdevKeyCode::KEY_F12 => KeyCode::F12,
        _ => return None,
    };
    Some(key_code)
}

fn map_evdev_to_control_punctuation_and_numpad(evdev_code: EvdevKeyCode) -> Option<KeyCode> {
    let key_code = match evdev_code {
        EvdevKeyCode::KEY_LEFTSHIFT => KeyCode::LeftShift,
        EvdevKeyCode::KEY_RIGHTSHIFT => KeyCode::RightShift,
        EvdevKeyCode::KEY_LEFTCTRL => KeyCode::LeftCtrl,
        EvdevKeyCode::KEY_RIGHTCTRL => KeyCode::RightCtrl,
        EvdevKeyCode::KEY_LEFTALT => KeyCode::LeftAlt,
        EvdevKeyCode::KEY_RIGHTALT => KeyCode::RightAlt,
        EvdevKeyCode::KEY_LEFTMETA => KeyCode::LeftSuper,
        EvdevKeyCode::KEY_RIGHTMETA => KeyCode::RightSuper,
        EvdevKeyCode::KEY_CAPSLOCK => KeyCode::CapsLock,
        EvdevKeyCode::KEY_UP => KeyCode::Up,
        EvdevKeyCode::KEY_DOWN => KeyCode::Down,
        EvdevKeyCode::KEY_LEFT => KeyCode::Left,
        EvdevKeyCode::KEY_RIGHT => KeyCode::Right,
        EvdevKeyCode::KEY_HOME => KeyCode::Home,
        EvdevKeyCode::KEY_END => KeyCode::End,
        EvdevKeyCode::KEY_PAGEUP => KeyCode::PageUp,
        EvdevKeyCode::KEY_PAGEDOWN => KeyCode::PageDown,
        EvdevKeyCode::KEY_INSERT => KeyCode::Insert,
        EvdevKeyCode::KEY_SPACE => KeyCode::Space,
        EvdevKeyCode::KEY_ENTER => KeyCode::Enter,
        EvdevKeyCode::KEY_TAB => KeyCode::Tab,
        EvdevKeyCode::KEY_ESC => KeyCode::Escape,
        EvdevKeyCode::KEY_BACKSPACE => KeyCode::Backspace,
        EvdevKeyCode::KEY_DELETE => KeyCode::Delete,
        EvdevKeyCode::KEY_PAUSE => KeyCode::Pause,
        EvdevKeyCode::KEY_SCROLLLOCK => KeyCode::ScrollLock,
        EvdevKeyCode::KEY_SYSRQ | EvdevKeyCode::KEY_PRINT => KeyCode::PrintScreen,
        EvdevKeyCode::KEY_MINUS => KeyCode::Minus,
        EvdevKeyCode::KEY_EQUAL => KeyCode::Equal,
        EvdevKeyCode::KEY_LEFTBRACE => KeyCode::LeftBracket,
        EvdevKeyCode::KEY_RIGHTBRACE => KeyCode::RightBracket,
        EvdevKeyCode::KEY_BACKSLASH => KeyCode::Backslash,
        EvdevKeyCode::KEY_SEMICOLON => KeyCode::Semicolon,
        EvdevKeyCode::KEY_APOSTROPHE => KeyCode::Apostrophe,
        EvdevKeyCode::KEY_GRAVE => KeyCode::Grave,
        EvdevKeyCode::KEY_COMMA => KeyCode::Comma,
        EvdevKeyCode::KEY_DOT => KeyCode::Period,
        EvdevKeyCode::KEY_SLASH => KeyCode::Slash,
        EvdevKeyCode::KEY_NUMLOCK => KeyCode::NumLock,
        EvdevKeyCode::KEY_KP0 => KeyCode::Numpad0,
        EvdevKeyCode::KEY_KP1 => KeyCode::Numpad1,
        EvdevKeyCode::KEY_KP2 => KeyCode::Numpad2,
        EvdevKeyCode::KEY_KP3 => KeyCode::Numpad3,
        EvdevKeyCode::KEY_KP4 => KeyCode::Numpad4,
        EvdevKeyCode::KEY_KP5 => KeyCode::Numpad5,
        EvdevKeyCode::KEY_KP6 => KeyCode::Numpad6,
        EvdevKeyCode::KEY_KP7 => KeyCode::Numpad7,
        EvdevKeyCode::KEY_KP8 => KeyCode::Numpad8,
        EvdevKeyCode::KEY_KP9 => KeyCode::Numpad9,
        EvdevKeyCode::KEY_KPPLUS => KeyCode::NumpadAdd,
        EvdevKeyCode::KEY_KPMINUS => KeyCode::NumpadSubtract,
        EvdevKeyCode::KEY_KPASTERISK => KeyCode::NumpadMultiply,
        EvdevKeyCode::KEY_KPSLASH => KeyCode::NumpadDivide,
        EvdevKeyCode::KEY_KPDOT => KeyCode::NumpadDecimal,
        EvdevKeyCode::KEY_KPENTER => KeyCode::NumpadEnter,
        _ => return None,
    };
    Some(key_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_mouse_buttons_round_trip_without_collisions() {
        let mut seen_evdev_codes = HashSet::new();
        for mouse_button in MouseButton::all_buttons() {
            let evdev_code = mouse_button_to_evdev(*mouse_button);
            assert!(seen_evdev_codes.insert(evdev_code));
            assert_eq!(evdev_to_mouse_button(evdev_code), Some(*mouse_button));
            assert_eq!(
                evdev_to_trigger(trigger_to_evdev(InputTrigger::Mouse(*mouse_button))),
                Some(InputTrigger::Mouse(*mouse_button))
            );
        }
    }

    #[test]
    fn all_key_codes_round_trip_without_collisions() {
        let mut seen_evdev_codes = HashSet::new();
        for key_code in KeyCode::all_keys() {
            let evdev_code = key_code_to_evdev(*key_code);
            assert_ne!(evdev_code, EvdevKeyCode::KEY_RESERVED);
            assert!(seen_evdev_codes.insert(evdev_code));
            assert_eq!(evdev_to_key_code(evdev_code), Some(*key_code));
            assert_eq!(
                evdev_to_trigger(trigger_to_evdev(InputTrigger::Key(*key_code))),
                Some(InputTrigger::Key(*key_code))
            );
        }
    }

    #[test]
    fn virtual_device_key_set_registers_every_sdk_key_and_button() {
        let registered_keys = all_virtual_device_keys();
        for mouse_button in MouseButton::all_buttons() {
            assert!(registered_keys.contains(mouse_button_to_evdev(*mouse_button)));
        }
        for key_code in KeyCode::all_keys() {
            assert!(registered_keys.contains(key_code_to_evdev(*key_code)));
        }
    }

    #[test]
    fn alternate_print_key_maps_to_print_screen() {
        assert_eq!(
            evdev_to_key_code(EvdevKeyCode::KEY_PRINT),
            Some(KeyCode::PrintScreen)
        );
        assert_eq!(
            evdev_to_key_code(EvdevKeyCode::KEY_SYSRQ),
            Some(KeyCode::PrintScreen)
        );
    }
}
