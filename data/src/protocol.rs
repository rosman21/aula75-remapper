//! The AULA F75 (and its "AULA 75" siblings) configuration protocol.
//!
//! Two transports carry the same payloads:
//!
//! * **2.4 GHz receiver** `3554:fa09`, interface 1, vendor usage page `0xFF02`:
//!   20-byte output/input report `0x13`
//!   `[13 cmd total index (sub<<4)|len data×14 checksum]`, checksum = sum of bytes 0..19.
//!   The same interface also carries the NKRO keyboard, consumer, system and mouse
//!   reports, so replies are filtered on report id `0x13`.
//! * **USB-C cable** `258a:010c`, interface 1: 520-byte feature report `0x06`
//!   `[06 cmd sub 00 total index size_lo size_hi data×512]`; reads set the header with
//!   `cmd | 0x80` and then get the feature report.
//!
//! Protocol facts come from the community's reverse engineering of AULA's own
//! Windows driver (see `docs/PROTOCOL.md` in this repository for sources). This module
//! is pure: framing, block layouts and key codes, with no I/O.

/// USB ids.
pub const RECEIVER_VID: u16 = 0x3554;
/// The 2.4 GHz receiver's product id.
pub const RECEIVER_PID: u16 = 0xfa09;
/// The keyboard over Bluetooth (view only: config traffic disrupts typing).
pub const BLUETOOTH_PID: u16 = 0xfa08;
/// The keyboard on the USB-C cable.
pub const WIRED_VID: u16 = 0x258a;
/// The keyboard on the USB-C cable.
pub const WIRED_PID: u16 = 0x010c;
/// The vendor collection that carries config traffic on the receiver.
pub const VENDOR_USAGE_PAGE: u16 = 0xff02;

/// How the app reaches the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// The 2.4 GHz USB receiver.
    Receiver,
    /// The USB-C cable.
    Wired,
}

impl Link {
    /// The name people see.
    pub fn name(self) -> &'static str {
        match self {
            Link::Receiver => "2.4 GHz receiver",
            Link::Wired => "USB-C cable",
        }
    }
    /// The id the view switches on.
    pub fn id(self) -> &'static str {
        match self {
            Link::Receiver => "receiver",
            Link::Wired => "wired",
        }
    }
}

/// Command numbers per transport (the receiver and the cable use different tables).
#[derive(Clone, Copy, Debug)]
pub struct Cmds {
    /// Key matrix, sub = layer.
    pub keymap: u8,
    /// Per-key LED colours, sub = colour group.
    pub led_matrix: u8,
    /// Macro buffer.
    pub macros: u8,
    /// The 128-byte LED settings block (effect, brightness, speed, sleep…).
    pub led: u8,
    /// Real-time frame, never acknowledged.
    pub realtime: u8,
    /// OR'd onto a command to read it.
    pub read_flag: u8,
}

/// The receiver's command table.
pub const WIRELESS: Cmds = Cmds {
    keymap: 0x01,
    led_matrix: 0x02,
    macros: 0x03,
    led: 0x04,
    realtime: 0x88,
    read_flag: 0x40,
};
/// The cable's command table.
pub const WIRED: Cmds = Cmds {
    keymap: 0x03,
    led_matrix: 0x06,
    macros: 0x05,
    led: 0x04,
    realtime: 0x08,
    read_flag: 0x80,
};
/// Receiver only: one byte, 1 when the keyboard is linked to the receiver (awake and in range).
pub const WIRELESS_ONLINE: u8 = 0x07;

impl Link {
    /// This link's command table.
    pub fn cmds(self) -> Cmds {
        match self {
            Link::Receiver => WIRELESS,
            Link::Wired => WIRED,
        }
    }
}

// ------------------------------------------------------------------ 20-byte receiver framing

/// The receiver's config report id.
pub const REPORT_ID: u8 = 0x13;
/// Bytes on the wire per receiver packet, report id included.
pub const PACKET_LEN: usize = 20;
/// Payload bytes per receiver packet.
pub const CHUNK: usize = 14;

/// Sum of bytes 0..19, the receiver's checksum.
pub fn checksum(p: &[u8]) -> u8 {
    p[..PACKET_LEN - 1]
        .iter()
        .fold(0u8, |s, b| s.wrapping_add(*b))
}

/// The packets that write `data` with `cmd`/`sub` to the receiver.
pub fn write_packets(cmd: u8, sub: u8, data: &[u8]) -> Vec<[u8; PACKET_LEN]> {
    let total = data.len().div_ceil(CHUNK).max(1);
    (0..total)
        .map(|i| {
            let chunk = &data[(i * CHUNK).min(data.len())..((i + 1) * CHUNK).min(data.len())];
            let mut p = [0u8; PACKET_LEN];
            p[0] = REPORT_ID;
            p[1] = cmd;
            p[2] = total as u8;
            p[3] = i as u8;
            p[4] = (sub << 4) | chunk.len() as u8;
            p[5..5 + chunk.len()].copy_from_slice(chunk);
            p[19] = checksum(&p);
            p
        })
        .collect()
}

