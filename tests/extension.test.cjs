const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const extension = path.join(__dirname, '../browser-extension');
const source = name => fs.readFileSync(path.join(extension, name), 'utf8');
const tick = () => new Promise(resolve => setImmediate(resolve));
/** Lets every pending promise chain in the worker settle. */
async function settle() { for (let i = 0; i < 25; i++) await tick(); }

function event() {
  return { listeners: [], addListener(fn) { this.listeners.push(fn); }, emit(...args) { return this.listeners.map(fn => fn(...args)); } };
}
function area(data = {}) {
  return {
    data,
    async get(key) { return key == null ? structuredClone(data) : { [key]: structuredClone(data[key]) }; },
    async set(values) { Object.assign(data, structuredClone(values)); },
    async remove(key) { delete data[key]; },
  };
}

/**
 * Loads the service worker in a sandbox with a fake `chrome`.
 * `previous` carries storage (and the downloads table) across a simulated worker restart.
 */
function worker(previous = {}) {
  const calls = { downloads: [], tabsCreate: [], notifications: [], started: [] };
  const table = previous.table || new Map();           // Chrome's download items by id
  const native = { sent: [], ports: [], reply: null, connectFails: false };
  const legacy = { online: false, submitted: [] };
  const timers = [];
  let now = 0;

  const chrome = {
    runtime: {
      id: 'test', onMessage: event(), onInstalled: event(), lastError: undefined,
      connectNative(name) {
        if (native.connectFails) throw new Error('Specified native messaging host not found.');
        const port = {
          name, onMessage: event(), onDisconnect: event(), disconnected: false,
          postMessage(message) {
            native.sent.push(message);
            Promise.resolve(native.reply ? native.reply(message, port) : undefined).then(response => {
              if (response && !port.disconnected) port.onMessage.emit(response);
            });
          },
          disconnect() { port.disconnected = true; },
          /** Simulates the host process ending (app closed, crash). */
          drop() { port.disconnected = true; port.onDisconnect.emit(port); },
        };
        native.ports.push(port);
        return port;
      },
    },
    storage: { local: previous.local || area(), session: previous.session || area() },
    action: { async setBadgeText() {}, async setBadgeBackgroundColor() {} },
    webRequest: { onBeforeSendHeaders: event(), onHeadersReceived: event(), onCompleted: event(), onErrorOccurred: event() },
    tabs: { onRemoved: event(), onUpdated: event(), async create(options) { calls.tabsCreate.push(options); }, async query() { return []; }, async update() {} },
    contextMenus: { onClicked: event(), removeAll(fn) { fn(); }, create() {} },
    notifications: { async create(options) { calls.notifications.push(options.message); } },
    alarms: { onAlarm: event(), async create() {} },
    downloads: {
      onCreated: event(), onDeterminingFilename: event(),
      async download(options) { calls.started.push(options); return 1000 + calls.started.length; },
      async search(query) { return table.has(query.id) ? [structuredClone(table.get(query.id))] : []; },
      async pause(id) { calls.downloads.push(['pause', id]); if (table.has(id)) table.get(id).paused = true; },
      async resume(id) { calls.downloads.push(['resume', id]); if (table.has(id)) table.get(id).paused = false; },
      async cancel(id) { calls.downloads.push(['cancel', id]); if (table.has(id)) table.get(id).state = 'interrupted'; },
      async erase(query) { calls.downloads.push(['erase', query.id]); table.delete(query.id); },
      async show() {},
    },
  };

  const context = vm.createContext({
    chrome, URL, AbortSignal, console, Map, Set, Promise, structuredClone,
    Date: { now: () => now },
    setTimeout(fn, ms) { const timer = { fn, at: now + ms, cleared: false }; timers.push(timer); return timer; },
    clearTimeout(timer) { if (timer) timer.cleared = true; },
    async fetch(url, options) {
      if (!legacy.online) throw new Error('offline');
      if (url.endsWith('/status')) return { ok: true, async json() { return { app: 'OpenIDM Core', status: 'online', active_downloads: 0 }; } };
      legacy.submitted.push({ url, body: JSON.parse(options.body) });
      return { ok: true, async json() { return { success: true, task: { id: 'legacy' } }; } };
    },
  });
  context.importScripts = (...names) => names.forEach(name => vm.runInContext(source(name), context, { filename: name }));
  vm.runInContext(source('background.js'), context, { filename: 'background.js' });

  const send = (request, sender = { id: 'test', url: 'chrome-extension://test/popup.html' }) =>
    new Promise(resolve => chrome.runtime.onMessage.listeners[0](request, sender, resolve));
  /** Records request headers and a download-like response for `url`, as the webRequest API would. */
  const sniff = (url, { mime = 'application/zip', tabId = 3, id = url, headers = [], method = 'GET', disposition } = {}) => {
    chrome.webRequest.onBeforeSendHeaders.emit({ tabId, requestId: id, requestHeaders: headers, method });
    chrome.webRequest.onHeadersReceived.emit({ tabId, requestId: id, url, statusCode: 200, type: 'main_frame',
      responseHeaders: [{ name: 'Content-Type', value: mime }, ...(disposition ? [{ name: 'Content-Disposition', value: disposition }] : [])] });
    chrome.webRequest.onCompleted.emit({ requestId: id });
  };
  /** A browser download begins, as when the user clicks a download link. */
  const startBrowserDownload = async (item) => {
    const full = { state: 'in_progress', paused: false, totalBytes: 0, danger: 'safe', ...item };
    table.set(full.id, full);
    chrome.downloads.onCreated.emit(structuredClone(full));
    await settle();
  };
  /** Runs fake timers due within `ms`. */
  const advance = async (ms) => {
    now += ms;
    for (const timer of [...timers]) if (!timer.cleared && timer.at <= now) { timer.cleared = true; timer.fn(); }
    await settle();
  };
  return { chrome, context, send, sniff, startBrowserDownload, advance, settle, calls, native, legacy, table,
    storage: { local: chrome.storage.local, session: chrome.storage.session } };
}

