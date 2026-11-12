"use strict";
const $ = id => document.getElementById(id);
let currentTab = null;
const modeHelp = {
  auto: "Download website otomatis masuk desktop saat aktif; jika belum aktif, tetap memakai browser.",
  browser: "File langsung diunduh oleh Chrome. Streaming dan halaman video memerlukan desktop.",
  core: "Download website diarahkan ke desktop. Jalankan OpenIDM.exe terlebih dahulu."
};
async function send(message) {
  const result = await chrome.runtime.sendMessage(message);
  if (!result || result.success === false || result.error) throw new Error(result?.error || "Ekstensi tidak merespons. Muat ulang ekstensi.");
  return result;
}
function feedback(message, error = false) {
  $("feedback").hidden = false;
  $("feedback").className = error ? "error" : "";
  $("feedback").textContent = message;
}
function bytes(value) {
  if (!(value > 0)) return "Ukuran belum diketahui";
  const i = Math.min(4, Math.floor(Math.log(value) / Math.log(1024)));
  return `${(value / 1024 ** i).toFixed(i ? 1 : 0)} ${["B", "KB", "MB", "GB", "TB"][i]}`;
}
function node(tag, className, text) {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text != null) element.textContent = text;
  return element;
}
async function startDownload(payload, button) {
  const label = button.textContent;
  button.disabled = true;
  button.textContent = "Memulai…";
  try {
    const result = await send({ action: "DOWNLOAD_VIDEO", tabId: currentTab?.id, payload });
    feedback(result.engine === "browser" ? "Unduhan dimulai di Chrome." : "Unduhan masuk antrean desktop. Pantau di Dashboard desktop.");
    await loadDownloads();
  } catch (error) { feedback(error.message, true); }
  finally { button.disabled = false; button.textContent = label; }
}
async function checkStatus() {
  $("status-text").textContent = "Memeriksa desktop…";
  const result = await send({ action: "CHECK_SERVER" });
  $("status-text").className = result.online ? "online" : "offline";
  $("status-text").textContent = result.online ? "Desktop terhubung" : "Desktop belum aktif";
  $("connection-help").textContent = result.online
    ? `${result.data.active_downloads} unduhan aktif di desktop.`
    : "Jalankan OpenIDM.exe agar download website otomatis masuk ke desktop.";
}
async function loadMedia() {
  [currentTab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const url = OpenIDM.httpUrl(currentTab?.url);
  $("page-title").textContent = currentTab?.title || "Halaman tidak tersedia";
  $("page-title").title = currentTab?.title || "";
  $("btn-page").disabled = !url;
  const { media = [] } = await send({ action: "GET_TAB_MEDIA", tabId: currentTab?.id });
  $("media-count").textContent = media.length ? `(${media.length})` : "";
  $("media-list").replaceChildren();
  if (!media.length) {
    $("media-list").append(node("p", "empty-state", url ? "Putar video lalu klik Periksa lagi. Untuk YouTube atau Drive, gunakan Kirim halaman." : "Buka halaman web untuk mendeteksi media. Halaman internal Chrome tidak dapat dipindai."));
    return;
  }
  for (const item of media) {
    const card = node("article", "media-card");
    const top = node("div", "media-card-top");
    top.append(node("span", "media-badge", item.mediaType === "stream" ? "STREAM · DESKTOP" : item.mediaType === "audio" ? "AUDIO" : "VIDEO"), node("span", "media-size", bytes(item.filesize)));
    const title = node("p", "media-url", item.url);
    title.title = item.url;
    const button = node("button", "", "Unduh media");
    button.addEventListener("click", () => {
      // Canonical platform URLs preserve both video and audio tracks.
      const target = OpenIDM.corePage(url) ? url : item.url;
      void startDownload({ url: target, contentType: item.contentType, filesize: item.filesize,
        category: item.mediaType === "audio" ? "Music" : "Video" }, button);
    });
    card.append(top, title, button);
    $("media-list").append(card);
  }
}
async function loadDownloads() {
  const { downloads = [] } = await send({ action: "GET_DOWNLOADS" });
  $("downloads-list").replaceChildren();
  if (!downloads.length) $("downloads-list").append(node("p", "empty-state", "Belum ada unduhan dari ekstensi ini."));
  for (const item of downloads) {
    const entry = node("div", "download-item");
    const name = node("p", "download-name", item.filename.split(/[\\/]/).pop() || "Mengambil nama file…");
    name.title = item.filename;
    const state = item.state === "complete" ? (item.exists ? "Selesai" : "File telah dipindahkan / dihapus") : item.paused ? "Dijeda" : item.state === "interrupted" ? `Terhenti: ${item.error || "coba lagi"}` : "Mengunduh";
    const row = node("div", "download-row");
    row.append(node("span", "download-detail", `${state} · ${bytes(item.bytesReceived)}`));
    const control = item.state === "complete" && item.exists ? "show" : item.paused || item.canResume ? "resume" : item.state === "in_progress" ? "pause" : null;
    if (control) {
      const button = node("button", "quiet", { show: "Folder", resume: "Lanjut", pause: "Jeda" }[control]);
      button.addEventListener("click", async () => {
        try { await send({ action: "CONTROL_DOWNLOAD", id: item.id, control }); await loadDownloads(); }
        catch (error) { feedback(error.message, true); }
      });
      row.append(button);
    }
    entry.append(name, row);
    if (item.state === "in_progress") {
      const progress = node("progress");
      progress.setAttribute("aria-label", `Progres ${name.textContent}`);
      if (item.totalBytes > 0) { progress.max = item.totalBytes; progress.value = item.bytesReceived; }
      entry.append(progress);
    }
    $("downloads-list").append(entry);
  }
}
$("download-form").addEventListener("submit", event => {
  event.preventDefault();
  void startDownload({ url: $("download-url").value.trim() }, $("btn-add"));
});
$("btn-page").addEventListener("click", () => {
  if (currentTab?.url) void startDownload({ url: currentTab.url }, $("btn-page"));
});
$("btn-open-dashboard").addEventListener("click", () => send({ action: "OPEN_DASHBOARD" }).catch(error => feedback(error.message, true)));
$("btn-downloads").addEventListener("click", () => chrome.tabs.create({ url: "chrome://downloads/" }));
$("btn-refresh").addEventListener("click", async () => {
  $("btn-refresh").disabled = true;
  try { await Promise.all([checkStatus(), loadMedia(), loadDownloads()]); }
  catch (error) { feedback(error.message, true); }
  finally { $("btn-refresh").disabled = false; }
});
async function saveSettings() {
  try {
    await send({ action: "SAVE_SETTINGS", settings: { mode: $("download-mode").value, saveAs: $("save-as").checked } });
    $("mode-help").textContent = modeHelp[$("download-mode").value];
  } catch (error) { feedback(error.message, true); }
}
$("download-mode").addEventListener("change", saveSettings);
$("save-as").addEventListener("change", saveSettings);
async function init() {
  const { settings } = await send({ action: "GET_SETTINGS" });
  $("download-mode").value = settings.mode;
  $("save-as").checked = settings.saveAs;
  $("mode-help").textContent = modeHelp[settings.mode];
  await Promise.all([checkStatus(), loadMedia(), loadDownloads()]);
}
void init().catch(error => feedback(error.message, true));
const refreshTimer = setInterval(() => { void loadDownloads().catch(() => {}); }, 1500);
window.addEventListener("pagehide", () => clearInterval(refreshTimer));
