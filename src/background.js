'use strict';

// Ask the page to toggle PiP. The top frame gets the first chance (it can use
// the full custom window); if it has no video, iframes fall back to native PiP.
async function trigger(tab) {
  if (!tab?.id) return;
  try {
    const res = await chrome.tabs.sendMessage(tab.id, { type: 'toggle-pip' }, { frameId: 0 });
    if (res?.handled) return;
  } catch {
    // No content script in this tab (chrome:// page, or tab opened before install).
    return;
  }
  chrome.tabs.sendMessage(tab.id, { type: 'toggle-pip-frames' }).catch(() => {});
}

chrome.action.onClicked.addListener(trigger);

chrome.commands.onCommand.addListener((command, tab) => {
  if (command === 'toggle-pip') trigger(tab);
});

// ---- companion app relay ----
// The browser starts the app on demand the first time we connect, and it
// exits when we disconnect. We keep it running between PiPs so later ones
// open instantly, and let it go after a while with nothing open.
const HOST = 'com.betterpip.host';
const IDLE_DISCONNECT_MS = 60 * 60 * 1000;

let native = null;
let hostReady = null; // Promise<boolean>
let idleTimer = 0;
let nextId = 1;
const appSessions = new Map(); // id -> content-script port

function connectHost() {
  if (hostReady) return hostReady;
  hostReady = new Promise((resolve) => {
    let settled = false;
    const settle = (ok) => {
      if (settled) return;
      settled = true;
      if (!ok) {
        try {
          native?.disconnect();
        } catch {}
        native = null;
        hostReady = null;
      }
      resolve(ok);
    };
    try {
      native = chrome.runtime.connectNative(HOST);
    } catch {
      settle(false);
      return;
    }
    const port = native;
    port.onMessage.addListener((msg) => {
      if (msg?.type === 'hello') return settle(true);
      appSessions.get(msg?.id)?.postMessage(msg);
    });
    port.onDisconnect.addListener(() => {
      const error = chrome.runtime.lastError?.message || 'The Better PiP app closed.';
      if (native === port) {
        native = null;
        hostReady = null;
      }
      settle(false);
      for (const p of appSessions.values()) p.postMessage({ type: 'app-error', error });
      appSessions.clear();
    });
    // Generous: macOS scans a newly built app on its first launch.
    setTimeout(() => settle(false), 10000);
  });
  return hostReady;
}

function scheduleIdleDisconnect() {
  clearTimeout(idleTimer);
  if (appSessions.size) return;
  idleTimer = setTimeout(() => {
    if (!appSessions.size && native) {
      native.disconnect();
      native = null;
      hostReady = null;
    }
  }, IDLE_DISCONNECT_MS);
}

chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== 'bp-app') return;
  const id = String(nextId++);
  appSessions.set(id, port);
  clearTimeout(idleTimer);
  port.onMessage.addListener(async (msg) => {
    if (!(await connectHost())) {
      port.postMessage({ type: 'app-error', error: 'The Better PiP app isn’t available.' });
      return;
    }
    native?.postMessage({ ...msg, id });
  });
  port.onDisconnect.addListener(() => {
    appSessions.delete(id);
    native?.postMessage({ type: 'close', id });
    scheduleIdleDisconnect();
  });
});

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg?.type === 'focus-tab' && sender.tab) {
    chrome.tabs.update(sender.tab.id, { active: true }).catch(() => {});
    chrome.windows.update(sender.tab.windowId, { focused: true }).catch(() => {});
  } else if (msg?.type === 'app-status') {
    connectHost().then((available) => {
      if (available) scheduleIdleDisconnect();
      sendResponse({ available });
    });
    return true;
  }
});
