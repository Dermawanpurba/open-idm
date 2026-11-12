/* Live check of the packaged Native Messaging host -> Rust -> video helper path. */
const { spawn } = require('node:child_process');
const path = require('node:path');
const assert = require('node:assert/strict');

async function main() {
  const root = path.resolve(process.argv[2]);
  const url = process.argv[3];
  const child = spawn(path.join(root, 'openidm-host.exe'), [], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  let buffer = Buffer.alloc(0), errors = '';
  const pending = new Map();
  child.stderr.on('data', bytes => { errors += bytes; });
  child.stdout.on('data', bytes => {
    buffer = Buffer.concat([buffer, bytes]);
    while (buffer.length >= 4 && buffer.length >= buffer.readUInt32LE(0) + 4) {
      const size = buffer.readUInt32LE(0);
      const reply = JSON.parse(buffer.subarray(4, size + 4));
      buffer = buffer.subarray(size + 4);
      const waiter = pending.get(reply.id);
      if (waiter) { pending.delete(reply.id); clearTimeout(waiter.timer); reply.ok ? waiter.resolve(reply) : waiter.reject(new Error(reply.error)); }
    }
  });
  child.on('exit', () => { for (const waiter of pending.values()) { clearTimeout(waiter.timer); waiter.reject(new Error('Native host exited: ' + errors)); } pending.clear(); });
  let id = 0;
  const call = message => new Promise((resolve, reject) => {
    const key = `native-video-check-${++id}`;
    const timer = setTimeout(() => { pending.delete(key); reject(new Error('Native request timed out')); }, 150000);
    pending.set(key, { resolve, reject, timer });
    const payload = Buffer.from(JSON.stringify({ ...message, id: key }));
    const size = Buffer.alloc(4); size.writeUInt32LE(payload.length);
    child.stdin.write(Buffer.concat([size, payload]));
  });
  try {
    const ping = await call({ type: 'ping' });
    assert.equal(ping.version, '2.0.1');
    assert.ok(ping.capabilities.includes('video'));
    const reply = await call({ type: 'info', url });
    assert.ok(reply.info.formats.some(format => format.format_id.startsWith('video:')));
    assert.ok(reply.info.formats.some(format => format.format_id === 'audio'));
    console.log(`PASS: packaged Native Messaging -> Rust -> YouTube: ${reply.info.title}; ${reply.info.formats.length} choices`);
    await call({ type: 'show', add_url: url });
    console.log('PASS: repaired portable app opened with the video URL');
  } finally {
    child.stdin.end();
    child.kill();
    child.stdin.destroy();
    child.stdout.destroy();
    child.stderr.destroy();
    child.unref();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
