use std::fmt;

use rand::Rng;
use rand_distr::{Distribution, Normal};
use serde::{Deserialize, Serialize};

use crate::{ScriptError, ScriptResult};

/// Upper cap for any single Gaussian timing bound in milliseconds (10 minutes).
pub const MAX_GAUSSIAN_BOUND_MS: f64 = 600_000.0;

/// Physical or virtual mouse buttons supported by ClickForge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Side,
    Extra,
}

const ALL_MOUSE_BUTTONS: [MouseButton; 5] = [
    MouseButton::Left,
    MouseButton::Right,
    MouseButton::Middle,
    MouseButton::Side,
    MouseButton::Extra,
];

impl MouseButton {
    /// Returns the human-readable UI label for this mouse button.
    pub fn display_label(self) -> &'static str {
        match self {
            Self::Left => "Left Mouse",
            Self::Right => "Right Mouse",
            Self::Middle => "Middle Mouse",
            Self::Side => "Mouse Side (Back)",
            Self::Extra => "Mouse Extra (Forward)",
        }
    }

    /// Returns a compact button label for badges and dense panels.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Left => "LMB",
            Self::Right => "RMB",
            Self::Middle => "MMB",
            Self::Side => "Mouse 4",
            Self::Extra => "Mouse 5",
        }
    }

    /// Returns all supported mouse buttons in display order.
    pub fn all_buttons() -> &'static [MouseButton] {
        &ALL_MOUSE_BUTTONS
    }
}

impl fmt::Display for MouseButton {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.display_label())
    }
}

/// Platform-neutral keyboard key codes mapped to Linux `evdev` and `egui` inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCode {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    LeftShift,
    RightShift,
    LeftCtrl,
    RightCtrl,
    LeftAlt,
    RightAlt,
    LeftSuper,
    RightSuper,
    CapsLock,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Space,
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Pause,
    ScrollLock,
    PrintScreen,
    Minus,
    Equal,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Apostrophe,
    Grave,
    Comma,
    Period,
    Slash,
    NumLock,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    NumpadEnter,
}

const ALL_KEY_CODES: [KeyCode; 103] = [
    KeyCode::A,
    KeyCode::B,
    KeyCode::C,
    KeyCode::D,
    KeyCode::E,
    KeyCode::F,
    KeyCode::G,
    KeyCode::H,
    KeyCode::I,
    KeyCode::J,
    KeyCode::K,
    KeyCode::L,
    KeyCode::M,
    KeyCode::N,
    KeyCode::O,
    KeyCode::P,
    KeyCode::Q,
    KeyCode::R,
    KeyCode::S,
    KeyCode::T,
    KeyCode::U,
    KeyCode::V,
    KeyCode::W,
    KeyCode::X,
    KeyCode::Y,
    KeyCode::Z,
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::F1,
    KeyCode::F2,
    KeyCode::F3,
    KeyCode::F4,
    KeyCode::F5,
    KeyCode::F6,
    KeyCode::F7,
    KeyCode::F8,
    KeyCode::F9,
    KeyCode::F10,
    KeyCode::F11,
    KeyCode::F12,
    KeyCode::LeftShift,
    KeyCode::RightShift,
    KeyCode::LeftCtrl,
    KeyCode::RightCtrl,
    KeyCode::LeftAlt,
    KeyCode::RightAlt,
    KeyCode::LeftSuper,
    KeyCode::RightSuper,
    KeyCode::CapsLock,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Left,
    KeyCode::Right,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::PageUp,
    KeyCode::PageDown,
    KeyCode::Insert,
    KeyCode::Space,
    KeyCode::Enter,
    KeyCode::Tab,
    KeyCode::Escape,
    KeyCode::Backspace,
    KeyCode::Delete,
    KeyCode::Pause,
    KeyCode::ScrollLock,
    KeyCode::PrintScreen,
    KeyCode::Minus,
    KeyCode::Equal,
    KeyCode::LeftBracket,
    KeyCode::RightBracket,
    KeyCode::Backslash,
    KeyCode::Semicolon,
    KeyCode::Apostrophe,
    KeyCode::Grave,
    KeyCode::Comma,
    KeyCode::Period,
    KeyCode::Slash,
    KeyCode::NumLock,
    KeyCode::Numpad0,
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad6,
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad9,
    KeyCode::NumpadAdd,
    KeyCode::NumpadSubtract,
    KeyCode::NumpadMultiply,
    KeyCode::NumpadDivide,
    KeyCode::NumpadDecimal,
    KeyCode::NumpadEnter,
];

