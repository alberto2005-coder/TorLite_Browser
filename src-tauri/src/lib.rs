// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tauri::{AppHandle, Manager, WebviewUrl, LogicalPosition, LogicalSize, Emitter};
use tauri::webview::WebviewBuilder;
use arti_client::TorClient;
use tor_rtcompat::PreferredRuntime;

struct AppState {
    tor_status: Mutex<String>,
    window_counter: std::sync::atomic::AtomicU32,
    active_visors: Mutex<HashMap<String, String>>,
}

// SOCKS5 connection handler
async fn handle_client(
    mut client: TcpStream,
    tor_client: TorClient<PreferredRuntime>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Read greeting
    let mut greeting = [0u8; 2];
    client.read_exact(&mut greeting).await?;
    if greeting[0] != 0x05 {
        return Err("Invalid SOCKS version".into());
    }
    let nmethods = greeting[1] as usize;
    let mut methods = vec![0u8; nmethods];
    client.read_exact(&mut methods).await?;

    // Check if NO AUTH (0x00) is supported
    if !methods.contains(&0x00) {
        client.write_all(&[0x05, 0xFF]).await?;
        return Err("No acceptable auth methods".into());
    }

    // Send selected method (NO AUTH)
    client.write_all(&[0x05, 0x00]).await?;

    // 2. Read request
    let mut req_header = [0u8; 4];
    client.read_exact(&mut req_header).await?;
    if req_header[0] != 0x05 {
        return Err("Invalid SOCKS version in request".into());
    }
    let cmd = req_header[1];
    let atyp = req_header[3];

    if cmd != 0x01 {
        client.write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
        return Err("Unsupported SOCKS command".into());
    }

    let target_host = match atyp {
        0x01 => {
            let mut ip = [0u8; 4];
            client.read_exact(&mut ip).await?;
            format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
        }
        0x03 => {
            let len = client.read_u8().await? as usize;
            let mut domain = vec![0u8; len];
            client.read_exact(&mut domain).await?;
            String::from_utf8(domain).map_err(|_| "Invalid UTF-8 domain")?
        }
        0x04 => {
            let mut ip = [0u8; 16];
            client.read_exact(&mut ip).await?;
            let ipv6 = std::net::Ipv6Addr::from(ip);
            format!("{}", ipv6)
        }
        _ => {
            client.write_all(&[0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
            return Err("Unsupported address type".into());
        }
    };

    let port = client.read_u16().await?;

    if target_host == "newwindow.local" {
        let _ = client.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await;
        return Ok(());
    }

    // Connect via TorClient
    println!("SOCKS5 Proxy connecting to {}:{} via Tor...", target_host, port);
    match tor_client.connect((target_host.as_str(), port)).await {
        Ok(mut tor_stream) => {
            println!("SOCKS5 Proxy successfully connected to {}:{}", target_host, port);
            client.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
            if let Err(e) = tokio::io::copy_bidirectional(&mut client, &mut tor_stream).await {
                println!("SOCKS5 bidirectional copy finished with error: {:?}", e);
            } else {
                println!("SOCKS5 connection to {}:{} closed cleanly", target_host, port);
            }
        }
        Err(e) => {
            println!("SOCKS5 Proxy failed to connect to {}:{}: {:?}", target_host, port, e);
            let _ = client.write_all(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await;
        }
    }

    Ok(())
}

fn spawn_browser_window(
    app: &AppHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Get next window ID
    let state = app.state::<AppState>();
    let id = state.window_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let window_label = format!("browser_{}", id);

    println!("Spawning browser window: {}", window_label);

    // 2. Create the window hosting the HTML UI (index.html)
    let _browser_window = tauri::webview::WebviewWindowBuilder::new(
        app,
        &window_label,
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("TorLite Browser")
    .inner_size(800.0, 600.0)
    .build()?;

    println!("Spawned browser window {} successfully!", window_label);
    Ok(())
}

async fn start_tor_and_proxy(
    app: AppHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Bind local SOCKS5 listener first to claim the port and avoid race conditions
    println!("Binding SOCKS5 proxy to 127.0.0.1:9150...");
    let listener = TcpListener::bind("127.0.0.1:9150").await?;
    println!("SOCKS5 proxy bound successfully to 127.0.0.1:9150");

    // Set status to "connecting"
    *app.state::<AppState>().tor_status.lock().unwrap() = "connecting".to_string();
    let _ = app.emit("tor-status", "connecting");

    // Set cache and state paths inside Tauri's app directories
    let cache_dir = app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir());
    let state_dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir());

    std::fs::create_dir_all(&cache_dir).ok();
    std::fs::create_dir_all(&state_dir).ok();

    let config_value = serde_json::json!({
        "storage": {
            "cache_dir": cache_dir.to_string_lossy().to_string(),
            "state_dir": state_dir.to_string_lossy().to_string(),
            "permissions": {
                "dangerously_trust_everyone": true
            }
        }
    });

    let builder: arti_client::config::TorClientConfigBuilder = serde_json::from_value(config_value)?;
    let config = builder.build()?;

    println!("Bootstrapping Tor client (this might take a few seconds)...");
    // Create TorClient and bootstrap
    let tor_client = match TorClient::builder()
        .config(config)
        .create_bootstrapped()
        .await
    {
        Ok(client) => {
            println!("Tor client bootstrapped successfully!");
            client
        }
        Err(e) => {
            println!("Failed to bootstrap Tor: {:?}", e);
            *app.state::<AppState>().tor_status.lock().unwrap() = "error".to_string();
            let _ = app.emit("tor-status", "error");
            return Err(e.into());
        }
    };

    *app.state::<AppState>().tor_status.lock().unwrap() = "connected".to_string();
    let _ = app.emit("tor-status", "connected");

    // Once connected, close the splash window and open the first browser window!
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = spawn_browser_window(&app_clone) {
            eprintln!("Error spawning first browser window: {:?}", e);
        } else {
            // Close the bootstrap/splash window (labeled "main") only after the new window is successfully open
            if let Some(main_window) = app_clone.get_webview_window("main") {
                let _ = main_window.close();
            }
        }
    });

    println!("Starting SOCKS5 proxy event loop...");
    loop {
        let (socket, peer_addr) = match listener.accept().await {
            Ok(val) => val,
            Err(e) => {
                println!("SOCKS5 accept error: {:?}", e);
                continue;
            }
        };

        println!("SOCKS5 accepted connection from {}", peer_addr);
        let tor_client_clone = tor_client.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(socket, tor_client_clone).await {
                println!("Error handling SOCKS5 client {}: {:?}", peer_addr, e);
            }
        });
    }
}

