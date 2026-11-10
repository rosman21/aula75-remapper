// AULA 75 Control's page module: the keyboard over WebHID (Chrome, Edge, Opera).
//
// The Rust data module (data/src/remote.rs) runs each call against a journal of the I/O done
// so far, and hands this module the one step it still needs as `native.later`:
//   { aula: 1, op, journal }  →  { journal: [...journal, [op, result]] }
// where result is { ok: value } or { err: message } ("asleep": the radio got no answer).
//
// Steps:
//   device              which keyboard is open: { mode: "device", link, product } | "none" |
//                       "unsupported" (no WebHID) | "demo" (under the agent, or ?demo)
//   connect             Chrome's device picker (needs the click that asked for it)
//   read  cmd sub len   a whole read of one command (cmd carries the read flag), as hex
//   write cmd sub data delay   a whole write of one command (data hex)
//   get name / put name data   the app's files, in local storage (hex)
//
// The packet code below is data/src/session.rs and data/src/protocol.rs, ported line for line.

const RECEIVER = { vendorId: 0x3554, productId: 0xfa09 };
const WIRED = { vendorId: 0x258a, productId: 0x010c };
const VENDOR_PAGE = 0xff02;

const REPORT_ID = 0x13;
const PACKET_LEN = 20;
const CHUNK = 14;
const WIRED_REPORT = 0x06;
const WIRED_LEN = 520;
const WIRED_CHUNK = 512;
const ASLEEP = 'asleep';

let agent = false;
let device = null; // { hid, link, product }
const inbox = [];
let wake = null;

export function connect(context) {
  agent = Boolean(context?.agent);
  if (typeof navigator !== 'undefined' && navigator.hid) {
    navigator.hid.addEventListener('disconnect', (e) => {
      if (device && e.device === device.hid) device = null;
    });
  }
}

// One conversation with the keyboard at a time: a status poll waits for a key write.
let line = Promise.resolve();
const queue = (f) => {
  const run = line.then(f, f);
  line = run.catch(() => {});
  return run;
};

