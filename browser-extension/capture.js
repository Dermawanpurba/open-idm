/* Hand real browser downloads to the OpenIDM app. A download is never lost and never fetched twice:
   the browser copy stays paused until the app has confirmed it, and resumes on any other outcome. */
const capturePrefix = "capture:";
const capturing = new Set();
const captureJobs = new Map();
const safeDanger = ["safe", "accepted", "allowlistedByPolicy", "deepScannedSafe"];

async function captureNotice(message) {
  try {
    await chrome.notifications.create({ type: "basic", iconUrl: "icons/icon128.png", title: "OpenIDM", message });
  } catch { /* Notifications can be disabled by the user. */ }
}

/** Gives the download back to the browser. */
async function releaseCapture(id) {
  try { await chrome.downloads.resume(id); } catch { /* It may already have completed or been removed. */ }
  await chrome.storage.local.remove(capturePrefix + id);
}

/** The app owns the file now: drop the browser copy and its entry in the downloads list. */
async function dropBrowserCopy(id) {
  try { await chrome.downloads.cancel(id); } catch { /* Already gone. */ }
  try { await chrome.downloads.erase({ id }); } catch { /* Cosmetic only. */ }
  await chrome.storage.local.remove(capturePrefix + id);
}

async function captureDownload(item) {
  const url = httpUrl(item.finalUrl || item.url);
  if (!url || item.byExtensionId || browserStartedUrls.has(url) || item.state !== "in_progress" || item.paused || capturing.has(item.id)) return;
  if (item.danger && !safeDanger.includes(item.danger)) return;
  capturing.add(item.id);
  const storageKey = capturePrefix + item.id;
  let paused = false;
  try {
    if ((await settings()).mode === "browser") return;
    await mutations;
    const { downloadResources = [] } = await chrome.storage.session.get("downloadResources");
    const resource = downloadResources.find(entry => entry.url === url && Date.now() - entry.timestamp < 300000);
    // Uploads, form posts and token-authorised requests cannot be replayed outside the browser.
    if (resource?.requiresBrowser || (resource?.method && resource.method !== "GET")) return;

    // Persist before pausing: if this worker dies, recovery must find and resume the download.
    await chrome.storage.local.set({ [storageKey]: { id: item.id, phase: "submitting" } });
    await chrome.downloads.pause(item.id);
    paused = true;

    const filename = safeFilename(item.filename) || resource?.filename || safeFilename(new URL(url).pathname.split("/").pop()) || "download";
    const headers = { ...(resource?.headers || {}) };
    if (!headers.Referer && httpUrl(item.referrer)) headers.Referer = item.referrer;
    // No timeout: the app's confirmation dialog stays open for as long as the user needs.
    const reply = await OpenIDMNative.request({ type: "capture", url, filename,
      filesize: Math.max(0, item.totalBytes || 0), headers, referrer: httpUrl(item.referrer) || undefined });

    if (reply.decision === "accepted" || reply.decision === "later") {
      // Record ownership first so a restart finishes the cancel instead of resuming a second copy.
      await chrome.storage.local.set({ [storageKey]: { id: item.id, phase: "accepted" } });
      await dropBrowserCopy(item.id);
    } else {
      await releaseCapture(item.id);
      if (reply.decision === "failed") {
        await captureNotice(`${reply.error || "OpenIDM tidak bisa mengunduh berkas ini."} Unduhan dilanjutkan di browser.`);
      }
    }
  } catch (error) {
    if (paused) await releaseCapture(item.id);
    else await chrome.storage.local.remove(storageKey);
    await captureNotice(error?.code === "app_unavailable"
      ? "OpenIDM tidak bisa dibuka. Buka OpenIDM sekali untuk mendaftarkan integrasi; unduhan dilanjutkan di browser."
      : "OpenIDM belum menerima unduhan ini. Unduhan dilanjutkan di browser.");
  } finally {
    capturing.delete(item.id);
  }
}

/** Settles captures left behind by a previous worker. Live captures are left alone. */
async function recoverCaptures() {
  const saved = await chrome.storage.local.get(null);
  for (const [key, record] of Object.entries(saved)) {
    if (!key.startsWith(capturePrefix) || capturing.has(record.id)) continue;
    try {
      const [item] = await chrome.downloads.search({ id: record.id });
      if (!item || item.state !== "in_progress" || !item.paused) await chrome.storage.local.remove(key);
      else if (record.phase === "accepted") await dropBrowserCopy(record.id);
      // The port that would have carried the answer died with the old worker: hand it back.
      else await releaseCapture(record.id);
    } catch { /* Try again on the next alarm. */ }
  }
}

function beginCapture(item) {
  if (captureJobs.has(item.id)) return captureJobs.get(item.id);
  const job = captureDownload(item).finally(() => captureJobs.delete(item.id));
  captureJobs.set(item.id, job);
  return job;
}

chrome.downloads.onCreated.addListener(item => {
  void beginCapture(item).catch(() => {});
});
// Hold filename finalization (and Chrome's "Save as" prompt) until the hand-off is decided.
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  void beginCapture(item).catch(() => {}).finally(() => suggest());
  return true;
});
void recoverCaptures().catch(() => {});
chrome.alarms.onAlarm.addListener(alarm => {
  if (alarm.name === "openidm-captures") void recoverCaptures().catch(() => {});
});
void chrome.alarms.create("openidm-captures", { periodInMinutes: 1 });
