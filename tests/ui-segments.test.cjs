const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function load() {
  const window = {};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../ui/segments.js'), 'utf8'), { window });
  return window.OpenIDMSegments;
}
const plain = value => JSON.parse(JSON.stringify(value));

const item = {
  filesize: 1000,
  segments: [
    { start: 500, end: 999, done: 100, active: true, speed: 2048 },
    { start: 0, end: 249, done: 250, active: false, speed: 0 },
    { start: 250, end: 499, done: 50, active: true, speed: 1024 },
  ],
};

test('blocks place each segment\'s downloaded part at its offset in the file', () => {
  const blocks = plain(load().blocks(item));
  assert.deepEqual(blocks, [
    { left: 0, width: 25, active: false },
    { left: 25, width: 5, active: true },
    { left: 50, width: 10, active: true },
  ]);
});

test('no blocks without segments or without a known size', () => {
  const s = load();
  assert.deepEqual(plain(s.blocks({ filesize: 1000 })), []);
  assert.deepEqual(plain(s.blocks({ filesize: 0, segments: item.segments })), []);
  assert.deepEqual(plain(s.blocks({ filesize: 1000, segments: [] })), []);
});

test('blocks are clamped to the bar even with inconsistent numbers', () => {
  const blocks = plain(load().blocks({ filesize: 100, segments: [{ start: 90, end: 199, done: 500, active: false, speed: 0 }] }));
  assert.deepEqual(blocks, [{ left: 90, width: 10, active: false }]);
});

test('connection rows list every segment in file order with its state', () => {
  const rows = plain(load().rows(item));
  assert.deepEqual(rows, [
    { number: 1, start: 0, end: 249, size: 250, done: 250, percent: 100, speed: 0, state: 'done' },
    { number: 2, start: 250, end: 499, size: 250, done: 50, percent: 20, speed: 1024, state: 'active' },
    { number: 3, start: 500, end: 999, size: 500, done: 100, percent: 20, speed: 2048, state: 'active' },
  ]);
});

test('an unfinished idle segment is waiting', () => {
  const rows = plain(load().rows({ filesize: 10, segments: [{ start: 0, end: 9, done: 3, active: false, speed: 0 }] }));
  assert.equal(rows[0].state, 'waiting');
});

test('summary counts active connections and adds up their speed', () => {
  assert.deepEqual(plain(load().summary(item)), { segments: 3, active: 2, speed: 3072 });
  assert.deepEqual(plain(load().summary({})), { segments: 0, active: 0, speed: 0 });
});
