const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Redirect console messages to Rust stdout
const originalLog = console.log;
console.log = function(...args) {
  originalLog.apply(console, args);
  invoke("log_console", { message: args.map(a => typeof a === 'object' ? JSON.stringify(a) : String(a)).join(' ') }).catch(() => {});
};
const originalError = console.error;
console.error = function(...args) {
  originalError.apply(console, args);
  invoke("log_console", { message: args.map(a => typeof a === 'object' ? JSON.stringify(a) : String(a)).join(' ') }).catch(() => {});
};
const originalWarn = console.warn;
console.warn = function(...args) {
  originalWarn.apply(console, args);
  invoke("log_console", { message: args.map(a => typeof a === 'object' ? JSON.stringify(a) : String(a)).join(' ') }).catch(() => {});
};

// Select DOM elements
const backBtn = document.getElementById("back-btn");
const forwardBtn = document.getElementById("forward-btn");
const reloadBtn = document.getElementById("reload-btn");
const homeBtn = document.getElementById("home-btn");
const urlInput = document.getElementById("url-input");
const onionBadge = document.getElementById("onion-badge");
const statusDot = document.getElementById("status-dot");
const statusText = document.getElementById("status-text");
const tabsContainer = document.getElementById("tabs-container");
const newTabBtn = document.getElementById("new-tab-btn");

// Tab State Management
let tabs = []; // { id, url, title, webview_label }
let activeTabId = null;

// Helper function to check if a string is a valid URL
function isUrl(str) {
  str = str.trim();
  if (/^(https?|file|about):\/\//i.test(str)) {
    return true;
  }
  if (/^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?(\/.*)?$/i.test(str)) {
    return true;
  }
  if (/^[a-z0-9-]+(\.[a-z0-9-]+)+(\/.*)?$/i.test(str)) {
    return true;
  }
  return false;
}

// Extract hostname or friendly label from URL for tab title
function getCleanTitleFromUrl(url) {
  if (!url || url === "about:blank") return "New Tab";
  try {
    const parsed = new URL(url);
    if (parsed.hostname.includes("duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion")) {
      return "DuckDuckGo";
    }
    return parsed.hostname;
  } catch {
    return url;
  }
}

// Render the Tab Bar UI
function renderTabs() {
  tabsContainer.innerHTML = "";
  tabs.forEach(tab => {
    const tabEl = document.createElement("div");
    tabEl.className = `tab ${tab.id === activeTabId ? "active" : ""}`;
    tabEl.addEventListener("click", () => switchTab(tab.id));

    const titleEl = document.createElement("span");
    titleEl.className = "tab-title";
    titleEl.textContent = tab.title;
    tabEl.appendChild(titleEl);

    const closeEl = document.createElement("span");
    closeEl.className = "tab-close-btn";
    closeEl.textContent = "×";
    closeEl.addEventListener("click", (e) => {
      e.stopPropagation();
      closeTab(tab.id);
    });
    tabEl.appendChild(closeEl);

    tabsContainer.appendChild(tabEl);
  });
}

// Create a new tab and its child webview in Rust
async function createNewTab(url = "http://duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion/") {
  const tabId = Date.now().toString() + Math.random().toString(36).substr(2, 5);
  const newTab = {
    id: tabId,
    url: url,
    title: "Loading...",
    webview_label: null
  };
  
  console.log("createNewTab: calling create_tab command in Rust with url: " + url + " and tabId: " + tabId);
  tabs.push(newTab);
  activeTabId = tabId;
  renderTabs();
  
  // Toggle loading spinner
  reloadBtn.classList.add("is-loading");

  try {
    const webview_label = await invoke("create_tab", { url: url });
    console.log("createNewTab: create_tab command returned webview_label: " + webview_label);
    newTab.webview_label = webview_label;

    // Listen for URL changes specific to this tab
    await listen(`url-changed-${webview_label}`, (event) => {
      const newUrl = event.payload;
      console.log("url-changed event received for " + webview_label + ": " + newUrl);
      newTab.url = newUrl;
      newTab.title = getCleanTitleFromUrl(newUrl);

      // Stop loading spinner
      reloadBtn.classList.remove("is-loading");

      if (activeTabId === tabId) {
        urlInput.value = newUrl;
        updateOnionBadge(newUrl);
      }
      renderTabs();
    });

    await switchTab(tabId);
  } catch (err) {
    console.error("Failed to create webview tab in Rust:", err);
    reloadBtn.classList.remove("is-loading");
    // Clean up if it failed
    tabs = tabs.filter(t => t.id !== tabId);
    renderTabs();
  }
}

// Switch active tab and toggle webview visibility in Rust
async function switchTab(tabId) {
  const tab = tabs.find(t => t.id === tabId);
  if (!tab) return;

  console.log("switchTab: switching to tabId: " + tabId + ", webview_label: " + tab.webview_label);
  activeTabId = tabId;
  urlInput.value = tab.url;
  updateOnionBadge(tab.url);
  renderTabs();

  const activeLabel = tab.webview_label;
  const inactiveLabels = tabs
    .filter(t => t.id !== tabId && t.webview_label !== null)
    .map(t => t.webview_label);

  if (activeLabel) {
    try {
      console.log("switchTab: calling activate_tab in Rust, active: " + activeLabel + ", inactive: " + inactiveLabels);
      await invoke("activate_tab", { activeLabel, inactiveLabels });
      console.log("switchTab: activate_tab succeeded");
    } catch (err) {
      console.error("Failed to activate tab webview:", err);
    }
  }
}

