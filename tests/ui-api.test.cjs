const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../ui/api.js'), 'utf8');

test('desktop video analysis preserves quality choices from Rust', async () => {
  const info = { title: 'Tutorial', filename: 'Tutorial.mp4', formats: [{ format_id: 'video:720', resolution: '720p' }], direct: false };
  const calls = [];
  const window = { __TAURI__: { core: { async invoke(command, args) { calls.push([command, args]); return info; } } } };
  vm.runInNewContext(source, { window });
  const response = await window.OpenIDMApi.info('https://youtu.be/test');
  assert.equal(response.info.formats[0].format_id, 'video:720');
  assert.equal(response.info.direct, false);
  assert.equal(calls[0][0], 'get_info');
});

test('desktop download forwards selected quality and reports backend errors', async () => {
  const calls = [];
  const window = { __TAURI__: { core: { async invoke(command, args) { calls.push([command, args]); return { id: 'video-1' }; } } } };
  vm.runInNewContext(source, { window });
  const result = await window.OpenIDMApi.download({ url: 'https://youtu.be/test', format_id: 'video:720', filename: 'Tutorial.mp4', category: 'Video' });
  assert.equal(calls[0][1].formatId, 'video:720');
  assert.equal(result.task.id, 'video-1');
  window.__TAURI__.core.invoke = async () => { throw new Error('helper missing'); };
  // Invoke is captured during initialization, so check a fresh adapter instance.
  vm.runInNewContext(source, { window });
  assert.equal((await window.OpenIDMApi.info('https://youtu.be/test')).error, 'helper missing');
});