const FILE = 'https://files.test/pack.zip';
const decide = decision => message => ({ id: message.id, ok: true, decision });

// ---- unchanged behaviour ----

test('URL classifier distinguishes full files, manifests, fragments and deceptive hosts', () => {
  const api = worker().context.OpenIDM;
  assert.equal(api.isDirect('https://cdn.test/file.PDF?token=x'), true);
  assert.equal(api.isDirect('https://cdn.test/media', 'video/mp4'), true);
  assert.equal(api.isDirect('https://cdn.test/live.m3u8'), false);
  assert.equal(api.mediaKind('https://cdn.test/segment-10.m4s', 'video/mp4'), null);
  assert.equal(api.mediaKind('https://cdn.test/master.m3u8'), 'stream');
  assert.equal(api.corePage('https://youtube.com.evil.test/file.mp4'), false);
  assert.equal(api.corePage('https://www.youtube.com/watch?v=abc'), true);
  for (const url of ['javascript:alert(1)', 'file:///C:/secret', 'blob:https://test/id', 'https://user:pass@test/a']) assert.equal(api.httpUrl(url), null);
  assert.equal(api.safeFilename('../../CON.txt'), '_CON.txt');
});

test('content scripts cannot modify settings, enumerate downloads or control unrelated IDs', async () => {
  const w = worker();
  const sender = { id: 'test', tab: { id: 2, url: 'https://page.test/' }, url: 'https://page.test/' };
  for (const request of [
    { action: 'SAVE_SETTINGS', settings: { mode: 'core' } },
    { action: 'GET_DOWNLOADS' },
    { action: 'CONTROL_DOWNLOAD', id: 99, control: 'pause' },
  ]) assert.equal((await w.send(request, sender)).success, false);
});

// ---- automatic capture ----

test('capture accepted: browser download is cancelled and no tab is opened', async () => {
  const w = worker();
  w.native.reply = decide('accepted');
  await w.startBrowserDownload({ id: 1, url: FILE, filename: 'C:\\Users\\u\\Downloads\\pack.zip', totalBytes: 10, referrer: 'https://page.test/' });
  assert.deepEqual(w.calls.downloads, [['pause', 1], ['cancel', 1], ['erase', 1]]);
  assert.equal(w.calls.tabsCreate.length, 0);
  assert.equal(w.calls.notifications.length, 0);
  const sent = w.native.sent[0];
  assert.equal(sent.type, 'capture');
  assert.equal(sent.url, FILE);
  assert.equal(sent.filename, 'pack.zip');
  assert.equal(sent.filesize, 10);
  assert.equal(sent.headers.Referer, 'https://page.test/');
  assert.equal(typeof sent.id, 'string');
  assert.deepEqual(Object.keys(w.storage.local.data).filter(k => k.startsWith('capture:')), []);
});

