// Confirmation dialog for a download taken over from the browser.
(function () {
  "use strict";
  const invoke = window.__TAURI__.core.invoke;
  const key = new URLSearchParams(location.search).get("key") || "";
  const $ = id => document.getElementById(id);
  const filename = $("confirm-filename");
  const buttons = [$("confirm-start"), $("confirm-later"), $("confirm-cancel")];
  const categories = {
    Video: "Video", Music: "Musik", Documents: "Dokumen", Programs: "Program", Compressed: "Arsip", General: "Umum",
  };

  function formatSize(bytes) {
    if (!(bytes > 0)) return "Tidak diketahui";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
    return `${(bytes / 1024 ** i).toLocaleString("id-ID", { maximumFractionDigits: i ? 1 : 0 })} ${units[i]}`;
  }

  function showError(message) {
    $("confirm-error").textContent = message;
    $("confirm-error").classList.remove("hidden");
  }

  async function decide(choice) {
    buttons.forEach(button => { button.disabled = true; });
    const chosen = $(`confirm-${choice}`);
    chosen.setAttribute("aria-busy", "true");
    try {
      // The app closes this window once it has the answer.
      await invoke("confirm_decide", { key, choice, filename: filename.value });
    } catch (error) {
      chosen.removeAttribute("aria-busy");
      buttons.forEach(button => { button.disabled = false; });
      showError(String(error));
    }
  }

  $("confirm-start").addEventListener("click", () => decide("start"));
  $("confirm-later").addEventListener("click", () => decide("later"));
  $("confirm-cancel").addEventListener("click", () => decide("cancel"));
  document.addEventListener("keydown", event => {
    if (event.key === "Escape") decide("cancel");
    else if (event.key === "Enter" && !event.target.closest("button")) decide("start");
  });

  invoke("confirm_get", { key }).then(info => {
    filename.value = info.filename;
    $("confirm-source").textContent = `Dari ${info.host}`;
    $("confirm-size").textContent = formatSize(info.filesize);
    $("confirm-category").textContent = categories[info.category] || info.category;
    $("confirm-dir").textContent = info.dir;
    $("confirm-dir").title = info.dir;
    $("confirm-start").focus();
  }).catch(error => {
    showError(String(error));
    buttons.slice(0, 2).forEach(button => { button.disabled = true; });
  });
})();