impl KeyCode {
    /// Returns the human-readable UI label for this key.
    pub fn display_label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
            Self::F => "F",
            Self::G => "G",
            Self::H => "H",
            Self::I => "I",
            Self::J => "J",
            Self::K => "K",
            Self::L => "L",
            Self::M => "M",
            Self::N => "N",
            Self::O => "O",
            Self::P => "P",
            Self::Q => "Q",
            Self::R => "R",
            Self::S => "S",
            Self::T => "T",
            Self::U => "U",
            Self::V => "V",
            Self::W => "W",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::Digit0 => "0",
            Self::Digit1 => "1",
            Self::Digit2 => "2",
            Self::Digit3 => "3",
            Self::Digit4 => "4",
            Self::Digit5 => "5",
            Self::Digit6 => "6",
            Self::Digit7 => "7",
            Self::Digit8 => "8",
            Self::Digit9 => "9",
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
            Self::LeftShift => "Left Shift",
            Self::RightShift => "Right Shift",
            Self::LeftCtrl => "Left Ctrl",
            Self::RightCtrl => "Right Ctrl",
            Self::LeftAlt => "Left Alt",
            Self::RightAlt => "Right Alt",
            Self::LeftSuper => "Left Super",
            Self::RightSuper => "Right Super",
            Self::CapsLock => "Caps Lock",
            Self::Up => "Up Arrow",
            Self::Down => "Down Arrow",
            Self::Left => "Left Arrow",
            Self::Right => "Right Arrow",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "Page Up",
            Self::PageDown => "Page Down",
            Self::Insert => "Insert",
            Self::Space => "Space",
            Self::Enter => "Enter",
            Self::Tab => "Tab",
            Self::Escape => "Escape",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::Pause => "Pause",
            Self::ScrollLock => "Scroll Lock",
            Self::PrintScreen => "Print Screen",
            Self::Minus => "-",
            Self::Equal => "=",
            Self::LeftBracket => "[",
            Self::RightBracket => "]",
            Self::Backslash => "\\",
            Self::Semicolon => ";",
            Self::Apostrophe => "'",
            Self::Grave => "`",
            Self::Comma => ",",
            Self::Period => ".",
            Self::Slash => "/",
            Self::NumLock => "Num Lock",
            Self::Numpad0 => "Numpad 0",
            Self::Numpad1 => "Numpad 1",
            Self::Numpad2 => "Numpad 2",
            Self::Numpad3 => "Numpad 3",
            Self::Numpad4 => "Numpad 4",
            Self::Numpad5 => "Numpad 5",
            Self::Numpad6 => "Numpad 6",
            Self::Numpad7 => "Numpad 7",
            Self::Numpad8 => "Numpad 8",
            Self::Numpad9 => "Numpad 9",
            Self::NumpadAdd => "Numpad +",
            Self::NumpadSubtract => "Numpad -",
            Self::NumpadMultiply => "Numpad *",
            Self::NumpadDivide => "Numpad /",
            Self::NumpadDecimal => "Numpad .",
            Self::NumpadEnter => "Numpad Enter",
        }
    }

    /// Returns all supported keyboard key codes in deterministic display order.
    pub fn all_keys() -> &'static [KeyCode] {
        &ALL_KEY_CODES
    }

    /// Maps a US-QWERTY ASCII character to `(key_code, requires_shift)`.
    pub fn ascii_char_to_key(character: char) -> Option<(KeyCode, bool)> {
        if character.is_ascii_lowercase() {
            let letter_key = map_lowercase_ascii_letter(character)?;
            return Some((letter_key, false));
        }
        if character.is_ascii_uppercase() {
            let letter_key = map_lowercase_ascii_letter(character.to_ascii_lowercase())?;
            return Some((letter_key, true));
        }
        map_non_letter_ascii_char(character)
    }
}

fn map_lowercase_ascii_letter(character: char) -> Option<KeyCode> {
    let key_code = match character {
        'a' => KeyCode::A,
        'b' => KeyCode::B,
        'c' => KeyCode::C,
        'd' => KeyCode::D,
        'e' => KeyCode::E,
        'f' => KeyCode::F,
        'g' => KeyCode::G,
        'h' => KeyCode::H,
        'i' => KeyCode::I,
        'j' => KeyCode::J,
        'k' => KeyCode::K,
        'l' => KeyCode::L,
        'm' => KeyCode::M,
        'n' => KeyCode::N,
        'o' => KeyCode::O,
        'p' => KeyCode::P,
        'q' => KeyCode::Q,
        'r' => KeyCode::R,
        's' => KeyCode::S,
        't' => KeyCode::T,
        'u' => KeyCode::U,
        'v' => KeyCode::V,
        'w' => KeyCode::W,
        'x' => KeyCode::X,
        'y' => KeyCode::Y,
        'z' => KeyCode::Z,
        _ => return None,
    };
    Some(key_code)
}

