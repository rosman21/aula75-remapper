//! The keyboard through the browser (WebHID). A web page can't block on USB, so a call that
//! needs the keyboard runs as a *replay*: the service runs against a journal of the I/O done so
//! far; the first operation the journal hasn't answered stops the run with [`NEED`], and the
//! source asks the page module (`modules/web/index.js`) to do it. The page answers with the
//! journal plus that result, and the call runs again, until it finishes without asking.
//!
//! The service's protocol code stays the same on every platform; only the bottom of the stack
//! (a whole read or write of one command) crosses to JavaScript. Every operation after the
//! first unanswered one also fails with [`NEED`], so a run that stops early never writes.

use std::cell::RefCell;

use serde_json::{json, Value as Json};

use crate::protocol::Link;
use crate::session::{Wire, ASLEEP};

/// The error a run stops with when the journal hasn't answered an operation yet.
pub const NEED: &str = "\u{0}aula:need";

#[derive(Default)]
struct Journal {
    /// `[op, result]` pairs, in the order the run asked them.
    entries: Vec<(Json, Json)>,
    cursor: usize,
    /// The first operation the journal couldn't answer.
    pending: Option<Json>,
    /// Off: every operation is refused (a host with its own USB never gets here).
    on: bool,
}

thread_local! {
    static JOURNAL: RefCell<Journal> = RefCell::new(Journal::default());
}

/// Start a run with the I/O answered so far.
pub fn begin(entries: Vec<(Json, Json)>) {
    JOURNAL.with(|j| {
        *j.borrow_mut() = Journal {
            entries,
            cursor: 0,
            pending: None,
            on: true,
        }
    });
}

/// End the run: the operation it stopped at (with the journal to send along), or `None` when
/// it finished.
pub fn finish() -> Option<(Json, Vec<(Json, Json)>)> {
    JOURNAL.with(|j| {
        let j = std::mem::take(&mut *j.borrow_mut());
        j.pending.map(|op| (op, j.entries))
    })
}

/// Is a run in progress? (The service only routes to the page during one.)
pub fn active() -> bool {
    JOURNAL.with(|j| j.borrow().on)
}

/// One operation: its recorded result, or [`NEED`] (recording it as the one to do).
pub fn io(op: Json) -> Result<Json, String> {
    JOURNAL.with(|j| {
        let mut j = j.borrow_mut();
        if !j.on || j.pending.is_some() {
            return Err(NEED.into());
        }
        if j.cursor < j.entries.len() {
            if j.entries[j.cursor].0 == op {
                let result = j.entries[j.cursor].1.clone();
                j.cursor += 1;
                return decode(result);
            }
            // The run went another way than last time (another call changed the state in
            // between): forget the rest and ask again from here.
            let at = j.cursor;
            j.entries.truncate(at);
        }
        j.pending = Some(op);
        Err(NEED.into())
    })
}

/// The page answers `{ "ok": value }` or `{ "err": message }`; `"asleep"` is the radio
/// getting no answer, which the protocol code treats as such.
fn decode(result: Json) -> Result<Json, String> {
    if let Some(e) = result.get("err") {
        let e = e.as_str().unwrap_or("The browser couldn't reach the keyboard.");
        return Err(if e == "asleep" { ASLEEP.into() } else { e.into() });
    }
    Ok(result.get("ok").cloned().unwrap_or(Json::Null))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Bytes from the page's hex.
pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .filter_map(|i| u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok())
        .collect()
}

/// A stored file the page keeps (in the browser's local storage): its bytes, if any.
pub fn get_file(name: &str) -> Result<Option<Vec<u8>>, String> {
    let r = io(json!({ "op": "get", "name": name }))?;
    Ok(r.as_str().map(unhex))
}

/// Keep a file in the browser's local storage.
pub fn put_file(name: &str, data: &[u8]) -> Result<(), String> {
    io(json!({ "op": "put", "name": name, "data": hex(data) })).map(|_| ())
}

/// The keyboard as the page has it open: whole reads and writes of one command.
pub struct RemoteWire {
    /// How the page reached it.
    pub link: Link,
}

impl Wire for RemoteWire {
    fn link(&self) -> Link {
        self.link
    }
    fn send_report(&mut self, _: &[u8]) -> Result<(), String> {
        Err(NEED.into())
    }
    fn recv_report(&mut self, _: i32) -> Result<Option<Vec<u8>>, String> {
        Err(NEED.into())
    }
    fn send_feature(&mut self, _: &[u8]) -> Result<(), String> {
        Err(NEED.into())
    }
    fn get_feature(&mut self) -> Result<Vec<u8>, String> {
        Err(NEED.into())
    }
    fn pause(&mut self, _: u64) {}
    fn block_write(
        &mut self,
        cmd: u8,
        sub: u8,
        data: &[u8],
        delay_ms: u64,
    ) -> Option<Result<(), String>> {
        Some(
            io(json!({ "op": "write", "cmd": cmd, "sub": sub, "data": hex(data), "delay": delay_ms }))
                .map(|_| ()),
        )
    }
    fn block_read(&mut self, cmd: u8, sub: u8, len: usize) -> Option<Result<Vec<u8>, String>> {
        Some(
            io(json!({ "op": "read", "cmd": cmd, "sub": sub, "len": len })).and_then(|r| {
                let b = unhex(r.as_str().unwrap_or_default());
                if b.len() < len {
                    Err(format!("The keyboard sent a short reply ({} bytes).", b.len()))
                } else {
                    Ok(b)
                }
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_stops_at_the_first_unanswered_operation_and_never_goes_past_it() {
        begin(Vec::new());
        assert_eq!(io(json!({"op": "a"})), Err(NEED.into()));
        assert_eq!(io(json!({"op": "b"})), Err(NEED.into()));
        let (op, entries) = finish().expect("stopped");
        assert_eq!(op, json!({"op": "a"}));
        assert!(entries.is_empty());
        begin(vec![(json!({"op": "a"}), json!({"ok": 1}))]);
        assert_eq!(io(json!({"op": "a"})), Ok(json!(1)));
        assert_eq!(io(json!({"op": "b"})), Err(NEED.into()));
        assert_eq!(finish().map(|f| f.0), Some(json!({"op": "b"})));
        begin(vec![(json!({"op": "a"}), json!({"err": "asleep"}))]);
        assert_eq!(io(json!({"op": "a"})), Err(ASLEEP.into()));
        assert!(finish().is_none());
        assert!(!active());
    }
}
