//! A conversation with the keyboard over some [`Wire`]: acknowledged writes, reads
//! reassembled from packets, and the jobs the app runs (lighting, key layers, macros).
//!
//! The receiver protocol follows AULA's driver: wait a few ms before each packet, read
//! replies until one answers the command, check its index, retry a CRC error (5 tries).

use crate::protocol::{self as p, Link};

/// Raw I/O with one keyboard interface.
pub trait Wire: Send {
    /// How it is connected.
    fn link(&self) -> Link;
    /// Receiver: write one 20-byte output report (report id first).
    fn send_report(&mut self, packet: &[u8]) -> Result<(), String>;
    /// Receiver: the next input report (report id first), or `None` after `timeout_ms`.
    fn recv_report(&mut self, timeout_ms: i32) -> Result<Option<Vec<u8>>, String>;
    /// Cable: set a 520-byte feature report (report id first).
    fn send_feature(&mut self, frame: &[u8]) -> Result<(), String>;
    /// Cable: get feature report 0x06 (report id first).
    fn get_feature(&mut self) -> Result<Vec<u8>, String>;
    /// Wait between packets (the simulator doesn't).
    fn pause(&mut self, ms: u64) {
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(std::time::Duration::from_millis(ms));
        #[cfg(target_arch = "wasm32")]
        let _ = ms;
    }
    /// A whole write of one command, when the wire does it itself (the browser's page module):
    /// `None` leaves it to [`Session`]'s packets.
    fn block_write(&mut self, _cmd: u8, _sub: u8, _data: &[u8], _delay_ms: u64) -> Option<Result<(), String>> {
        None
    }
    /// A whole read of one command (`cmd` already carries the read flag), when the wire does it
    /// itself.
    fn block_read(&mut self, _cmd: u8, _sub: u8, _len: usize) -> Option<Result<Vec<u8>, String>> {
        None
    }
}

/// A keyboard to talk to.
pub struct Session {
    wire: Box<dyn Wire>,
    cmds: p::Cmds,
}

/// Said when the receiver answers nothing: the keyboard is asleep or out of range.
pub const ASLEEP: &str =
    "The keyboard didn't answer. Press any key on it to wake it, then try again.";

impl Session {
    /// Talk over `wire`.
    pub fn new(wire: Box<dyn Wire>) -> Session {
        let cmds = wire.link().cmds();
        Session { wire, cmds }
    }

    /// Wait (the simulator doesn't).
    pub fn pause(&mut self, ms: u64) {
        self.wire.pause(ms);
    }

    /// How it is connected.
    pub fn link(&self) -> Link {
        self.wire.link()
    }

    // -------------------------------------------------------------- receiver

    fn recv_packet(&mut self, timeout_ms: i32) -> Result<Option<(Vec<u8>, p::Reply)>, String> {
        // Typing also arrives on this interface (NKRO, media, mouse reports): skip it.
        for _ in 0..64 {
            match self.wire.recv_report(timeout_ms)? {
                None => return Ok(None),
                Some(pk) => {
                    if let Some(r) = p::parse_reply(&pk) {
                        return Ok(Some((pk, r)));
                    }
                }
            }
        }
        Ok(None)
    }

    fn drain(&mut self) -> Result<(), String> {
        while self.wire.recv_report(0)?.is_some() {}
        Ok(())
    }

    fn wireless_write(
        &mut self,
        cmd: u8,
        sub: u8,
        data: &[u8],
        delay_ms: u64,
    ) -> Result<(), String> {
        let wants_ack = cmd & 0x80 == 0 && cmd != 0x06;
        let packets = p::write_packets(cmd, sub, data);
        for (i, packet) in packets.iter().enumerate() {
            let mut tries = 0;
            loop {
                self.wire.pause(delay_ms);
                self.drain()?;
                self.wire.send_report(packet)?;
                if !wants_ack {
                    break;
                }
                let mut ack = None;
                for _ in 0..20 {
                    match self.recv_packet(1000)? {
                        None => break,
                        Some((pk, r)) if pk[1] & 0x7f == cmd => {
                            ack = Some((pk[0], r));
                            break;
                        }
                        Some(_) => {}
                    }
                }
                match ack {
                    Some((_, r)) if !r.crc_error && r.index as usize == i => break,
                    Some((_, r)) if !r.crc_error => {
                        return Err(format!("The keyboard acknowledged packet {} instead of {i} (command 0x{cmd:02x}).", r.index));
                    }
                    _ if tries >= 4 => return Err(ASLEEP.into()),
                    _ => tries += 1,
                }
            }
        }
        Ok(())
    }