fn map_non_letter_ascii_char(character: char) -> Option<(KeyCode, bool)> {
    let mapping = match character {
        '0' => (KeyCode::Digit0, false),
        '1' => (KeyCode::Digit1, false),
        '2' => (KeyCode::Digit2, false),
        '3' => (KeyCode::Digit3, false),
        '4' => (KeyCode::Digit4, false),
        '5' => (KeyCode::Digit5, false),
        '6' => (KeyCode::Digit6, false),
        '7' => (KeyCode::Digit7, false),
        '8' => (KeyCode::Digit8, false),
        '9' => (KeyCode::Digit9, false),
        ')' => (KeyCode::Digit0, true),
        '!' => (KeyCode::Digit1, true),
        '@' => (KeyCode::Digit2, true),
        '#' => (KeyCode::Digit3, true),
        '$' => (KeyCode::Digit4, true),
        '%' => (KeyCode::Digit5, true),
        '^' => (KeyCode::Digit6, true),
        '&' => (KeyCode::Digit7, true),
        '*' => (KeyCode::Digit8, true),
        '(' => (KeyCode::Digit9, true),
        ' ' => (KeyCode::Space, false),
        '\n' | '\r' => (KeyCode::Enter, false),
        '\t' => (KeyCode::Tab, false),
        '-' => (KeyCode::Minus, false),
        '_' => (KeyCode::Minus, true),
        '=' => (KeyCode::Equal, false),
        '+' => (KeyCode::Equal, true),
        '[' => (KeyCode::LeftBracket, false),
        '{' => (KeyCode::LeftBracket, true),
        ']' => (KeyCode::RightBracket, false),
        '}' => (KeyCode::RightBracket, true),
        '\\' => (KeyCode::Backslash, false),
        '|' => (KeyCode::Backslash, true),
        ';' => (KeyCode::Semicolon, false),
        ':' => (KeyCode::Semicolon, true),
        '\'' => (KeyCode::Apostrophe, false),
        '"' => (KeyCode::Apostrophe, true),
        '`' => (KeyCode::Grave, false),
        '~' => (KeyCode::Grave, true),
        ',' => (KeyCode::Comma, false),
        '<' => (KeyCode::Comma, true),
        '.' => (KeyCode::Period, false),
        '>' => (KeyCode::Period, true),
        '/' => (KeyCode::Slash, false),
        '?' => (KeyCode::Slash, true),
        _ => return None,
    };
    Some(mapping)
}

impl fmt::Display for KeyCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.display_label())
    }
}

/// Unified physical input trigger representing either a mouse button or a keyboard key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
pub enum InputTrigger {
    Mouse(MouseButton),
    Key(KeyCode),
}

impl InputTrigger {
    /// Returns the human-readable UI label for this input trigger.
    pub fn display_label(self) -> &'static str {
        match self {
            Self::Mouse(mouse_button) => mouse_button.display_label(),
            Self::Key(key_code) => key_code.display_label(),
        }
    }

    /// Returns the inner key code when this trigger is a keyboard key.
    pub fn key_code(self) -> Option<KeyCode> {
        match self {
            Self::Key(key_code) => Some(key_code),
            Self::Mouse(_) => None,
        }
    }

    /// Returns the inner mouse button when this trigger is a mouse button.
    pub fn mouse_button(self) -> Option<MouseButton> {
        match self {
            Self::Mouse(mouse_button) => Some(mouse_button),
            Self::Key(_) => None,
        }
    }
}

impl fmt::Display for InputTrigger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.display_label())
    }
}

/// Validated parameters for sampling a clamped Gaussian delay in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GaussianRangeMs {
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub std_dev_ms: f64,
}

impl GaussianRangeMs {
    /// Constructs and validates a clamped Gaussian timing distribution in milliseconds.
    pub fn new(min_ms: f64, max_ms: f64, mean_ms: f64, std_dev_ms: f64) -> ScriptResult<Self> {
        let range = Self {
            min_ms,
            max_ms,
            mean_ms,
            std_dev_ms,
        };
        range.validate()?;
        Ok(range)
    }

    /// Validates that all fields are finite, non-negative, ordered, and within safety bounds.
    pub fn validate(&self) -> ScriptResult<()> {
        let all_finite = self.min_ms.is_finite()
            && self.max_ms.is_finite()
            && self.mean_ms.is_finite()
            && self.std_dev_ms.is_finite();
        if !all_finite {
            return Err(ScriptError::InvalidGaussianRange {
                reason: "all Gaussian timing fields must be finite numbers".to_string(),
            });
        }
        if self.min_ms < 0.0 || self.max_ms < 0.0 {
            return Err(ScriptError::InvalidGaussianRange {
                reason: "min_ms and max_ms must be non-negative".to_string(),
            });
        }
        if self.min_ms > self.max_ms {
            return Err(ScriptError::InvalidGaussianRange {
                reason: format!(
                    "min_ms ({}) cannot exceed max_ms ({})",
                    self.min_ms, self.max_ms
                ),
            });
        }
        if self.max_ms > MAX_GAUSSIAN_BOUND_MS {
            return Err(ScriptError::InvalidGaussianRange {
                reason: format!(
                    "max_ms ({}) exceeds limit of {} ms",
                    self.max_ms, MAX_GAUSSIAN_BOUND_MS
                ),
            });
        }
        if self.mean_ms < self.min_ms || self.mean_ms > self.max_ms {
            return Err(ScriptError::InvalidGaussianRange {
                reason: format!(
                    "mean_ms ({}) must fall inside [{}..{}]",
                    self.mean_ms, self.min_ms, self.max_ms
                ),
            });
        }
        if self.std_dev_ms < 0.0 {
            return Err(ScriptError::InvalidGaussianRange {
                reason: "std_dev_ms must be non-negative".to_string(),
            });
        }
        Ok(())
    }

