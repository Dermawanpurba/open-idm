// OpenIDM Extension - Background Service Worker (Manifest V3)

const OPENIDM_API = "http://127.0.0.1:6899/api";
const detectedMedia = {}; // Map of tabId -> [ { url, type, size, filename, headers } ]
const tabHeaders = {}; // Map of tabId -> { Referer, User-Agent, Cookie, Origin }

// Capture request headers (Cookie, Referer, User-Agent, Origin) like IDM
chrome.webRequest.onBeforeSendHeaders.addListener(
  (details) => {
    if (details.tabId <= 0) return;
    if (!tabHeaders[details.tabId]) {
      tabHeaders[details.tabId] = {};
    }
    if (details.requestHeaders) {
      for (const h of details.requestHeaders) {
        const name = h.name.toLowerCase();
        if (name === "referer") {
          tabHeaders[details.tabId]["Referer"] = h.value;
        } else if (name === "user-agent") {
          tabHeaders[details.tabId]["User-Agent"] = h.value;
        } else if (name === "cookie") {
          tabHeaders[details.tabId]["Cookie"] = h.value;
        } else if (name === "origin") {
          tabHeaders[details.tabId]["Origin"] = h.value;
        }
      }
    }
  },
  { urls: ["<all_urls>"] },
  ["requestHeaders", "extraHeaders"]
);

// Domains that exclusively or primarily serve video streaming content
const VIDEO_DOMAINS = [
  "vod3.cf.dmcdn.net",
  "dmcdn.net",
  "dailymotion.com"
];

// Non-media extensions to exclude from video CDN domains
const NON_MEDIA_EXTS = [
  ".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".css", ".js", ".json", ".vtt", ".srt", ".html", ".htm", ".txt", ".woff", ".woff2"
];

// MIME Types to capture
const TARGET_MIMES = [
  "video/",
  "audio/",
  "application/vnd.apple.mpegurl",
  "application/x-mpegurl",
  "application/dash+xml"
];

// File extensions to capture
const TARGET_EXTS = [
  ".mp4", ".mkv", ".webm", ".m3u8", ".mpd", ".flv", ".mp3", ".m4a",
  ".ts", ".m4v", ".f4v", ".mov", ".avi", ".ogg", ".wav"
];

function isMediaRequest(url, contentType) {
  if (!url) return false;
  const lowerUrl = url.toLowerCase();
  const cleanUrl = lowerUrl.split("?")[0].split("#")[0];

  // 1. Explicit Video CDN Domains (e.g. vod3.cf.dmcdn.net)
  const isVideoDomain = VIDEO_DOMAINS.some(domain => lowerUrl.includes(domain));
  if (isVideoDomain) {
    if (NON_MEDIA_EXTS.some(ext => cleanUrl.endsWith(ext))) {
      return false;
    }
    return true;
  }

  // 2. Check File Extensions
  const hasExt = TARGET_EXTS.some(ext => cleanUrl.endsWith(ext));
  if (hasExt) return true;

  // 3. Check Content-Type header
  if (contentType) {
    const lowerMime = contentType.toLowerCase();
    if (TARGET_MIMES.some(mime => lowerMime.includes(mime))) {
      return true;
    }
    if (lowerMime.includes("octet-stream")) {
      const mediaPatterns = ["/video", "/frag", "/segment", "/stream", "/media", "/hls", ".ts", ".m4s"];
      if (mediaPatterns.some(p => lowerUrl.includes(p))) {
        return true;
      }
    }
  }

  return false;
}

