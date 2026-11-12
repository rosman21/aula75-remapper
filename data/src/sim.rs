//! A simulated F75 behind a 2.4 GHz receiver, at the packet level: it acknowledges writes,
//! answers reads in 14-byte chunks and keeps what it was sent. The web build, the tests and
//! `AULA_SIMULATE=1` drive it, so the whole session code runs without hardware.

use std::collections::VecDeque;

use crate::layout;
use crate::protocol::{self as p, Link};
use crate::session::Wire;

/// The simulated keyboard's state.
pub struct SimKeyboard {
    /// The LED settings block.
    pub led: Vec<u8>,
    /// Four key layers.
    pub layers: Vec<Vec<u8>>,
    /// The macro buffer, by 512-byte page.
    pub macro_pages: Vec<Vec<u8>>,
    /// Per-key colour planes.
    pub matrix: Vec<u8>,
    /// Linked to its receiver.
    pub online: bool,
    inbox: VecDeque<Vec<u8>>,
    pending: Option<(u8, u8, Vec<u8>)>,
}

impl Default for SimKeyboard {
    fn default() -> Self {
        let mut led = vec![0u8; p::LED_BLOCK_LEN];
        p::edit_led_block(&mut led, 3, 7, 2, 0);
        p::set_sleep(&mut led, 300);
        led[0x38] = 0xff;
        led[0x39] = 0xff;
        // As many keyboards on a Mac are: in Mac mode (Fn+E).
        led[p::OS_MODE_OFFSET] = p::OS_MAC;
        led[126] = 0x5a;
        led[127] = 0xa5;
        let mut fn_layer = vec![0u8; p::KEYMAP_LEN];
        p::set_code(&mut fn_layer, 53, p::FN_CODE);
        SimKeyboard {
            led,
            layers: vec![
                layout::factory_base(),
                fn_layer,
                vec![0; p::KEYMAP_LEN],
                vec![0; p::KEYMAP_LEN],
            ],
            macro_pages: Vec::new(),
            matrix: vec![0; p::MATRIX_LEN * 3],
            online: true,
            inbox: VecDeque::new(),
            pending: None,
        }
    }
}

impl SimKeyboard {
    fn reply(&mut self, cmd: u8, total: u8, index: u8, sub: u8, chunk: &[u8]) {
        let mut pk = [0u8; p::PACKET_LEN];
        pk[0] = p::REPORT_ID;
        pk[1] = cmd;
        pk[2] = total;
        pk[3] = index;
        pk[4] = (sub << 4) | chunk.len() as u8;
        pk[5..5 + chunk.len()].copy_from_slice(chunk);
        pk[19] = p::checksum(&pk);
        self.inbox.push_back(pk.to_vec());
    }

    fn data_for(&self, cmd: u8, sub: u8) -> Option<Vec<u8>> {
        let c = p::WIRELESS;
        match cmd & !c.read_flag {
            x if x == c.led => Some(self.led.clone()),
            x if x == c.keymap => self.layers.get(sub as usize).cloned(),
            x if x == c.led_matrix => Some(self.matrix.clone()),
            x if x == c.macros => Some(self.macro_pages.concat()),
            _ => None,
        }
    }

    fn store(&mut self, cmd: u8, sub: u8, data: Vec<u8>) {
        let c = p::WIRELESS;
        match cmd {
            x if x == c.led => self.led = data,
            x if x == c.keymap => {
                if let Some(layer) = self.layers.get_mut(sub as usize) {
                    *layer = data;
                }
            }
            x if x == c.led_matrix => self.matrix = data,
            x if x == c.macros => {
                let page = sub as usize;
                if page == 0 {
                    self.macro_pages.clear();
                }
                self.macro_pages.resize(page + 1, Vec::new());
                self.macro_pages[page] = data;
            }
            _ => {}
        }
    }
}

impl Wire for SimKeyboard {
    fn link(&self) -> Link {
        Link::Receiver
    }

    fn send_report(&mut self, pk: &[u8]) -> Result<(), String> {
        if pk.len() != p::PACKET_LEN || pk[0] != p::REPORT_ID {
            return Err("not a 20-byte 0x13 report".into());
        }
        if pk[19] != p::checksum(pk) {
            self.reply(pk[1] | 0x80, pk[2], pk[3], 0, &[]);
            return Ok(());
        }
        let (cmd, total, index, sub, len) =
            (pk[1], pk[2], pk[3], pk[4] >> 4, (pk[4] & 0x0f) as usize);
        if cmd == p::WIRELESS_ONLINE {
            let online = u8::from(self.online);
            self.reply(cmd, 1, 0, 0, &[online]);
            return Ok(());
        }
        if !self.online {
            return Ok(()); // asleep: nothing answers
        }
        if cmd & p::WIRELESS.read_flag != 0 {
            if let Some(data) = self.data_for(cmd, sub) {
                let chunks: Vec<Vec<u8>> = data.chunks(p::CHUNK).map(<[u8]>::to_vec).collect();
                let n = chunks.len() as u8;
                for (i, chunk) in chunks.iter().enumerate() {
                    self.reply(cmd, n, i as u8, sub, chunk);
                }
            }
            return Ok(());
        }
        let chunk = pk[5..5 + len.min(p::CHUNK)].to_vec();
        match (&mut self.pending, index) {
            (_, 0) => self.pending = Some((cmd, sub, chunk)),
            (Some((c, s, buf)), _) if *c == cmd && *s == sub => buf.extend(chunk),
            _ => return Err("packet out of order".into()),
        }
        self.reply(cmd, total, index, sub, &[]);
        if index + 1 == total {
            if let Some((c, s, buf)) = self.pending.take() {
                self.store(c, s, buf);
            }
        }
        Ok(())
    }

    fn recv_report(&mut self, _timeout_ms: i32) -> Result<Option<Vec<u8>>, String> {
        Ok(self.inbox.pop_front())
    }

    fn send_feature(&mut self, _frame: &[u8]) -> Result<(), String> {
        Err("the simulator is a receiver".into())
    }

    fn get_feature(&mut self) -> Result<Vec<u8>, String> {
        Err("the simulator is a receiver".into())
    }

    fn pause(&mut self, _ms: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[test]
    fn a_session_reads_and_writes_the_simulator() {
        let mut s = Session::new(Box::new(SimKeyboard::default()));
        assert!(s.online().unwrap());
        assert_eq!(s.lighting().unwrap().mode, 3);
        let l = s.set_lighting(2, 9, 4, 5).unwrap();
        assert_eq!((l.mode, l.brightness, l.speed, l.colour), (2, 9, 4, 5));
        assert_eq!(s.lighting().unwrap(), l);
        let mut base = s.keymap(0).unwrap();
        assert!(layout::base_plausible(&base));
        p::set_code(&mut base, 3, p::key_code(0x29, 0)); // Caps → Esc
        let macros = crate::macros::buffer(&[crate::macros::Macro {
            name: "hi".into(),
            steps: vec![crate::macros::Step::Text {
                text: "hello".repeat(40),
            }],
            mode: crate::macros::Mode::Once,
        }])
        .unwrap();
        s.write_keymap(0, &base, Some(&macros)).unwrap();
        assert_eq!(p::code_at(&s.keymap(0).unwrap(), 3), [0, 0, 0, 0x29]);
    }

    #[test]
    fn an_asleep_keyboard_says_so() {
        let sim = SimKeyboard {
            online: false,
            ..SimKeyboard::default()
        };
        let mut s = Session::new(Box::new(sim));
        assert!(!s.online().unwrap());
        assert_eq!(s.lighting().unwrap_err(), crate::session::ASLEEP);
    }
}
