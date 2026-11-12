/* OpenIDM MV3 worker. Downloads outlive this worker. */
importScripts("shared.js");
const API = "http://127.0.0.1:6899/api";
const { httpUrl, mediaKind, isDirect, safeFilename, corePage, defaults } = OpenIDM;
const requestHeaders = new Map();
const browserStartedUrls = new Set();
let mutations = Promise.resolve();
function serial(operation) {
  const result = mutations.then(operation);
  mutations = result.catch(() => {});
  return result;
}
const key = tabId => `media:${tabId}`;
const isUI = sender => sender.url?.startsWith(`chrome-extension://${chrome.runtime.id}/`) || !sender.tab;
async function getMedia(tabId) {
  const data = await chrome.storage.session.get(key(tabId));
  return data[key(tabId)] || [];
}
async function badge(tabId, count) {
  try {
    await chrome.action.setBadgeText({ tabId, text: count ? String(count) : "" });
    await chrome.action.setBadgeBackgroundColor({ tabId, color: "#2563eb" });
  } catch { /* The tab may have closed. */ }
}
async function remember(tabId, item) {
  if (tabId < 0) return;
  return serial(async () => {
    const media = await getMedia(tabId);
    const index = media.findIndex(m => m.url === item.url);
    if (index >= 0) media[index] = { ...media[index], ...item };
    else media.unshift(item);
    media.splice(60);
    await chrome.storage.session.set({ [key(tabId)]: media });
    await badge(tabId, media.length);
  });
}
async function settings() {
  const { settings: saved } = await chrome.storage.local.get("settings");
  return { ...defaults, ...saved };
}
async function api(path, body, timeout = 25000) {
  const response = await fetch(`${API}${path}`, {
    method: body ? "POST" : "GET",
    headers: body ? { "Content-Type": "application/json" } : {},
    ...(body ? { body: JSON.stringify(body) } : {}),
    signal: AbortSignal.timeout(timeout)
  });
  const data = await response.json();
  if (!response.ok || data.success === false || data.error) {
    const error = new Error(data.error || `OpenIDM merespons HTTP ${response.status}.`);
    error.rejected = true;
    throw error;
  }
  return data;
}
async function status() {
  try {
    const data = await api("/status", null, 1800);
    return { online: data.app === "OpenIDM Core" && data.status === "online", data };
  } catch { return { online: false }; }
}
async function browserDownload(payload, options) {
  const filename = safeFilename(payload.filename);
  browserStartedUrls.add(payload.url);
  let id;
  try {
    id = await chrome.downloads.download({ url: payload.url, saveAs: options.saveAs,
      conflictAction: "uniquify", ...(filename ? { filename: `OpenIDM/${filename}` } : {}) });
  } finally {
    browserStartedUrls.delete(payload.url);
  }
  await serial(async () => {
    const { browserDownloads = [] } = await chrome.storage.local.get("browserDownloads");
    await chrome.storage.local.set({ browserDownloads: [id, ...browserDownloads].slice(0, 30) });
  });
  return { success: true, engine: "browser", downloadId: id };
}
async function download(payload, tabId, sender = {}) {
  const url = httpUrl(payload.url);
  if (!url) throw new Error("Masukkan tautan HTTP atau HTTPS yang valid.");
  const options = await settings();
  const mode = isUI(sender) && payload.engine ? payload.engine : options.mode;
  const media = Number.isInteger(tabId) ? await getMedia(tabId) : [];
  const match = media.find(m => m.url === url);
  const direct = !corePage(url) && isDirect(url, match?.contentType || payload.contentType, payload.kind);
  if (mode === "browser") {
    if (!direct) throw new Error("Tautan ini memerlukan OpenIDM Desktop. Pilih mode Otomatis atau Desktop, lalu jalankan OpenIDM.exe.");
    return browserDownload({ ...payload, url }, options);
  }
  const core = await status();
  if (!core.online) {
    if (mode === "auto" && direct) return browserDownload({ ...payload, url }, options);
    throw new Error("OpenIDM Desktop belum aktif. Jalankan OpenIDM.exe, lalu coba lagi. File langsung juga bisa memakai mode Chrome.");
  }
  // Headers belong to this exact resource, never an unrelated request in its tab.
  const headers = { ...(match?.headers || {}) };
  if (!headers.Referer && httpUrl(sender.tab?.url)) headers.Referer = sender.tab.url;
  const body = {
    url, title: String(payload.title || "").slice(0, 300) || undefined,
    filename: safeFilename(payload.filename) || undefined,
    format_id: String(payload.format_id || (direct ? "direct" : "best")),
    category: ["Video", "Music", "Documents", "Programs", "Compressed", "General"].includes(payload.category) ? payload.category : "General",
    filesize: Math.max(0, Number(payload.filesize) || 0), headers,
    ...(isUI(sender) && payload.request_id ? { request_id: payload.request_id } : {})
  };
  // A slow core may already have queued it: never retry via Chrome after submission.
  return { ...await api("/download", body), engine: "core" };
}
chrome.webRequest.onBeforeSendHeaders.addListener(details => {
  if (details.tabId < 0) return;
  const headers = {};
  for (const h of details.requestHeaders || []) {
    const name = { referer: "Referer", "user-agent": "User-Agent", origin: "Origin", cookie: "Cookie" }[h.name.toLowerCase()];
    if (name) headers[name] = h.value;
  }
  requestHeaders.set(details.requestId, { headers, method: details.method || "GET",
    requiresBrowser: (details.requestHeaders || []).some(h => h.name.toLowerCase() === "authorization") });
  if (requestHeaders.size > 500) requestHeaders.delete(requestHeaders.keys().next().value);
}, { urls: ["http://*/*", "https://*/*"] }, ["requestHeaders", "extraHeaders"]);
chrome.webRequest.onHeadersReceived.addListener(details => {
  if (details.tabId < 0 || details.statusCode >= 400) return;
  const headers = Object.fromEntries((details.responseHeaders || []).map(h => [h.name.toLowerCase(), h.value || ""]));
  const kind = mediaKind(details.url, headers["content-type"]);
  const context = requestHeaders.get(details.requestId) || {};
  if (kind || /attachment/i.test(headers["content-disposition"] || "") ||
      (details.type === "main_frame" && !/text\/html/i.test(headers["content-type"] || ""))) {
    void serial(async () => {
      const { downloadResources = [] } = await chrome.storage.session.get("downloadResources");
      const next = downloadResources.filter(item => item.url !== details.url && Date.now() - item.timestamp < 300000);
      const disposition = headers["content-disposition"] || "";
      const named = /filename\*=UTF-8''([^;]+)|filename="([^"]+)"|filename=([^;]+)/i.exec(disposition);
      let filename = named ? named[1] || named[2] || named[3] : "";
      try { filename = decodeURIComponent(filename); } catch { /* Keep the server's original filename. */ }
      next.unshift({ url: details.url, tabId: details.tabId, ...context, filename: safeFilename(filename), timestamp: Date.now() });
      await chrome.storage.session.set({ downloadResources: next.slice(0, 100) });
    }).catch(() => {});
  }
  if (!kind) return;
  const total = /\/(\d+)$/.exec(headers["content-range"] || "");
  void remember(details.tabId, { url: details.url, frameId: details.frameId, contentType: headers["content-type"] || "", mediaType: kind,
    filesize: Number(total?.[1] || headers["content-length"]) || 0,
    headers: context.headers || {}, timestamp: Date.now() }).catch(() => {});
}, { urls: ["http://*/*", "https://*/*"] }, ["responseHeaders"]);
for (const event of [chrome.webRequest.onCompleted, chrome.webRequest.onErrorOccurred]) {
  event.addListener(details => requestHeaders.delete(details.requestId), { urls: ["http://*/*", "https://*/*"] });
}
chrome.tabs.onRemoved.addListener(tabId => { void serial(() => chrome.storage.session.remove(key(tabId))); });
chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (change.url) void serial(async () => { await chrome.storage.session.remove(key(tabId)); await badge(tabId, 0); });
});
async function handle(request, sender) {
  const tabId = isUI(sender) ? (request.tabId ?? sender.tab?.id) : sender.tab?.id;
  switch (request.action) {
    case "GET_TAB_MEDIA": {
      await mutations;
      const media = await getMedia(tabId);
      return { media: media.filter(item => isUI(sender) || item.frameId === sender.frameId).map(({ headers, ...item }) => item) };
    }
    case "CHECK_SERVER": return status();
    case "GET_SETTINGS": return { settings: await settings() };
    case "SAVE_SETTINGS": {
      if (!isUI(sender)) throw new Error("Pengaturan hanya dapat diubah dari popup.");
      const next = request.settings || {};
      if (!["auto", "core", "browser"].includes(next.mode)) throw new Error("Mode tidak valid.");
      const saved = { mode: next.mode, saveAs: Boolean(next.saveAs) };
      await chrome.storage.local.set({ settings: saved });
      return { success: true, settings: saved };
    }
    case "DOWNLOAD_VIDEO": return download(request.payload || {}, tabId, sender);
    case "FETCH_INFO": {
      const url = httpUrl(request.url);
      if (!url) throw new Error("Tautan tidak valid.");
      return api("/info", { url });
    }
    case "OPEN_DASHBOARD": {
      const url = httpUrl(request.url);
      await chrome.tabs.create({ url: `http://127.0.0.1:6899/${url ? `?addUrl=${encodeURIComponent(url)}` : ""}` });
      return { success: true };
    }
    case "GET_DOWNLOADS": {
      if (!isUI(sender)) throw new Error("Buka popup untuk melihat unduhan.");
      const { browserDownloads = [] } = await chrome.storage.local.get("browserDownloads");
      const items = await Promise.all(browserDownloads.slice(0, 6).map(id => chrome.downloads.search({ id })));
      return { downloads: items.flat().map(item => ({ id: item.id, filename: item.filename, state: item.state,
        paused: item.paused, bytesReceived: item.bytesReceived, totalBytes: item.totalBytes,
        canResume: item.canResume, error: item.error, exists: item.exists })) };
    }
    case "CONTROL_DOWNLOAD": {
      if (!isUI(sender)) throw new Error("Buka popup untuk mengatur unduhan.");
      const { browserDownloads = [] } = await chrome.storage.local.get("browserDownloads");
      if (!browserDownloads.includes(request.id)) throw new Error("Unduhan tidak ditemukan.");
      const controls = { pause: "pause", resume: "resume", show: "show" };
      if (!controls[request.control]) throw new Error("Perintah tidak valid.");
      await chrome.downloads[controls[request.control]](request.id);
      return { success: true };
    }
    default: throw new Error("Perintah tidak dikenal.");
  }
}
chrome.runtime.onMessage.addListener((request, sender, sendResponse) => {
  if (sender.id !== chrome.runtime.id) return false;
  handle(request, sender).then(sendResponse).catch(error => sendResponse({ success: false, error: error.message }));
  return true;
});
importScripts("capture.js");
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: "openidm-download", title: "Unduh dengan OpenIDM", contexts: ["link", "video", "audio", "page"], documentUrlPatterns: ["http://*/*", "https://*/*"] });
  });
});
chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== "openidm-download") return;
  void download({ url: info.linkUrl || info.srcUrl || info.pageUrl,
    kind: ["video", "audio"].includes(info.mediaType) ? "direct" : undefined }, tab?.id, { tab }).then(result => {
    return chrome.notifications.create({ type: "basic", iconUrl: "icons/icon128.png", title: "OpenIDM",
      message: result.engine === "browser" ? "Unduhan dimulai di Chrome." : "Unduhan dikirim ke OpenIDM Desktop." });
  }).catch(error => {
    void chrome.notifications.create({ type: "basic", iconUrl: "icons/icon128.png", title: "OpenIDM — unduhan belum dimulai", message: error.message });
  });
});
