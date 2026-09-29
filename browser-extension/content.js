// OpenIDM Content Script - Floating "Download this video" Panel

(function() {
  "use strict";

  const OPENIDM_API = "http://127.0.0.1:6899/api";
  let activePanel = null;
  let cachedVideoInfo = null;
  let isFetchingFormats = false;

  // Format Bytes Helper
  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "";
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return ` (~${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]})`;
  }

  // Toast Notification
  function showToast(message, type = "success") {
    let toast = document.getElementById("openidm-toast");
    if (!toast) {
      toast = document.createElement("div");
      toast.id = "openidm-toast";
      document.body.appendChild(toast);
    }
    toast.className = `openidm-toast openidm-toast-${type} show`;
    toast.textContent = message;

    setTimeout(() => {
      toast.classList.remove("show");
    }, 3500);
  }

  // Create or attach floating button to player container
  function attachDownloadButton(targetContainer, videoEl) {
    if (targetContainer.querySelector(".openidm-floating-panel")) {
      return;
    }

    const panel = document.createElement("div");
    panel.className = "openidm-floating-panel";
    panel.innerHTML = `
      <div class="openidm-btn-pill" id="openidm-trigger-btn" title="Download this video with OpenIDM">
        <svg class="openidm-play-icon" viewBox="0 0 24 24" fill="currentColor">
          <polygon points="5 3 19 12 5 21 5 3"></polygon>
        </svg>
        <span class="openidm-btn-text">Download this video</span>
        <svg class="openidm-arrow-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="6 9 12 15 18 9"></polyline>
        </svg>
      </div>
      <button class="openidm-close-btn" title="Sembunyikan">✕</button>

      <div class="openidm-dropdown" id="openidm-dropdown">
        <div class="openidm-dropdown-header">Pilih Kualitas Video:</div>
        <div class="openidm-dropdown-list" id="openidm-formats-list">
          <div class="openidm-loading-item">Memuat resolusi...</div>
        </div>
      </div>
    `;

    // Ensure relative positioning on parent
    const style = window.getComputedStyle(targetContainer);
    if (style.position === "static") {
      targetContainer.style.position = "relative";
    }

    targetContainer.appendChild(panel);
    activePanel = panel;

    const triggerBtn = panel.querySelector("#openidm-trigger-btn");
    const closeBtn = panel.querySelector(".openidm-close-btn");
    const dropdown = panel.querySelector("#openidm-dropdown");
    const formatsList = panel.querySelector("#openidm-formats-list");

    // Close button handler
    closeBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      panel.style.display = "none";
    });

    // Toggle dropdown
    triggerBtn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const isOpen = dropdown.classList.toggle("open");
      if (isOpen) {
        await loadVideoFormats(formatsList, videoEl);
      }
    });

    // Close on click outside
    document.addEventListener("click", (e) => {
      if (!panel.contains(e.target)) {
        dropdown.classList.remove("open");
      }
    });
  }

  // Load formats from OpenIDM Core or fallback to direct video stream
  async function loadVideoFormats(container, videoEl) {
    if (cachedVideoInfo && cachedVideoInfo.formats && cachedVideoInfo.formats.length > 0) {
      renderFormats(container, cachedVideoInfo);
      return;
    }

    if (isFetchingFormats) return;
    isFetchingFormats = true;
    container.innerHTML = `<div class="openidm-loading-item">Menganalisis stream video...</div>`;

    const currentUrl = window.location.href;

    try {
      // 1. Try query OpenIDM Core for rich formats
      const res = await fetch(`${OPENIDM_API}/info`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ url: currentUrl })
      });
      const data = await res.json();

      if (data.success && data.info) {
        cachedVideoInfo = data.info;
        renderFormats(container, cachedVideoInfo);
        isFetchingFormats = false;
        return;
      }
    } catch (err) {
      // OpenIDM Core offline or error, continue to fallback
    }

    // 2. Fallback: Extract from <video> src or sniffed media
    const videoSrc = videoEl.currentSrc || videoEl.src;
    const pageTitle = document.title.replace(" - YouTube", "").trim() || "Video Download";

    if (videoSrc) {
      const fallbackInfo = {
        title: pageTitle,
        url: videoSrc,
        formats: [
          {
            format_id: "best",
            quality: "Video Asli (Direct Stream)",
            resolution: "Original",
            ext: "mp4",
            filesize: 0,
            has_video: true,
            has_audio: true
          }
        ]
      };
      renderFormats(container, fallbackInfo);
    } else {
      container.innerHTML = `
        <div class="openidm-error-item">
          Pastikan OpenIDM Desktop aktif di background.<br>
          <button class="openidm-retry-btn" id="openidm-btn-retry">Coba Lagi</button>
        </div>
      `;
      const retryBtn = container.querySelector("#openidm-btn-retry");
      if (retryBtn) {
        retryBtn.addEventListener("click", () => {
          isFetchingFormats = false;
          loadVideoFormats(container, videoEl);
        });
      }
    }
    isFetchingFormats = false;
  }

  // Render formats list inside dropdown
  function renderFormats(container, info) {
    container.innerHTML = "";
    const formats = info.formats || [];

    if (formats.length === 0) {
      container.innerHTML = `<div class="openidm-empty-item">Tidak ada format yang ditemukan.</div>`;
      return;
    }

    formats.forEach(f => {
      const item = document.createElement("div");
      item.className = "openidm-dropdown-item";

      const sizeStr = f.filesize_formatted || formatBytes(f.filesize);
      item.innerHTML = `
        <div class="openidm-item-left">
          <span class="openidm-badge-res">${f.resolution}</span>
          <span class="openidm-item-label">${f.quality}</span>
        </div>
        <span class="openidm-item-size">${sizeStr}</span>
      `;

      item.addEventListener("click", (e) => {
        e.stopPropagation();
        triggerDownload(info.url || window.location.href, info.title, f.format_id, f.category || "Video");
        const dropdown = container.closest(".openidm-dropdown");
        if (dropdown) dropdown.classList.remove("open");
      });

      container.appendChild(item);
    });
  }

  // Send download request to OpenIDM Core
  async function triggerDownload(url, title, format_id, category) {
    showToast("Mengirim unduhan ke OpenIDM Desktop...", "info");

    try {
      const res = await fetch(`${OPENIDM_API}/download`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          url: url,
          title: title,
          format_id: format_id,
          category: category || "Video"
        })
      });

      const data = await res.json();
      if (data.success) {
        showToast("✓ Unduhan berhasil ditambahkan ke antrean OpenIDM!");
      } else {
        showToast("Gagal: " + (data.error || "Gagal memulai unduhan"), "error");
      }
    } catch (err) {
      showToast("Gagal terhubung ke OpenIDM. Jalankan aplikasi desktop terlebih dahulu!", "error");
    }
  }

  // Scanner for video players on the page
  function scanAndAttach() {
    // 1. YouTube Player Specific
    const ytPlayer = document.querySelector("#movie_player, .html5-video-player");
    const ytVideo = document.querySelector("video.html5-main-video, video");

    if (ytPlayer && ytVideo) {
      attachDownloadButton(ytPlayer, ytVideo);
      return;
    }

    // 2. Generic HTML5 Videos
    const videos = document.querySelectorAll("video");
    videos.forEach(video => {
      // Must be visible and reasonably sized
      const rect = video.getBoundingClientRect();
      if (rect.width >= 240 && rect.height >= 140) {
        const parent = video.parentElement || video;
        attachDownloadButton(parent, video);
      }
    });
  }

  // Observe DOM changes (for SPAs like YouTube)
  const observer = new MutationObserver(() => {
    scanAndAttach();
  });

  observer.observe(document.body, {
    childList: true,
    subtree: true
  });

  // Reset cached info on URL changes
  let lastUrl = location.href;
  setInterval(() => {
    if (location.href !== lastUrl) {
      lastUrl = location.href;
      cachedVideoInfo = null;
      if (activePanel) {
        const dropdown = activePanel.querySelector("#openidm-dropdown");
        if (dropdown) dropdown.classList.remove("open");
      }
      scanAndAttach();
    }
  }, 1000);

  // Initial scan
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", scanAndAttach);
  } else {
    scanAndAttach();
  }

})();