export async function later(request) {
  if (request?.aula !== 1) throw new Error('AULA 75 Control: not a keyboard step');
  let result;
  try {
    result = { ok: await queue(() => step(request.op)) };
  } catch (e) {
    result = { err: e?.asleep ? ASLEEP : `USB: ${e?.message ?? e}` };
  }
  return { journal: [...request.journal, [request.op, result]] };
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const hex = (b) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
const unhex = (s) => Uint8Array.from((s ?? '').match(/../g) ?? [], (h) => parseInt(h, 16));
const asleep = () => Object.assign(new Error('the keyboard did not answer'), { asleep: true });

async function step(op) {
  switch (op.op) {
    case 'device': return await current();
    case 'connect': return await pick();
    case 'read': return hex(await read(op.cmd, op.sub, op.len));
    case 'write': await write(op.cmd, op.sub, unhex(op.data), op.delay ?? 15); return null;
    case 'get': return localStorage.getItem(`aula75:${op.name}`);
    case 'put': localStorage.setItem(`aula75:${op.name}`, op.data); return null;
    default: throw new Error(`unknown step ${op.op}`);
  }
}

// ------------------------------------------------------------------ finding the keyboard

const isConfig = (d) => {
  const pages = (d.collections ?? []).map((c) => c.usagePage);
  if (d.vendorId === RECEIVER.vendorId && d.productId === RECEIVER.productId) return pages.includes(VENDOR_PAGE);
  if (d.vendorId === WIRED.vendorId && d.productId === WIRED.productId) return pages.some((p) => p >= 0xff00);
  return false;
};

async function use(candidates) {
  const configs = candidates.filter(isConfig);
  // The cable is faster: prefer it when both are there.
  const hid = configs.find((d) => d.productId === WIRED.productId) ?? configs[0];
  if (!hid) return false;
  if (!hid.opened) await hid.open();
  if (device?.hid !== hid) {
    device?.hid.removeEventListener('inputreport', onReport);
    hid.addEventListener('inputreport', onReport);
  }
  const link = hid.productId === WIRED.productId ? 'wired' : 'receiver';
  device = { hid, link, product: hid.productName || 'AULA F75' };
  return true;
}

async function current() {
  if (agent || new URLSearchParams(location.search).has('demo')) return { mode: 'demo' };
  if (typeof navigator === 'undefined' || !navigator.hid) return { mode: 'unsupported' };
  if (!device || !device.hid.opened) {
    device = null;
    await use(await navigator.hid.getDevices());
  }
  return device ? { mode: 'device', link: device.link, product: device.product } : { mode: 'none' };
}

async function pick() {
  if (!navigator.hid) return { ok: false, message: 'This browser can’t reach USB devices: use Chrome or Edge.' };
  let chosen;
  try {
    chosen = await navigator.hid.requestDevice({
      filters: [{ ...RECEIVER, usagePage: VENDOR_PAGE }, WIRED],
    });
  } catch (e) {
    return { ok: false, message: `Chrome didn’t show its device list (${e.message}). Choose Connect keyboard again.` };
  }
  if (!chosen.length) return { ok: false, message: 'No keyboard was chosen.' };
  // Chrome hands back one device per interface: take every interface it granted.
  const all = await navigator.hid.getDevices();
  if (await use([...chosen, ...all])) return { ok: true, message: '' };
  return { ok: false, message: 'That device isn’t an AULA F75’s receiver or cable. Choose the one named “2.4G Wireless Receiver” or the keyboard itself.' };
}

function need() {
  if (!device || !device.hid.opened) throw new Error('the keyboard is not connected');
  return device;
}

// ------------------------------------------------------------------ receiver (20-byte reports)

function onReport(e) {
  const data = new Uint8Array(e.data.buffer, e.data.byteOffset, e.data.byteLength);
  if (e.reportId !== REPORT_ID) return; // typing on the same interface
  const pk = new Uint8Array(1 + data.length);
  pk[0] = e.reportId;
  pk.set(data, 1);
  inbox.push(pk);
  if (inbox.length > 256) inbox.shift();
  if (wake) { const w = wake; wake = null; w(); }
}

async function recv(ms) {
  if (inbox.length) return inbox.shift();
  if (ms <= 0) return null;
  await new Promise((r) => { wake = r; setTimeout(r, ms); });
  wake = null;
  return inbox.shift() ?? null;
}

const checksum = (p) => p.subarray(0, PACKET_LEN - 1).reduce((s, b) => (s + b) & 0xff, 0);

function writePackets(cmd, sub, data) {
  const total = Math.max(1, Math.ceil(data.length / CHUNK));
  return Array.from({ length: total }, (_, i) => {
    const chunk = data.subarray(Math.min(i * CHUNK, data.length), Math.min((i + 1) * CHUNK, data.length));
    const p = new Uint8Array(PACKET_LEN);
    p[0] = REPORT_ID; p[1] = cmd; p[2] = total; p[3] = i; p[4] = (sub << 4) | chunk.length;
    p.set(chunk, 5);
    p[19] = checksum(p);
    return p;
  });
}

function readRequest(cmd, sub) {
  const p = new Uint8Array(PACKET_LEN);
  p[0] = REPORT_ID; p[1] = cmd; p[2] = 1; p[4] = sub << 4;
  p[19] = checksum(p);
  return p;
}

function parseReply(p) {
  if (p.length < PACKET_LEN || p[0] !== REPORT_ID) return null;
  return {
    cmd: p[1] & 0x7f,
    crcError: (p[1] & 0x80) !== 0 || (p[2] & 0x80) !== 0,
    total: p[2] & 0x7f,
    index: p[3],
    len: Math.min(p[4] & 0x0f, CHUNK),
  };
}

const send = (p) => need().hid.sendReport(p[0], p.subarray(1));
const drain = () => { inbox.length = 0; };

async function recvPacket(ms) {
  for (let i = 0; i < 64; i++) {
    const pk = await recv(ms);
    if (!pk) return null;
    const r = parseReply(pk);
    if (r) return [pk, r];
  }
  return null;
}

async function wirelessWrite(cmd, sub, data, delay) {
  const wantsAck = (cmd & 0x80) === 0 && cmd !== 0x06;
  const packets = writePackets(cmd, sub, data);
  for (let i = 0; i < packets.length; i++) {
    for (let tries = 0; ; tries++) {
      await sleep(delay);
      drain();
      await send(packets[i]);
      if (!wantsAck) break;
      let ack = null;
      for (let n = 0; n < 20; n++) {
        const got = await recvPacket(1000);
        if (!got) break;
        if ((got[0][1] & 0x7f) === cmd) { ack = got[1]; break; }
      }
      if (ack && !ack.crcError && ack.index === i) break;
      if (ack && !ack.crcError) throw new Error(`the keyboard acknowledged packet ${ack.index} instead of ${i}`);
      if (tries >= 4) throw asleep();
    }
  }
}

async function wirelessRead(cmd, sub, len) {
  let out = new Uint8Array(Math.max(len, CHUNK));
  let got = [];
  for (let attempt = 0; attempt < 4; attempt++) {
    if (attempt > 0) await sleep(60);
    drain();
    await send(readRequest(cmd, sub));
    for (let r; (r = await recvPacket(500)); ) {
      const [pk, h] = r;
      if (h.cmd !== (cmd & 0x7f)) continue;
      if (h.crcError) break;
      const total = Math.max(h.total, 1);
      if (got.length !== total) got = new Array(total).fill(false);
      if (h.index >= total) continue;
      const off = h.index * CHUNK;
      if (out.length < off + h.len) { const bigger = new Uint8Array(off + h.len); bigger.set(out); out = bigger; }
      out.set(pk.subarray(5, 5 + h.len), off);
      got[h.index] = true;
      if (got.every(Boolean)) return out.slice(0, len);
    }
  }
  throw asleep();
}

// ------------------------------------------------------------------ cable (520-byte feature reports)

function wiredPackets(cmd, sub, len, data) {
  const total = Math.max(1, Math.ceil(len / WIRED_CHUNK));
  return Array.from({ length: total }, (_, i) => {
    const off = i * WIRED_CHUNK;
    const size = Math.min(len - Math.min(off, len), WIRED_CHUNK);
    const p = new Uint8Array(WIRED_LEN);
    p[0] = WIRED_REPORT; p[1] = cmd; p[2] = sub; p[4] = total; p[5] = i;
    p[6] = size & 0xff; p[7] = size >> 8;
    if (data) p.set(data.subarray(off, off + size), 8);
    return p;
  });
}

async function wiredSend(frame) {
  let last;
  for (let i = 0; i < 3; i++) {
    try { await need().hid.sendFeatureReport(frame[0], frame.subarray(1)); return; } catch (e) { last = e; }
    await sleep(70);
  }
  throw new Error(`the keyboard refused a feature report: ${last?.message ?? last}`);
}

async function wiredWrite(cmd, sub, data) {
  for (const frame of wiredPackets(cmd, sub, data.length, data)) {
    await sleep(20);
    await wiredSend(frame);
  }
}

async function wiredRead(cmd, sub, len) {
  const out = [];
  for (const frame of wiredPackets(cmd, sub, len, null)) {
    const size = frame[6] | (frame[7] << 8);
    await sleep(20);
    await wiredSend(frame);
    await sleep(20);
    const view = await need().hid.receiveFeatureReport(WIRED_REPORT);
    let buf = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
    if (buf[0] !== WIRED_REPORT) { const withId = new Uint8Array(buf.length + 1); withId[0] = WIRED_REPORT; withId.set(buf, 1); buf = withId; }
    if (buf.length < 8 + size) throw new Error(`the keyboard sent a short reply (${buf.length} bytes)`);
    out.push(...buf.subarray(8, 8 + size));
  }
  return Uint8Array.from(out);
}

// ------------------------------------------------------------------ either

const read = (cmd, sub, len) => (need().link === 'wired' ? wiredRead(cmd, sub, len) : wirelessRead(cmd, sub, len));
const write = (cmd, sub, data, delay) => (need().link === 'wired' ? wiredWrite(cmd, sub, data) : wirelessWrite(cmd, sub, data, delay));
