/* Transfer real browser downloads only after the desktop has begun receiving data. */
const capturePrefix = "capture:";
const capturing = new Set();
const captureJobs = new Map();
const dashboardUrl = "http://127.0.0.1:6899/";

async function captureNotice(message) {
  try {
    await chrome.notifications.create({ type: "basic", iconUrl: "icons/icon128.png", title: "OpenIDM", message });
  } catch { /* Notifications can be disabled by the user. */ }
}

async function showCapturedDashboard() {
  return serial(async () => {
    const tabs = await chrome.tabs.query({ url: `${dashboardUrl}*` });
    if (tabs.length) await chrome.tabs.update(tabs[0].id, { active: true });
    else await chrome.tabs.create({ url: dashboardUrl });
  });
}

async function capturedTask(requestId) {
  const data = await api(`/downloads?request_id=${encodeURIComponent(requestId)}`, null, 4000);
  return data.downloads?.[0];
}

async function settleCapture(record, wait = false) {
  let task;
  for (let attempt = 0; attempt < (wait ? 40 : 1); attempt++) {
    task = await capturedTask(record.requestId);
    if (task?.status === "error" || task?.status === "completed" || task?.downloaded_bytes > 0) break;
    if (wait) await new Promise(resolve => setTimeout(resolve, 250));
  }
  if (task?.status === "error") {
    await chrome.downloads.resume(record.id);
    await chrome.storage.local.remove(capturePrefix + record.id);
    await captureNotice("Website memerlukan unduhan melalui browser. Unduhan dilanjutkan di browser.");
  } else if (task?.status === "completed" || task?.downloaded_bytes > 0) {
    // Persist ownership before cancel: a worker restart must not start a second copy.
    await chrome.storage.local.set({ [capturePrefix + record.id]: { ...record, phase: "accepted" } });
    await chrome.downloads.cancel(record.id);
    await chrome.storage.local.remove(capturePrefix + record.id);
    await showCapturedDashboard();
  } else {
    await captureNotice("Menunggu konfirmasi OpenIDM. Unduhan browser dijeda agar tidak ganda. Periksa Dashboard atau lanjutkan dari daftar unduhan browser.");
  }
}

async function captureDownload(item) {
  const url = httpUrl(item.finalUrl || item.url);
  if (!url || item.byExtensionId || browserStartedUrls.has(url) || item.state !== "in_progress" || item.paused || capturing.has(item.id)) return;
  if (new URL(url).origin === new URL(dashboardUrl).origin) return;
  if (item.danger && !["safe", "accepted", "allowlistedByPolicy", "deepScannedSafe"].includes(item.danger)) return;
  capturing.add(item.id);
  let record;
  try {
    if ((await settings()).mode === "browser") return;
    await mutations;
    const { downloadResources = [] } = await chrome.storage.session.get("downloadResources");
    const resource = downloadResources.find(entry => entry.url === url && Date.now() - entry.timestamp < 300000);
    if (resource?.requiresBrowser || (resource?.method && resource.method !== "GET")) return;
    const storageKey = capturePrefix + item.id;
    record = { id: item.id, phase: "preflight", requestId: `${chrome.runtime.id}:${item.id}:${item.startTime || ""}` };
    await chrome.storage.local.set({ [storageKey]: record });
    await chrome.downloads.pause(item.id);
    const core = await status();
    if (!core.online || !core.data?.capabilities?.includes("browser-download-handoff")) {
      await chrome.downloads.resume(item.id);
      await chrome.storage.local.remove(storageKey);
      await captureNotice(core.online
        ? "Perbarui OpenIDM.exe agar mendukung pengalihan otomatis. Unduhan ini tetap dilanjutkan di browser."
        : "OpenIDM belum aktif. Jalankan OpenIDM.exe; unduhan ini tetap dilanjutkan di browser.");
      return;
    }
    record.phase = "submitting";
    await chrome.storage.local.set({ [storageKey]: record });
    const filename = safeFilename(item.filename) || resource?.filename || safeFilename(new URL(url).pathname.split("/").pop()) || "download_file";
    const headers = { ...(resource?.headers || {}) };
    if (!headers.Referer && httpUrl(item.referrer)) headers.Referer = item.referrer;
    // Browser-provided HTTP download metadata is authoritative, including extensionless endpoints.
    await api("/download", { url, filename, title: filename, category: "General", format_id: "direct",
      filesize: Math.max(0, item.totalBytes || 0), headers, request_id: record.requestId });
    await settleCapture(record, true);
  } catch (error) {
    if (record?.phase === "preflight" || (record && error.rejected)) {
      try { await chrome.downloads.resume(item.id); } catch { /* It may already have completed. */ }
      await chrome.storage.local.remove(capturePrefix + item.id);
      if (error.rejected) await captureNotice("OpenIDM belum menerima unduhan ini. Unduhan dilanjutkan di browser.");
    } else if (record) {
      // A lost response can mean the desktop accepted it. Reconcile; never POST it twice.
      try { await settleCapture(record, true); }
      catch { await captureNotice("Konfirmasi OpenIDM terputus. Periksa Dashboard sebelum melanjutkan unduhan browser yang dijeda."); }
    }
  } finally {
    capturing.delete(item.id);
  }
}

async function recoverCaptures() {
  const saved = await chrome.storage.local.get(null);
  for (const [key, record] of Object.entries(saved)) {
    if (!key.startsWith(capturePrefix)) continue;
    try {
      const [item] = await chrome.downloads.search({ id: record.id });
      if (!item || item.state !== "in_progress" || !item.paused) {
        await chrome.storage.local.remove(key);
      } else if (record.phase === "accepted") {
        await chrome.downloads.cancel(record.id);
        await chrome.storage.local.remove(key);
      } else if (record.phase === "preflight") {
        await chrome.downloads.resume(record.id);
        await chrome.storage.local.remove(key);
      } else {
        await settleCapture(record);
      }
    } catch { /* Keep the recovery record until the desktop can be reached. */ }
  }
}

function beginCapture(item) {
  if (captureJobs.has(item.id)) return captureJobs.get(item.id);
  const job = captureDownload(item).finally(() => captureJobs.delete(item.id));
  captureJobs.set(item.id, job);
  return job;
}

// Hold filename finalization, including when onCreated already began this handoff.
chrome.downloads.onCreated.addListener(item => {
  void beginCapture(item).catch(() => {});
});
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  void beginCapture(item).catch(() => {}).finally(() => suggest());
  return true;
});
void recoverCaptures().catch(() => {});
chrome.alarms.onAlarm.addListener(alarm => {
  if (alarm.name === "openidm-captures") void recoverCaptures().catch(() => {});
});
void chrome.alarms.create("openidm-captures", { periodInMinutes: 1 });
