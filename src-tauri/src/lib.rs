// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tauri::{AppHandle, Manager, WebviewUrl, LogicalPosition, LogicalSize, Emitter};
use tauri::webview::WebviewBuilder;
use arti_client::TorClient;
use tor_rtcompat::PreferredRuntime;

struct AppState {
    tor_status: Mutex<String>,
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

    // Once connected, navigate the child webview (visor) to the default homepage
    if let Some(webview) = app.get_webview("visor") {
        let homepage = "http://duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion/";
        if let Ok(url) = homepage.parse::<tauri::Url>() {
            println!("Navigating visor webview to default homepage: {}", homepage);
            let _ = webview.navigate(url);
        }
    }

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
fn get_tor_status(state: tauri::State<'_, AppState>) -> String {
    state.tor_status.lock().unwrap().clone()
}

#[tauri::command]
fn navigate_to(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if let Some(webview) = app.get_webview("visor") {
        let parsed_url = url.parse::<tauri::Url>().map_err(|e| e.to_string())?;
        webview.navigate(parsed_url).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn go_back(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(webview) = app.get_webview("visor") {
        webview.eval("window.history.back()").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn go_forward(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(webview) = app.get_webview("visor") {
        webview.eval("window.history.forward()").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn reload_page(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(webview) = app.get_webview("visor") {
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
        })
        .invoke_handler(tauri::generate_handler![
            get_tor_status,
            navigate_to,
            go_back,
            go_forward,
            reload_page
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = start_tor_and_proxy(app_handle).await {
                    eprintln!("CRITICAL ERROR in start_tor_and_proxy: {:?}", e);
                }
            });

            // Get the main window
            let main_window = app.get_webview_window("main").ok_or("Failed to get main window")?;
            let _ = main_window.set_title("Antigravity Onion Browser");

            // Setup child Webview
            let proxy_url = tauri::Url::parse("socks5://127.0.0.1:9150").unwrap();
            let webview_builder = WebviewBuilder::new(
                "visor",
                WebviewUrl::External("about:blank".parse().unwrap())
            )
            .auto_resize()
            .proxy_url(proxy_url);

            let app_handle_clone = app.handle().clone();
            let webview_builder = webview_builder.on_navigation(move |url| {
                let _ = app_handle_clone.emit("url-changed", url.as_str());
                true
            });

            // Add the child to the main window
            main_window.as_ref().window().add_child(
                webview_builder,
                LogicalPosition::new(0.0, 70.0),
                LogicalSize::new(800.0, 530.0)
            )?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::Resized(size) = event {
                    if let Some(child_webview) = window.get_webview("visor") {
                        let scale_factor = window.scale_factor().unwrap_or(1.0);
                        let logical_size = size.to_logical::<f64>(scale_factor);
                        let _ = child_webview.set_position(tauri::LogicalPosition::new(0.0, 70.0));
                        let _ = child_webview.set_size(tauri::LogicalSize::new(logical_size.width, logical_size.height - 70.0));
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