// Commands
#[tauri::command]
fn log_console(message: String) {
    println!("JS Console: {}", message);
}

#[tauri::command]
fn get_tor_status(state: tauri::State<'_, AppState>) -> String {
    state.tor_status.lock().unwrap().clone()
}

#[tauri::command]
async fn create_tab(
    window: tauri::Window,
    url: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    let id = state.window_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let visor_label = format!("visor_{}", id);

    println!("create_tab: creating visor webview: {} with initial url: {}", visor_label, url);

    let proxy_url = tauri::Url::parse("socks5://127.0.0.1:9150").unwrap();

    let init_script = r#"
        document.addEventListener('click', function(e) {
            var target = e.target.closest('a');
            if (target && target.getAttribute('target') === '_blank') {
                e.preventDefault();
                var url = target.href;
                window.location.href = "http://newwindow.local/?url=" + encodeURIComponent(url);
            }
        }, true);
        window.open = function(url) {
            if (url) {
                window.location.href = "http://newwindow.local/?url=" + encodeURIComponent(url);
            }
            return window;
        };
    "#;

    let window_clone2 = window.clone();
    let mut webview_builder = WebviewBuilder::new(
        &visor_label,
        WebviewUrl::External(url.parse().map_err(|e: url::ParseError| e.to_string())?)
    )
    .auto_resize()
    .proxy_url(proxy_url)
    .initialization_script(init_script)
    .on_new_window(move |url, _features| {
        println!("Intercepted native new window request for URL: {}", url);
        let _ = window_clone2.emit("open-new-tab", url);
        tauri::webview::NewWindowResponse::Deny
    });

    #[cfg(target_os = "windows")]
    {
        let mut visor_data_dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir());
        visor_data_dir.push(format!("visor_profile_{}", id));
        println!("Setup visor data directory for tab: {:?}", visor_data_dir);
        std::fs::create_dir_all(&visor_data_dir).ok();
        webview_builder = webview_builder
            .data_directory(visor_data_dir)
            .additional_browser_args("--proxy-server=\"socks5://127.0.0.1:9150\" --host-resolver-rules=\"MAP * ~NOTFOUND , EXCLUDE 127.0.0.1\"");
    }

    let visor_label_clone = visor_label.clone();
    let window_clone = window.clone();
    let webview_builder = webview_builder.on_navigation(move |url| {
        let url_str = url.as_str();
        println!("Visor {} navigating to: {}", visor_label_clone, url_str);

        if url_str.starts_with("http://newwindow.local/") {
            if let Some(query) = url.query() {
                let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
                    .into_owned()
                    .collect();
                if let Some((_, target_url)) = params.iter().find(|(k, _)| k == "url") {
                    println!("Intercepted new tab navigation to: {}", target_url);
                    let _ = window_clone.emit("open-new-tab", target_url.clone());
                }
            }
            return false;
        }

        let event_name = format!("url-changed-{}", visor_label_clone);
        let _ = window_clone.emit(&event_name, url_str);
        true
    });

    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let size = window.inner_size().unwrap_or(tauri::PhysicalSize::new(800, 600));
    let logical_size = size.to_logical::<f64>(scale_factor);

    window.add_child(
        webview_builder,
        LogicalPosition::new(0.0, 110.0),
        LogicalSize::new(logical_size.width, logical_size.height - 110.0)
    ).map_err(|e| e.to_string())?;

    Ok(visor_label)
}

