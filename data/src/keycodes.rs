//! What a key can do: the targets people pick from, and the names of codes read back.
//!
//! A target id is a stable string the view passes back:
//! `k:<usage>` a key, `c:<usage>` a media/consumer usage, `m:<mods>` a modifier alone,
//! `macro:<n>` macro n (0-based), `fn`, `none`, and `s:<usage>:<mods>` a ready-made shortcut
//! (⇧⌘4 and the like). Modifiers held with a `k:` key are passed beside it.

use crate::protocol::{consumer_code, key_code, macro_code, Code, FN_CODE};

/// HID usage of Escape.
pub const ESC: u8 = 0x29;

/// One pickable target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Stable id the view passes back.
    pub id: String,
    /// The name people see.
    pub label: String,
    /// The section it is listed under.
    pub group: &'static str,
}

const NAMED: &[(&str, u8)] = &[
    ("Enter", 0x28),
    ("Esc", 0x29),
    ("Backspace", 0x2a),
    ("Tab", 0x2b),
    ("Space", 0x2c),
    ("-", 0x2d),
    ("=", 0x2e),
    ("[", 0x2f),
    ("]", 0x30),
    ("\\", 0x31),
    (";", 0x33),
    ("'", 0x34),
    ("`", 0x35),
    (",", 0x36),
    (".", 0x37),
    ("/", 0x38),
    ("Caps Lock", 0x39),
    ("Print Screen", 0x46),
    ("Scroll Lock", 0x47),
    ("Pause", 0x48),
    ("Insert", 0x49),
    ("Home", 0x4a),
    ("Page Up", 0x4b),
    ("Delete", 0x4c),
    ("End", 0x4d),
    ("Page Down", 0x4e),
    ("Right", 0x4f),
    ("Left", 0x50),
    ("Down", 0x51),
    ("Up", 0x52),
    ("Menu", 0x65),
];

const MEDIA: &[(&str, u16)] = &[
    ("Play / Pause", 0xcd),
    ("Next Track", 0xb5),
    ("Previous Track", 0xb6),
    ("Stop", 0xb7),
    ("Mute", 0xe2),
    ("Volume Up", 0xe9),
    ("Volume Down", 0xea),
    ("Brightness Up", 0x6f),
    ("Brightness Down", 0x70),
    ("Calculator", 0x192),
    ("Browser", 0x196),
    ("Mail", 0x18a),
    ("Search", 0x221),
];

/// Modifier bits with their Mac names.
pub const MODIFIERS: &[(&str, u8)] = &[
    ("Left Command", 0x08),
    ("Left Option", 0x04),
    ("Left Control", 0x01),
    ("Left Shift", 0x02),
    ("Right Command", 0x80),
    ("Right Option", 0x40),
    ("Right Control", 0x10),
    ("Right Shift", 0x20),
];

/// The label of a key usage, if it has one.
pub fn usage_label(usage: u8) -> Option<String> {
    match usage {
        0x04..=0x1d => Some(((b'A' + usage - 0x04) as char).to_string()),
        0x1e..=0x26 => Some(((b'1' + usage - 0x1e) as char).to_string()),
        0x27 => Some("0".into()),
        0x3a..=0x45 => Some(format!("F{}", usage - 0x39)),
        0x68..=0x73 => Some(format!("F{}", usage - 0x68 + 13)),
        _ => NAMED.iter().find(|n| n.1 == usage).map(|n| n.0.to_string()),
    }
}

