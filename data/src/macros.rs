//! Macros as people author them (steps) and as the keyboard stores them (events).
//!
//! On the keyboard, the macro buffer is `N × [addr u16 LE, size u16 LE]` (addresses from the
//! buffer's start), then per macro: `name_len u8`, the name in UTF-16LE (≤ 30 chars), and its
//! events, 4 bytes each: `[(type << 4) | released << 7 | delay[19:16], delay[15:8], delay[7:0], usage]`
//! with type 0 a key and 1 a modifier (`0xe0..=0xe7`); the delay (ms) is the wait after the event.
//! Only the used bytes are sent. A key plays a macro when its slot holds `[3, mode, count, index]`.

use serde::{Deserialize, Serialize};

/// Most macros the app keeps.
pub const MAX_MACROS: usize = 16;
/// The buffer's capacity on the keyboard.
pub const BUFFER_CAPACITY: usize = 0x1000;

/// How a macro key plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Once per press.
    #[default]
    Once,
    /// Again and again while held.
    Hold,
    /// Starts on one press, stops on the next.
    Toggle,
}

impl Mode {
    /// The key code's mode byte.
    pub fn byte(self) -> u8 {
        match self {
            Mode::Once => 1,
            Mode::Hold => 2,
            Mode::Toggle => 4,
        }
    }
    /// From the view's id.
    pub fn parse(s: &str) -> Mode {
        match s {
            "hold" => Mode::Hold,
            "toggle" => Mode::Toggle,
            _ => Mode::Once,
        }
    }
    /// The view's id.
    pub fn id(self) -> &'static str {
        match self {
            Mode::Once => "once",
            Mode::Hold => "hold",
            Mode::Toggle => "toggle",
        }
    }
}

/// One authored step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Step {
    /// Type this text (US layout).
    Text {
        /// The text.
        text: String,
    },
    /// Press and release a key with modifiers held (`usage` 0: the modifiers alone).
    Combo {
        /// HID usage of the key.
        usage: u8,
        /// Modifier bits (byte 1 of a key code).
        mods: u8,
    },
    /// Wait.
    Wait {
        /// Milliseconds.
        ms: u32,
    },
}

/// A macro as the app keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Macro {
    /// The name people see (and the keyboard stores).
    pub name: String,
    /// What it does, in order.
    pub steps: Vec<Step>,
    /// How its key plays it.
    #[serde(default)]
    pub mode: Mode,
}

/// One keyboard event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// HID usage.
    pub usage: u8,
    /// Pressed (or released).
    pub down: bool,
    /// Wait after it, in ms.
    pub delay_ms: u32,
}

/// The delay between keystrokes when typing.
pub const TYPE_DELAY_MS: u32 = 12;
const HOLD_MS: u32 = 4;

/// HID usage and Shift for a character on a US layout, or `None`.
pub fn char_usage(ch: char) -> Option<(u8, bool)> {
    const PLAIN: &str = "1234567890\n\t -=[]\\;'`,./";
    const PLAIN_CODES: [u8; 24] = [
        0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2b, 0x2c, 0x2d, 0x2e,
        0x2f, 0x30, 0x31, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
    ];
    const SHIFTED: &str = "!@#$%^&*()";
    const SHIFTED_PUNCT: &str = "_+{}|:\"~<>?";
    const SHIFTED_PUNCT_CODES: [u8; 11] = [
        0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
    ];
    if ch.is_ascii_lowercase() {
        return Some((0x04 + (ch as u8 - b'a'), false));
    }
    if ch.is_ascii_uppercase() {
        return Some((0x04 + (ch as u8 - b'A'), true));
    }
    if let Some(i) = PLAIN.chars().position(|c| c == ch) {
        return Some((PLAIN_CODES[i], false));
    }
    if let Some(i) = SHIFTED.chars().position(|c| c == ch) {
        return Some((PLAIN_CODES[i], true));
    }
    SHIFTED_PUNCT
        .chars()
        .position(|c| c == ch)
        .map(|i| (SHIFTED_PUNCT_CODES[i], true))
}

/// The characters of `text` a macro can't type.
pub fn untypable(text: &str) -> String {
    text.chars().filter(|c| char_usage(*c).is_none()).collect()
}

const LEFT_SHIFT: u8 = 0xe1;

fn mod_usages(mods: u8) -> impl Iterator<Item = u8> {
    (0..8)
        .filter(move |b| mods & (1 << b) != 0)
        .map(|b| 0xe0 + b)
}

/// The events a macro's steps make.
pub fn events(steps: &[Step]) -> Vec<Event> {
    let mut ev: Vec<Event> = Vec::new();
    let wait = |ev: &mut Vec<Event>, ms: u32| {
        if let Some(last) = ev.last_mut() {
            last.delay_ms = last.delay_ms.saturating_add(ms).min(0xfffff);
        }
    };
    for step in steps {
        match step {
            Step::Text { text } => {
                for ch in text.chars() {
                    let Some((usage, shift)) = char_usage(ch) else {
                        continue;
                    };
                    if shift {
                        ev.push(Event {
                            usage: LEFT_SHIFT,
                            down: true,
                            delay_ms: HOLD_MS,
                        });
                    }
                    ev.push(Event {
                        usage,
                        down: true,
                        delay_ms: HOLD_MS,
                    });
                    ev.push(Event {
                        usage,
                        down: false,
                        delay_ms: TYPE_DELAY_MS,
                    });
                    if shift {
                        ev.push(Event {
                            usage: LEFT_SHIFT,
                            down: false,
                            delay_ms: HOLD_MS,
                        });
                    }
                }
            }
            Step::Combo { usage, mods } => {
                let held: Vec<u8> = mod_usages(*mods).collect();
                for m in &held {
                    ev.push(Event {
                        usage: *m,
                        down: true,
                        delay_ms: HOLD_MS,
                    });
                }
                if *usage != 0 {
                    ev.push(Event {
                        usage: *usage,
                        down: true,
                        delay_ms: HOLD_MS * 4,
                    });
                    ev.push(Event {
                        usage: *usage,
                        down: false,
                        delay_ms: HOLD_MS,
                    });
                }
                for m in held.iter().rev() {
                    ev.push(Event {
                        usage: *m,
                        down: false,
                        delay_ms: HOLD_MS,
                    });
                }
                if let Some(last) = ev.last_mut() {
                    last.delay_ms = TYPE_DELAY_MS;
                }
            }
            Step::Wait { ms } => wait(&mut ev, *ms),
        }
    }
    ev
}

