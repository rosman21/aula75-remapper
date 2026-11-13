//! The AULA F75 physical layout: 80 keys in six rows and the knob's press, each with its matrix index
//! (the same number addresses the key's 4-byte slot in a layer and its LED:
//! `index = column * 6 + row`, from AULA's `KB.ini`), its legend and its factory code.

use crate::keycodes as kc;
use crate::protocol::{key_code, Code, FN_CODE, MOD_LALT, MOD_LCTRL, MOD_LGUI, MOD_LSHIFT};

/// One key on the board.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    /// Matrix / LED index.
    pub index: u8,
    /// The legend.
    pub label: &'static str,
    /// Width in key units (1u = one letter key).
    pub width: f32,
    /// Space before it, in key units.
    pub gap: f32,
    /// The code the keyboard ships with on the base layer.
    pub factory: Code,
}

const fn k(index: u8, label: &'static str, width: f32, gap: f32, factory: Code) -> Key {
    Key {
        index,
        label,
        width,
        gap,
        factory,
    }
}
const fn u(usage: u8) -> Code {
    [0, 0, 0, usage]
}
const fn m(mods: u8) -> Code {
    [0, mods, 0, 0]
}

/// Rows top to bottom; within a row, left to right.
pub const ROWS: &[&[Key]] = &[
    &[
        k(0, "Esc", 1.0, 0.0, u(kc::ESC)),
        k(12, "F1", 1.0, 0.5, u(0x3a)),
        k(18, "F2", 1.0, 0.0, u(0x3b)),
        k(24, "F3", 1.0, 0.0, u(0x3c)),
        k(30, "F4", 1.0, 0.0, u(0x3d)),
        k(36, "F5", 1.0, 0.5, u(0x3e)),
        k(42, "F6", 1.0, 0.0, u(0x3f)),
        k(48, "F7", 1.0, 0.0, u(0x40)),
        k(54, "F8", 1.0, 0.0, u(0x41)),
        k(60, "F9", 1.0, 0.5, u(0x42)),
        k(66, "F10", 1.0, 0.0, u(0x43)),
        k(72, "F11", 1.0, 0.0, u(0x44)),
        k(78, "F12", 1.0, 0.0, u(0x45)),
        k(KNOB_INDEX, "Knob", 1.0, 0.75, KNOB_FACTORY),
    ],
    &[
        k(1, "`", 1.0, 0.0, u(0x35)),
        k(7, "1", 1.0, 0.0, u(0x1e)),
        k(13, "2", 1.0, 0.0, u(0x1f)),
        k(19, "3", 1.0, 0.0, u(0x20)),
        k(25, "4", 1.0, 0.0, u(0x21)),
        k(31, "5", 1.0, 0.0, u(0x22)),
        k(37, "6", 1.0, 0.0, u(0x23)),
        k(43, "7", 1.0, 0.0, u(0x24)),
        k(49, "8", 1.0, 0.0, u(0x25)),
        k(55, "9", 1.0, 0.0, u(0x26)),
        k(61, "0", 1.0, 0.0, u(0x27)),
        k(67, "-", 1.0, 0.0, u(0x2d)),
        k(73, "=", 1.0, 0.0, u(0x2e)),
        k(79, "Backspace", 2.0, 0.0, u(0x2a)),
        k(85, "Del", 1.0, 0.25, u(0x4c)),
    ],
    &[
        k(2, "Tab", 1.5, 0.0, u(0x2b)),
        k(8, "Q", 1.0, 0.0, u(0x14)),
        k(14, "W", 1.0, 0.0, u(0x1a)),
        k(20, "E", 1.0, 0.0, u(0x08)),
        k(26, "R", 1.0, 0.0, u(0x15)),
        k(32, "T", 1.0, 0.0, u(0x17)),
        k(38, "Y", 1.0, 0.0, u(0x1c)),
        k(44, "U", 1.0, 0.0, u(0x18)),
        k(50, "I", 1.0, 0.0, u(0x0c)),
        k(56, "O", 1.0, 0.0, u(0x12)),
        k(62, "P", 1.0, 0.0, u(0x13)),
        k(68, "[", 1.0, 0.0, u(0x2f)),
        k(74, "]", 1.0, 0.0, u(0x30)),
        k(80, "\\", 1.5, 0.0, u(0x31)),
        k(86, "PgUp", 1.0, 0.25, u(0x4b)),
    ],
    &[
        k(3, "Caps", 1.75, 0.0, u(0x39)),
        k(9, "A", 1.0, 0.0, u(0x04)),
        k(15, "S", 1.0, 0.0, u(0x16)),
        k(21, "D", 1.0, 0.0, u(0x07)),
        k(27, "F", 1.0, 0.0, u(0x09)),
        k(33, "G", 1.0, 0.0, u(0x0a)),
        k(39, "H", 1.0, 0.0, u(0x0b)),
        k(45, "J", 1.0, 0.0, u(0x0d)),
        k(51, "K", 1.0, 0.0, u(0x0e)),
        k(57, "L", 1.0, 0.0, u(0x0f)),
        k(63, ";", 1.0, 0.0, u(0x33)),
        k(69, "'", 1.0, 0.0, u(0x34)),
        k(81, "Enter", 2.25, 0.0, u(0x28)),
        k(87, "PgDn", 1.0, 0.25, u(0x4e)),
    ],
    &[
        k(4, "⇧ Shift", 2.25, 0.0, m(MOD_LSHIFT)),
        k(10, "Z", 1.0, 0.0, u(0x1d)),
        k(16, "X", 1.0, 0.0, u(0x1b)),
        k(22, "C", 1.0, 0.0, u(0x06)),
        k(28, "V", 1.0, 0.0, u(0x19)),
        k(34, "B", 1.0, 0.0, u(0x05)),
        k(40, "N", 1.0, 0.0, u(0x11)),
        k(46, "M", 1.0, 0.0, u(0x10)),
        k(52, ",", 1.0, 0.0, u(0x36)),
        k(58, ".", 1.0, 0.0, u(0x37)),
        k(64, "/", 1.0, 0.0, u(0x38)),
        k(70, "⇧ Shift", 1.75, 0.0, m(0x20)),
        k(82, "Up", 1.0, 0.0, u(0x52)),
        k(88, "End", 1.0, 0.25, u(0x4d)),
    ],
    &[
        k(5, "⌃ Ctrl", 1.25, 0.0, m(MOD_LCTRL)),
        k(11, "⌘ Cmd", 1.25, 0.0, m(MOD_LGUI)),
        k(17, "⌥ Opt", 1.25, 0.0, m(MOD_LALT)),
        k(35, "Space", 6.25, 0.0, u(0x2c)),
        k(53, "Fn", 1.0, 0.0, FN_CODE),
        k(59, "⌃ Ctrl", 1.0, 0.0, m(0x10)),
        k(77, "Left", 1.0, 0.25, u(0x50)),
        k(83, "Down", 1.0, 0.0, u(0x51)),
        k(89, "Right", 1.0, 0.0, u(0x4f)),
    ],
];