/// Ready-made Mac shortcuts, one click each: (group, name, key usage, modifiers).
/// Modifier bits: ⌃ 0x01, ⇧ 0x02, ⌥ 0x04, ⌘ 0x08.
pub const SHORTCUTS: &[(&str, &str, u8, u8)] = &[
    ("Screenshots", "Whole screen", 0x20, 0x0a),          // ⇧⌘3
    ("Screenshots", "Selected area", 0x21, 0x0a),         // ⇧⌘4
    ("Screenshots", "Screenshot and record", 0x22, 0x0a), // ⇧⌘5
    ("Screenshots", "Area to clipboard", 0x21, 0x0b),     // ⌃⇧⌘4
    ("Screenshots", "Screen to clipboard", 0x20, 0x0b),   // ⌃⇧⌘3
    ("Mac shortcuts", "Copy", 0x06, 0x08),
    ("Mac shortcuts", "Paste", 0x19, 0x08),
    ("Mac shortcuts", "Cut", 0x1b, 0x08),
    ("Mac shortcuts", "Undo", 0x1d, 0x08),
    ("Mac shortcuts", "Redo", 0x1d, 0x0a),
    ("Mac shortcuts", "Select all", 0x04, 0x08),
    ("Mac shortcuts", "Find", 0x09, 0x08),
    ("Mac shortcuts", "Save", 0x16, 0x08),
    ("Mac shortcuts", "Spotlight", 0x2c, 0x08),
    ("Mac shortcuts", "Switch apps", 0x2b, 0x08),
    ("Mac shortcuts", "Mission Control", 0x52, 0x01),
    ("Mac shortcuts", "Emoji & symbols", 0x2c, 0x09),
    ("Mac shortcuts", "Lock screen", 0x14, 0x09),
    ("Mac shortcuts", "Force quit", 0x29, 0x0c),
    ("Mac shortcuts", "New tab", 0x17, 0x08),
    ("Mac shortcuts", "Close window", 0x1a, 0x08),
    ("Mac shortcuts", "Quit app", 0x14, 0x08),
];

/// The ready-made shortcut a code is, if any: its target id and name.
pub fn shortcut_of(code: Code) -> Option<(String, &'static str)> {
    let [0, mods, _, usage] = code else { return None };
    SHORTCUTS
        .iter()
        .find(|s| s.2 == usage && s.3 == mods)
        .map(|s| (format!("s:{}:{}", s.2, s.3), s.1))
}

/// The target a code shows as chosen in the picker: a ready-made shortcut when it is one.
pub fn selected(code: Code) -> (String, u8) {
    let (id, mods) = target_of(code);
    (shortcut_of(code).map_or(id, |s| s.0), mods)
}

/// Everything a key can be set to (macros listed by the caller, who knows their names).
pub fn targets(macros: &[String]) -> Vec<Target> {
    let mut v = vec![
        Target {
            id: "none".into(),
            label: "Disabled".into(),
            group: "Special",
        },
        Target {
            id: "fn".into(),
            label: "Fn".into(),
            group: "Special",
        },
    ];
    v.extend(SHORTCUTS.iter().map(|&(group, name, usage, mods)| Target {
        id: format!("s:{usage}:{mods}"),
        label: format!("{name} {}", describe(key_code(usage, mods), &[])),
        group,
    }));
    let key = |usage: u8, group| Target {
        id: format!("k:{usage}"),
        label: usage_label(usage).unwrap_or_default(),
        group,
    };
    v.extend((0x04..=0x1d).map(|u| key(u, "Letters")));
    v.extend((0x1e..=0x27).map(|u| key(u, "Numbers")));
    v.extend(
        (0x3a..=0x45)
            .chain(0x68..=0x73)
            .map(|u| key(u, "Function keys")),
    );
    v.extend(NAMED.iter().map(|n| key(n.1, "Editing and navigation")));
    v.extend(MODIFIERS.iter().map(|(name, bit)| Target {
        id: format!("m:{bit}"),
        label: (*name).into(),
        group: "Modifiers",
    }));
    v.extend(MEDIA.iter().map(|(name, usage)| Target {
        id: format!("c:{usage}"),
        label: (*name).into(),
        group: "Media and system",
    }));
    v.extend(macros.iter().enumerate().map(|(i, name)| Target {
        id: format!("macro:{i}"),
        label: name.clone(),
        group: "Macros",
    }));
    v
}

/// The code for target `id` with modifiers `mods` held (keys only), or `None` for an unknown id.
pub fn code_for(id: &str, mods: u8) -> Option<Code> {
    let (kind, rest) = id.split_once(':').unwrap_or((id, ""));
    match kind {
        "none" => Some([0; 4]),
        "fn" => Some(FN_CODE),
        "k" => rest.parse().ok().map(|u| key_code(u, mods)),
        "s" => {
            let (u, m) = rest.split_once(':')?;
            Some(key_code(u.parse().ok()?, m.parse().ok()?))
        }
        "m" => rest.parse::<u8>().ok().map(|b| key_code(0, b | mods)),
        "c" => rest.parse().ok().map(consumer_code),
        "macro" => rest.parse().ok().map(|i| macro_code(i, 1, 1)),
        _ => None,
    }
}