#[tauri::command]
async fn activate_tab(
    window: tauri::Window,
    active_label: String,
    inactive_labels: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let window_label = window.label().to_string();
    state.active_visors.lock().unwrap().insert(window_label, active_label.clone());

    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let size = window.inner_size().unwrap_or(tauri::PhysicalSize::new(800, 600));
    let logical_size = size.to_logical::<f64>(scale_factor);

    if let Some(active_webview) = window.get_webview(&active_label) {
        let _ = active_webview.set_position(tauri::LogicalPosition::new(0.0, 110.0));
        let _ = active_webview.set_size(tauri::LogicalSize::new(logical_size.width, logical_size.height - 110.0));
    }

    for inactive_label in inactive_labels {
        if let Some(inactive_webview) = window.get_webview(&inactive_label) {
            let _ = inactive_webview.set_position(tauri::LogicalPosition::new(0.0, 0.0));
            let _ = inactive_webview.set_size(tauri::LogicalSize::new(0.0, 0.0));
        }
    }

    Ok(())
}

#[tauri::command]
async fn close_tab(window: tauri::Window, label: String, app: tauri::AppHandle) -> Result<(), String> {
    println!("close_tab: closing visor webview: {}", label);
    if let Some(webview) = window.get_webview(&label) {
        webview.close().map_err(|e| e.to_string())?;
    }

    // Attempt to delete profile folder after 1 second delay
    let id_str = label["visor_".len()..].to_string();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if let Ok(mut visor_data_dir) = app.path().app_data_dir() {
            visor_data_dir.push(format!("visor_profile_{}", id_str));
            if visor_data_dir.exists() {
                println!("Deleting closed tab profile: {:?}", visor_data_dir);
                let _ = std::fs::remove_dir_all(&visor_data_dir);
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn navigate_to(window: tauri::Window, label: String, url: String) -> Result<(), String> {
    if let Some(webview) = window.get_webview(&label) {
        let parsed_url = url.parse::<tauri::Url>().map_err(|e| e.to_string())?;
        webview.navigate(parsed_url).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn go_back(window: tauri::Window, label: String) -> Result<(), String> {
    if let Some(webview) = window.get_webview(&label) {
        webview.eval("window.history.back()").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn go_forward(window: tauri::Window, label: String) -> Result<(), String> {
    if let Some(webview) = window.get_webview(&label) {
        webview.eval("window.history.forward()").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn reload_page(window: tauri::Window, label: String) -> Result<(), String> {
    if let Some(webview) = window.get_webview(&label) {
        webview.eval("window.location.reload()").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            tor_status: Mutex::new("connecting".to_string()),
            window_counter: std::sync::atomic::AtomicU32::new(0),
            active_visors: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            log_console,
            get_tor_status,
            create_tab,
            activate_tab,
            close_tab,
            navigate_to,
            go_back,
            go_forward,
            reload_page
        ])
        .setup(|app| {
            println!("Setup: cleaning up old profile directories...");
            if let Ok(app_data_dir) = app.path().app_data_dir() {
                if app_data_dir.exists() {
                    if let Ok(entries) = std::fs::read_dir(&app_data_dir) {
                        for entry in entries {
                            if let Ok(entry) = entry {
                                let path = entry.path();
                                if path.is_dir() {
                                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                        if name.starts_with("visor_profile_") {
                                            println!("Cleaning up old profile: {:?}", path);
                                            let _ = std::fs::remove_dir_all(&path);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            println!("Setup: starting Tor and proxy backend task...");
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = start_tor_and_proxy(app_handle).await {
                    eprintln!("CRITICAL ERROR in start_tor_and_proxy: {:?}", e);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            let label = window.label();
            if label.starts_with("browser_") {
                if let tauri::WindowEvent::Resized(size) = event {
                    let state = window.state::<AppState>();
                    let active_visors = state.active_visors.lock().unwrap();
                    if let Some(active_label) = active_visors.get(label) {
                        if let Some(child_webview) = window.get_webview(active_label) {
                            let scale_factor = window.scale_factor().unwrap_or(1.0);
                            let logical_size = size.to_logical::<f64>(scale_factor);
                            let _ = child_webview.set_position(tauri::LogicalPosition::new(0.0, 110.0));
                            let _ = child_webview.set_size(tauri::LogicalSize::new(logical_size.width, logical_size.height - 110.0));
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
