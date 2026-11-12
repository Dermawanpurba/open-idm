// OpenIDM Desktop Application — Frontend Controller

(function () {
  "use strict";

  // Application State
  let currentCategory = "all";
  let currentStatus = "all";
  let downloadsData = [];
  let selectedVideoInfo = null;
  const selectedTaskIds = new Set();
  const gdriveHistory = [];

  // DOM Elements - Navigation & Table
  const downloadsList = document.getElementById("downloads-list");
  const emptyState = document.getElementById("empty-state");
  const searchInput = document.getElementById("search-input");
  const checkAll = document.getElementById("check-all");
  const categoryFilter = document.getElementById("category-filter");
  const statusFilter = document.getElementById("status-filter");

  // DOM Elements - Action Toolbar
  const btnOpenAddModal = document.getElementById("btn-open-add-modal");
  const btnEmptyAdd = document.getElementById("btn-empty-add");
  const btnOpenFolder = document.getElementById("btn-open-folder");
  const btnResumeSelected = document.getElementById("btn-resume-selected");
  const btnPauseSelected = document.getElementById("btn-pause-selected");
  const btnDeleteSelected = document.getElementById("btn-delete-selected");
  const btnSettings = document.getElementById("btn-settings");
  const navBtnSettings = document.getElementById("nav-btn-settings");
  const navBtnAbout = document.getElementById("nav-btn-about");

  // DOM Elements - Statusbar & Storage
  const statActive = document.getElementById("stat-active");
  const statDone = document.getElementById("stat-done");
  const statTotal = document.getElementById("stat-total");
  const statSpeedDown = document.getElementById("stat-speed-down");
  const statSpeedUp = document.getElementById("stat-speed-up");
  const engineTitle = document.getElementById("engine-title");
  const engineSubtitle = document.getElementById("engine-subtitle");
  const engineDot = document.getElementById("engine-dot");
  const storagePercent = document.getElementById("storage-percent");
  const storageMeta = document.getElementById("storage-meta");
  const storageBarFill = document.getElementById("storage-bar-fill");

  // DOM Elements - Category Badges
  const badgeAll = document.getElementById("badge-all");
  const badgeVideo = document.getElementById("badge-video");
  const badgeMusic = document.getElementById("badge-music");
  const badgeCompressed = document.getElementById("badge-compressed");
  const badgeDocs = document.getElementById("badge-docs");
  const badgePrograms = document.getElementById("badge-programs");

  // DOM Elements - Add Modal
  const addModal = document.getElementById("add-modal");
  const btnCloseModal = document.getElementById("btn-close-modal");
  const btnCancelModal = document.getElementById("btn-cancel-modal");
  const btnFetchInfo = document.getElementById("btn-fetch-info");
  const btnStartDownload = document.getElementById("btn-start-download");
  const inputUrl = document.getElementById("input-url");
  const selectFormat = document.getElementById("select-format");
  const selectCategory = document.getElementById("select-category");
  const videoPreviewCard = document.getElementById("video-preview-card");
  const previewThumb = document.getElementById("preview-thumb");
  const previewTitle = document.getElementById("preview-title");
  const previewDuration = document.getElementById("preview-duration");
  const groupFormat = document.getElementById("group-format");

  // DOM Elements - Google Drive Panel
  const gdriveFolderPanel = document.getElementById("gdrive-folder-panel");
  const gdriveFilesList = document.getElementById("gdrive-files-list");
  const gdriveCountLabel = document.getElementById("gdrive-count-label");
  const btnGdriveSelectAll = document.getElementById("btn-gdrive-select-all");
  const btnGdriveDeselectAll = document.getElementById("btn-gdrive-deselect-all");

  // DOM Elements - Error & About Modals
  const errorModal = document.getElementById("error-modal");
  const errorModalTitle = document.getElementById("error-modal-title");
  const errorModalMsg = document.getElementById("error-modal-msg");
  const errorModalRetry = document.getElementById("error-modal-retry");
  const aboutModal = document.getElementById("about-modal");
  const btnCloseAbout = document.getElementById("btn-close-about");
  const btnOkAbout = document.getElementById("btn-ok-about");

  // Window Controls
  const btnWinMin = document.getElementById("btn-win-min");
  const btnWinMax = document.getElementById("btn-win-max");
  const btnWinClose = document.getElementById("btn-win-close");

  // View Switcher
  const btnViewList = document.getElementById("btn-view-list");
  const btnViewGrid = document.getElementById("btn-view-grid");

  // Utilities
  function escapeHtml(str) {
    if (!str) return "";
    return String(str)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#039;");
  }

  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "0 B";
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`;
  }

  function formatSpeed(bytesPerSec) {
    if (!bytesPerSec || bytesPerSec <= 0) return "0 KB/s";
    return `${formatBytes(bytesPerSec)}/s`;
  }

  function formatEta(seconds) {
    if (!seconds || seconds <= 0 || !isFinite(seconds)) return "-";
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    const h = Math.floor(m / 60);
    if (h > 0) return `${h}j ${m % 60}m`;
    if (m > 0) return `${m}m ${s}d`;
    return `${s}s`;
  }

  function getFileExtension(filename) {
    if (!filename) return "FILE";
    const parts = filename.split(".");
    if (parts.length > 1) {
      return parts.pop().toUpperCase();
    }
    return "FILE";
  }

  // Fetch Downloads from API
  async function fetchDownloads() {
    try {
      const data = await OpenIDMApi.listDownloads(currentCategory, currentStatus);
      if (data && data.success) {
        downloadsData = data.downloads || [];
        renderDownloads();
        renderSegmentPanel();
        updateBadges();
      }
    } catch (err) {
      console.error("Gagal mengambil data unduhan:", err);
    }
  }

  // Update Toolbar Selected Actions State
  function updateToolbarActions() {
    const count = selectedTaskIds.size;
    const hasSelected = count > 0;
    if (btnResumeSelected) btnResumeSelected.disabled = !hasSelected;
    if (btnPauseSelected) btnPauseSelected.disabled = !hasSelected;
    if (btnDeleteSelected) btnDeleteSelected.disabled = !hasSelected;
  }

  // Render Table Rows
  function renderDownloads() {
    const query = (searchInput ? searchInput.value : "").toLowerCase().trim();
    const filtered = downloadsData.filter(item => {
      if (!query) return true;
      const title = (item.title || "").toLowerCase();
      const filename = (item.filename || "").toLowerCase();
      return title.includes(query) || filename.includes(query);
    });

    // Calculate total download speed
    let totalSpeed = 0;
    let activeCount = 0;
    let doneCount = 0;

    downloadsData.forEach(d => {
      if (d.status === "downloading") {
        activeCount++;
        if (d.speed) totalSpeed += d.speed;
      } else if (d.status === "completed") {
        doneCount++;
      }
    });

    if (statActive) statActive.textContent = activeCount;
    if (statDone) statDone.textContent = doneCount;
    if (statTotal) statTotal.textContent = downloadsData.length;
    if (statSpeedDown) statSpeedDown.textContent = formatSpeed(totalSpeed);

    if (filtered.length === 0) {
      if (downloadsList) downloadsList.innerHTML = "";
      if (emptyState) emptyState.classList.remove("hidden");
      updateToolbarActions();
      return;
    }

    if (emptyState) emptyState.classList.add("hidden");

    let html = "";
    filtered.forEach(item => {
      const isSelected = selectedTaskIds.has(item.id);
      const isDownloading = item.status === "downloading";
      const isCompleted = item.status === "completed";
      const isPaused = item.status === "paused";
      const isError = item.status === "error";

      const progressPercent = Math.min(100, Math.max(0, Math.round(item.progress || 0)));
      const ext = getFileExtension(item.filename);
      const categoryName = item.category || "Video";

      // Thumbnail
      let thumbHtml = "";
      if (item.thumbnail) {
        thumbHtml = `
          <div class="file-thumb-wrap">
            <img src="${escapeHtml(item.thumbnail)}" alt="" loading="lazy">
          </div>`;
      } else {
        thumbHtml = `
          <div class="file-thumb-wrap">
            <div class="file-icon-placeholder">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polygon points="23 7 16 12 23 17 23 7"></polygon>
                <rect x="1" y="5" width="15" height="14" rx="2" ry="2"></rect>
              </svg>
            </div>
          </div>`;
      }

      // Progress bar fill class
      let progressClass = "completed";
      if (isDownloading) progressClass = "downloading";
      else if (isPaused) progressClass = "paused";
      else if (isError) progressClass = "error";

      // Status badge label
      let statusBadgeHtml = "";
      if (isCompleted) {
        statusBadgeHtml = `
          <span class="status-pill completed">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" style="width:13px;height:13px;"><polyline points="20 6 9 17 4 12"></polyline></svg>
            Selesai
          </span>`;
      } else if (isDownloading) {
        statusBadgeHtml = `
          <span class="status-pill downloading">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:13px;height:13px;"><line x1="12" y1="5" x2="12" y2="19"></line><polyline points="19 12 12 19 5 12"></polyline></svg>
            Mengunduh
          </span>`;
      } else if (isPaused) {
        statusBadgeHtml = `
          <span class="status-pill paused">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:13px;height:13px;"><rect x="6" y="4" width="4" height="16"></rect><rect x="14" y="4" width="4" height="16"></rect></svg>
            Dijeda
          </span>`;
      } else if (isError) {
        statusBadgeHtml = `
          <span class="status-pill error" data-act="error" data-id="${escapeHtml(item.id)}" title="Klik untuk lihat detail error">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:13px;height:13px;"><circle cx="12" cy="12" r="10"></circle><line x1="12" y1="8" x2="12" y2="12"></line><line x1="12" y1="16" x2="12.01" y2="16"></line></svg>
            Error
          </span>`;
      } else {
        statusBadgeHtml = `<span class="status-pill">${escapeHtml(item.status)}</span>`;
      }

      // Inline Row Actions
      let rowActionsHtml = '<div class="row-actions-group">';
      if (isCompleted) {
        rowActionsHtml += `
          <button class="row-act-btn" data-act="open-file" data-id="${escapeHtml(item.id)}" title="Buka Berkas" aria-label="Buka berkas">
            <svg viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3"></polygon></svg>
          </button>`;
      } else if (isDownloading) {
        rowActionsHtml += `
          <button class="row-act-btn" data-act="pause" data-id="${escapeHtml(item.id)}" title="Jeda" aria-label="Jeda">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="6" y="4" width="4" height="16"></rect><rect x="14" y="4" width="4" height="16"></rect></svg>
          </button>`;
      } else if (isPaused || isError) {
        rowActionsHtml += `
          <button class="row-act-btn" data-act="resume" data-id="${escapeHtml(item.id)}" title="Lanjutkan" aria-label="Lanjutkan">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="5 3 19 12 5 21 5 3"></polygon></svg>
          </button>`;
      }

      rowActionsHtml += `
        <button class="row-act-btn" data-act="open-folder" data-id="${escapeHtml(item.id)}" title="Buka Folder" aria-label="Buka folder">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path></svg>
        </button>
        <button class="row-act-btn delete-act" data-act="delete" data-id="${escapeHtml(item.id)}" title="Hapus" aria-label="Hapus">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
        </button>
      </div>`;

      html += `
        <tr data-id="${escapeHtml(item.id)}" class="${isSelected ? 'row-selected' : ''}">
          <td style="text-align: center;">
            <input type="checkbox" class="custom-checkbox task-check" data-id="${escapeHtml(item.id)}" ${isSelected ? 'checked' : ''}>
          </td>
          <td>
            <div class="file-cell">
              ${thumbHtml}
              <div class="file-meta-col">
                <span class="file-title" title="${escapeHtml(item.title || item.filename)}">${escapeHtml(item.title || item.filename)}</span>
                <span class="file-subtitle">${escapeHtml(categoryName)} • ${escapeHtml(ext)}</span>
              </div>
            </div>
          </td>
          <td data-label="Ukuran">
            <span class="cell-num">${item.filesize > 0 ? formatBytes(item.filesize) : "-"}</span>
          </td>
          <td data-label="Progress">
            <div class="progress-col">
              ${renderProgressTrack(item, progressClass, progressPercent)}
              <span class="progress-text">${progressPercent}%</span>
            </div>
          </td>
          <td data-label="Kecepatan">
            <span class="cell-num">${isDownloading ? formatSpeed(item.speed) : "-"}</span>
          </td>
          <td data-label="Sisa Waktu">
            <span class="cell-num">${isDownloading ? formatEta(item.eta) : "-"}</span>
          </td>
          <td data-label="Status">
            ${statusBadgeHtml}
          </td>
          <td class="column-actions">
            ${rowActionsHtml}
          </td>
        </tr>`;
    });

    if (downloadsList) {
      downloadsList.innerHTML = html;
    }

    // Update check-all status
    if (checkAll) {
      const visibleCheckboxes = document.querySelectorAll(".task-check");
      const checkedBoxes = document.querySelectorAll(".task-check:checked");
      checkAll.checked = visibleCheckboxes.length > 0 && checkedBoxes.length === visibleCheckboxes.length;
    }

    updateToolbarActions();
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
      const cat = d.category || "Video";
      if (counts[cat] !== undefined) {
        counts[cat]++;
      }
    });

    if (badgeAll) badgeAll.textContent = counts.all;
    if (badgeVideo) badgeVideo.textContent = counts.Video;
    if (badgeMusic) badgeMusic.textContent = counts.Music;
    if (badgeCompressed) badgeCompressed.textContent = counts.Compressed;
    if (badgeDocs) badgeDocs.textContent = counts.Documents;
    if (badgePrograms) badgePrograms.textContent = counts.Programs;
  }

  // Fetch Server Status & Disk Space
  async function checkServerStatus() {
    try {
      const data = await OpenIDMApi.getStatus();
      if (data && data.online) {
        if (engineDot) engineDot.style.background = "#34d399";
        if (engineTitle) engineTitle.textContent = data.title;
        if (engineSubtitle) engineSubtitle.textContent = data.subtitle;

        // Update Disk Usage
        if (data.disk) {
          const percent = data.disk.percent || 50;
          const usedText = data.disk.used_gb ? `${data.disk.used_gb} GB` : "512.4 GB";
          const totalText = data.disk.total_gb ? `${data.disk.total_gb} GB` : "1.0 TB";

          if (storagePercent) storagePercent.textContent = `${percent}%`;
          if (storageMeta) storageMeta.textContent = `${usedText} / ${totalText}`;
          if (storageBarFill) storageBarFill.style.width = `${percent}%`;
        }
      }
    } catch (err) {
      if (engineDot) engineDot.style.background = "#f87171";
      if (engineTitle) engineTitle.textContent = "Mesin Unduh Terputus";
      if (engineSubtitle) engineSubtitle.textContent = "Mulai ulang OpenIDM";
    }
  }

  // Global Actions
  window.controlDownload = async function (id, action) {
    try {
      await OpenIDMApi.control(id, action);
      fetchDownloads();
    } catch (err) {
      console.error(`Gagal melakukan aksi ${action}:`, err);
    }
  };

  window.deleteDownload = async function (id) {
    if (!confirm("Hapus unduhan ini dari daftar?")) return;
    try {
      await OpenIDMApi.control(id, "delete");
      selectedTaskIds.delete(id);
      fetchDownloads();
    } catch (err) {
      console.error("Gagal menghapus unduhan:", err);
    }
  };

  window.openFile = async function (id) {
    try {
      await OpenIDMApi.openFile(id);
    } catch (err) {
      console.error("Gagal membuka file:", err);
    }
  };

  window.openFolder = async function (id) {
    try {
      await OpenIDMApi.openFolder(id);
    } catch (err) {
      console.error("Gagal membuka folder:", err);
    }
  };

  window.showErrorModal = function (id) {
    const item = downloadsData.find(d => d.id === id);
    if (!item) return;

    if (errorModalTitle) errorModalTitle.textContent = item.title || "Detail Kesalahan Unduhan";
    if (errorModalMsg) errorModalMsg.textContent = item.error_message || "Koneksi stream media terputus atau gagal.";
    if (errorModalRetry) {
      errorModalRetry.onclick = () => {
        window.hideErrorModal();
        window.controlDownload(item.id, "resume");
      };
    }
    if (errorModal) errorModal.classList.remove("hidden");
  };

  window.hideErrorModal = function () {
    if (errorModal) errorModal.classList.add("hidden");
  };

  // ---- Multi-connection segments (IDM-style bar and connection panel) ----
  const segmentModal = document.getElementById("segment-modal");
  const segmentTitle = document.getElementById("segment-modal-file");
  const segmentSummary = document.getElementById("segment-summary");
  const segmentBar = document.getElementById("segment-bar");
  const segmentList = document.getElementById("segment-list");
  const segmentEmpty = document.getElementById("segment-empty");
  let segmentTaskId = null;
  const segmentStates = { active: "Mengunduh", done: "Selesai", waiting: "Menunggu" };

  function blocksHtml(item, extraClass) {
    return OpenIDMSegments.blocks(item).map(block =>
      `<span class="seg-block ${extraClass} ${block.active ? "is-active" : ""}" style="left:${block.left}%;width:${block.width}%"></span>`
    ).join("");
  }

  function renderProgressTrack(item, progressClass, progressPercent) {
    const blocks = OpenIDMSegments.blocks(item);
    if (!blocks.length || item.status === "completed") {
      return `<div class="progress-track"><div class="progress-fill ${progressClass}" style="width: ${progressPercent}%;"></div></div>`;
    }
    const info = OpenIDMSegments.summary(item);
    const label = info.active > 0 ? `${info.active} koneksi aktif, ${info.segments} segmen` : `${info.segments} segmen`;
    return `<button type="button" class="progress-track segmented" data-act="segments" data-id="${escapeHtml(item.id)}"
              title="${label}. Klik untuk detail koneksi." aria-label="${label}. Lihat detail koneksi.">
              ${blocksHtml(item, progressClass)}
            </button>`;
  }

  function renderSegmentPanel() {
    if (!segmentModal || !segmentTaskId) return;
    const item = downloadsData.find(d => d.id === segmentTaskId);
    if (!item) { window.hideSegmentModal(); return; }
    segmentTitle.textContent = item.title || item.filename || "";
    const rows = OpenIDMSegments.rows(item);
    const info = OpenIDMSegments.summary(item);
    segmentEmpty.classList.toggle("hidden", rows.length > 0);
    segmentBar.parentElement.classList.toggle("hidden", rows.length === 0);
    segmentSummary.textContent = rows.length
      ? `${info.active} koneksi aktif dari ${info.segments} segmen · ${formatSpeed(info.speed)}`
      : "Unduhan ini memakai satu koneksi.";
    segmentBar.innerHTML = blocksHtml(item, item.status === "downloading" ? "downloading" : item.status);
    segmentList.innerHTML = rows.map(row => `
      <tr class="seg-row ${row.state}">
        <td class="cell-num">${row.number}</td>
        <td class="cell-num">${formatBytes(row.start)} - ${formatBytes(row.end + 1)}</td>
        <td>
          <div class="seg-progress">
            <div class="progress-track"><div class="progress-fill ${row.state === "done" ? "completed" : "downloading"}" style="width:${row.percent}%"></div></div>
            <span class="cell-num">${formatBytes(row.done)} / ${formatBytes(row.size)}</span>
          </div>
        </td>
        <td class="cell-num">${row.state === "active" ? formatSpeed(row.speed) : "-"}</td>
        <td><span class="seg-state ${row.state}">${segmentStates[row.state]}</span></td>
      </tr>`).join("");
  }

  window.showSegmentModal = function (id) {
    segmentTaskId = id;
    renderSegmentPanel();
    if (segmentModal) segmentModal.classList.remove("hidden");
    const close = document.getElementById("btn-close-segments");
    if (close) close.focus();
  };

  window.hideSegmentModal = function () {
    segmentTaskId = null;
    if (segmentModal) segmentModal.classList.add("hidden");
  };

  document.querySelectorAll("[data-close-segments]").forEach(el => el.addEventListener("click", () => window.hideSegmentModal()));
  if (segmentModal) {
    segmentModal.addEventListener("keydown", e => { if (e.key === "Escape") window.hideSegmentModal(); });
    segmentModal.addEventListener("click", e => { if (e.target === segmentModal) window.hideSegmentModal(); });
  }

  // Row actions (delegated: the app's CSP forbids inline handlers)
  const rowActions = {
    "error": id => window.showErrorModal(id),
    "open-file": id => window.openFile(id),
    "open-folder": id => window.openFolder(id),
    "pause": id => window.controlDownload(id, "pause"),
    "resume": id => window.controlDownload(id, "resume"),
    "delete": id => window.deleteDownload(id),
    "segments": id => window.showSegmentModal(id),
  };
  if (downloadsList) {
    downloadsList.addEventListener("click", (e) => {
      const target = e.target.closest("[data-act]");
      if (target && rowActions[target.dataset.act]) rowActions[target.dataset.act](target.dataset.id);
    });
  }
  document.querySelectorAll("[data-close-error]").forEach(el => {
    el.addEventListener("click", () => window.hideErrorModal());
  });

  // Checkbox Selection Events
  if (downloadsList) {
    downloadsList.addEventListener("change", (e) => {
      if (e.target.classList.contains("task-check")) {
        const id = e.target.dataset.id;
        if (e.target.checked) {
          selectedTaskIds.add(id);
        } else {
          selectedTaskIds.delete(id);
        }
        renderDownloads();
      }
    });
  }

  if (checkAll) {
    checkAll.addEventListener("change", () => {
      const isChecked = checkAll.checked;
      document.querySelectorAll(".task-check").forEach(cb => {
        cb.checked = isChecked;
        const id = cb.dataset.id;
        if (isChecked) selectedTaskIds.add(id);
        else selectedTaskIds.delete(id);
      });
      renderDownloads();
    });
  }

  // Toolbar Multi-Selection Actions
  if (btnResumeSelected) {
    btnResumeSelected.addEventListener("click", async () => {
      for (const id of selectedTaskIds) {
        await window.controlDownload(id, "resume");
      }
      fetchDownloads();
    });
  }

  if (btnPauseSelected) {
    btnPauseSelected.addEventListener("click", async () => {
      for (const id of selectedTaskIds) {
        await window.controlDownload(id, "pause");
      }
      fetchDownloads();
    });
  }

  if (btnDeleteSelected) {
    btnDeleteSelected.addEventListener("click", async () => {
      const count = selectedTaskIds.size;
      if (count === 0) return;
      if (!confirm(`Hapus ${count} unduhan terpilih dari daftar?`)) return;
      for (const id of Array.from(selectedTaskIds)) {
        await OpenIDMApi.control(id, "delete");
        selectedTaskIds.delete(id);
      }
      fetchDownloads();
    });
  }

  // Open Folder Button
  if (btnOpenFolder) {
    btnOpenFolder.addEventListener("click", () => {
      if (downloadsData.length > 0) {
        window.openFolder(downloadsData[0].id);
      } else {
        OpenIDMApi.openDownloadDir().then(r => { if (r && r.success === false) alert(r.error); });
      }
    });
  }

  // Search Input
  if (searchInput) {
    searchInput.addEventListener("input", renderDownloads);
  }

  // Category Filter
  if (categoryFilter) {
    categoryFilter.addEventListener("click", (e) => {
      const item = e.target.closest(".nav-item");
      if (!item) return;

      categoryFilter.querySelectorAll(".nav-item").forEach(el => el.classList.remove("active"));
      item.classList.add("active");
      currentCategory = item.dataset.category || "all";
      fetchDownloads();
    });
  }

  // Status Filter
  if (statusFilter) {
    statusFilter.addEventListener("click", (e) => {
      const item = e.target.closest(".nav-item");
      if (!item) return;

      if (item.classList.contains("active")) {
        item.classList.remove("active");
        currentStatus = "all";
      } else {
        statusFilter.querySelectorAll(".nav-item").forEach(el => el.classList.remove("active"));
        item.classList.add("active");
        currentStatus = item.dataset.status || "all";
      }
      fetchDownloads();
    });
  }

  // Modal Show / Hide
  function showModal() {
    if (addModal) addModal.classList.remove("hidden");
    if (inputUrl) {
      inputUrl.value = "";
      inputUrl.focus();
    }
  }

  function hideModal() {
    gdriveHistory.length = 0;
    if (addModal) addModal.classList.add("hidden");
    if (videoPreviewCard) videoPreviewCard.classList.add("hidden");
    if (gdriveFolderPanel) gdriveFolderPanel.classList.add("hidden");
    if (groupFormat) groupFormat.classList.remove("hidden");
    if (selectFormat) selectFormat.innerHTML = `<option value="best">Kualitas Terbaik (Otomatis)</option>`;
    selectedVideoInfo = null;
    if (btnStartDownload) {
      btnStartDownload.textContent = "Mulai Unduh";
      btnStartDownload.disabled = false;
    }
  }

  if (btnOpenAddModal) btnOpenAddModal.addEventListener("click", showModal);
  if (btnEmptyAdd) btnEmptyAdd.addEventListener("click", showModal);
  if (btnCloseModal) btnCloseModal.addEventListener("click", hideModal);
  if (btnCancelModal) btnCancelModal.addEventListener("click", hideModal);

  // URL Analysis
  if (inputUrl) inputUrl.addEventListener("input", () => {
    selectedVideoInfo = null;
    if (videoPreviewCard) videoPreviewCard.classList.add("hidden");
    if (selectFormat) selectFormat.innerHTML = '<option value="best">Kualitas Terbaik (Otomatis)</option>';
  });
  if (btnFetchInfo && inputUrl) {
    btnFetchInfo.addEventListener("click", async () => {
      const url = inputUrl.value.trim();
      if (!url) {
        alert("Masukkan URL video terlebih dahulu!");
        return;
      }

      btnFetchInfo.disabled = true;
      btnFetchInfo.textContent = "Menganalisis...";
      if (btnStartDownload) btnStartDownload.disabled = true;

      try {
        const data = await OpenIDMApi.info(url);
        if (inputUrl.value.trim() !== url) return;
        if (!data.success || !data.info) {
          alert(data.error || "Tautan tidak bisa dianalisis.");
          return;
        }

        selectedVideoInfo = data.info;
        if (previewTitle) previewTitle.textContent = selectedVideoInfo.title;
        if (previewDuration) {
          previewDuration.textContent = selectedVideoInfo.duration_formatted
            ? `Durasi: ${selectedVideoInfo.duration_formatted}`
            : `Ukuran: ${selectedVideoInfo.filesize_formatted || "Adaptive"}`;
        }
        if (previewThumb) {
          if (selectedVideoInfo.thumbnail) {
            previewThumb.src = selectedVideoInfo.thumbnail;
            previewThumb.classList.remove("hidden");
          } else {
            previewThumb.classList.add("hidden");
          }
        }
        if (videoPreviewCard) videoPreviewCard.classList.remove("hidden");

        // Google Drive Folder mode
        if (selectedVideoInfo.is_folder) {
          if (groupFormat) groupFormat.classList.add("hidden");
          if (gdriveFolderPanel) gdriveFolderPanel.classList.remove("hidden");
          if (gdriveFilesList) {
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
          }
        } else {
          if (gdriveFolderPanel) gdriveFolderPanel.classList.add("hidden");
          if (groupFormat) groupFormat.classList.remove("hidden");

          if (selectFormat) {
            selectFormat.innerHTML = "";
            const formats = selectedVideoInfo.formats || [];
            if (formats.length > 0) {
              formats.forEach(f => {
                const opt = document.createElement("option");
                opt.value = f.format_id;
                opt.textContent = `${f.quality} - ${f.filesize_formatted || "Adaptive"}`;
                selectFormat.appendChild(opt);
              });
            } else {
              selectFormat.innerHTML = `<option value="best">Kualitas Terbaik (${selectedVideoInfo.filesize_formatted || "Otomatis"})</option>`;
            }
          }
        }

        if (selectCategory && selectedVideoInfo.category) {
          selectCategory.value = selectedVideoInfo.category;
        }
      } catch (err) {
        alert("Tautan tidak bisa dianalisis: " + err.message);
      } finally {
        btnFetchInfo.disabled = false;
        btnFetchInfo.textContent = "Analisis URL";
        if (btnStartDownload) btnStartDownload.disabled = false;
      }
    });
  }

  // Start Download from Modal
  if (btnStartDownload) {
    btnStartDownload.addEventListener("click", async () => {
      const url = inputUrl ? inputUrl.value.trim() : "";
      if (!url) {
        alert("Masukkan URL terlebih dahulu!");
        return;
      }

      const category = selectCategory ? selectCategory.value : "Video";
      const format_id = selectFormat ? selectFormat.value : "best";

      let targetFilesize = 0;
      let targetFilename = selectedVideoInfo ? selectedVideoInfo.filename : null;

      if (selectedVideoInfo && selectedVideoInfo.formats && selectedVideoInfo.formats.length > 0) {
        const matched = selectedVideoInfo.formats.find(f => f.format_id === format_id);
        if (matched) {
          targetFilesize = matched.filesize || 0;
          const ext = matched.ext || "mp4";
          if (selectedVideoInfo.title) {
            let cleanTitle = selectedVideoInfo.title.replace(/[\\/*?:"<>|]/g, "").trim();
            targetFilename = `${cleanTitle}.${ext}`;
          }
        }
      }

      const payload = {
        url: (selectedVideoInfo && selectedVideoInfo.url) || url,
        category,
        format_id,
        title: selectedVideoInfo ? selectedVideoInfo.title : null,
        filename: targetFilename,
        thumbnail: selectedVideoInfo ? selectedVideoInfo.thumbnail : "",
        filesize: targetFilesize,
      };

      btnStartDownload.disabled = true;
      btnStartDownload.textContent = "Menambahkan...";

      try {
        const data = await OpenIDMApi.download(payload);
        if (data.success) {
          hideModal();
          fetchDownloads();
        } else {
          alert(data.error || "Unduhan tidak bisa ditambahkan.");
        }
      } catch (err) {
        alert("Terjadi kesalahan: " + err.message);
      } finally {
        btnStartDownload.disabled = false;
        btnStartDownload.textContent = "Mulai Unduh";
      }
    });
  }

  // Window Controls
  if (btnWinClose) {
    btnWinClose.addEventListener("click", async () => {
      if (OpenIDMApi.native) {
        // The desktop app keeps running in the tray so downloads continue.
        OpenIDMApi.window.hide();
        return;
      }
      if (confirm("Tutup aplikasi OpenIDM?")) {
        try {
          await OpenIDMApi.quit();
        } catch (e) {}
        window.close();
      }
    });
  }

  if (btnWinMin) {
    btnWinMin.addEventListener("click", () => OpenIDMApi.window.minimize());
  }

  if (btnWinMax) {
    btnWinMax.addEventListener("click", () => OpenIDMApi.window.toggleMaximize());
  }

  // Modals About / Settings
  function showAbout() {
    if (aboutModal) aboutModal.classList.remove("hidden");
  }
  function hideAbout() {
    if (aboutModal) aboutModal.classList.add("hidden");
  }

  const openSettings = () => (window.OpenIDMSettings ? window.OpenIDMSettings.open() : showAbout());
  if (btnSettings) btnSettings.addEventListener("click", openSettings);
  if (navBtnSettings) navBtnSettings.addEventListener("click", openSettings);
  if (navBtnAbout) navBtnAbout.addEventListener("click", showAbout);
  if (btnCloseAbout) btnCloseAbout.addEventListener("click", hideAbout);
  if (btnOkAbout) btnOkAbout.addEventListener("click", hideAbout);

  // View Switcher Buttons
  if (btnViewList && btnViewGrid) {
    btnViewList.addEventListener("click", () => {
      btnViewList.classList.add("active");
      btnViewGrid.classList.remove("active");
    });
    btnViewGrid.addEventListener("click", () => {
      btnViewGrid.classList.add("active");
      btnViewList.classList.remove("active");
    });
  }

  // "Tambah URL" requested by the browser extension
  OpenIDMApi.onAddUrl(url => {
    showModal();
    if (inputUrl && url) inputUrl.value = url;
  });

  // Initial Execution & Polling
  fetchDownloads();
  checkServerStatus();
  setInterval(fetchDownloads, 1000);
  setInterval(checkServerStatus, 4000);
})();
