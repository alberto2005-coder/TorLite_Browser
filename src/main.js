const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Select DOM elements
const backBtn = document.getElementById("back-btn");
const forwardBtn = document.getElementById("forward-btn");
const reloadBtn = document.getElementById("reload-btn");
const homeBtn = document.getElementById("home-btn");
const urlInput = document.getElementById("url-input");
const onionBadge = document.getElementById("onion-badge");
const statusDot = document.getElementById("status-dot");
const statusText = document.getElementById("status-text");

// Helper function to check if a string is a valid URL
function isUrl(str) {
  str = str.trim();
  // Match protocol (http, https, file, about)
  if (/^(https?|file|about):\/\//i.test(str)) {
    return true;
  }
  // Match localhost or common IP addresses
  if (/^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?(\/.*)?$/i.test(str)) {
    return true;
  }
  // Match common domain names and .onion domains
  if (/^[a-z0-9-]+(\.[a-z0-9-]+)+(\/.*)?$/i.test(str)) {
    return true;
  }
  return false;
}

// Function to handle navigation
async function handleNavigate() {
  let targetUrl = urlInput.value.trim();
  if (!targetUrl) return;

  if (!isUrl(targetUrl)) {
    // Route to DuckDuckGo search over Tor
    targetUrl = `http://duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion/?q=${encodeURIComponent(targetUrl)}`;
  } else {
    // If it's a URL but doesn't have a protocol, prepend http://
    if (!/^(https?|file|about):\/\//i.test(targetUrl)) {
      targetUrl = "http://" + targetUrl;
    }
  }

  // Update input to show clean target URL
  urlInput.value = targetUrl;
  
  try {
    await invoke("navigate_to", { url: targetUrl });
  } catch (err) {
    console.error("Navigation failed:", err);
  }
}

// Update the Tor connection status indicator in the UI
function updateTorStatus(status) {
  // Clear status classes
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
    // Unknown or other status
    statusDot.classList.add("connecting");
    statusText.textContent = status;
  }
}

// Initialize the application and wire up event listeners
window.addEventListener("DOMContentLoaded", async () => {
  // 1. Fetch and show the initial Tor status
  try {
    const initialStatus = await invoke("get_tor_status");
    updateTorStatus(initialStatus);
  } catch (err) {
    console.error("Failed to get initial Tor status:", err);
    updateTorStatus("error");
  }

  // 2. Wire up command triggers for navigation controls
  backBtn.addEventListener("click", async () => {
    try {
      await invoke("go_back");
    } catch (err) {
      console.error("Failed to go back:", err);
    }
  });

  forwardBtn.addEventListener("click", async () => {
    try {
      await invoke("go_forward");
    } catch (err) {
      console.error("Failed to go forward:", err);
    }
  });

  reloadBtn.addEventListener("click", async () => {
    try {
      await invoke("reload_page");
    } catch (err) {
      console.error("Failed to reload:", err);
    }
  });

  homeBtn.addEventListener("click", async () => {
    const homepage = "http://duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion/";
    urlInput.value = homepage;
    try {
      await invoke("navigate_to", { url: homepage });
    } catch (err) {
      console.error("Failed to navigate home:", err);
    }
  });

  // 3. Address bar input events
  urlInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      urlInput.blur();
      handleNavigate();
    }
  });

  // Auto-select text on focus for easier navigation
  urlInput.addEventListener("focus", () => {
    urlInput.select();
  });

  // 4. Listen for backend events
  try {
    // Listen for URL changes inside the child WebView (visor)
    await listen("url-changed", (event) => {
      const newUrl = event.payload;
      urlInput.value = newUrl;
      
      // Toggle onion badge glow
      if (newUrl.toLowerCase().includes(".onion")) {
        onionBadge.classList.add("is-onion");
      } else {
        onionBadge.classList.remove("is-onion");
      }
    });

    // Listen for real-time Tor status updates
    await listen("tor-status", (event) => {
      updateTorStatus(event.payload);
    });
  } catch (err) {
    console.error("Failed to set up event listeners:", err);
  }
});
