/* Page UI delegates every network request to the extension worker. */
(function () {
  "use strict";
  if (!document.body || location.origin === "http://127.0.0.1:6899") return;
  const cache = new Map();
  let lastUrl = location.href;
  function send(message) {
    return new Promise((resolve, reject) => {
      chrome.runtime.sendMessage(message, response => {
        const error = chrome.runtime.lastError;
        if (error) reject(new Error("Ekstensi diperbarui. Muat ulang halaman ini."));
        else if (!response || response.error || response.success === false) reject(new Error(response?.error || "Ekstensi tidak merespons."));
        else resolve(response);
      });
    });
  }
  function toast(message, type = "success") {
    let element = document.getElementById("openidm-toast");
    if (!element) {
      element = document.createElement("div");
      element.id = "openidm-toast";
      element.setAttribute("role", "status");
      document.body.append(element);
    }
    element.className = `openidm-toast openidm-toast-${type} show`;
    element.textContent = message;
    clearTimeout(element.hideTimer);
    element.hideTimer = setTimeout(() => element.classList.remove("show"), 5000);
  }
  function bytes(value) {
    if (!(value > 0)) return "";
    const i = Math.min(4, Math.floor(Math.log(value) / Math.log(1024)));
    return `${(value / 1024 ** i).toFixed(1)} ${["B", "KB", "MB", "GB", "TB"][i]}`;
  }
  function playerUrl(player) {
    if (player.tagName === "IFRAME") {
      const src = player.getAttribute("data-litespeed-src") || player.getAttribute("src") || player.getAttribute("data-src") || "";
      const dm = src.match(/(?:dailymotion\.com\/(?:embed\/)?video\/|geo\.dailymotion\.com\/player\/[^\s"'<>]+\?video=)([a-zA-Z0-9]+)/i);
      if (dm) return `https://www.dailymotion.com/video/${dm[1]}`;
      try { return new URL(src, location.href).href; } catch { return location.href; }
    }
    return location.href;
  }
  async function loadFormats(container, player) {
    const url = playerUrl(player);
    const source = player.currentSrc || player.src || "";
    const cacheKey = `${url}|${source}`;
    container.textContent = "Menganalisis video di OpenIDM Desktop…";
    try {
      let info = cache.get(cacheKey);
      if (!info) {
        const response = await send({ action: "FETCH_INFO", url });
        info = response.info;
        if (info?.formats?.length) {
          if (cache.size >= 20) cache.delete(cache.keys().next().value);
          cache.set(cacheKey, info);
        }
      }
      if (playerUrl(player) !== url || (player.currentSrc || player.src || "") !== source) return;
      if (info?.formats?.length) { render(container, info); return; }
    } catch { /* Direct files can still be downloaded without the companion. */ }
    try {
      const { media } = await send({ action: "GET_TAB_MEDIA" });
      // Never use another player's stream as an implicit fallback.
      const direct = /^https?:\/\//i.test(source) && player.tagName !== "IFRAME" ? source : null;
      const candidates = media.filter(item => item.mediaType !== "audio");
      const candidate = direct ? { url: direct } : candidates.length === 1 ? candidates[0] : null;
      if (candidate) {
        const platform = OpenIDM.corePage(url);
        render(container, { url: platform ? url : candidate.url, title: document.title, formats: [{ format_id: platform ? "best" : "direct", resolution: "Original", quality: platform ? "Unduh lengkap lewat desktop" : "Unduh media terdeteksi", filesize: candidate.filesize, contentType: candidate.contentType, direct: !platform }] });
        return;
      }
    } catch { /* Show actionable recovery below. */ }
    container.textContent = "Jalankan run.bat untuk pilihan kualitas. Putar video atau buka popup ekstensi untuk memilih media terdeteksi.";
    const retry = document.createElement("button");
    retry.className = "openidm-retry-btn";
    retry.textContent = "Coba lagi";
    retry.addEventListener("click", () => loadFormats(container, player));
    container.append(retry);
  }
  function render(container, info) {
    container.replaceChildren();
    for (const format of info.formats || []) {
      const item = document.createElement("button");
      item.className = "openidm-dropdown-item";
      const left = document.createElement("span");
      left.className = "openidm-item-left";
      const badge = document.createElement("span");
      badge.className = "openidm-badge-res";
      badge.textContent = format.resolution || "Original";
      const label = document.createElement("span");
      label.className = "openidm-item-label";
      label.textContent = format.quality || format.ext || "Video";
      const size = document.createElement("span");
      size.className = "openidm-item-size";
      size.textContent = format.filesize_formatted || bytes(format.filesize);
      left.append(badge, label);
      item.append(left, size);
      item.addEventListener("click", async event => {
        event.stopPropagation();
        if (item.disabled) return;
        item.disabled = true;
        toast("Memulai unduhan…", "info");
        const payload = { url: info.url || location.href, format_id: format.format_id, category: format.category || "Video", filesize: format.filesize || 0 };
        if (format.direct) { payload.kind = "direct"; payload.contentType = format.contentType || ""; }
        else {
          payload.title = info.title;
          const title = (info.title || "video").replace(/[\\/*?:"<>|]/g, "").trim();
          const resolution = format.resolution;
          payload.filename = `${title}${resolution && !["Default", "Original", "Audio"].includes(resolution) ? ` [${resolution}]` : ""}.${format.ext || "mp4"}`;
        }
        try {
          const response = await send({ action: "DOWNLOAD_VIDEO", payload });
          toast(response.engine === "browser" ? "Unduhan dimulai di Chrome." : "Unduhan ditambahkan ke antrean OpenIDM Desktop.");
          container.closest(".openidm-dropdown")?.classList.remove("open");
        } catch (error) { toast(error.message, "error"); }
        finally { item.disabled = false; }
      });
      container.append(item);
    }
  }
  function attach(container, player) {
    if (container.querySelector(":scope > .openidm-floating-panel")) return;
    const panel = document.createElement("div");
    panel.className = "openidm-floating-panel";
    panel.innerHTML = `<button class="openidm-btn-pill" type="button" aria-expanded="false"><svg class="openidm-play-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M12 3v12m-5-5 5 5 5-5M5 16v5h14v-5"/></svg><span class="openidm-btn-text">Download this video</span></button><button class="openidm-close-btn" type="button" aria-label="Sembunyikan tombol OpenIDM">×</button><div class="openidm-dropdown"><div class="openidm-dropdown-header">Pilih kualitas video</div><div class="openidm-dropdown-list" aria-live="polite"></div></div>`;
    if (getComputedStyle(container).position === "static") container.style.position = "relative";
    container.append(panel);
    const trigger = panel.querySelector(".openidm-btn-pill");
    const dropdown = panel.querySelector(".openidm-dropdown");
    let loading = false;
    trigger.addEventListener("click", async event => {
      event.stopPropagation();
      const open = dropdown.classList.toggle("open");
      trigger.setAttribute("aria-expanded", String(open));
      if (open && !loading) {
        loading = true;
        try { await loadFormats(panel.querySelector(".openidm-dropdown-list"), player); }
        finally { loading = false; }
      }
    });
    panel.querySelector(".openidm-close-btn").addEventListener("click", event => { event.stopPropagation(); panel.hidden = true; panel.style.setProperty("display", "none", "important"); });
    panel.addEventListener("keydown", event => {
      if (event.key === "Escape") { dropdown.classList.remove("open"); trigger.setAttribute("aria-expanded", "false"); trigger.focus(); }
    });
  }
  function scan() {
    if (location.hostname === "drive.google.com") {
      if (!document.getElementById("openidm-gdrive-btn")) {
        const pill = document.createElement("button");
        pill.id = "openidm-gdrive-btn";
        pill.className = "openidm-btn-pill";
        pill.textContent = "Buka di OpenIDM Desktop";
        pill.addEventListener("click", () => send({ action: "OPEN_DASHBOARD", url: location.href }).catch(error => toast(error.message, "error")));
        document.body.append(pill);
      }
      return;
    }
    for (const video of document.querySelectorAll("video")) {
      const rect = video.getBoundingClientRect();
      if (rect.width >= 240 && rect.height >= 140) attach(video.closest("#movie_player, .html5-video-player") || video.parentElement, video);
    }
    for (const frame of document.querySelectorAll("iframe")) {
      const src = frame.getAttribute("data-litespeed-src") || frame.getAttribute("src") || frame.getAttribute("data-src") || "";
      try {
        const host = new URL(src, location.href).hostname;
        if (["dailymotion.com", "youtube.com", "youtube-nocookie.com", "youtu.be", "ok.ru", "odnoklassniki.ru", "vimeo.com"].some(d => host === d || host.endsWith(`.${d}`))) {
          attach(frame.closest(".player-embed, .video-content, .player-area, .video-container, .embed-responsive") || frame.parentElement, frame);
        }
      } catch { /* Ignore malformed embedded player URLs. */ }
    }
  }
  let scanTimer;
  new MutationObserver(() => {
    clearTimeout(scanTimer);
    scanTimer = setTimeout(scan, 200);
  }).observe(document.body, { childList: true, subtree: true });
  document.addEventListener("click", event => {
    for (const panel of document.querySelectorAll(".openidm-floating-panel")) {
      if (!panel.contains(event.target)) {
        panel.querySelector(".openidm-dropdown")?.classList.remove("open");
        panel.querySelector(".openidm-btn-pill")?.setAttribute("aria-expanded", "false");
      }
    }
  });
  setInterval(() => {
    if (location.href === lastUrl) return;
    lastUrl = location.href;
    cache.clear();
    document.querySelectorAll(".openidm-dropdown.open").forEach(dropdown => dropdown.classList.remove("open"));
    scan();
  }, 1000);
  scan();
})();
