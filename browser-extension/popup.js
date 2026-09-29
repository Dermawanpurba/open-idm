// OpenIDM Extension - Popup Script

const statusIndicator = document.getElementById("status-indicator");
const statusText = document.getElementById("status-text");
const mediaList = document.getElementById("media-list");
const btnOpenDashboard = document.getElementById("btn-open-dashboard");

function formatBytes(bytes) {
  if (!bytes || bytes <= 0) return "Ukuran tidak diketahui";
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`;
}

// Check OpenIDM Core Status
chrome.runtime.sendMessage({ action: "CHECK_SERVER" }, (response) => {
  if (response && response.online) {
    statusIndicator.className = "status-indicator online";
    statusText.textContent = "Core Online (Port 6899)";
  } else {
    statusIndicator.className = "status-indicator offline";
    statusText.textContent = "Core Offline (Jalankan run.bat)";
  }
});

// Load Media for Active Tab
chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
  if (!tabs || tabs.length === 0) return;
  const currentTab = tabs[0];

  chrome.runtime.sendMessage({ action: "GET_TAB_MEDIA", tabId: currentTab.id }, (response) => {
    const media = response && response.media ? response.media : [];

    if (media.length === 0) {
      mediaList.innerHTML = `<div class="empty-state">Tidak ada media streaming yang terdeteksi di tab ini. Coba putar video di halaman web.</div>`;
      return;
    }

    mediaList.innerHTML = "";
    media.forEach((item, index) => {
      const card = document.createElement("div");
      card.className = "media-card";

      const ext = item.url.split("?")[0].split(".").pop().toUpperCase() || "MEDIA";
      const sizeStr = formatBytes(item.filesize);

      card.innerHTML = `
        <div class="media-card-top">
          <span class="media-badge">${ext}</span>
          <span class="media-size">${sizeStr}</span>
        </div>
        <div class="media-url" title="${item.url}">${item.url}</div>
        <button class="btn-download-sm" id="btn-dl-${index}">Unduh via OpenIDM</button>
      `;

      card.querySelector(`#btn-dl-${index}`).addEventListener("click", () => {
        const btn = card.querySelector(`#btn-dl-${index}`);
        btn.textContent = "Mengirim...";
        btn.disabled = true;

        // If on YouTube or social platforms, use the main page URL so yt-dlp fetches full video+audio tracks
        const tabUrl = currentTab.url || "";
        const isPlatform = [
          "youtube.com", "youtu.be", "tiktok.com", "instagram.com", "twitter.com", "x.com"
        ].some(p => tabUrl.toLowerCase().includes(p));

        const targetUrl = (isPlatform && tabUrl.startsWith("http")) ? tabUrl : item.url;

        chrome.runtime.sendMessage({
          action: "DOWNLOAD_VIDEO",
          payload: {
            url: targetUrl,
            category: "Video",
            format_id: "best",
            title: currentTab.title || "Video Download"
          }
        }, (res) => {
          if (res && res.success) {
            btn.textContent = "✓ Terkirim!";
            btn.style.background = "#2563EB";
          } else {
            btn.textContent = "Gagal kirim";
            btn.style.background = "#EF4444";
          }
        });
      });

      mediaList.appendChild(card);
    });
  });
});

// Open Dashboard
btnOpenDashboard.addEventListener("click", () => {
  chrome.tabs.create({ url: "http://127.0.0.1:6899" });
});
