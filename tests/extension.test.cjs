const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const extension = path.join(__dirname, '../browser-extension');
const source = name => fs.readFileSync(path.join(extension, name), 'utf8');
function event() { return { listeners: [], addListener(fn) { this.listeners.push(fn); }, emit(...args) { for (const fn of this.listeners) fn(...args); } }; }
function area(data = {}) { return { data, async get(key) { return { [key]: structuredClone(data[key]) }; }, async set(values) { Object.assign(data, structuredClone(values)); }, async remove(key) { delete data[key]; } }; }
function worker(previous = {}) {
  const submitted = [], native = [];
  const chrome = {
    runtime: { id: 'test', onMessage: event(), onInstalled: event() },
    storage: { local: previous.local || area(), session: previous.session || area() },
    action: { async setBadgeText() {}, async setBadgeBackgroundColor() {} },
    webRequest: { onBeforeSendHeaders: event(), onHeadersReceived: event(), onCompleted: event(), onErrorOccurred: event() },
    tabs: { onRemoved: event(), onUpdated: event(), async create() {} },
    contextMenus: { onClicked: event(), removeAll(fn) { fn(); }, create() {} },
    notifications: { async create() {} },
    downloads: { async download(options) { native.push(options); return native.length; }, async search() { return []; }, async pause() {}, async resume() {}, async show() {} }
  };
  let online = false, postError = false;
  const context = vm.createContext({ chrome, URL, AbortSignal, console, Date, Map, Promise,
    async fetch(url, options) {
      if (!online) throw new Error('offline');
      if (url.endsWith('/status')) return { ok: true, async json() { return { app: 'OpenIDM Core', status: 'online' }; } };
      submitted.push(JSON.parse(options.body));
      if (postError) throw new Error('response lost');
      return { ok: true, async json() { return { success: true, task: { id: 'fixture' } }; } };
    }
  });
  context.importScripts = name => vm.runInContext(source(name), context);
  vm.runInContext(source('background.js'), context);
  const send = (request, sender = { id: 'test', url: 'chrome-extension://test/popup.html' }) => new Promise(resolve => chrome.runtime.onMessage.listeners[0](request, sender, resolve));
  const sniff = (url, mime = 'video/mp4', tabId = 0, id = url, headers = []) => {
    chrome.webRequest.onBeforeSendHeaders.emit({ tabId, requestId: id, requestHeaders: headers });
    chrome.webRequest.onHeadersReceived.emit({ tabId, requestId: id, url, statusCode: 206,
      responseHeaders: [{ name: 'Content-Type', value: mime }, { name: 'Content-Range', value: 'bytes 0-1023/8192' }] });
    chrome.webRequest.onCompleted.emit({ requestId: id });
  };
  return { chrome, send, sniff, native, submitted, context,
    online(value = true) { online = value; }, failPost() { postError = true; } };
}
test('URL classifier distinguishes full files, manifests, fragments and deceptive hosts', () => {
  const { context } = worker();
  const api = context.OpenIDM;
  assert.equal(api.isDirect('https://cdn.test/file.PDF?token=x'), true);
  assert.equal(api.isDirect('https://cdn.test/media', 'video/mp4'), true);
  assert.equal(api.isDirect('https://cdn.test/live.m3u8'), false);
  assert.equal(api.isDirect('https://cdn.test/media', 'application/dash+xml'), false);
  assert.equal(api.mediaKind('https://cdn.test/segment-10.m4s', 'video/mp4'), null);
  assert.equal(api.mediaKind('https://cdn.test/master.m3u8'), 'stream');
  assert.equal(api.corePage('https://youtube.com.evil.test/file.mp4'), false);
  assert.equal(api.corePage('https://www.youtube.com/watch?v=abc'), true);
  for (const url of ['javascript:alert(1)', 'file:///C:/secret', 'blob:https://test/id', 'https://user:pass@test/a']) assert.equal(api.httpUrl(url), null);
  assert.equal(api.safeFilename('../../CON.txt'), '_CON.txt');
});
test('offline auto starts a direct Chrome download and persists its ID', async () => {
  const w = worker();
  const result = await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://cdn.test/book.pdf', filename: '../book.pdf' } });
  assert.equal(result.engine, 'browser');
  assert.equal(w.native[0].filename, 'OpenIDM/book.pdf');
  assert.deepEqual(w.chrome.storage.local.data.browserDownloads, [1]);
});
test('offline platform and manifest URLs fail without downloading HTML or a manifest', async () => {
  const w = worker();
  for (const url of ['https://youtube.com/watch?v=a', 'https://cdn.test/a.m3u8', 'https://drive.google.com/drive/folders/a']) {
    const result = await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url } });
    assert.equal(result.success, false);
    assert.match(result.error, /run\.bat/);
  }
  assert.equal(w.native.length, 0);
});
test('media survives a worker restart; concurrent responses deduplicate and cap the list', async () => {
  const w = worker();
  for (let i = 0; i < 75; i++) w.sniff(`https://cdn.test/${i}.mp4`);
  w.sniff('https://cdn.test/74.mp4');
  const first = await w.send({ action: 'GET_TAB_MEDIA', tabId: 0 });
  assert.equal(first.media.length, 60);
  assert.equal(first.media[0].filesize, 8192);
  const restarted = worker(w.chrome.storage);
  const restored = await restarted.send({ action: 'GET_TAB_MEDIA', tabId: 0 });
  assert.equal(restored.media.length, 60);
  restarted.chrome.tabs.onUpdated.emit(0, { url: 'https://other.test' });
  assert.equal((await restarted.send({ action: 'GET_TAB_MEDIA', tabId: 0 })).media.length, 0);
});
test('desktop receives only exact-resource headers and never cookie/Authorization', async () => {
  const w = worker(); w.online();
  w.sniff('https://cdn.test/video.mp4', 'video/mp4', 4, 'a', [
    { name: 'Referer', value: 'https://page.test/' }, { name: 'Cookie', value: 'secret' }, { name: 'Authorization', value: 'secret' }
  ]);
  w.sniff('https://tracker.test/ad.mp4', 'video/mp4', 4, 'b', [{ name: 'Referer', value: 'https://wrong.test/' }]);
  const media = await w.send({ action: 'GET_TAB_MEDIA', tabId: 4 });
  assert.equal('headers' in media.media[0], false);
  const result = await w.send({ action: 'DOWNLOAD_VIDEO', tabId: 4, payload: { url: 'https://cdn.test/video.mp4', headers: { Cookie: 'injected' } } });
  assert.equal(result.engine, 'core');
  assert.deepEqual(w.submitted[0].headers, { Referer: 'https://page.test/' });
});
test('a failed POST never falls back and creates a duplicate browser download', async () => {
  const w = worker(); w.online(); w.failPost();
  const result = await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://cdn.test/a.pdf' } });
  assert.equal(result.success, false);
  assert.equal(w.submitted.length, 1);
  assert.equal(w.native.length, 0);
});
test('saved browser mode works online; core mode requires desktop', async () => {
  const w = worker(); w.online();
  await w.send({ action: 'SAVE_SETTINGS', settings: { mode: 'browser', saveAs: true } });
  assert.equal((await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://cdn.test/a.zip' } })).engine, 'browser');
  assert.equal(w.native[0].saveAs, true);
  await w.send({ action: 'SAVE_SETTINGS', settings: { mode: 'core', saveAs: false } });
  w.online(false);
  assert.equal((await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://cdn.test/b.zip' } })).success, false);
  assert.equal(w.native.length, 1);
});
test('content scripts cannot modify settings, enumerate downloads or control unrelated IDs', async () => {
  const w = worker();
  const sender = { id: 'test', tab: { id: 2, url: 'https://page.test/' }, url: 'https://page.test/' };
  for (const request of [
    { action: 'SAVE_SETTINGS', settings: { mode: 'core' } },
    { action: 'GET_DOWNLOADS' },
    { action: 'CONTROL_DOWNLOAD', id: 99, control: 'pause' }
  ]) assert.equal((await w.send(request, sender)).success, false);
  assert.equal((await w.send({ action: 'CONTROL_DOWNLOAD', id: 99, control: 'pause' })).success, false);
});