test('capture later: treated like accepted', async () => {
  const w = worker();
  w.native.reply = decide('later');
  await w.startBrowserDownload({ id: 2, url: FILE });
  assert.deepEqual(w.calls.downloads.map(c => c[0]), ['pause', 'cancel', 'erase']);
});

test('capture declined: the browser download resumes silently', async () => {
  const w = worker();
  w.native.reply = decide('declined');
  await w.startBrowserDownload({ id: 3, url: FILE });
  assert.deepEqual(w.calls.downloads, [['pause', 3], ['resume', 3]]);
  assert.equal(w.calls.notifications.length, 0);
});

test('capture failed: resumes in the browser and says why', async () => {
  const w = worker();
  w.native.reply = m => ({ id: m.id, ok: true, decision: 'failed', error: 'Server menolak unduhan (HTTP 403).' });
  await w.startBrowserDownload({ id: 4, url: FILE });
  assert.deepEqual(w.calls.downloads, [['pause', 4], ['resume', 4]]);
  assert.equal(w.calls.notifications.length, 1);
  assert.match(w.calls.notifications[0], /HTTP 403/);
  assert.match(w.calls.notifications[0], /dilanjutkan di browser/);
});

test('app_unavailable: resumes in the browser and tells the user to open OpenIDM once', async () => {
  const w = worker();
  w.native.reply = m => ({ id: m.id, ok: false, error: 'app_unavailable' });
  await w.startBrowserDownload({ id: 5, url: FILE });
  assert.deepEqual(w.calls.downloads, [['pause', 5], ['resume', 5]]);
  assert.match(w.calls.notifications[0], /Buka OpenIDM sekali/);
});

test('host not installed: connectNative throws, download resumes', async () => {
  const w = worker();
  w.native.connectFails = true;
  await w.startBrowserDownload({ id: 6, url: FILE });
  assert.deepEqual(w.calls.downloads, [['pause', 6], ['resume', 6]]);
  assert.match(w.calls.notifications[0], /Buka OpenIDM sekali/);
});

test('port drops before a reply: the browser download resumes', async () => {
  const w = worker();
  w.native.reply = (message, port) => { setImmediate(() => port.drop()); };
  await w.startBrowserDownload({ id: 7, url: FILE });
  assert.deepEqual(w.calls.downloads, [['pause', 7], ['resume', 7]]);
  assert.deepEqual(Object.keys(w.storage.local.data).filter(k => k.startsWith('capture:')), []);
});

test('capture has no time limit: a reply after two minutes is still honoured', async () => {
  const w = worker();
  let answer;
  w.native.reply = message => new Promise(resolve => { answer = () => resolve({ id: message.id, ok: true, decision: 'accepted' }); });
  await w.startBrowserDownload({ id: 8, url: FILE });
  await w.advance(120_000);
  assert.deepEqual(w.calls.downloads, [['pause', 8]], 'still waiting for the user');
  // The periodic recovery alarm must not hand an in-flight capture back to the browser.
  w.chrome.alarms.onAlarm.emit({ name: 'openidm-captures' });
  await w.settle();
  assert.deepEqual(w.calls.downloads, [['pause', 8]]);
  answer();
  await w.settle();
  assert.deepEqual(w.calls.downloads.map(c => c[0]), ['pause', 'cancel', 'erase']);
});

test('two captures at once are answered independently', async () => {
  const w = worker();
  const answers = new Map();
  w.native.reply = message => new Promise(resolve => answers.set(message.url, decision => resolve({ id: message.id, ok: true, decision })));
  await w.startBrowserDownload({ id: 11, url: 'https://files.test/a.zip' });
  await w.startBrowserDownload({ id: 12, url: 'https://files.test/b.zip' });
  answers.get('https://files.test/b.zip')('declined');
  await w.settle();
  assert.deepEqual(w.calls.downloads.filter(c => c[1] === 12), [['pause', 12], ['resume', 12]]);
  assert.deepEqual(w.calls.downloads.filter(c => c[1] === 11), [['pause', 11]]);
  answers.get('https://files.test/a.zip')('accepted');
  await w.settle();
  assert.deepEqual(w.calls.downloads.filter(c => c[1] === 11).map(c => c[0]), ['pause', 'cancel', 'erase']);
});