// Sniff responses
chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId <= 0) return;

    let contentType = "";
    let contentLength = 0;

    if (details.responseHeaders) {
      for (const h of details.responseHeaders) {
        const name = h.name.toLowerCase();
        if (name === "content-type") {
          contentType = h.value;
        } else if (name === "content-length") {
          contentLength = parseInt(h.value, 10) || 0;
        }
      }
    }

    // 1. Sniff embedded Dailymotion video IDs (e.g. from geo.dailymotion.com iframe or metadata API)
    const dmMatch = details.url.match(/(?:dailymotion\.com\/(?:embed\/)?video\/|geo\.dailymotion\.com\/player\/[^\s\"\'<>]+\?video=|dailymotion\.com\/player\/metadata\/video\/)([a-zA-Z0-9]+)/i);
    if (dmMatch) {
      const canonicalDmUrl = `https://www.dailymotion.com/video/${dmMatch[1]}`;
      if (!detectedMedia[details.tabId]) {
        detectedMedia[details.tabId] = [];
      }
      const exists = detectedMedia[details.tabId].some(m => m.url === canonicalDmUrl);
      if (!exists) {
        detectedMedia[details.tabId].unshift({
          url: canonicalDmUrl,
          contentType: "video/mp4",
          filesize: 0,
          mediaType: "video",
          title: `Dailymotion Video (${dmMatch[1]})`,
          headers: tabHeaders[details.tabId] ? { ...tabHeaders[details.tabId] } : {},
          timestamp: Date.now()
        });
        const count = detectedMedia[details.tabId].length;
        chrome.action.setBadgeText({ tabId: details.tabId, text: count > 0 ? count.toString() : "" });
        chrome.action.setBadgeBackgroundColor({ tabId: details.tabId, color: "#2563EB" });
      }
    }

    // 2. Direct media sniff
    if (isMediaRequest(details.url, contentType)) {
      if (!detectedMedia[details.tabId]) {
        detectedMedia[details.tabId] = [];
      }

      // Avoid duplicates
      const exists = detectedMedia[details.tabId].some(m => m.url === details.url);
      if (!exists) {
        let mediaType = "video";
        if (contentType && contentType.toLowerCase().includes("audio")) {
          mediaType = "audio";
        }
        let finalContentType = contentType;
        if (!finalContentType && (details.url.includes("vod3.cf.dmcdn.net") || details.url.includes("dmcdn.net"))) {
          finalContentType = "video/mp4";
        }

        detectedMedia[details.tabId].push({
          url: details.url,
          contentType: finalContentType || "video/mp4",
          filesize: contentLength,
          mediaType: mediaType,
          headers: tabHeaders[details.tabId] ? { ...tabHeaders[details.tabId] } : {},
          timestamp: Date.now()
        });

        // Update badge
        const count = detectedMedia[details.tabId].length;
        chrome.action.setBadgeText({
          tabId: details.tabId,
          text: count > 0 ? count.toString() : ""
        });
        chrome.action.setBadgeBackgroundColor({
          tabId: details.tabId,
          color: "#2563EB"
        });
      }
    }
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

// Clean up closed tabs
chrome.tabs.onRemoved.addListener((tabId) => {
  delete detectedMedia[tabId];
  delete tabHeaders[tabId];
});

// Reset when tab navigates to a new page
chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === "loading") {
    detectedMedia[tabId] = [];
    chrome.action.setBadgeText({ tabId, text: "" });
  }
});

// Message listener for content.js and popup.js
chrome.runtime.onMessage.addListener((request, sender, sendResponse) => {
  const tabId = sender.tab ? sender.tab.id : request.tabId;

  if (request.action === "GET_TAB_MEDIA") {
    sendResponse({ media: detectedMedia[tabId] || [] });
    return true;
  }

  if (request.action === "CHECK_SERVER") {
    fetch(`${OPENIDM_API}/status`)
      .then(res => res.json())
      .then(data => sendResponse({ online: true, data }))
      .catch(() => sendResponse({ online: false }));
    return true;
  }

  if (request.action === "DOWNLOAD_VIDEO") {
    const payload = request.payload || {};
    if (!payload.headers && tabId && tabHeaders[tabId]) {
      payload.headers = tabHeaders[tabId];
    }
    fetch(`${OPENIDM_API}/download`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload)
    })
      .then(res => res.json())
      .then(data => sendResponse(data))
      .catch(err => sendResponse({ success: false, error: err.message }));
    return true;
  }

  if (request.action === "FETCH_INFO") {
    const headers = (tabId && tabHeaders[tabId]) ? tabHeaders[tabId] : {};
    fetch(`${OPENIDM_API}/info`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url: request.url, headers })
    })
      .then(res => res.json())
      .then(data => sendResponse(data))
      .catch(err => sendResponse({ success: false, error: err.message }));
    return true;
  }
});