    fn wireless_read(&mut self, cmd: u8, sub: u8, len: usize) -> Result<Vec<u8>, String> {
        let mut out = vec![0u8; len.max(p::CHUNK)];
        let mut got: Vec<bool> = Vec::new();
        for attempt in 0..4 {
            if attempt > 0 {
                self.wire.pause(60);
            }
            self.drain()?;
            self.wire.send_report(&p::read_request(cmd, sub))?;
            while let Some((pk, r)) = self.recv_packet(500)? {
                if r.cmd != cmd & 0x7f {
                    continue;
                }
                if r.crc_error {
                    break;
                }
                let total = r.total.max(1) as usize;
                if got.len() != total {
                    got = vec![false; total];
                }
                let i = r.index as usize;
                if i >= total {
                    continue;
                }
                let off = i * p::CHUNK;
                if out.len() < off + r.len {
                    out.resize(off + r.len, 0);
                }
                out[off..off + r.len].copy_from_slice(&pk[5..5 + r.len]);
                got[i] = true;
                if got.iter().all(|g| *g) {
                    out.truncate(len);
                    return Ok(out);
                }
            }
        }
        Err(ASLEEP.into())
    }

    // -------------------------------------------------------------- cable

    fn wired_send(&mut self, frame: &[u8]) -> Result<(), String> {
        let mut last = String::new();
        for _ in 0..3 {
            match self.wire.send_feature(frame) {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
            self.wire.pause(70);
        }
        Err(format!("The keyboard refused a feature report: {last}"))
    }

    fn wired_write(&mut self, cmd: u8, sub: u8, data: &[u8]) -> Result<(), String> {
        for frame in p::wired_packets(cmd, sub, data.len(), Some(data)) {
            self.wire.pause(20);
            self.wired_send(&frame)?;
        }
        Ok(())
    }

    fn wired_read(&mut self, cmd: u8, sub: u8, len: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(len);
        for frame in p::wired_packets(cmd, sub, len, None) {
            let size = frame[6] as usize | (frame[7] as usize) << 8;
            self.wire.pause(20);
            self.wired_send(&frame)?;
            self.wire.pause(20);
            let mut buf = self.wire.get_feature()?;
            if buf.first() != Some(&p::WIRED_REPORT) {
                buf.insert(0, p::WIRED_REPORT); // a host that strips the id
            }
            if buf.len() < 8 + size {
                return Err(format!(
                    "The keyboard sent a short reply ({} bytes).",
                    buf.len()
                ));
            }
            out.extend_from_slice(&buf[8..8 + size]);
        }
        Ok(out)
    }

    // -------------------------------------------------------------- either

    fn write(&mut self, cmd: u8, sub: u8, data: &[u8], delay_ms: u64) -> Result<(), String> {
        if let Some(r) = self.wire.block_write(cmd, sub, data, delay_ms) {
            return r;
        }
        match self.link() {
            Link::Receiver => self.wireless_write(cmd, sub, data, delay_ms),
            Link::Wired => self.wired_write(cmd, sub, data),
        }
    }

    fn read(&mut self, cmd: u8, sub: u8, len: usize) -> Result<Vec<u8>, String> {
        let rc = cmd | self.cmds.read_flag;
        if let Some(r) = self.wire.block_read(rc, sub, len) {
            return r;
        }
        match self.link() {
            Link::Receiver => self.wireless_read(rc, sub, len),
            Link::Wired => self.wired_read(rc, sub, len),
        }
    }

    /// A whole read the page module was asked for (`cmd` with its read flag), over packets.
    pub fn read_raw(&mut self, cmd: u8, sub: u8, len: usize) -> Result<Vec<u8>, String> {
        match self.link() {
            Link::Receiver => self.wireless_read(cmd, sub, len),
            Link::Wired => self.wired_read(cmd, sub, len),
        }
    }

    /// A whole write the page module was asked for, over packets.
    pub fn write_raw(&mut self, cmd: u8, sub: u8, data: &[u8], delay_ms: u64) -> Result<(), String> {
        self.write(cmd, sub, data, delay_ms)
    }

    // -------------------------------------------------------------- jobs

    /// Receiver: is the keyboard linked (awake and in range)? The cable: always.
    pub fn online(&mut self) -> Result<bool, String> {
        match self.link() {
            Link::Receiver => match self
                .wire
                .block_read(p::WIRELESS_ONLINE, 0, 1)
                .unwrap_or_else(|| self.wireless_read(p::WIRELESS_ONLINE, 0, 1))
            {
                Ok(b) => Ok(b[0] != 0),
                Err(e) if e == ASLEEP => Ok(false),
                Err(e) => Err(e),
            },
            Link::Wired => Ok(true),
        }
    }

    /// The 128-byte LED settings block, checked.
    pub fn led_block(&mut self) -> Result<Vec<u8>, String> {
        let b = self.read(self.cmds.led, 0, p::LED_BLOCK_LEN)?;
        if !p::led_block_valid(&b) {
            return Err("The keyboard's settings block didn't end in 5A A5, so nothing was written. Is this an AULA F75?".into());
        }
        Ok(b)
    }

    fn edit_block(&mut self, f: impl FnOnce(&mut [u8])) -> Result<p::Lighting, String> {
        let mut b = self.led_block()?;
        f(&mut b);
        self.write(self.cmds.led, 0, &b, 15)?;
        p::lighting_from_block(&b)
            .ok_or_else(|| "The keyboard reports an effect this app doesn't know.".into())
    }

    /// The keyboard's OS mode (Fn+W / Fn+E / Fn+Q), settings byte 27.
    pub fn os_mode(&mut self) -> Result<u8, String> {
        Ok(self.led_block()?[p::OS_MODE_OFFSET])
    }

    /// Set the OS mode, then read it back.
    pub fn set_os_mode(&mut self, mode: u8) -> Result<u8, String> {
        let mut b = self.led_block()?;
        b[p::OS_MODE_OFFSET] = mode;
        self.write(self.cmds.led, 0, &b, 15)?;
        self.os_mode()
    }

    /// The selected effect and sleep timer.
    pub fn lighting(&mut self) -> Result<p::Lighting, String> {
        let b = self.led_block()?;
        p::lighting_from_block(&b).ok_or_else(|| {
            format!(
                "The keyboard reports effect {}, which this app doesn't know.",
                b[10]
            )
        })
    }

    /// Select an effect.
    pub fn set_lighting(
        &mut self,
        mode: u8,
        brightness: u8,
        speed: u8,
        colour: u8,
    ) -> Result<p::Lighting, String> {
        self.edit_block(|b| p::edit_led_block(b, mode, brightness, speed, colour))
    }

    /// Set the sleep timer.
    pub fn set_sleep(&mut self, seconds: u32) -> Result<p::Lighting, String> {
        self.edit_block(|b| p::set_sleep(b, seconds))
    }

    /// Per-key colours, then the per-key effect.
    pub fn set_per_key(
        &mut self,
        colours: &[[u8; 3]],
        brightness: u8,
    ) -> Result<p::Lighting, String> {
        self.write(self.cmds.led_matrix, 0, &p::matrix_payload(colours), 15)?;
        self.set_lighting(p::CUSTOM_MODE, brightness, 0, 0)
    }

    /// One key layer (512 bytes).
    pub fn keymap(&mut self, layer: u8) -> Result<Vec<u8>, String> {
        self.read(self.cmds.keymap, layer, p::KEYMAP_LEN)
    }

    /// Write a key layer, then (when there are any) the macros, as AULA's driver does.
    pub fn write_keymap(
        &mut self,
        layer: u8,
        data: &[u8],
        macros: Option<&[u8]>,
    ) -> Result<(), String> {
        self.write(self.cmds.keymap, layer, data, 15)?;
        let Some(buf) = macros.filter(|b| !b.is_empty()) else {
            return Ok(());
        };
        self.wire.pause(100);
        match self.link() {
            Link::Wired => self.write(self.cmds.macros, 0, buf, 20),
            Link::Receiver => {
                for (page, chunk) in buf.chunks(0x200).enumerate() {
                    self.write(self.cmds.macros, page as u8, chunk, 15)?;
                    self.wire.pause(100);
                }
                Ok(())
            }
        }
    }
}
