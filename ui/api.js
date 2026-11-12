// OpenIDM UI <-> backend adapter.
// Inside the desktop app it calls Rust commands; opened from the legacy Python core it uses HTTP.
(function () {
  "use strict";

  const tauri = window.__TAURI__;
  const native = Boolean(tauri && tauri.core && tauri.core.invoke);
  const invoke = native ? tauri.core.invoke : null;

  function fail(error) {
    return { success: false, error: String((error && error.message) || error || "Terjadi kesalahan.") };
  }

  async function http(path, body) {
    const res = await fetch(path, body
      ? { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }
      : undefined);
    return res.json();
  }

  /** Runs a command and reports failure as `{ success: false, error }` instead of throwing. */
  async function call(command, args, wrap) {
    try {
      const value = await invoke(command, args);
      return wrap ? wrap(value) : { success: true, value };
    } catch (error) {
      return fail(error);
    }
  }

  const GB = 1024 ** 3;

  window.OpenIDMApi = {
    native,

    listDownloads(category, status) {
      if (native) {
        return call("list_downloads", {
          category: category === "all" ? null : category,
          status: status === "all" ? null : status,
        }, downloads => ({ success: true, downloads }));
      }
      let url = "/api/downloads?";
      if (category !== "all") url += `category=${encodeURIComponent(category)}&`;
      if (status !== "all") url += `status=${encodeURIComponent(status)}&`;
      return http(url);
    },

    async getStatus() {
      if (native) {
        const s = await invoke("get_status");
        return {
          online: true,
          title: "OpenIDM Siap",
          subtitle: s.active_downloads > 0 ? `${s.active_downloads} unduhan aktif` : "Tidak ada unduhan aktif",
          version: s.version,
          disk: s.disk && s.disk.total > 0 ? {
            percent: s.disk.percent,
            used_gb: ((s.disk.total - s.disk.free) / GB).toFixed(1),
            total_gb: (s.disk.total / GB).toFixed(1),
          } : null,
        };
      }
      const data = await http("/api/status");
      return {
        online: data.status === "online",
        title: "Core Engine Terhubung",
        subtitle: data.ffmpeg_available ? "FFmpeg Siap" : "FFmpeg Tidak Ditemukan",
        version: data.version,
        disk: data.disk || null,
      };
    },

    control(id, action, deleteFile) {
      if (native) return call("control_download", { id, action, deleteFile: Boolean(deleteFile) });
      return http("/api/control", { id, action, delete_file: Boolean(deleteFile) });
    },

    openFile(id) {
      return native ? call("open_file", { id }) : http("/api/open-file", { id });
    },

    openFolder(id) {
      return native ? call("open_folder", { id }) : http("/api/open-folder", { id });
    },

    openDownloadDir() {
      return native ? call("open_download_dir") : Promise.resolve(fail("Folder unduhan: Downloads\\OpenIDM"));
    },

    info(url) {
      if (native) {
        return call("get_info", { url }, info => ({
          success: true,
          info: { ...info, url: info.url || url },
        }));
      }
      return http("/api/info", { url });
    },

    download(payload) {
      if (native) {
        return call("add_download", {
          url: payload.url,
          filename: payload.filename || null,
          category: payload.category || null,
          formatId: payload.format_id || null,
        }, task => ({ success: true, task }));
      }
      return http("/api/download", { ...payload, category: payload.category || "General" });
    },

    quit() {
      return native ? call("quit_app") : http("/api/shutdown", {});
    },

    window: {
      minimize: () => (native ? call("win_minimize") : Promise.resolve(window.blur())),
      toggleMaximize: () => {
        if (native) return call("win_toggle_maximize");
        if (!document.fullscreenElement) return document.documentElement.requestFullscreen().catch(() => {});
        return document.exitFullscreen().catch(() => {});
      },
      hide: () => call("win_hide"),
    },

    getSettings: () => call("get_settings", undefined, settings => ({ success: true, settings })),
    saveSettings: settings => call("save_settings", { settings }, saved => ({ success: true, settings: saved })),
    pickFolder: () => call("pick_folder", undefined, path => ({ success: true, path })),
    browserStatus: () => call("browser_status", undefined, status => ({ success: true, status })),
    registerBrowser: () => call("register_browser", undefined, status => ({ success: true, status })),

    /** Calls `handler(url)` when the browser extension asks the app to open "Tambah URL". */
    onAddUrl(handler) {
      if (native && tauri.event) tauri.event.listen("openidm://add-url", event => handler(String(event.payload || "")));
    },
  };
})();
