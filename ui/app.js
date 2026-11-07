// OpenIDM Dashboard Frontend Logic

let currentCategory = "all";
let currentStatus = "all";
let downloadsData = [];
let detectedFormats = [];
let selectedVideoInfo = null;
let expandedTaskIds = new Set();

window.toggleConnectionDetails = function(id, event) {
  if (event) event.stopPropagation();
  if (expandedTaskIds.has(id)) {
    expandedTaskIds.delete(id);
  } else {
    expandedTaskIds.add(id);
  }
  renderDownloads();
};

function escapeHtml(str) {
  if (!str) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

window.showErrorModal = function(id) {
  const item = downloadsData.find(d => d.id === id);
  if (!item) return;

  const modal = document.getElementById("error-modal");
  const titleEl = document.getElementById("error-modal-title");
  const msgEl = document.getElementById("error-modal-msg");
  const btnRetry = document.getElementById("error-modal-retry");

  if (titleEl) titleEl.textContent = item.title || "Detail Kesalahan Unduhan";
  if (msgEl) msgEl.textContent = item.error_message || "Terjadi kesalahan koneksi atau stream media tidak dapat diakses.";
  if (btnRetry) {
    btnRetry.onclick = () => {
      hideErrorModal();
      controlDownload(item.id, 'resume');
    };
  }
  if (modal) modal.classList.remove("hidden");
};

window.hideErrorModal = function() {
  const modal = document.getElementById("error-modal");
  if (modal) modal.classList.add("hidden");
};

// DOM Elements
const downloadsList = document.getElementById("downloads-list");
const emptyState = document.getElementById("empty-state");
const totalSpeedEl = document.getElementById("total-speed");
const searchInput = document.getElementById("search-input");
const categoryFilter = document.getElementById("category-filter");
const statusFilter = document.getElementById("status-filter");

// Modal Elements
const addModal = document.getElementById("add-modal");
const btnOpenAddModal = document.getElementById("btn-open-add-modal");
const btnCloseModal = document.getElementById("btn-close-modal");
const btnCancelModal = document.getElementById("btn-cancel-modal");
const btnEmptyAdd = document.getElementById("btn-empty-add");
const btnFetchInfo = document.getElementById("btn-fetch-info");
const btnStartDownload = document.getElementById("btn-start-download");
const inputUrl = document.getElementById("input-url");
const selectFormat = document.getElementById("select-format");
const selectCategory = document.getElementById("select-category");
const videoPreviewCard = document.getElementById("video-preview-card");
const previewThumb = document.getElementById("preview-thumb");
const previewTitle = document.getElementById("preview-title");
const previewDuration = document.getElementById("preview-duration");
const btnOpenFolder = document.getElementById("btn-open-folder");
const btnRefresh = document.getElementById("btn-refresh");

// Google Drive Batch Elements
const gdriveFolderPanel = document.getElementById("gdrive-folder-panel");
const gdriveFilesList = document.getElementById("gdrive-files-list");
const gdriveCountLabel = document.getElementById("gdrive-count-label");
const btnGdriveSelectAll = document.getElementById("btn-gdrive-select-all");
const btnGdriveDeselectAll = document.getElementById("btn-gdrive-deselect-all");
const groupFormat = document.getElementById("group-format");

// Badges
const badgeAll = document.getElementById("badge-all");
const badgeVideo = document.getElementById("badge-video");
const badgeMusic = document.getElementById("badge-music");
const badgeCompressed = document.getElementById("badge-compressed");
const badgeDocs = document.getElementById("badge-docs");
const badgePrograms = document.getElementById("badge-programs");

const STATUS_LABEL = { downloading: "Mengunduh", completed: "Selesai", paused: "Dijeda", error: "Error", queued: "Antre", pending: "Menunggu" };
const ICON_CHEVRON_DOWN = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>';
const ICON_CHEVRON_UP = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="18 15 12 9 6 15"></polyline></svg>';
const ICON_LAYERS = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="12 2 2 7 12 12 22 7 12 2"></polygon><polyline points="2 17 12 22 22 17"></polyline><polyline points="2 12 12 17 22 12"></polyline></svg>';
const ICON_ALERT = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path><line x1="12" y1="9" x2="12" y2="13"></line><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>';

function setText(id, v) {
  const el = document.getElementById(id);
  if (el) el.textContent = v;
}

// Utility: Format Bytes
function formatBytes(bytes) {
  if (!bytes || bytes <= 0) return "0 B";
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`;
}

// Utility: Format Speed
function formatSpeed(bytesPerSec) {
  if (!bytesPerSec || bytesPerSec <= 0) return "0 KB/s";
  return `${formatBytes(bytesPerSec)}/s`;
}

// Utility: Format ETA
function formatEta(seconds) {
  if (!seconds || seconds <= 0 || !isFinite(seconds)) return "-";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  const h = Math.floor(m / 60);
  if (h > 0) return `${h}j ${m % 60}m`;
  if (m > 0) return `${m}m ${s}d`;
  return `${s} dtk`;
}

// Fetch Downloads from Backend
async function fetchDownloads() {
  try {
    let url = `/api/downloads?`;
    if (currentCategory !== "all") url += `category=${encodeURIComponent(currentCategory)}&`;
    if (currentStatus !== "all") url += `status=${encodeURIComponent(currentStatus)}&`;

    const res = await fetch(url);
    const data = await res.json();
    if (data.success) {
      downloadsData = data.downloads || [];
      renderDownloads();
      updateBadges();
    }
  } catch (err) {
    console.error("Gagal mengambil data unduhan:", err);
  }
}

// Render Table
function renderDownloads() {
  const query = searchInput.value.toLowerCase().trim();
  const filtered = downloadsData.filter(item => {
    if (!query) return true;
    return (
      (item.title && item.title.toLowerCase().includes(query)) ||
      (item.filename && item.filename.toLowerCase().includes(query))
    );
  });

  // Calculate Total Speed
  let totalSpeed = 0;
  downloadsData.forEach(d => {
    if (d.status === "downloading" && d.speed) {
      totalSpeed += d.speed;
    }
  });
  totalSpeedEl.textContent = formatSpeed(totalSpeed);
  setText("stat-active", downloadsData.filter(d => d.status === "downloading").length);
  setText("stat-done", downloadsData.filter(d => d.status === "completed").length);
  setText("stat-total", downloadsData.length);

  if (filtered.length === 0) {
    downloadsList.innerHTML = "";
    emptyState.classList.remove("hidden");
    return;
  }

  emptyState.classList.add("hidden");

  let html = "";
  filtered.forEach(item => {
    const isDownloading = item.status === "downloading";
    const isCompleted = item.status === "completed";
    const isPaused = item.status === "paused";
    const isError = item.status === "error";

    // Auto-expand downloading tasks if user hasn't explicitly collapsed
    if (isDownloading && !item._seen_auto_expand && expandedTaskIds.size === 0) {
      expandedTaskIds.add(item.id);
      item._seen_auto_expand = true;
    }

    const thumbHtml = item.thumbnail
      ? `<img src="${escapeHtml(item.thumbnail)}" class="file-thumb" alt="" loading="lazy">`
      : `<div class="file-icon-box">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polygon points="23 7 16 12 23 17 23 7"></polygon>
            <rect x="1" y="5" width="15" height="14" rx="2" ry="2"></rect>
          </svg>
        </div>`;

    const progressPercent = item.progress || 0;
    const progressFillClass = isCompleted ? "completed" : isError ? "error" : "";

    html += `
      <tr data-id="${item.id}" class="${expandedTaskIds.has(item.id) ? 'row-expanded' : ''}">
        <td>
          <div class="file-cell">
            ${thumbHtml}
            <div class="file-info">
              <span class="file-title" title="${escapeHtml(item.title)}">${escapeHtml(item.title)}</span>
              <span class="file-meta">${escapeHtml(item.filename)}</span>
            </div>
          </div>
        </td>
        <td class="cell-num">${item.filesize > 0 ? formatBytes(item.filesize) : "-"}</td>
        <td>
          <div class="progress-box">
            <div class="progress-bar-bg">
              <div class="progress-bar-fill ${progressFillClass}" style="width: ${progressPercent}%;"></div>
            </div>
            <div class="progress-label">
              <span>${progressPercent}%</span>
              <span>${formatBytes(item.downloaded_bytes)}</span>
            </div>
          </div>
        </td>
        <td class="cell-num">
          ${isDownloading ? formatSpeed(item.speed) : "-"}
          ${(isDownloading || isPaused || isError) ? `
            <div style="margin-top: 4px;">
              <button class="btn-toggle-connection ${isError ? 'is-error' : ''}" onclick="toggleConnectionDetails('${item.id}', event)" aria-expanded="${expandedTaskIds.has(item.id)}" title="Lihat/Sembunyikan pembagian koneksi">
                ${isError ? ICON_ALERT + ' Info Kendala' : ICON_LAYERS + ` ${item.num_chunks || 8} Koneksi`} ${expandedTaskIds.has(item.id) ? ICON_CHEVRON_UP : ICON_CHEVRON_DOWN}
              </button>
            </div>
          ` : ''}
        </td>
        <td>
          <span class="status-pill ${escapeHtml(item.status)}" ${item.error_message ? `title="${escapeHtml(item.error_message)}" role="button" tabindex="0" onclick="showErrorModal('${item.id}')"` : ''}>${STATUS_LABEL[item.status] || escapeHtml(item.status)}</span>
        </td>
        <td>
          <div class="row-actions">
            ${
              isDownloading
                ? `<button class="btn btn-ghost btn-icon" onclick="controlDownload('${item.id}', 'pause')" title="Jeda" aria-label="Jeda">
                     <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="6" y="4" width="4" height="16"></rect><rect x="14" y="4" width="4" height="16"></rect></svg>
                   </button>`
                : isPaused || isError
                ? `<button class="btn btn-ghost btn-icon" onclick="controlDownload('${item.id}', 'resume')" title="${isError ? 'Coba Lagi (Retry)' : 'Lanjutkan'}" aria-label="${isError ? 'Coba lagi' : 'Lanjutkan'}">
                     <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="5 3 19 12 5 21 5 3"></polygon></svg>
                   </button>`
                : ""
            }
            ${
              isCompleted
                ? `<button class="btn btn-ghost btn-icon" onclick="openFile('${item.id}')" title="Buka Berkas" aria-label="Buka berkas">
                     <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="5 3 19 12 5 21 5 3"></polygon></svg>
                   </button>`
                : ""
            }
            <button class="btn btn-ghost btn-icon" onclick="openFolder('${item.id}')" title="Buka Folder" aria-label="Buka folder">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path></svg>
            </button>
            <button class="btn btn-ghost btn-icon btn-danger-ghost" onclick="deleteDownload('${item.id}')" title="Hapus" aria-label="Hapus">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
            </button>
          </div>
        </td>
      </tr>
    `;

    // Render Connection Inspector Row if expanded
    if (expandedTaskIds.has(item.id)) {
      if (isError) {
        html += `
          <tr class="connection-inspector-row">
            <td colspan="6">
              <div class="connection-card error-card">
                <div class="connection-error-banner">
                  <div class="connection-error-icon">
                    <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2">
                      <circle cx="12" cy="12" r="10"></circle>
                      <line x1="12" y1="8" x2="12" y2="12"></line>
                      <line x1="12" y1="16" x2="12.01" y2="16"></line>
                    </svg>
                  </div>
                  <div class="connection-error-content">
                    <div class="connection-error-title">Gagal Mengunduh Berkas</div>
                    <div class="connection-error-message">${escapeHtml(item.error_message || "Koneksi ke server terputus atau waktu habis. Klik 'Coba Lagi' untuk menyambungkan kembali.")}</div>
                  </div>
                  <div class="connection-error-actions">
                    <button class="btn btn-primary btn-sm" onclick="controlDownload('${item.id}', 'resume')" title="Coba unduh ulang sekarang">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="23 4 23 10 17 10"></polyline><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"></path></svg>
                      Coba Lagi (Retry)
                    </button>
                    <button class="btn btn-outline btn-sm" onclick="deleteDownload('${item.id}')" title="Hapus dari antrean">
                      Hapus
                    </button>
                  </div>
                </div>
              </div>
            </td>
          </tr>
        `;
      } else {
        const chunks = item.chunks || [];
        const numChunks = item.num_chunks || (chunks.length || 8);

        let slotsHtml = "";
        if (chunks.length > 0) {
          chunks.forEach(c => {
            slotsHtml += `
              <div class="connection-segment-slot" title="Koneksi #${c.num}: ${c.percent}% (${formatBytes(c.downloaded)} / ${formatBytes(c.total)})">
                <div class="connection-segment-fill" style="width: ${c.percent}%;"></div>
              </div>
            `;
          });
        } else {
          for (let i = 0; i < numChunks; i++) {
            slotsHtml += `
              <div class="connection-segment-slot">
                <div class="connection-segment-fill" style="width: ${progressPercent}%;"></div>
              </div>
            `;
          }
        }

        let tableRowsHtml = "";
        if (chunks.length > 0) {
          chunks.forEach(c => {
            const isDone = c.status === "Selesai";
            const isActive = !isDone && c.status && c.status.includes("Menerima");
            const dotClass = isDone ? "done" : "live";
            tableRowsHtml += `
              <tr>
                <td class="num" style="font-weight: 600; width: 45px; text-align: center;">${c.num}</td>
                <td class="num">${formatBytes(c.downloaded)}</td>
                <td class="num" style="color: var(--color-text-muted);">${formatBytes(c.total)} (${c.percent}%)</td>
                <td>
                  <span class="connection-status-dot ${dotClass} ${isActive ? 'active' : ''}"></span>
                  <span>${escapeHtml(c.status)}</span>
                </td>
              </tr>
            `;
          });
        } else {
          const loadingText = isDownloading ? "Sedang menghubungkan ke server & mengunduh stream..." : isPaused ? "Unduhan dijeda" : isCompleted ? "Unduhan selesai" : "Menunggu koneksi...";
          tableRowsHtml = `
            <tr>
              <td colspan="4" style="text-align: center; color: var(--color-text-muted); padding: 10px;">
                ${loadingText}
              </td>
            </tr>
          `;
        }

        html += `
          <tr class="connection-inspector-row">
            <td colspan="6">
              <div class="connection-card">
                <div class="connection-card-header">
                  <div class="connection-badges">
                    <span class="badge-connections">${numChunks} koneksi multi-segmen</span>
                    <span class="badge-resume">Kemampuan resume: ${item.can_resume !== false ? 'Ya' : 'Tidak'}</span>
                  </div>
                  <div class="connection-meta-stats">
                    <span>Tingkat transfer: <strong>${formatSpeed(item.speed)}</strong></span>
                    <span>Waktu tersisa: <strong>${formatEta(item.eta)}</strong></span>
                    <span>Diunduh: <strong>${formatBytes(item.downloaded_bytes)} (${progressPercent}%)</strong></span>
                  </div>
                </div>

                <div class="connection-bar-wrapper">
                  <div class="connection-bar-title">Progres posisi mulai dan unduh berdasarkan koneksi:</div>
                  <div class="connection-segments-track">
                    ${slotsHtml}
                  </div>
                </div>

                <div class="connection-table-wrapper">
                  <table class="connection-table">
                    <thead>
                      <tr>
                        <th style="width: 45px; text-align: center;">N.</th>
                        <th style="width: 140px;">Diunduh</th>
                        <th style="width: 160px;">Total Segmen</th>
                        <th>Status Koneksi</th>
                      </tr>
                    </thead>
                    <tbody>
                      ${tableRowsHtml}
                    </tbody>
                  </table>
                </div>
              </div>
            </td>
          </tr>
        `;
      }
    }
  });

  downloadsList.innerHTML = html;
}

// Update Category Badges
function updateBadges() {
  const counts = {
    all: downloadsData.length,
    Video: 0,
    Music: 0,
    Compressed: 0,
    Documents: 0,
    Programs: 0,
  };

  downloadsData.forEach(d => {
    if (counts[d.category] !== undefined) {
      counts[d.category]++;
    }
  });

  badgeAll.textContent = counts.all;
  badgeVideo.textContent = counts.Video;
  badgeMusic.textContent = counts.Music;
  badgeCompressed.textContent = counts.Compressed;
  badgeDocs.textContent = counts.Documents;
  badgePrograms.textContent = counts.Programs;
}

// Control Actions
window.controlDownload = async function(id, action) {
  try {
    await fetch("/api/control", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, action })
    });
    fetchDownloads();
  } catch (err) {
    console.error(`Gagal melakukan aksi ${action}:`, err);
  }
};

window.deleteDownload = async function(id) {
  if (!confirm("Apakah Anda yakin ingin menghapus riwayat unduhan ini?")) return;
  try {
    await fetch("/api/control", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, action: "delete" })
    });
    fetchDownloads();
  } catch (err) {
    console.error("Gagal menghapus unduhan:", err);
  }
};

window.openFile = async function(id) {
  try {
    await fetch("/api/open-file", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id })
    });
  } catch (err) {
    console.error("Gagal membuka file:", err);
  }
};

window.openFolder = async function(id) {
  try {
    await fetch("/api/open-folder", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id })
    });
  } catch (err) {
    console.error("Gagal membuka folder:", err);
  }
};

// Modal Operations
function showModal() {
  addModal.classList.remove("hidden");
  inputUrl.focus();
}

function hideModal() {
  addModal.classList.add("hidden");
  inputUrl.value = "";
  videoPreviewCard.classList.add("hidden");
  if (gdriveFolderPanel) gdriveFolderPanel.classList.add("hidden");
  if (groupFormat) groupFormat.classList.remove("hidden");
  selectFormat.innerHTML = `<option value="best">Kualitas Terbaik (Otomatis)</option>`;
  selectedVideoInfo = null;
  btnStartDownload.textContent = "Mulai Unduh";
  btnStartDownload.disabled = false;
}

btnOpenAddModal.addEventListener("click", showModal);
btnEmptyAdd.addEventListener("click", showModal);
btnCloseModal.addEventListener("click", hideModal);
btnCancelModal.addEventListener("click", hideModal);

// Google Drive Helper Functions
function updateGdriveSelectionCount() {
  const checks = document.querySelectorAll(".gdrive-check");
  const checked = document.querySelectorAll(".gdrive-check:checked");
  if (gdriveCountLabel) {
    gdriveCountLabel.textContent = `${checked.length} dari ${checks.length} berkas dipilih`;
  }
  if (btnStartDownload) {
    if (checked.length === 0) {
      btnStartDownload.textContent = "Pilih Minimal 1 Berkas";
      btnStartDownload.disabled = true;
    } else if (checked.length === checks.length) {
      btnStartDownload.textContent = `Unduh Semua (${checked.length} Berkas)`;
      btnStartDownload.disabled = false;
    } else {
      btnStartDownload.textContent = `Unduh (${checked.length} Berkas Terpilih)`;
      btnStartDownload.disabled = false;
    }
  }
}

if (gdriveFilesList) {
  gdriveFilesList.addEventListener("change", (e) => {
    if (e.target.classList.contains("gdrive-check")) {
      updateGdriveSelectionCount();
    }
  });
}

if (btnGdriveSelectAll) {
  btnGdriveSelectAll.addEventListener("click", () => {
    document.querySelectorAll(".gdrive-check").forEach(cb => cb.checked = true);
    updateGdriveSelectionCount();
  });
}

if (btnGdriveDeselectAll) {
  btnGdriveDeselectAll.addEventListener("click", () => {
    document.querySelectorAll(".gdrive-check").forEach(cb => cb.checked = false);
    updateGdriveSelectionCount();
  });
}

// Auto-detect video stream & cloud drive domains on input paste/type
inputUrl.addEventListener("paste", () => {
  setTimeout(() => {
    const val = inputUrl.value.trim().toLowerCase();
    if (val.includes("drive.google.com") || val.includes("docs.google.com")) {
      btnFetchInfo.click();
    }
  }, 100);
});

inputUrl.addEventListener("input", () => {
  const val = inputUrl.value.trim().toLowerCase();
  if (val.includes("vod3.cf.dmcdn.net") || val.includes("dmcdn.net") || val.includes("dailymotion.com")) {
    selectCategory.value = "Video";
  }
});

// Fetch Video Info from URL
btnFetchInfo.addEventListener("click", async () => {
  let url = inputUrl.value.trim();
  if (!url) {
    alert("Masukkan URL terlebih dahulu!");
    return;
  }

  // Auto-normalize embedded YouTube / nocookie URLs to official watch format
  const ytEmbed = url.match(/(?:youtube\.com\/(?:embed\/|v\/|shorts\/)|youtube-nocookie\.com\/embed\/|youtu\.be\/)([a-zA-Z0-9_-]{11})/i);
  if (ytEmbed) {
    url = `https://www.youtube.com/watch?v=${ytEmbed[1]}`;
    inputUrl.value = url;
  }

  if (url.toLowerCase().includes("vod3.cf.dmcdn.net") || url.toLowerCase().includes("dmcdn.net") || ytEmbed) {
    selectCategory.value = "Video";
  }

  btnFetchInfo.disabled = true;
  btnFetchInfo.textContent = "Menganalisis...";

  try {
    const res = await fetch("/api/info", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url })
    });
    const data = await res.json();

    if (!data.success || !data.info) {
      alert("Gagal membaca video: " + (data.error || "Format tidak didukung"));
      return;
    }

    selectedVideoInfo = data.info;
    previewTitle.textContent = selectedVideoInfo.title;
    previewDuration.textContent = selectedVideoInfo.duration_formatted ? `Durasi: ${selectedVideoInfo.duration_formatted}` : `Ukuran: ${selectedVideoInfo.filesize_formatted}`;
    if (selectedVideoInfo.thumbnail) {
      previewThumb.src = selectedVideoInfo.thumbnail;
      previewThumb.classList.remove("hidden");
    } else {
      previewThumb.classList.add("hidden");
    }
    videoPreviewCard.classList.remove("hidden");

    if (selectedVideoInfo.is_folder) {
      // Google Drive Folder mode
      if (groupFormat) groupFormat.classList.add("hidden");
      if (gdriveFolderPanel) gdriveFolderPanel.classList.remove("hidden");

      gdriveFilesList.innerHTML = "";
      const files = selectedVideoInfo.files || [];
      files.forEach((f, idx) => {
        const itemEl = document.createElement("label");
        itemEl.className = "gdrive-file-item";
        itemEl.innerHTML = `
          <input type="checkbox" class="gdrive-check" checked data-idx="${idx}" data-id="${escapeHtml(f.id)}">
          <span class="gdrive-file-name" title="${escapeHtml(f.name)}">${escapeHtml(f.name)}</span>
          <span class="gdrive-file-size">${escapeHtml(f.size_formatted || "")}</span>
        `;
        gdriveFilesList.appendChild(itemEl);
      });
      updateGdriveSelectionCount();
    } else {
      // Standard single file / stream mode
      if (gdriveFolderPanel) gdriveFolderPanel.classList.add("hidden");
      if (groupFormat) groupFormat.classList.remove("hidden");
      btnStartDownload.textContent = "Mulai Unduh";
      btnStartDownload.disabled = false;

      // Populate Formats
      selectFormat.innerHTML = "";
      const formats = selectedVideoInfo.formats || [];
      if (formats.length > 0) {
        formats.forEach((f, idx) => {
          const opt = document.createElement("option");
          opt.value = f.format_id;
          opt.textContent = `${f.quality} - ${f.filesize_formatted || "Adaptive"}`;
          selectFormat.appendChild(opt);
        });
        // Update preview to top format initially
        const topFmt = formats[0];
        const durText = selectedVideoInfo.duration_formatted ? `Durasi: ${selectedVideoInfo.duration_formatted} • ` : "";
        previewDuration.textContent = `${durText}Ukuran: ${topFmt.filesize_formatted || "Adaptive"} (${topFmt.resolution})`;
      } else {
        selectFormat.innerHTML = `<option value="best">Kualitas Terbaik (${selectedVideoInfo.filesize_formatted || "Otomatis"})</option>`;
      }
    }

    if (selectedVideoInfo.category) {
      selectCategory.value = selectedVideoInfo.category;
    }

  } catch (err) {
    alert("Koneksi gagal ke server OpenIDM: " + err.message);
  } finally {
    btnFetchInfo.disabled = false;
    btnFetchInfo.textContent = "Analisis URL";
  }
});