fn blob(m: &Macro) -> Vec<u8> {
    let name: Vec<u8> = m
        .name
        .encode_utf16()
        .take(30)
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut b = vec![name.len() as u8];
    b.extend(name);
    for e in events(&m.steps) {
        let d = e.delay_ms.min(0xfffff);
        let kind: u8 = u8::from((0xe0..=0xe7).contains(&e.usage));
        let mut head = (kind << 4) | ((d >> 16) as u8 & 0x0f);
        if !e.down {
            head |= 0x80;
        }
        b.extend([head, (d >> 8) as u8, d as u8, e.usage]);
    }
    b
}

/// The bytes the keyboard takes: the address table, then each macro.
pub fn buffer(macros: &[Macro]) -> Result<Vec<u8>, String> {
    let blobs: Vec<Vec<u8>> = macros.iter().map(blob).collect();
    let mut out = Vec::new();
    let mut addr = macros.len() * 4;
    for b in &blobs {
        out.extend((addr as u16).to_le_bytes());
        out.extend((b.len() as u16).to_le_bytes());
        addr += b.len();
    }
    for b in blobs {
        out.extend(b);
    }
    if out.len() > BUFFER_CAPACITY {
        return Err(format!(
            "The macros take {} bytes; the keyboard holds {}. Shorten some text.",
            out.len(),
            BUFFER_CAPACITY
        ));
    }
    Ok(out)
}

/// A one-line summary of a macro's steps.
pub fn summary(m: &Macro) -> String {
    let parts: Vec<String> = m
        .steps
        .iter()
        .map(|s| match s {
            Step::Text { text } => format!("Type “{}”", text.replace('\n', "⏎")),
            Step::Combo { usage, mods } => {
                format!(
                    "Press {}",
                    crate::keycodes::describe(crate::protocol::key_code(*usage, *mods), &[])
                )
            }
            Step::Wait { ms } => format!("Wait {ms} ms"),
        })
        .collect();
    if parts.is_empty() {
        "No steps yet".into()
    } else {
        parts.join(", then ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_uses_key_and_modifier_events() {
        let m = Macro {
            name: "hi".into(),
            steps: vec![Step::Text { text: "Hi".into() }],
            mode: Mode::Once,
        };
        let b = buffer(&[m]).unwrap();
        // table: addr 4, size = 1 + 4 (name) + 6 events × 4
        assert_eq!(&b[..4], &[4, 0, 29, 0]);
        assert_eq!(&b[4..9], &[4, b'h', 0, b'i', 0]);
        assert_eq!(&b[9..13], &[0x10, 0, 4, 0xe1]); // Shift down: a modifier
        assert_eq!(&b[13..17], &[0x00, 0, 4, 0x0b]); // H down: a key
        assert_eq!(&b[17..21], &[0x80, 0, 12, 0x0b]); // H up: bit 7
        assert_eq!(&b[21..25], &[0x90, 0, 4, 0xe1]); // Shift up
        assert_eq!(b.len(), 4 + 5 + 6 * 4);
    }

    #[test]
    fn combos_hold_their_modifiers_and_waits_extend_the_last_event() {
        let ev = events(&[
            Step::Combo {
                usage: 0x06,
                mods: 0x08,
            },
            Step::Wait { ms: 500 },
        ]);
        let usages: Vec<(u8, bool)> = ev.iter().map(|e| (e.usage, e.down)).collect();
        assert_eq!(
            usages,
            vec![(0xe3, true), (0x06, true), (0x06, false), (0xe3, false)]
        );
        assert_eq!(ev.last().unwrap().delay_ms, TYPE_DELAY_MS + 500);
    }

    #[test]
    fn addresses_follow_each_other() {
        let m = |s: &str| Macro {
            name: "m".into(),
            steps: vec![Step::Text { text: s.into() }],
            mode: Mode::Once,
        };
        let b = buffer(&[m("a"), m("bb")]).unwrap();
        let (a0, s0) = (
            u16::from_le_bytes([b[0], b[1]]) as usize,
            u16::from_le_bytes([b[2], b[3]]) as usize,
        );
        let (a1, s1) = (
            u16::from_le_bytes([b[4], b[5]]) as usize,
            u16::from_le_bytes([b[6], b[7]]) as usize,
        );
        assert_eq!((a0, a1), (8, 8 + s0));
        assert_eq!(a1 + s1, b.len());
    }

    #[test]
    fn too_much_text_is_refused() {
        let m = Macro {
            name: "big".into(),
            steps: vec![Step::Text {
                text: "a".repeat(2000),
            }],
            mode: Mode::Once,
        };
        assert!(buffer(&[m]).is_err());
    }
}
