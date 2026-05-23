# TorLite Browser 🧅

An experimental, lightweight, tabbed web browser built using **Tauri v2** and **Rust**, designed to route web traffic securely through the **Tor network** with a focus on amnesic privacy and multi-profile tab isolation.

---

## Features

- **Tor Integration**: Uses the official Tor Project's **Arti** client to bootstrap and connect directly to the Tor network.
- **Local SOCKS5 Proxy**: Spawns an internal SOCKS5 proxy server to route WebView2 traffic through Tor.
- **Tabbed Interface**: Supports multiple tabs in a single window with unified control, reload, back, forward, and address bars.
- **Multi-Profile Isolation**: Every tab is assigned its own unique data directory, meaning they do not share cookies, cache, or local storage.
- **Amnesic Data Cleanup**:
  - Automatically deletes a tab's profile directory from the disk 1 second after it is closed.
  - Automatically purges all stale profile directories on application startup, leaving no persistent browsing history, cache, or cookies.
- **Native Context Interception**: Intercepts native WebView2 new window requests (e.g., right-clicking a link and choosing "Open in new window" or target `_blank` clicks) and opens them as a new tab instead of a separate OS window.
- **Onion Status Indicator**: Glowing indicator that turns purple when visiting `.onion` hidden services and displays the real-time Tor bootstrap status.

---

## Prerequisites

To build and run this project, make sure you have the following installed:

1. **Rust & Cargo**: [Install Rust](https://www.rust-lang.org/tools/install) (nightly or stable).
2. **Node.js & npm**: [Install Node.js](https://nodejs.org/).
3. **Microsoft Edge WebView2**: (Pre-installed on Windows 10/11; otherwise, download the WebView2 Runtime).
4. **C++ Build Tools**: Required by Tauri on Windows (Visual Studio Build Tools with C++ workload).

---

## Installation & Setup

1. Clone or copy the project directory.
2. In the root directory, install the dependencies:
   ```bash
   npm install
   ```
3. Run the development server (watches files and rebuilds automatically):
   ```bash
   npm run tauri dev
   ```
4. Build the production package:
   ```bash
   npm run tauri build
   ```

---

## Architectural Layout

- **`src-tauri/src/lib.rs`**: Main Rust backend managing the SOCKS5 proxy server loop, Tor bootstrapping via `arti-client`, child webview lifetime hooks, window event resizes, and native new window interception.
- **`src/bootstrap.html`**: A clean, animated splash screen that monitors the Tor connection status.
- **`src/index.html` & `src/styles.css`**: The modern glassmorphism browser layout, navigation controls, and tab bar.
- **`src/main.js`**: Frontend JS controller coordinating tab states, UI rendering, address bar inputs, and Tauri IPC commands.

---

## Credits, Licenses & Copyrights

This browser is built upon and makes use of the following open-source technologies:

### 1. Tauri Framework
- **Description**: Tauri is a framework for building tiny, blazing fast binaries for all major desktop platforms.
- **URL**: [https://tauri.app/](https://tauri.app/)
- **License**: Licensed under either the [MIT License](https://github.com/tauri-apps/tauri/blob/dev/LICENSE-MIT) or the [Apache License, Version 2.0](https://github.com/tauri-apps/tauri/blob/dev/LICENSE-APACHE).

### 2. Arti (Tor Client)
- **Description**: Arti is the Tor Project's ongoing project to write a new implementation of the Tor client in Rust.
- **URL**: [https://gitlab.torproject.org/tpo/core/arti](https://gitlab.torproject.org/tpo/core/arti)
- **License**: Dual-licensed under the [MIT License](https://gitlab.torproject.org/tpo/core/arti/-/blob/main/LICENSE-MIT) and the [Apache License, Version 2.0](https://gitlab.torproject.org/tpo/core/arti/-/blob/main/LICENSE-APACHE).
- *Disclaimer: "Tor" and the "Onion Logo" are registered trademarks of The Tor Project, Inc. This project is an independent experiment and is not affiliated with or endorsed by The Tor Project.*

### 3. Wry (Webview Library)
- **Description**: Wry is the underlying cross-platform webview rendering library utilized by Tauri.
- **URL**: [https://github.com/tauri-apps/wry](https://github.com/tauri-apps/wry)
- **License**: Licensed under either the [MIT License](https://github.com/tauri-apps/wry/blob/dev/LICENSE-MIT) or the [Apache License, Version 2.0](https://github.com/tauri-apps/wry/blob/dev/LICENSE-APACHE).

---

## License

This project is open-source and is licensed under the same dual-licensing scheme as Tauri and Arti:
- **MIT License**
- **Apache License, Version 2.0**