test('browser mode: nothing is paused and nothing is sent to the app', async () => {
  const w = worker();
  await w.send({ action: 'SAVE_SETTINGS', settings: { mode: 'browser', saveAs: false } });
  await w.startBrowserDownload({ id: 13, url: FILE });
  assert.deepEqual(w.calls.downloads, []);
  assert.equal(w.native.sent.length, 0);
});

test('POST downloads and Authorization-protected downloads stay in the browser', async () => {
  const w = worker();
  w.native.reply = decide('accepted');
  w.sniff('https://files.test/form.zip', { method: 'POST' });
  w.sniff('https://files.test/private.zip', { headers: [{ name: 'Authorization', value: 'Bearer x' }] });
  await w.settle();
  await w.startBrowserDownload({ id: 14, url: 'https://files.test/form.zip' });
  await w.startBrowserDownload({ id: 15, url: 'https://files.test/private.zip' });
  assert.deepEqual(w.calls.downloads, []);
  assert.equal(w.native.sent.length, 0);
});

test('downloads started by an extension, dangerous or non-http downloads are skipped', async () => {
  const w = worker();
  w.native.reply = decide('accepted');
  await w.startBrowserDownload({ id: 16, url: FILE, byExtensionId: 'someone' });
  await w.startBrowserDownload({ id: 17, url: 'https://files.test/virus.exe', danger: 'file' });
  await w.startBrowserDownload({ id: 18, url: 'blob:https://files.test/abc' });
  await w.startBrowserDownload({ id: 19, url: 'data:text/plain,hi' });
  assert.deepEqual(w.calls.downloads, []);
});

test('headers: only Cookie/Referer/User-Agent/Origin of that exact resource are sent', async () => {
  const w = worker();
  w.native.reply = decide('accepted');
  w.sniff(FILE, { disposition: 'attachment; filename="Motorgrader Group-001.zip"', headers: [
    { name: 'Cookie', value: 'sid=1' }, { name: 'Referer', value: 'https://page.test/' },
    { name: 'User-Agent', value: 'UA' }, { name: 'Origin', value: 'https://page.test' }, { name: 'X-Secret', value: 'no' },
  ] });
  w.sniff('https://other.test/x.zip', { headers: [{ name: 'Cookie', value: 'other=1' }] });
  await w.settle();
  await w.startBrowserDownload({ id: 20, url: FILE, filename: '' });
  const sent = w.native.sent[0];
  // JSON round-trip: the object was created in the worker's realm.
  assert.deepEqual(JSON.parse(JSON.stringify(sent.headers)),
    { Cookie: 'sid=1', Referer: 'https://page.test/', 'User-Agent': 'UA', Origin: 'https://page.test' });
  assert.equal(sent.filename, 'Motorgrader Group-001.zip', 'server-provided name is used when Chrome has none yet');
});

test('recovery: a worker restart mid-capture hands the paused download back', async () => {
  const w = worker();
  w.native.reply = () => new Promise(() => {});      // dialog still open when the worker dies
  await w.startBrowserDownload({ id: 21, url: FILE });
  assert.deepEqual(w.calls.downloads, [['pause', 21]]);
  const restarted = worker({ local: w.storage.local, session: w.storage.session, table: w.table });
  await restarted.settle();
  assert.deepEqual(restarted.calls.downloads, [['resume', 21]]);
  assert.deepEqual(Object.keys(restarted.storage.local.data).filter(k => k.startsWith('capture:')), []);
});

test('recovery: a restart after the app accepted finishes the cancel instead of resuming', async () => {
  const w = worker();
  w.table.set(22, { id: 22, state: 'in_progress', paused: true, url: FILE });
  w.storage.local.data['capture:22'] = { id: 22, phase: 'accepted' };
  const restarted = worker({ local: w.storage.local, session: w.storage.session, table: w.table });
  await restarted.settle();
  assert.deepEqual(restarted.calls.downloads.map(c => c[0]), ['cancel', 'erase']);
});

// ---- manual downloads and routing ----

