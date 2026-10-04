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
      const tabUrl = currentTab.url || "";
      if (tabUrl.includes("anichin.") || tabUrl.includes("dailymotion.") || tabUrl.includes("youtube.") || tabUrl.includes("youtu.be")) {
        mediaList.innerHTML = `
          <div class="media-card">
            <div class="media-card-top">
              <span class="media-badge">STREAMING PAGE</span>
              <span class="media-size">Deteksi Otomatis</span>
            </div>
            <div class="media-url" title="${tabUrl}">${tabUrl}</div>
            <button class="btn-download-sm" id="btn-dl-tab">Analisis & Unduh Video Halaman Ini</button>
          </div>
        `;
        document.getElementById("btn-dl-tab").addEventListener("click", () => {
          const btn = document.getElementById("btn-dl-tab");
          btn.textContent = "Menganalisis & Mengirim...";
          btn.disabled = true;
          chrome.runtime.sendMessage({
            action: "DOWNLOAD_VIDEO",
            payload: {
              url: tabUrl,
              category: "Video",
              format_id: "best",
              title: currentTab.title || "Video Download",
              headers: { "Referer": tabUrl }
            }
          }, (res) => {
            if (res && res.success) {
              btn.textContent = "✓ Terkirim ke OpenIDM!";
              btn.style.background = "#2563EB";
            } else {
              btn.textContent = "Gagal kirim";
              btn.style.background = "#EF4444";
            }
          });
        });
        return;
      }

      mediaList.innerHTML = `<div class="empty-state">Tidak ada media streaming yang terdeteksi di tab ini. Coba putar video di halaman web.</div>`;
      return;
    }

    mediaList.innerHTML = "";
    media.forEach((item, index) => {
      const card = document.createElement("div");
      card.className = "media-card";

      let ext = "VIDEO";
      const cleanPath = item.url.split("?")[0].split("#")[0];
      const match = cleanPath.match(/\.([a-zA-Z0-9]{2,5})$/);
      if (match) {
        ext = match[1].toUpperCase();
      } else if (item.url.includes("vod3.cf.dmcdn.net") || item.url.includes("dmcdn.net") || item.url.includes("dailymotion.com")) {
        ext = "VIDEO (DM)";
      } else if (item.mediaType === "audio") {
        ext = "AUDIO";
      } else {
        ext = "VIDEO";
      }
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

        // If on YouTube, Dailymotion or social platforms, use the main page URL so yt-dlp fetches full video+audio tracks
        const tabUrl = currentTab.url || "";
        const isPlatform = [
          "youtube.com", "youtu.be", "tiktok.com", "instagram.com", "twitter.com", "x.com", "dailymotion.com"
        ].some(p => tabUrl.toLowerCase().includes(p));

        const isDmCdn = item.url.includes("vod3.cf.dmcdn.net") || item.url.includes("dmcdn.net");
        // Prioritize canonical Dailymotion URL if item is already resolved, otherwise use tabUrl for platform
        let targetUrl = item.url;
        if (isPlatform && !isDmCdn && tabUrl.startsWith("http") && !item.url.includes("dailymotion.com/video/")) {
          targetUrl = tabUrl;
        }

        chrome.runtime.sendMessage({
          action: "DOWNLOAD_VIDEO",
          payload: {
            url: targetUrl,
            category: "Video",
            format_id: "best",
            filesize: item.filesize || 0,
            title: currentTab.title || "Video Download",
            headers: item.headers || { "Referer": currentTab.url || "" }
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