/// The target id and held modifiers of a code, for selecting it in the picker.
pub fn target_of(code: Code) -> (String, u8) {
    match code {
        [0, 0, 0, 0] => ("none".into(), 0),
        c if c == FN_CODE => ("fn".into(), 0),
        [0, mods, _, 0] => (format!("m:{mods}"), 0),
        [0, mods, _, usage] => (format!("k:{usage}"), mods),
        [2, _, hi, lo] => (format!("c:{}", u16::from(hi) << 8 | u16::from(lo)), 0),
        [3, _, _, index] => (format!("macro:{index}"), 0),
        _ => ("none".into(), 0),
    }
}

fn mod_prefix(mods: u8) -> String {
    let mut s = String::new();
    for (bits, sym) in [(0x11u8, "⌃"), (0x44, "⌥"), (0x22, "⇧"), (0x88, "⌘")] {
        if mods & bits != 0 {
            s.push_str(sym);
        }
    }
    s
}

/// A short name for what a code does: `⌘C`, `Volume Up`, `Macro 2`, `Disabled`.
pub fn describe(code: Code, macros: &[String]) -> String {
    match code {
        [0, 0, 0, 0] => "Disabled".into(),
        c if c == FN_CODE => "Fn".into(),
        // The keyboard's own functions (Fn layer as shipped).
        [7, 0, 0, 0x18] => "Windows mode".into(),
        [7, 0, 0, 0x19] => "Android mode".into(),
        [7, 0, 0, 0x1a] => "Mac mode".into(),
        [0, mods, _, 0] => {
            let names: Vec<&str> = MODIFIERS
                .iter()
                .filter(|m| mods & m.1 != 0)
                .map(|m| m.0)
                .collect();
            if names.len() == 1 {
                names[0].to_string()
            } else {
                mod_prefix(mods)
            }
        }
        [0, mods, _, usage] => {
            format!(
                "{}{}",
                mod_prefix(mods),
                usage_label(usage).unwrap_or_else(|| format!("0x{usage:02x}"))
            )
        }
        [2, _, hi, lo] => {
            let usage = u16::from(hi) << 8 | u16::from(lo);
            MEDIA
                .iter()
                .find(|m| m.1 == usage)
                .map_or_else(|| format!("Media 0x{usage:x}"), |m| m.0.to_string())
        }
        [3, _, _, index] => macros
            .get(index as usize)
            .cloned()
            .unwrap_or_else(|| format!("Macro {}", index + 1)),
        c => c
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{MOD_LGUI, MOD_LSHIFT};

    #[test]
    fn targets_round_trip_through_codes() {
        for t in targets(&["Hello".into()]) {
            let code = code_for(&t.id, 0).unwrap();
            assert_eq!(selected(code).0, t.id, "{}", t.label);
        }
    }

    #[test]
    fn combos_keep_their_modifiers() {
        let code = code_for("k:6", MOD_LGUI).unwrap();
        assert_eq!(code, [0, 0x08, 0, 6]);
        assert_eq!(target_of(code), ("k:6".into(), MOD_LGUI));
        assert_eq!(describe(code, &[]), "⌘C");
        assert_eq!(
            describe(code_for("k:33", MOD_LGUI | MOD_LSHIFT).unwrap(), &[]),
            "⇧⌘4"
        );
        assert_eq!(describe(code_for("m:2", 0).unwrap(), &[]), "Left Shift");
        // A ready-made shortcut is the same code as building it by hand, and is shown as itself.
        let shot = code_for("s:33:10", 0).unwrap();
        assert_eq!(shot, code_for("k:33", MOD_LGUI | MOD_LSHIFT).unwrap());
        assert_eq!(selected(shot), ("s:33:10".into(), MOD_LGUI | MOD_LSHIFT));
        assert_eq!(shortcut_of(shot).map(|s| s.1), Some("Selected area"));
        assert_eq!(describe(code_for("c:233", 0).unwrap(), &[]), "Volume Up");
    }
}
