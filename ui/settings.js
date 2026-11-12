// Settings dialog. Only available inside the desktop app.
(function () {
  "use strict";
  if (!window.OpenIDMApi || !OpenIDMApi.native) return;

  const $ = id => document.getElementById(id);
  const modal = $("settings-modal");
  const dir = $("set-download-dir");
  const maxConcurrent = $("set-max-concurrent");
  const confirmDialog = $("set-confirm");
  const autostart = $("set-autostart");
  const errorBox = $("settings-error");
  const btnSave = $("btn-save-settings");
  const btnPick = $("btn-pick-folder");
  const btnRegister = $("btn-register-browser");
  let current = null;

  function showError(message) {
    errorBox.textContent = message || "";
    errorBox.classList.toggle("hidden", !message);
  }

  function setBusy(button, busy, busyLabel) {
    if (busy) {
      button.dataset.label = button.textContent;
      button.textContent = busyLabel;
    } else if (button.dataset.label) {
      button.textContent = button.dataset.label;
    }
    button.disabled = busy;
    button.setAttribute("aria-busy", String(busy));
  }

  function renderIntegration(status) {
    const paint = (el, registered) => {
      const ok = Boolean(registered && status.host_exists);
      el.textContent = ok ? "Terdaftar" : "Belum terdaftar";
      el.classList.toggle("ok", ok);
      el.classList.toggle("warn", !ok);
    };
    paint($("int-chrome"), status.chrome);
    paint($("int-edge"), status.edge);
  }

  async function refreshIntegration() {
    const result = await OpenIDMApi.browserStatus();
    if (result.success) renderIntegration(result.status);
  }

  async function open() {
    showError("");
    const result = await OpenIDMApi.getSettings();
    if (!result.success) {
      alert(result.error);
      return;
    }
    current = result.settings;
    dir.value = current.download_dir;
    maxConcurrent.value = String(current.max_concurrent);
    confirmDialog.checked = current.confirm_dialog;
    autostart.checked = current.autostart;
    modal.classList.remove("hidden");
    dir.focus();
    refreshIntegration();
  }

  function close() {
    modal.classList.add("hidden");
  }

  btnPick.addEventListener("click", async () => {
    setBusy(btnPick, true, "Memilih");
    const result = await OpenIDMApi.pickFolder();
    setBusy(btnPick, false);
    if (result.success && result.path) dir.value = result.path;
    else if (!result.success) showError(result.error);
  });

  btnSave.addEventListener("click", async () => {
    showError("");
    setBusy(btnSave, true, "Menyimpan");
    const result = await OpenIDMApi.saveSettings({
      ...current,
      download_dir: dir.value.trim(),
      max_concurrent: Number(maxConcurrent.value),
      confirm_dialog: confirmDialog.checked,
      autostart: autostart.checked,
    });
    setBusy(btnSave, false);
    if (result.success) close();
    else showError(result.error);
  });

  btnRegister.addEventListener("click", async () => {
    setBusy(btnRegister, true, "Mendaftarkan");
    const result = await OpenIDMApi.registerBrowser();
    setBusy(btnRegister, false);
    if (result.success) renderIntegration(result.status);
    else showError(result.error);
  });

  $("btn-quit-app").addEventListener("click", () => {
    if (confirm("Keluar dari OpenIDM? Unduhan yang berjalan akan dijeda dan bisa dilanjutkan nanti.")) OpenIDMApi.quit();
  });
  $("btn-close-settings").addEventListener("click", close);
  $("btn-cancel-settings").addEventListener("click", close);
  modal.addEventListener("keydown", event => {
    if (event.key === "Escape") close();
  });

  window.OpenIDMSettings = { open };
})();