/// The one packet that asks the receiver for `cmd`'s data.
pub fn read_request(cmd: u8, sub: u8) -> [u8; PACKET_LEN] {
    let mut p = [0u8; PACKET_LEN];
    p[0] = REPORT_ID;
    p[1] = cmd;
    p[2] = 1;
    p[4] = sub << 4;
    p[19] = checksum(&p);
    p
}

/// A receiver reply's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The command it answers (bit 7 cleared).
    pub cmd: u8,
    /// The keyboard reported a checksum error.
    pub crc_error: bool,
    /// Packets in the whole reply.
    pub total: u8,
    /// This packet's index.
    pub index: u8,
    /// Payload bytes in this packet.
    pub len: usize,
}

/// The header of a receiver packet, or `None` for another report.
pub fn parse_reply(p: &[u8]) -> Option<Reply> {
    if p.len() < PACKET_LEN || p[0] != REPORT_ID {
        return None;
    }
    Some(Reply {
        cmd: p[1] & 0x7f,
        crc_error: p[1] & 0x80 != 0 || p[2] & 0x80 != 0,
        total: p[2] & 0x7f,
        index: p[3],
        len: ((p[4] & 0x0f) as usize).min(CHUNK),
    })
}

// ------------------------------------------------------------------ 520-byte cable framing

/// The cable's feature report id.
pub const WIRED_REPORT: u8 = 0x06;
/// Bytes per cable frame, report id included.
pub const WIRED_LEN: usize = 520;
/// Payload bytes per cable frame.
pub const WIRED_CHUNK: usize = 512;

/// The frames that carry `len` bytes of `cmd` over the cable: with `data` a write,
/// without it the headers of a read.
pub fn wired_packets(cmd: u8, sub: u8, len: usize, data: Option<&[u8]>) -> Vec<Vec<u8>> {
    let total = len.div_ceil(WIRED_CHUNK).max(1);
    (0..total)
        .map(|i| {
            let off = i * WIRED_CHUNK;
            let size = (len - off.min(len)).min(WIRED_CHUNK);
            let mut p = vec![0u8; WIRED_LEN];
            p[0] = WIRED_REPORT;
            p[1] = cmd;
            p[2] = sub;
            p[4] = total as u8;
            p[5] = i as u8;
            p[6] = (size & 0xff) as u8;
            p[7] = (size >> 8) as u8;
            if let Some(d) = data {
                p[8..8 + size].copy_from_slice(&d[off..off + size]);
            }
            p
        })
        .collect()
}

// ------------------------------------------------------------------ lighting

/// The LED settings block's length.
pub const LED_BLOCK_LEN: usize = 128;
/// Per-key LEDs: 21 columns × 6 rows, `led = column * 6 + row`.
pub const LED_COUNT: usize = 126;
/// Slots in a key layer.
pub const MATRIX_LEN: usize = 128;
/// Bytes in a key layer.
pub const KEYMAP_LEN: usize = MATRIX_LEN * 4;
/// The per-key "self-define" effect.
pub const CUSTOM_MODE: u8 = 21;
/// Where the sleep timer lives in the LED block.
pub const SLEEP_OFFSET: usize = 0x18;

/// A built-in lighting effect.
#[derive(Clone, Copy, Debug)]
pub struct Effect {
    /// The hardware id.
    pub id: u8,
    /// The name people see.
    pub name: &'static str,
    /// Takes a speed.
    pub speed: bool,
    /// Takes a colour.
    pub colour: bool,
}