// Dynamic format selection preview update
selectFormat.addEventListener("change", () => {
  if (!selectedVideoInfo || !selectedVideoInfo.formats || selectedVideoInfo.formats.length === 0) return;
  const currentFormatId = selectFormat.value;
  const matched = selectedVideoInfo.formats.find(f => f.format_id === currentFormatId);
  if (matched) {
    const sizeText = matched.filesize_formatted || (matched.filesize > 0 ? formatBytes(matched.filesize) : "Adaptive");
    const durText = selectedVideoInfo.duration_formatted ? `Durasi: ${selectedVideoInfo.duration_formatted} • ` : "";
    previewDuration.textContent = `${durText}Ukuran: ${sizeText} (${matched.resolution || matched.quality})`;
  }
});

// Start Download from Modal
btnStartDownload.addEventListener("click", async () => {
  const url = inputUrl.value.trim();
  if (!url) {
    alert("Masukkan URL terlebih dahulu!");
    return;
  }

  const category = selectCategory.value;

  // Handle Google Drive folder batch download
  if (selectedVideoInfo && selectedVideoInfo.is_folder) {
    const checkedBoxes = Array.from(document.querySelectorAll(".gdrive-check:checked"));
    if (checkedBoxes.length === 0) {
      alert("Pilih minimal satu berkas untuk diunduh!");
      return;
    }

    const itemsToDownload = checkedBoxes.map(cb => {
      const idx = parseInt(cb.dataset.idx, 10);
      const f = selectedVideoInfo.files[idx];
      return {
        url: f.download_url,
        title: f.name,
        filename: f.name,
        category: f.category || category,
        filesize: f.filesize || 0,
        thumbnail: f.thumbnail || ""
      };
    });

    btnStartDownload.disabled = true;
    btnStartDownload.textContent = `Menambahkan ${itemsToDownload.length} berkas...`;

    try {
      const res = await fetch("/api/batch-download", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ items: itemsToDownload })
      });
      const data = await res.json();
      if (data.success) {
        hideModal();
        fetchDownloads();
      } else {
        alert("Gagal menambahkan berkas: " + (data.error || ""));
      }
    } catch (err) {
      alert("Terjadi kesalahan saat memulai batch download: " + err.message);
    } finally {
      btnStartDownload.disabled = false;
      btnStartDownload.textContent = "Mulai Unduh";
    }
    return;
  }

  const format_id = selectFormat.value;

  let targetFilesize = 0;
  let targetFilename = selectedVideoInfo ? selectedVideoInfo.filename : null;
  let targetResolution = "";

  if (selectedVideoInfo && selectedVideoInfo.formats && selectedVideoInfo.formats.length > 0) {
    const matched = selectedVideoInfo.formats.find(f => f.format_id === format_id);
    if (matched) {
      targetFilesize = matched.filesize || 0;
      targetResolution = matched.resolution || "";
      const ext = matched.ext || "mp4";
      if (selectedVideoInfo.title) {
        let cleanTitle = selectedVideoInfo.title.replace(/\[?\b(2160p|1440p|1080p|720p|480p|360p|240p|144p)\b\]?/gi, "").trim();
        cleanTitle = cleanTitle.replace(/\s+/g, " ").replace(/[\\/*?:"<>|]/g, "").trim();
        if (targetResolution && targetResolution !== "Default" && targetResolution !== "Original" && targetResolution !== "Audio") {
          targetFilename = `${cleanTitle} [${targetResolution}].${ext}`;
        } else {
          targetFilename = `${cleanTitle}.${ext}`;
        }
      }
    } else {
      targetFilesize = selectedVideoInfo.filesize || 0;
    }
  }

  const payload = {
    url: (selectedVideoInfo && selectedVideoInfo.url) || url,
    category,
    format_id,
    title: selectedVideoInfo ? selectedVideoInfo.title : null,
    filename: targetFilename,
    thumbnail: selectedVideoInfo ? selectedVideoInfo.thumbnail : "",
    filesize: targetFilesize
  };

  btnStartDownload.disabled = true;
  btnStartDownload.textContent = "Menambahkan...";

  try {
    const res = await fetch("/api/download", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload)
    });
    const data = await res.json();
    if (data.success) {
      hideModal();
      fetchDownloads();
    } else {
      alert("Gagal menambahkan unduhan: " + (data.error || ""));
    }
  } catch (err) {
    alert("Terjadi kesalahan saat memulai unduhan: " + err.message);
  } finally {
    btnStartDownload.disabled = false;
    btnStartDownload.textContent = "Mulai Unduh";
  }
});

// Category Filter Click
categoryFilter.addEventListener("click", (e) => {
  const item = e.target.closest(".nav-item");
  if (!item) return;

  categoryFilter.querySelectorAll(".nav-item").forEach(el => el.classList.remove("active"));
  item.classList.add("active");
  currentCategory = item.dataset.category;
  fetchDownloads();
});

// Status Filter Click
statusFilter.addEventListener("click", (e) => {
  const item = e.target.closest(".nav-item");
  if (!item) return;

  if (item.classList.contains("active")) {
    item.classList.remove("active");
    currentStatus = "all";
  } else {
    statusFilter.querySelectorAll(".nav-item").forEach(el => el.classList.remove("active"));
    item.classList.add("active");
    currentStatus = item.dataset.status;
  }
  fetchDownloads();
});

// Global open folder
btnOpenFolder.addEventListener("click", () => {
  // Open root downloads directory via first item or general
  if (downloadsData.length > 0) {
    openFolder(downloadsData[0].id);
  } else {
    alert("Folder unduhan berada di: Downloads/OpenIDM");
  }
});

btnRefresh.addEventListener("click", () => {
  fetchDownloads();
  checkServerStatus();
});
searchInput.addEventListener("input", renderDownloads);

async function checkServerStatus() {
  try {
    const res = await fetch("/api/status");
    const data = await res.json();
    const statusLabel = document.querySelector(".server-status .status-label");
    const statusDot = document.querySelector(".server-status .status-dot");
    if (statusLabel && statusDot) {
      if (data.status === "online") {
        statusDot.className = "status-dot online";
        statusLabel.textContent = data.ffmpeg_available 
          ? "Core Engine Terhubung • FFmpeg Siap" 
          : "Core Engine Terhubung • FFmpeg Tidak Ditemukan";
      }
    }
  } catch (err) {
    const statusLabel = document.querySelector(".server-status .status-label");
    const statusDot = document.querySelector(".server-status .status-dot");
    if (statusLabel && statusDot) {
      statusDot.className = "status-dot offline";
      statusLabel.textContent = "Server Terputus";
    }
  }
}

// Initial checks & Polling interval
fetchDownloads();
checkServerStatus();
setInterval(fetchDownloads, 1000);
setInterval(checkServerStatus, 5000);

// Check query param e.g. ?addUrl=...
const urlParams = new URLSearchParams(window.location.search);
const addUrlParam = urlParams.get("addUrl");
if (addUrlParam) {
  showModal();
  inputUrl.value = addUrlParam;
  setTimeout(() => btnFetchInfo.click(), 250);
}

// Tombol Shutdown / Matikan Server
const btnShutdown = document.getElementById("btn-shutdown");
if (btnShutdown) {
  btnShutdown.addEventListener("click", async () => {
    if (confirm("Apakah Anda yakin ingin mematikan aplikasi OpenIDM?")) {
      try {
        await fetch("/api/shutdown", { method: "POST" });
      } catch (e) {}
      alert("OpenIDM Core berhasil dimatikan. Tab ini dapat ditutup.");
    }
  });
}

// Tema terang/gelap (disimpan di localStorage bila tersedia)
(function initTheme() {
  let saved = null;
  try { saved = localStorage.getItem("openidm-theme"); } catch (e) {}
  if (saved === "light") { document.body.classList.remove("dark"); document.body.classList.add("light"); }
  const btn = document.getElementById("btn-theme");
  if (btn) btn.addEventListener("click", () => {
    const toLight = !document.body.classList.contains("light");
    document.body.classList.toggle("light", toLight);
    document.body.classList.toggle("dark", !toLight);
    try { localStorage.setItem("openidm-theme", toLight ? "light" : "dark"); } catch (e) {}
  });
})();

// Esc menutup modal; Enter/Space pada status error membuka detail
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    if (!addModal.classList.contains("hidden")) hideModal();
    hideErrorModal();
  }
  if ((e.key === "Enter" || e.key === " ") && e.target.classList && e.target.classList.contains("status-pill") && e.target.getAttribute("role") === "button") {
    e.preventDefault();
    e.target.click();
  }
});