    /// Samples a delay in milliseconds from the Gaussian distribution clamped to `[min_ms, max_ms]`.
    pub fn sample_clamped_ms(&self) -> f64 {
        let mut thread_rng = rand::rng();
        self.sample_clamped_with_rng(&mut thread_rng)
    }

    /// Samples a clamped delay using a caller-provided random number generator.
    pub fn sample_clamped_with_rng<RandomGenerator: Rng + ?Sized>(
        &self,
        random_generator: &mut RandomGenerator,
    ) -> f64 {
        let safe_min_ms = if self.min_ms.is_finite() && self.min_ms >= 0.0 {
            self.min_ms
        } else {
            0.0
        };
        let safe_max_ms = if self.max_ms.is_finite() && self.max_ms >= safe_min_ms {
            self.max_ms
        } else {
            safe_min_ms
        };
        if !self.std_dev_ms.is_finite() || self.std_dev_ms <= 0.0 {
            return self.mean_ms.clamp(safe_min_ms, safe_max_ms);
        }
        let Ok(normal_distribution) = Normal::new(self.mean_ms, self.std_dev_ms) else {
            return self.mean_ms.clamp(safe_min_ms, safe_max_ms);
        };
        let raw_sample_ms = normal_distribution.sample(random_generator);
        if !raw_sample_ms.is_finite() {
            return self.mean_ms.clamp(safe_min_ms, safe_max_ms);
        }
        raw_sample_ms.clamp(safe_min_ms, safe_max_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_range_rejects_invalid_bounds() {
        assert!(GaussianRangeMs::new(20.0, 10.0, 15.0, 2.5).is_err());
        assert!(GaussianRangeMs::new(-1.0, 10.0, 5.0, 2.0).is_err());
        assert!(GaussianRangeMs::new(10.0, 20.0, 25.0, 2.5).is_err());
        assert!(GaussianRangeMs::new(10.0, 20.0, 15.0, -0.5).is_err());
        assert!(GaussianRangeMs::new(f64::NAN, 20.0, 15.0, 2.5).is_err());
    }

    #[test]
    fn gaussian_sampling_stays_within_bounds_with_expected_mean() -> ScriptResult<()> {
        let range_ms = GaussianRangeMs::new(10.0, 20.0, 15.0, 2.5)?;
        let sample_count: usize = 10_000;
        let mut sum_ms = 0.0;
        let mut first_sample_ms: Option<f64> = None;
        let mut has_variance = false;

        for _sample_index in 0..sample_count {
            let sample_ms = range_ms.sample_clamped_ms();
            assert!((10.0..=20.0).contains(&sample_ms));
            let reference_ms = *first_sample_ms.get_or_insert(sample_ms);
            if (sample_ms - reference_ms).abs() > f64::EPSILON {
                has_variance = true;
            }
            sum_ms += sample_ms;
        }

        let observed_mean_ms = sum_ms / (sample_count as f64);
        assert!(has_variance);
        assert!((14.5..=15.5).contains(&observed_mean_ms));
        Ok(())
    }

    #[test]
    fn ascii_character_mapping_handles_letters_symbols_and_whitespace() {
        assert_eq!(KeyCode::ascii_char_to_key('a'), Some((KeyCode::A, false)));
        assert_eq!(KeyCode::ascii_char_to_key('A'), Some((KeyCode::A, true)));
        assert_eq!(
            KeyCode::ascii_char_to_key('5'),
            Some((KeyCode::Digit5, false))
        );
        assert_eq!(
            KeyCode::ascii_char_to_key('%'),
            Some((KeyCode::Digit5, true))
        );
        assert_eq!(
            KeyCode::ascii_char_to_key(' '),
            Some((KeyCode::Space, false))
        );
        assert_eq!(
            KeyCode::ascii_char_to_key('\n'),
            Some((KeyCode::Enter, false))
        );
        assert_eq!(KeyCode::ascii_char_to_key('é'), None);
    }
}