/// The effects in LED-block slot order (the slot decides where brightness/speed live).
pub const EFFECTS: &[Effect] = &[
    Effect {
        id: 1,
        name: "Static",
        speed: false,
        colour: true,
    },
    Effect {
        id: 2,
        name: "Breathing",
        speed: true,
        colour: true,
    },
    Effect {
        id: 3,
        name: "Rainbow",
        speed: true,
        colour: false,
    },
    Effect {
        id: 4,
        name: "Flash away",
        speed: true,
        colour: true,
    },
    Effect {
        id: 5,
        name: "Raindrops",
        speed: true,
        colour: true,
    },
    Effect {
        id: 6,
        name: "Rainbow wheel",
        speed: true,
        colour: true,
    },
    Effect {
        id: 7,
        name: "Ripples",
        speed: true,
        colour: true,
    },
    Effect {
        id: 8,
        name: "Stars twinkle",
        speed: true,
        colour: true,
    },
    Effect {
        id: 9,
        name: "Shadow fade",
        speed: true,
        colour: true,
    },
    Effect {
        id: 10,
        name: "Retro snake",
        speed: true,
        colour: true,
    },
    Effect {
        id: 11,
        name: "Neon stream",
        speed: true,
        colour: true,
    },
    Effect {
        id: 12,
        name: "Reaction",
        speed: true,
        colour: true,
    },
    Effect {
        id: 13,
        name: "Sine wave",
        speed: true,
        colour: true,
    },
    Effect {
        id: 14,
        name: "Scanner",
        speed: true,
        colour: true,
    },
    Effect {
        id: 15,
        name: "Windmill",
        speed: true,
        colour: false,
    },
    Effect {
        id: 16,
        name: "Waterfall",
        speed: true,
        colour: false,
    },
    Effect {
        id: 17,
        name: "Blossoming",
        speed: true,
        colour: false,
    },
    Effect {
        id: 18,
        name: "Storm",
        speed: true,
        colour: true,
    },
    Effect {
        id: 21,
        name: "Per-key colours",
        speed: false,
        colour: false,
    },
    Effect {
        id: 0,
        name: "Off",
        speed: false,
        colour: false,
    },
];

/// The colour nibble's palette; 7 is "random".
pub const PALETTE: &[(&str, [u8; 3])] = &[
    ("Red", [255, 0, 0]),
    ("Green", [0, 255, 0]),
    ("Blue", [0, 0, 255]),
    ("Yellow", [255, 255, 0]),
    ("Magenta", [255, 0, 255]),
    ("Cyan", [0, 255, 255]),
    ("White", [255, 255, 255]),
    ("Random", [128, 128, 128]),
];

/// Sleep choices: seconds → the block's hardware value. 0 never sleeps.
pub const SLEEP: &[(u32, u8)] = &[
    (0, 0),
    (60, 2),
    (90, 3),
    (120, 4),
    (180, 6),
    (240, 8),
    (300, 10),
    (600, 20),
    (900, 30),
    (1200, 40),
];

/// The block ends in `5A A5`: anything else is noise, and is never written back.
pub fn led_block_valid(b: &[u8]) -> bool {
    b.len() >= LED_BLOCK_LEN && b[126] == 0x5a && b[127] == 0xa5
}

fn slot(mode: u8) -> Option<usize> {
    EFFECTS.iter().position(|e| e.id == mode)
}

/// Select `mode` with its brightness (0–9), speed (0–4) and colour (0–7).
pub fn edit_led_block(b: &mut [u8], mode: u8, brightness: u8, speed: u8, colour: u8) {
    b[9] = u8::from(mode == CUSTOM_MODE);
    b[10] = mode;
    if let Some(i) = slot(mode) {
        let o = 0x3a + i * 2;
        b[o] = brightness.min(9);
        b[o + 1] = (speed.min(4) << 4) | (colour & 0x0f);
    }
}

/// The effect a block selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lighting {
    /// The hardware effect id.
    pub mode: u8,
    /// 0–9.
    pub brightness: u8,
    /// 0–4.
    pub speed: u8,
    /// A [`PALETTE`] index.
    pub colour: u8,
    /// Seconds before sleep; 0 never.
    pub sleep: u32,
}

/// What a block selects, or `None` when its effect id is unknown.
pub fn lighting_from_block(b: &[u8]) -> Option<Lighting> {
    let i = slot(b[10])?;
    let o = 0x3a + i * 2;
    Some(Lighting {
        mode: b[10],
        brightness: b[o].min(9),
        speed: (b[o + 1] >> 4).min(4),
        colour: (b[o + 1] & 0x0f).min(7),
        sleep: SLEEP
            .iter()
            .find(|s| s.1 == b[SLEEP_OFFSET])
            .map_or(300, |s| s.0),
    })
}

/// Write the sleep timer (seconds from [`SLEEP`]) into a block.
pub fn set_sleep(b: &mut [u8], seconds: u32) {
    b[SLEEP_OFFSET] = SLEEP.iter().find(|s| s.0 == seconds).map_or(10, |s| s.1);
}

/// Per-key colours as the keyboard takes them: three 126-byte planes (R, G, B) padded to 384.
pub fn matrix_payload(colours: &[[u8; 3]]) -> Vec<u8> {
    let mut buf = vec![0u8; MATRIX_LEN * 3];
    for (led, px) in colours.iter().enumerate().take(LED_COUNT) {
        for ch in 0..3 {
            buf[ch * LED_COUNT + led] = px[ch];
        }
    }
    buf
}

// ------------------------------------------------------------------ key codes

/// The 4-byte code a key slot holds:
/// `[0, mods, 0, usage]` a key (with modifiers held), `[2, 0, hi, lo]` a consumer (media) usage,
/// `[3, mode, count, index]` a macro, `[0x0d, 0, 0, 0]` the Fn key, all zeros disabled.
pub type Code = [u8; 4];