// Close tab and destroy its webview in Rust
async function closeTab(tabId) {
  const tabIndex = tabs.findIndex(t => t.id === tabId);
  if (tabIndex === -1) return;

  const tab = tabs[tabIndex];
  console.log("closeTab: closing tabId: " + tabId + ", webview_label: " + tab.webview_label);
  if (tab.webview_label) {
    try {
      await invoke("close_tab", { label: tab.webview_label });
      console.log("closeTab: close_tab succeeded");
    } catch (err) {
      console.error("Failed to close tab webview:", err);
    }
  }

  tabs.splice(tabIndex, 1);

  if (activeTabId === tabId) {
    if (tabs.length > 0) {
      const nextIndex = Math.min(tabIndex, tabs.length - 1);
      await switchTab(tabs[nextIndex].id);
    } else {
      await createNewTab();
    }
  } else {
    renderTabs();
    // Update Rust visibility state for remaining tabs
    const activeTab = tabs.find(t => t.id === activeTabId);
    if (activeTab && activeTab.webview_label) {
      const inactiveLabels = tabs.filter(t => t.id !== activeTabId && t.webview_label !== null).map(t => t.webview_label);
      await invoke("activate_tab", { activeLabel: activeTab.webview_label, inactiveLabels });
    }
  }
}

// Handle navigation trigger (pressing Enter or clicking Go)
async function handleNavigate() {
  const activeTab = tabs.find(t => t.id === activeTabId);
  if (!activeTab || !activeTab.webview_label) return;

  let targetUrl = urlInput.value.trim();
  if (!targetUrl) return;

  if (!isUrl(targetUrl)) {
    targetUrl = `http://duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion/?q=${encodeURIComponent(targetUrl)}`;
  } else {
    if (!/^(https?|file|about):\/\//i.test(targetUrl)) {
      targetUrl = "http://" + targetUrl;
    }
  }

  urlInput.value = targetUrl;
  activeTab.url = targetUrl;
  
  // Turn on loading spinner
  reloadBtn.classList.add("is-loading");

  try {
    await invoke("navigate_to", { label: activeTab.webview_label, url: targetUrl });
  } catch (err) {
    console.error("Navigation failed:", err);
    reloadBtn.classList.remove("is-loading");
  }
}

// Toggle onion badge glow
function updateOnionBadge(url) {
  if (url && url.toLowerCase().includes(".onion")) {
    onionBadge.classList.add("is-onion");
  } else {
    onionBadge.classList.remove("is-onion");
  }
}

// Update the Tor connection status indicator in the UI
function updateTorStatus(status) {
  statusDot.classList.remove("connecting", "connected", "error");
  
  if (status === "connecting") {
    statusDot.classList.add("connecting");
    statusText.textContent = "Bootstrapping Tor...";
  } else if (status === "connected") {
    statusDot.classList.add("connected");
    statusText.textContent = "Tor Connected";
  } else if (status === "error") {
    statusDot.classList.add("error");
    statusText.textContent = "Tor Error";
  } else {
    statusDot.classList.add("connecting");
    statusText.textContent = status;
  }
}

// Initialize the application and wire up event listeners
window.addEventListener("DOMContentLoaded", async () => {
  // 1. Fetch initial Tor status
  try {
    const initialStatus = await invoke("get_tor_status");
    updateTorStatus(initialStatus);
  } catch (err) {
    console.error("Failed to get initial Tor status:", err);
    updateTorStatus("error");
  }

  // 2. Wire up command triggers for navigation controls
  backBtn.addEventListener("click", async () => {
    const activeTab = tabs.find(t => t.id === activeTabId);
    if (activeTab && activeTab.webview_label) {
      try {
        await invoke("go_back", { label: activeTab.webview_label });
      } catch (err) {
        console.error("Failed to go back:", err);
      }
    }
  });

  forwardBtn.addEventListener("click", async () => {
    const activeTab = tabs.find(t => t.id === activeTabId);
    if (activeTab && activeTab.webview_label) {
      try {
        await invoke("go_forward", { label: activeTab.webview_label });
      } catch (err) {
        console.error("Failed to go forward:", err);
      }
    }
  });

  reloadBtn.addEventListener("click", async () => {
    const activeTab = tabs.find(t => t.id === activeTabId);
    if (activeTab && activeTab.webview_label) {
      // Toggle loading spinner
      reloadBtn.classList.add("is-loading");
      try {
        await invoke("reload_page", { label: activeTab.webview_label });
      } catch (err) {
        console.error("Failed to reload:", err);
        reloadBtn.classList.remove("is-loading");
      }
    }
  });

  homeBtn.addEventListener("click", async () => {
    const activeTab = tabs.find(t => t.id === activeTabId);
    if (activeTab && activeTab.webview_label) {
      const homepage = "http://duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion/";
      urlInput.value = homepage;
      reloadBtn.classList.add("is-loading");
      try {
        await invoke("navigate_to", { label: activeTab.webview_label, url: homepage });
      } catch (err) {
        console.error("Failed to navigate home:", err);
        reloadBtn.classList.remove("is-loading");
      }
    }
  });

  // 3. New Tab Button listener
  newTabBtn.addEventListener("click", () => {
    createNewTab();
  });

  // 4. Address bar input events
  urlInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      urlInput.blur();
      handleNavigate();
    }
  });

  urlInput.addEventListener("focus", () => {
    urlInput.select();
  });

  // 5. Listen for global backend events
  try {
    // Listen for real-time Tor status updates
    await listen("tor-status", (event) => {
      updateTorStatus(event.payload);
    });

    // Listen for requests to open a link in a new tab (target="_blank" interceptor)
    await listen("open-new-tab", async (event) => {
      const targetUrl = event.payload;
      await createNewTab(targetUrl);
    });
  } catch (err) {
    console.error("Failed to set up event listeners:", err);
  }

  // 6. Spawn the initial default tab
  await createNewTab();
});
