/* URL classification shared by the worker, popup and regression tests. */
(function (root) {
  "use strict";
  const defaults = { mode: "auto", saveAs: false };
  function httpUrl(value) {
    try {
      const url = new URL(String(value).trim());
      return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password ? url.href : null;
    } catch { return null; }
  }
  function corePage(value) {
    if (!httpUrl(value)) return false;
    const host = new URL(value).hostname;
    return ["youtube.com", "youtu.be", "tiktok.com", "instagram.com", "twitter.com", "x.com", "dailymotion.com", "vimeo.com", "facebook.com", "drive.google.com", "docs.google.com"].some(d => host === d || host.endsWith(`.${d}`));
  }
  function mediaKind(value, mime = "") {
    if (!httpUrl(value)) return null;
    const path = new URL(value).pathname.toLowerCase();
    mime = mime.toLowerCase().split(";")[0].trim();
    if (/\.(ts|m4s|cmfv|cmfa)$/.test(path) || /(?:^|\/)(?:segment|frag(?:ment)?)[-_\d]/.test(path)) return null;
    if (/\.(m3u8|mpd)$/.test(path) || /mpegurl|dash\+xml/.test(mime)) return "stream";
    if (mime.startsWith("video/") || /\.(mp4|webm|mkv|mov|avi|m4v|flv)$/.test(path)) return "video";
    if (mime.startsWith("audio/") || /\.(mp3|m4a|aac|flac|wav|ogg|opus)$/.test(path)) return "audio";
    return null;
  }
  function isDirect(value, mime = "", kind = "") {
    if (!httpUrl(value) || corePage(value)) return false;
    const path = new URL(value).pathname.toLowerCase();
    if (/\.(m3u8|mpd|ts|m4s|cmfv|cmfa)$/.test(path) || /mpegurl|dash\+xml/i.test(mime)) return false;
    return kind === "direct" || ["video", "audio"].includes(mediaKind(value, mime)) ||
      /\.(zip|rar|7z|tar|gz|bz2|pdf|docx?|xlsx?|pptx?|txt|csv|exe|msi|apk|dmg|iso|deb|png|jpe?g|webp)$/.test(path) ||
      /^(application\/(pdf|zip|octet-stream|x-7z-compressed)|image\/)/i.test(mime);
  }
  function safeFilename(value) {
    if (!value) return "";
    let name = String(value).split(/[\\/]/).pop().replace(/[<>:"|?*\x00-\x1f]/g, "").replace(/^\.+|[. ]+$/g, "").trim().slice(0, 180).replace(/[. ]+$/g, "");
    if (/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(name)) name = `_${name}`;
    return name;
  }
  root.OpenIDM = { defaults, httpUrl, corePage, mediaKind, isDirect, safeFilename };
})(globalThis);