/// The base layer.
pub const LAYER_BASE: u8 = 0;
/// The layer while Fn is held.
pub const LAYER_FN: u8 = 1;

/// Modifier bits in a key code's byte 1.
pub const MOD_LCTRL: u8 = 0x01;
/// Left Shift.
pub const MOD_LSHIFT: u8 = 0x02;
/// Left Alt / Option.
pub const MOD_LALT: u8 = 0x04;
/// Left Win / Command.
pub const MOD_LGUI: u8 = 0x08;

/// A key (with `mods` held).
pub fn key_code(usage: u8, mods: u8) -> Code {
    [0, mods, 0, usage]
}
/// A consumer (media) usage.
pub fn consumer_code(usage: u16) -> Code {
    [2, 0, (usage >> 8) as u8, usage as u8]
}
/// Macro `index` (0-based); mode 1 plays `count` times, 2 repeats while held, 4 toggles.
pub fn macro_code(index: u8, mode: u8, count: u8) -> Code {
    [3, mode, count, index]
}
/// The Fn key.
pub const FN_CODE: Code = [0x0d, 0, 0, 0];

/// The code at matrix index `i` of a layer.
pub fn code_at(layer: &[u8], i: usize) -> Code {
    [
        layer[i * 4],
        layer[i * 4 + 1],
        layer[i * 4 + 2],
        layer[i * 4 + 3],
    ]
}

/// Set the code at matrix index `i`.
pub fn set_code(layer: &mut [u8], i: usize, code: Code) {
    layer[i * 4..i * 4 + 4].copy_from_slice(&code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_packets_carry_their_checksum_and_chunks() {
        let data: Vec<u8> = (0..30).collect();
        let ps = write_packets(0x01, 1, &data);
        assert_eq!(ps.len(), 3);
        assert_eq!(&ps[0][..5], &[0x13, 0x01, 3, 0, 0x1e]);
        assert_eq!(ps[2][4], 0x12); // sub 1, 2 bytes left
        for p in &ps {
            assert_eq!(p[19], checksum(p));
        }
        assert_eq!(read_request(0x44, 0), {
            let mut p = [0u8; 20];
            p[0] = 0x13;
            p[1] = 0x44;
            p[2] = 1;
            p[19] = 0x13 + 0x44 + 1;
            p
        });
    }

    #[test]
    fn replies_are_filtered_by_report_id() {
        assert_eq!(parse_reply(&[0x04; 20]), None);
        let mut p = [0u8; 20];
        p[..5].copy_from_slice(&[0x13, 0x41, 0x25, 3, 0x0e]);
        assert_eq!(
            parse_reply(&p),
            Some(Reply {
                cmd: 0x41,
                crc_error: false,
                total: 0x25,
                index: 3,
                len: 14
            })
        );
        p[1] = 0xc1;
        assert!(parse_reply(&p).unwrap().crc_error);
    }

    #[test]
    fn cable_frames_split_at_512() {
        let data = vec![7u8; 600];
        let ps = wired_packets(0x05, 0, data.len(), Some(&data));
        assert_eq!(ps.len(), 2);
        assert_eq!(&ps[0][..8], &[0x06, 0x05, 0, 0, 2, 0, 0x00, 0x02]);
        assert_eq!(&ps[1][..8], &[0x06, 0x05, 0, 0, 2, 1, 88, 0]);
        assert_eq!(ps[1][8 + 87], 7);
        assert_eq!(ps[1][8 + 88], 0);
    }

    #[test]
    fn led_block_round_trips_an_effect() {
        let mut b = vec![0u8; 128];
        b[126] = 0x5a;
        b[127] = 0xa5;
        assert!(led_block_valid(&b));
        edit_led_block(&mut b, 2, 7, 3, 4);
        set_sleep(&mut b, 600);
        assert_eq!(
            lighting_from_block(&b),
            Some(Lighting {
                mode: 2,
                brightness: 7,
                speed: 3,
                colour: 4,
                sleep: 600
            })
        );
        assert_eq!(b[0x3c], 7); // Breathing is slot 1
        assert_eq!(b[0x3d], 0x34);
        edit_led_block(&mut b, CUSTOM_MODE, 9, 0, 0);
        assert_eq!(b[9], 1);
    }

    #[test]
    fn codes_sit_four_bytes_a_slot() {
        let mut layer = vec![0u8; KEYMAP_LEN];
        set_code(&mut layer, 2, key_code(0x06, MOD_LGUI));
        assert_eq!(&layer[8..12], &[0, 0x08, 0, 0x06]);
        assert_eq!(code_at(&layer, 2), key_code(0x06, MOD_LGUI));
        assert_eq!(consumer_code(0xcd), [2, 0, 0, 0xcd]);
        assert_eq!(consumer_code(0x192), [2, 0, 1, 0x92]);
    }
}