test('direct file from the popup goes to the app as "add"', async () => {
  const w = worker();
  w.native.reply = m => ({ id: m.id, ok: true, task_id: 'abc123' });
  const result = await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://cdn.test/book.pdf', filename: '../book.pdf' } });
  assert.equal(result.engine, 'app');
  assert.equal(w.native.sent[0].type, 'add');
  assert.equal(w.native.sent[0].filename, 'book.pdf');
  assert.equal(w.legacy.submitted.length, 0);
});

test('direct file with the app unavailable falls back to a Chrome download in auto mode', async () => {
  const w = worker();
  w.native.reply = m => ({ id: m.id, ok: false, error: 'app_unavailable' });
  const result = await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://cdn.test/book.pdf', filename: '../book.pdf' } });
  assert.equal(result.engine, 'browser');
  assert.equal(w.calls.started[0].url, 'https://cdn.test/book.pdf');
  assert.equal(w.calls.started[0].filename, 'OpenIDM/book.pdf');
});

test('a rejected "add" is reported, never retried through Chrome', async () => {
  const w = worker();
  w.native.reply = m => ({ id: m.id, ok: false, error: 'Tautan tidak valid.' });
  const result = await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://cdn.test/book.pdf' } });
  assert.equal(result.success, false);
  assert.equal(result.error, 'Tautan tidak valid.');
  assert.equal(w.calls.started.length, 0);
});

test('video pages and streams still go to the legacy core over HTTP', async () => {
  const w = worker();
  w.legacy.online = true;
  const result = await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://www.youtube.com/watch?v=a' } });
  assert.equal(result.engine, 'core');
  assert.equal(w.legacy.submitted[0].body.url, 'https://www.youtube.com/watch?v=a');
  assert.equal(w.native.sent.length, 0);
});

test('video pages with the legacy core offline fail with a clear message and download nothing', async () => {
  const w = worker();
  for (const url of ['https://youtube.com/watch?v=a', 'https://cdn.test/a.m3u8', 'https://drive.google.com/drive/folders/a']) {
    const result = await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url } });
    assert.equal(result.success, false);
    assert.match(result.error, /OpenIDM Legacy/);
  }
  assert.equal(w.calls.started.length, 0);
  assert.equal(w.native.sent.length, 0);
});

test('browser mode downloads direct files through Chrome', async () => {
  const w = worker();
  await w.send({ action: 'SAVE_SETTINGS', settings: { mode: 'browser', saveAs: true } });
  assert.equal((await w.send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://cdn.test/a.zip' } })).engine, 'browser');
  assert.equal(w.calls.started[0].saveAs, true);
  assert.equal(w.native.sent.length, 0);
});

test('OPEN_DASHBOARD shows the app window instead of opening a tab', async () => {
  const w = worker();
  w.native.reply = m => ({ id: m.id, ok: true });
  await w.send({ action: 'OPEN_DASHBOARD', url: 'https://cdn.test/a.zip' });
  assert.equal(w.native.sent[0].type, 'show');
  assert.equal(w.native.sent[0].add_url, 'https://cdn.test/a.zip');
  assert.equal(w.calls.tabsCreate.length, 0);
});

test('CHECK_SERVER reports the app and the legacy core separately; a silent app times out', async () => {
  const w = worker();
  w.native.reply = m => ({ id: m.id, ok: true, version: '2.0.0' });
  w.legacy.online = true;
  assert.deepEqual(JSON.parse(JSON.stringify(await w.send({ action: 'CHECK_SERVER' }))), { online: true, version: '2.0.0', legacy: true });

  const silent = worker();
  silent.native.reply = () => new Promise(() => {});
  const pending = silent.send({ action: 'CHECK_SERVER' });
  await silent.settle();
  await silent.advance(10_000);
  const offline = await pending;
  assert.equal(offline.online, false);
  assert.equal(offline.legacy, false);
});

test('the native port is closed after an idle period and reopened on demand', async () => {
  const w = worker();
  w.native.reply = m => ({ id: m.id, ok: true, version: '2.0.0' });
  await w.send({ action: 'CHECK_SERVER' });
  assert.equal(w.native.ports.length, 1);
  await w.advance(60_000);
  assert.equal(w.native.ports[0].disconnected, true);
  await w.send({ action: 'CHECK_SERVER' });
  assert.equal(w.native.ports.length, 2);
});
