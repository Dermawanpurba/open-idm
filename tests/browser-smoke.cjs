/* Run with Node + Puppeteer/Chrome for Testing. Uses a temporary browser profile. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const puppeteer = require('puppeteer');
const listen = (server, port = 0) => new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolve); });
const close = server => new Promise(resolve => server.listening ? server.close(resolve) : resolve());
async function main() {
  const results = path.resolve(__dirname, '../test-results');
  fs.mkdirSync(results, { recursive: true });
  const downloadDir = fs.mkdtempSync(path.join(os.tmpdir(), 'openidm-download-test-'));
  const submissions = [], errors = [];
  let coreOnline = false;
  const file = Buffer.from('OpenIDM browser download smoke test\n');
  const slowFile = Buffer.alloc(1024 * 1024, 42);
  const media = http.createServer((req, res) => {
    if (req.url === '/slow.txt') {
      const range = /bytes=(\d+)-/.exec(req.headers.range || '');
      let offset = Number(range?.[1] || 0);
      res.writeHead(range ? 206 : 200, { 'Content-Type': 'text/plain', 'Content-Length': slowFile.length - offset,
        'Content-Disposition': 'attachment; filename="slow.txt"', 'Accept-Ranges': 'bytes',
        ...(range ? { 'Content-Range': `bytes ${offset}-${slowFile.length - 1}/${slowFile.length}` } : {}) });
      const timer = setInterval(() => {
        res.write(slowFile.subarray(offset, offset + 16384));
        offset += 16384;
        if (offset >= slowFile.length) { clearInterval(timer); res.end(); }
      }, 40);
      res.on('close', () => clearInterval(timer));
      return;
    }
    if (req.url === '/sample.txt') {
      res.writeHead(200, { 'Content-Type': 'text/plain', 'Content-Disposition': 'attachment; filename="sample.txt"', 'Content-Length': file.length });
      return res.end(file);
    }
    if (req.url === '/clip.mp4') {
      res.writeHead(200, { 'Content-Type': 'video/mp4', 'Content-Length': 64 });
      return res.end(Buffer.alloc(64));
    }
    res.setHeader('Content-Type', 'text/html');
    res.end('<!doctype html><html lang="id"><title>OpenIDM video fixture</title><body><h1>Video fixture</h1><div style="position:relative;width:640px;height:360px"><video controls style="width:640px;height:360px" src="/clip.mp4"></video></div></body></html>');
  });
  const core = http.createServer((req, res) => {
    const json = (data, status = 200) => { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(data)); };
    if (!coreOnline) return json({ error: 'offline fixture' }, 503);
    if (req.url === '/api/status') return json({ app: 'OpenIDM Core', status: 'online', active_downloads: 0 });
    let body = '';
    req.on('data', data => { body += data; });
    req.on('end', () => {
      const data = body ? JSON.parse(body) : {};
      if (req.url === '/api/info') return json({ success: true, info: { url: data.url, title: '<img src=x onerror=alert(1)>', formats: [{ format_id: 'fixture-720', resolution: '720p', quality: 'Fixture HD', ext: 'mp4', filesize: 1000 }] } });
      if (req.url === '/api/download') { submissions.push(data); return json({ success: true, task: { id: 'test-task' } }); }
      json({ error: 'not found' }, 404);
    });
  });
  let browser;
  try {
    await listen(media);
    // Fail on a busy port rather than affecting an existing desktop server.
    await listen(core, 6899);
    browser = await puppeteer.launch({ headless: true, enableExtensions: true });
    const id = await browser.installExtension(path.resolve(__dirname, '../browser-extension'));
    const target = await browser.waitForTarget(t => t.type() === 'service_worker' && t.url().includes(id));
    let worker = await target.worker();
    worker.on('console', msg => { if (msg.type() === 'error') errors.push(msg.text()); });
    const session = await browser.target().createCDPSession();
    await session.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: downloadDir });
    const page = await browser.newPage();
    page.on('pageerror', error => errors.push(error.message));
    const origin = `http://127.0.0.1:${media.address().port}`;
    await page.goto(`${origin}/page`);
    await page.waitForSelector('.openidm-floating-panel');
    const tabId = await worker.evaluate(async origin => (await chrome.tabs.query({})).find(tab => tab.url === `${origin}/page`).id, origin);
    let detected;
    for (let i = 0; i < 20; i++) {
      detected = await worker.evaluate(tabId => handle({ action: 'GET_TAB_MEDIA', tabId }, {}), tabId);
      if (detected.media.length) break;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert.equal(detected.media[0].mediaType, 'video');
    console.log('PASS: actual webRequest media detection and floating button');
    const popup = await browser.newPage();
    popup.setDefaultTimeout(10000);
    popup.on('pageerror', error => errors.push(error.message));
    await popup.setViewport({ width: 390, height: 600, deviceScaleFactor: 1 });
    await popup.goto(`chrome-extension://${id}/popup.html`);
    await popup.waitForFunction(() => document.querySelector('#status-text').textContent === 'Desktop belum aktif');
    await worker.evaluate(tabId => chrome.tabs.update(tabId, { active: true }), tabId);
    await popup.evaluate(() => loadMedia());
    assert.equal(await popup.$eval('#media-count', element => element.textContent), '(1)');
    console.log('PASS: popup displays detected media');
    await popup.bringToFront();
    await popup.screenshot({ path: path.join(results, 'popup-offline.png'), fullPage: true });
    await popup.type('#download-url', `${origin}/sample.txt`);
    await popup.click('#btn-add');
    console.log('Submitted browser download form');
    await popup.waitForFunction(() => document.querySelector('#feedback').textContent.includes('dimulai di Chrome'));
    for (let i = 0; i < 50 && !fs.existsSync(path.join(downloadDir, 'sample.txt')); i++) await new Promise(resolve => setTimeout(resolve, 100));
    assert.deepEqual(fs.readFileSync(path.join(downloadDir, 'sample.txt')), file);
    await popup.waitForFunction(async () => { await loadDownloads(); return document.querySelector('#downloads-list').textContent.includes('Selesai'); });
    console.log('PASS: offline popup → worker → Chrome download → matching file bytes + progress');
    const refused = await popup.evaluate(() => send({ action: 'DOWNLOAD_VIDEO', payload: { url: 'https://www.youtube.com/watch?v=fixture' } }).catch(error => ({ error: error.message })));
    assert.match(refused.error, /run\.bat/);
    console.log('PASS: offline platform does not download an HTML page');
    const slow = await popup.evaluate(origin => send({ action: 'DOWNLOAD_VIDEO', payload: { url: `${origin}/slow.txt`, engine: 'browser' } }), origin);
    await popup.evaluate(id => send({ action: 'CONTROL_DOWNLOAD', id, control: 'pause' }), slow.downloadId);
    assert.equal(await popup.evaluate(async id => (await chrome.downloads.search({ id }))[0].paused, slow.downloadId), true);
    await popup.evaluate(id => send({ action: 'CONTROL_DOWNLOAD', id, control: 'resume' }), slow.downloadId);
    await popup.waitForFunction(async id => (await chrome.downloads.search({ id }))[0].state === 'complete', {}, slow.downloadId);
    assert.deepEqual(fs.readFileSync(path.join(downloadDir, 'slow.txt')), slowFile);
    console.log('PASS: native pause/resume preserves complete file bytes');
    coreOnline = true;
    await popup.evaluate(() => checkStatus());
    assert.match(await popup.$eval('#status-text', element => element.textContent), /terhubung/);
    // The content-script UI talks to a core that deliberately has no CORS headers.
    await page.bringToFront();
    await page.click('.openidm-btn-pill');
    await page.waitForSelector('.openidm-dropdown-item');
    assert.match(await page.$eval('.openidm-dropdown-item', element => element.textContent), /720p/);
    await page.click('.openidm-dropdown-item');
    await page.waitForFunction(() => document.querySelector('#openidm-toast').textContent.includes('antrean'));
    assert.equal(submissions[0].format_id, 'fixture-720');
    assert.equal(submissions[0].url, `${origin}/page`);
    assert.equal(await page.$eval('.openidm-dropdown-list', element => element.querySelectorAll('img').length), 0);
    console.log('PASS: content-script format analysis and download through worker without page CORS');
    await popup.bringToFront();
    await popup.select('#download-mode', 'browser');
    await popup.waitForFunction(async () => (await chrome.storage.local.get('settings')).settings.mode === 'browser');
    await popup.reload();
    await popup.waitForFunction(() => document.querySelector('#download-mode').value === 'browser');
    console.log('PASS: settings persist after popup reload');
    await popup.screenshot({ path: path.join(results, 'popup-online.png'), fullPage: true });
    await page.screenshot({ path: path.join(results, 'video-panel.png') });
    assert.equal(await popup.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    assert.deepEqual(errors, []);
    console.log(`PASS: no script errors or horizontal overflow; screenshots: ${results}`);
    console.log(`Chrome: ${await browser.version()}`);
  } finally {
    await browser?.close();
    await Promise.all([close(media), close(core)]);
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
