# AULA F75 configuration protocol: notes and sources

These protocol facts come from community reverse engineering of AULA's Windows driver
(`OemDrv.exe` and its `KB.ini`). This app's code is its own implementation of those facts.

- [trenbolone1122/aula-f75-linux](https://github.com/trenbolone1122/aula-f75-linux) (F75 Studio),
  `docs/PROTOCOL.md`: the receiver and cable framing, command tables, LED block, key codes and
  macro buffer. Verified on a real F75 over both links.
- [vinicenter/aula-f75-webdriver](https://github.com/vinicenter/aula-f75-webdriver): a WebHID port of F75 Studio.
- [cloudglides/aula-f75-linux-drivers](https://github.com/cloudglides/aula-f75-linux-drivers) (MIT):
  cable notes, recovery (Fn+Esc factory reset) and enumeration notes.

## Observed on this machine (2026-11-10)

The receiver enumerates as `3554:fa09`, "Compx 2.4G Wireless Receiver", with two interfaces:

- Interface 0: a boot keyboard (usage page 1, usage 6).
- Interface 1: usage page `0xFF02` usage 2 with report `0x13` (19 bytes in and out). It also
  carries report `0x05` consumer, `0x03` system, `0x04` NKRO keyboard and `0x07` mouse, plus
  feature report `0x06` (7 bytes) on page `0xFF04`.

Opening interface 1 on macOS returns `kIOReturnNotPermitted` (`0xE00002E2`) until the process
has Input Monitoring permission.

## Summary

| Function | Receiver cmd (read = cmd \| 0x40) | Cable cmd (read = cmd \| 0x80) |
|---|---|---|
| Key layer (sub = layer, 128 × 4 bytes) | `0x01` | `0x03` |
| Per-key LED planes | `0x02` | `0x06` |
| Macro buffer (receiver: 512-byte pages, sub = page) | `0x03` | `0x05` |
| LED settings block (128 bytes, ends `5A A5`) | `0x04` | `0x04` |
| Online flag (receiver only, 1 byte) | read `0x07` | — |
| Real-time frame (unacknowledged) | `0x88` | `0x08` |

The rest of the detail (packet layout, LED-block offsets, key-code and macro event encoding) is
in the doc comments of `data/src/protocol.rs`, `session.rs` and `macros.rs`.
