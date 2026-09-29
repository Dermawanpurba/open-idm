// OpenIDM Extension - Background Service Worker (Manifest V3)

const OPENIDM_API = "http://127.0.0.1:6899/api";
const detectedMedia = {}; // Map of tabId -> [ { url, type, size, filename } ]

// MIME Types to capture
const TARGET_MIMES = [
  "video/mp4",
  "video/webm",
  "video/ogg",
  "video/quicktime",
  "video/x-flv",
  "application/vnd.apple.mpegurl",
  "application/x-mpegurl",
  "application/dash+xml",
  "audio/mpeg",
  "audio/mp4",
  "audio/ogg",
  "audio/wav"
];

// File extensions to capture
const TARGET_EXTS = [".mp4", ".mkv", ".webm", ".m3u8", ".mpd", ".flv", ".mp3", ".m4a"];

function isMediaRequest(url, contentType) {
  const cleanUrl = url.split("?")[0].toLowerCase();
  const hasExt = TARGET_EXTS.some(ext => cleanUrl.endsWith(ext));
  const hasMime = contentType && TARGET_MIMES.some(mime => contentType.toLowerCase().includes(mime));
  return hasExt || hasMime;
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

    if (isMediaRequest(details.url, contentType)) {
      if (!detectedMedia[details.tabId]) {
        detectedMedia[details.tabId] = [];
      }

      // Avoid duplicates
      const exists = detectedMedia[details.tabId].some(m => m.url === details.url);
      if (!exists) {
        detectedMedia[details.tabId].push({
          url: details.url,
          contentType: contentType,
          filesize: contentLength,
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
    fetch(`${OPENIDM_API}/download`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(request.payload)
    })
      .then(res => res.json())
      .then(data => sendResponse(data))
      .catch(err => sendResponse({ success: false, error: err.message }));
    return true;
  }

  if (request.action === "FETCH_INFO") {
    fetch(`${OPENIDM_API}/info`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url: request.url })
    })
      .then(res => res.json())
      .then(data => sendResponse(data))
      .catch(err => sendResponse({ success: false, error: err.message }));
    return true;
  }
});
