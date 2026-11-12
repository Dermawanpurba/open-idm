/* Native Messaging client for the OpenIDM desktop app. The browser starts the app on demand. */
(function (root) {
  "use strict";
  const HOST = "com.openidm.host";
  const IDLE_MS = 30000;
  const pending = new Map();
  let port = null;
  let counter = 0;
  let idleTimer = null;

  function unavailable(message) {
    const error = new Error(message || "OpenIDM tidak bisa dibuka.");
    error.code = "app_unavailable";
    return error;
  }

  function finish(id) {
    const entry = pending.get(id);
    if (!entry) return null;
    pending.delete(id);
    clearTimeout(entry.timer);
    // Let the service worker sleep again once nothing is waiting on the app.
    if (!pending.size && port) {
      clearTimeout(idleTimer);
      const idle = port;
      idleTimer = setTimeout(() => {
        if (!pending.size && port === idle) { port = null; idle.disconnect(); }
      }, IDLE_MS);
    }
    return entry;
  }

  function connect() {
    if (port) return port;
    const opened = chrome.runtime.connectNative(HOST);
    opened.onMessage.addListener(message => {
      const entry = finish(message && message.id);
      if (!entry) return;
      if (message.ok) { entry.resolve(message); return; }
      if (message.error === "app_unavailable") { entry.reject(unavailable()); return; }
      const error = new Error(message.error || "OpenIDM menolak permintaan.");
      error.code = "rejected";
      entry.reject(error);
    });
    opened.onDisconnect.addListener(() => {
      void chrome.runtime.lastError; // Reading it marks the error as handled.
      if (port === opened) port = null;
      // Whatever was unanswered will never be answered: callers must hand downloads back.
      for (const id of [...pending.keys()]) finish(id).reject(unavailable());
    });
    port = opened;
    return opened;
  }

  /**
   * Sends one request and resolves with the app's reply.
   * Rejects with `code: "app_unavailable"` when the app cannot be reached and
   * `code: "rejected"` when it refused. No time limit unless `options.timeout` is given.
   */
  function request(message, options = {}) {
    return new Promise((resolve, reject) => {
      clearTimeout(idleTimer);
      let target;
      try { target = connect(); } catch { reject(unavailable()); return; }
      const id = `${Date.now()}-${++counter}`;
      const entry = { resolve, reject, timer: null };
      if (options.timeout) {
        entry.timer = setTimeout(() => { if (finish(id)) reject(unavailable("OpenIDM tidak merespons.")); }, options.timeout);
      }
      pending.set(id, entry);
      try { target.postMessage({ ...message, id }); } catch { finish(id); reject(unavailable()); }
    });
  }

  root.OpenIDMNative = { request };
})(globalThis);