/// Every key, top-left to bottom-right.
pub fn keys() -> impl Iterator<Item = &'static Key> {
    ROWS.iter().flat_map(|r| r.iter())
}

/// The key right of the left Ctrl, which ships as ⌘ (the Windows key).
pub const LEFT_CMD_INDEX: u8 = 11;
/// The key left of Space, which ships as ⌥ (Alt).
pub const LEFT_OPT_INDEX: u8 = 17;

/// Pressing the knob, right of F12 (over Del). Only the press has a slot in the key layers:
/// turning it is built into the keyboard (the volume in Windows mode; in Mac mode it sends no
/// key at all), so it can't be changed here.
pub const KNOB_INDEX: u8 = 84;
/// The knob's press as shipped: the keyboard's own "next lighting effect".
pub const KNOB_FACTORY: Code = [7, 0, 0, 0x1d];

/// E, whose Fn-layer entry is Fn+E (Mac mode, as shipped).
pub const E_INDEX: u8 = 20;

/// F1-F12's matrix indices.
pub const F_KEYS: [u8; 12] = [12, 18, 24, 30, 36, 42, 48, 54, 60, 66, 72, 78];

/// The key at matrix index `index`.
pub fn key(index: u8) -> Option<&'static Key> {
    keys().find(|k| k.index == index)
}

/// The base layer as it ships.
pub fn factory_base() -> Vec<u8> {
    let mut layer = vec![0u8; crate::protocol::KEYMAP_LEN];
    for k in keys() {
        crate::protocol::set_code(&mut layer, k.index as usize, k.factory);
    }
    layer
}

/// A base layer we are willing to edit: Esc where it belongs and most letters and digits at
/// their factory codes (catches a shifted or truncated read, or another keyboard model).
pub fn base_plausible(layer: &[u8]) -> bool {
    if layer.len() < crate::protocol::KEYMAP_LEN
        || crate::protocol::code_at(layer, 0) != key_code(kc::ESC, 0)
    {
        return false;
    }
    let checks: Vec<bool> = keys()
        .filter(|k| k.label.len() == 1 && k.label.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|k| crate::protocol::code_at(layer, k.index as usize) == k.factory)
        .collect();
    checks.iter().filter(|ok| **ok).count() * 10 >= checks.len() * 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eighty_keys_and_the_knob_with_unique_indexes() {
        let mut seen: Vec<u8> = keys().map(|k| k.index).collect();
        assert_eq!(seen.len(), 81);
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 81);
        // The knob sits over Del, the last column.
        let x = |row: usize, index: u8| -> f32 {
            ROWS[row].iter().take_while(|k| k.index != index).map(|k| k.width + k.gap).sum::<f32>()
                + key(index).unwrap().gap
        };
        assert_eq!(x(0, KNOB_INDEX), x(1, 85));
        assert!(seen
            .iter()
            .all(|i| (*i as usize) < crate::protocol::MATRIX_LEN - 1));
    }

    #[test]
    fn rows_are_about_sixteen_units() {
        for row in ROWS.iter() {
            let w: f32 = row.iter().map(|k| k.width + k.gap).sum();
            assert!((15.0..=16.5).contains(&w), "row width {w}");
        }
    }

    #[test]
    fn the_factory_layer_is_plausible() {
        let mut layer = factory_base();
        assert!(base_plausible(&layer));
        layer.rotate_left(4);
        assert!(!base_plausible(&layer));
    }
}
