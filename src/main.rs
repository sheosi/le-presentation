use axum::{
    extract::{Form, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    response::Redirect,
    routing::{get, post},
    Router,
};
use notify::Config as NotifyConfig;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use ordermap::OrderMap;
use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;
use std::{
    collections::hash_map::DefaultHasher,
    env,
    error::Error,
    fs,
    hash::{Hash, Hasher},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::join;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tower_http::{cors::CorsLayer, services::ServeDir};

mod cmd;
mod dashboard;
mod html_generator;
use html_generator::generate_empty_folder_html;
use html_generator::generate_html;
use html_generator::generate_no_folder_html;
use html_generator::PresentationFile;

use crate::cmd::{app, open_link, try_run};

#[derive(Clone)]
struct RunningConf(Arc<Mutex<RunningConfInner>>);

struct RunningConfInner {
    presentations_dir: String,
    limiter_on: bool,
    config: Config,
    presentation_version: String,
    reload_tx: broadcast::Sender<()>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Config {}

/// Load configuration from ".env" if available
fn load_env() {
    if let Ok(env_path) = env::current_exe() {
        let env_dir = env_path.parent().unwrap_or(Path::new("."));
        if env_dir.join(".env").exists() {
            for line in fs::read_to_string(env_dir.join(".env"))
                .unwrap_or_default()
                .lines()
            {
                if let Some(eq_pos) = line.find('=') {
                    let (key, value) = line.split_at(eq_pos);
                    env::set_var(key.trim(), &value[1..].trim());
                }
            }
        }
    }
}

async fn load_easy_effects() {
    try_run(cmd::app("flatpak").map(|mut a| {
        a.args(["run", "com.github.wwmm.easyeffects"]);
        a
    }));
    try_run(cmd::app("flatpak").map(|mut a| {
        a.args(["run", "com.github.wwmm.easyeffects", "-b", "2"]);
        a
    }))
}

#[tokio::main]
async fn main() {
    // Load environment variables
    load_env();

    // Get presentations directory
    let presentations_dir = env::var("PRESENTATIONS_DIR").unwrap_or_else(|_| {
        println!("PRESENTATIONS_DIR not set, using current directory");
        std::env::current_dir()
            .expect("Failed to get current dir")
            .to_string_lossy()
            .to_string()
    });

    // Create presentations directory if it doesn't exist
    create_presentations_dir_if_needed(&presentations_dir);

    let config = Config {};

    // Create broadcast channel for reload messages
    let (reload_tx, _) = broadcast::channel(16);

    // Create shared state
    let run_conf = RunningConf(Arc::new(Mutex::new(RunningConfInner {
        presentations_dir: presentations_dir.clone(),
        limiter_on: true,
        config,
        presentation_version: String::new(),
        reload_tx: reload_tx.clone(),
    })));

    // Build our application with a route
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/presentation", get(generate_html_endpoint))
        .route("/presentation/ws", get(websocket_handler))
        .route("/settings/limiter", post(limiter_handler))
        .route("/settings/volume", post(volume_handler))
        .layer(CorsLayer::permissive())
        .with_state(run_conf.clone())
        .fallback_service(ServeDir::new(&presentations_dir));

    // Run the server
    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("Server listening on {}", addr);
    println!("Presentations directory: {}", presentations_dir);

    // Start WiFi hotspot if embedded-device feature is enabled
    #[cfg(feature = "embedded-device")]
    {
        println!("Embedded device mode: Starting WiFi hotspot...");
        tokio::spawn(start_wifi_hotspot());
    }

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    open_link(&format!("http://localhost:{}/presentation", port));

    // Spawn file watcher task
    tokio::spawn(watch_presentations_dir(run_conf.clone()));

    let (server_res, _) = join!(axum::serve(listener, app), load_easy_effects());
    server_res.unwrap();
}

/// Watch the presentations directory for changes and broadcast reloads
async fn watch_presentations_dir(run_conf: RunningConf) {
    let presentations_dir = run_conf.0.lock().expect("").presentations_dir.clone();

    // Wait for directory to exist
    let path = Path::new(&presentations_dir);
    while !path.exists() {
        sleep(Duration::from_secs(2)).await;
    }

    // Now set up the watcher
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    let tx_clone = tx.clone();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                // Only care about file creation/removal/modification
                match event.kind {
                    notify::EventKind::Create(_)
                    | notify::EventKind::Remove(_)
                    | notify::EventKind::Modify(_) => {
                        let _ = tx_clone.try_send(());
                    }
                    _ => {}
                }
            }
        },
        NotifyConfig::default().with_poll_interval(Duration::from_millis(500)),
    )
    .expect("Failed to create file watcher");

    watcher
        .watch(Path::new(&presentations_dir), RecursiveMode::NonRecursive)
        .expect("Failed to watch presentations directory");

    println!(
        "Watching presentations directory for changes: {}",
        presentations_dir
    );

    // Debounce: wait for changes to settle before reloading
    let mut pending_reload = false;
    let debounce_duration = Duration::from_millis(500);

    // Track previous state to detect folder creation/deletion
    let mut folder_existed = true;

    loop {
        tokio::select! {
            // Wait for file system events
            _ = rx.recv() => {
                pending_reload = true;
            }
            // Debounce timer
            _ = sleep(debounce_duration), if pending_reload => {
                pending_reload = false;

                let path = Path::new(&presentations_dir);
                let exists_now = path.exists();

                // Detect folder creation
                if exists_now && !folder_existed {
                    println!("Presentations folder created, broadcasting reload...");
                    let _ = run_conf.0.lock().expect("").reload_tx.send(());
                    folder_existed = true;
                    continue;
                }

                // Detect folder deletion
                if !exists_now && folder_existed {
                    println!("Presentations folder deleted, broadcasting reload...");
                    let _ = run_conf.0.lock().expect("").reload_tx.send(());
                    let mut conf = run_conf.0.lock().expect("");
                    conf.presentation_version = String::new();
                    folder_existed = false;
                    continue;
                }

                folder_existed = exists_now;

                if !exists_now {
                    continue;
                }

                // Re-scan files and check if version changed
                match get_presentation_files(&presentations_dir) {
                    Ok(files) => {
                        let new_version = compute_version(&files);
                        let mut conf = run_conf.0.lock().expect("Failed to lock config");

                        if conf.presentation_version != new_version {
                            println!("Presentation files changed, broadcasting reload...");
                            let _ = conf.reload_tx.send(());
                        }
                        conf.presentation_version = new_version;
                    }
                    Err(e) => {
                        eprintln!("Error scanning presentations: {}", e);
                    }
                }
            }
        }
    }
}

/// Start WiFi hotspot for embedded device mode
#[cfg(feature = "embedded-device")]
async fn start_wifi_hotspot() {
    // Wait a bit for NetworkManager to be ready
    sleep(Duration::from_secs(2)).await;

    // Check if nmcli is available
    if app("nmcli").is_none() {
        eprintln!("WiFi hotspot: nmcli not found. Cannot start hotspot.");
        return;
    }

    // Get WiFi interface
    let interface = match get_wifi_interface().await {
        Some(iface) => iface,
        None => {
            eprintln!("WiFi hotspot: No WiFi interface found");
            return;
        }
    };

    println!("WiFi hotspot: Using interface {}", interface);

    // Generate a simple password based on hostname or use a default
    let ssid = env::var("HOTSPOT_SSID").unwrap_or_else(|_| "Presentation-Device".to_string());
    let password = env::var("HOTSPOT_PASSWORD").unwrap_or_else(|_| "present123".to_string());

    // Stop any existing hotspot connection
    if let Some(mut cmd) = app("nmcli") {
        cmd.args(["connection", "down", "Hotspot"]);
        let _ = cmd.status();
    }

    // Delete old hotspot connection if exists
    if let Some(mut cmd) = app("nmcli") {
        cmd.args(["connection", "delete", "Hotspot"]);
        let _ = cmd.status();
    }

    // Create and start hotspot
    println!("WiFi hotspot: Creating hotspot with SSID '{}'...", ssid);

    if let Some(mut cmd) = app("nmcli") {
        cmd.args([
            "dev", "wifi", "hotspot", "ifname", &interface, "ssid", &ssid, "password", &password,
        ]);

        match cmd.status() {
            Ok(status) if status.success() => {
                println!("WiFi hotspot: Successfully started!");
                println!("WiFi hotspot: SSID: {}", ssid);
                println!("WiFi hotspot: Password: {}", password);
            }
            Ok(_) => {
                eprintln!("WiFi hotspot: Failed to start hotspot");
            }
            Err(e) => {
                eprintln!("WiFi hotspot: Error running nmcli: {}", e);
            }
        }
    }
}

/// Get the first available WiFi interface
#[cfg(feature = "embedded-device")]
async fn get_wifi_interface() -> Option<String> {
    if let Some(mut cmd) = app("nmcli") {
        cmd.args(["-t", "-f", "DEVICE,TYPE", "dev", "show"]);
        if let Ok(output) = cmd.output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                // Format: DEVICE:TYPE
                if line.contains(":wifi") || line.contains(":wireless") {
                    if let Some(device) = line.split(':').next() {
                        return Some(device.to_string());
                    }
                }
            }
        }
    }

    // Fallback: try common interface names
    for iface in &["wlan0", "wlp2s0", "wlp3s0", "wlp1s0", "wifi0"] {
        let path = Path::new("/sys/class/net").join(iface);
        if path.exists() {
            return Some(iface.to_string());
        }
    }

    None
}

// Handler for dashboard
async fn dashboard_handler(State(config): State<RunningConf>) -> impl IntoResponse {
    let limiter_on = config.0.lock().expect("").limiter_on;
    let volume = get_current_volume();
    let html = dashboard::main_dashboard(limiter_on, volume);
    ([("Content-Type", "text/html; charset=utf-8")], html).into_response()
}

/// Check if volume control is available
fn volume_control_available() -> bool {
    app("wpctl").is_some() || app("pactl").is_some()
}

fn compute_version(files: &OrderMap<String, PresentationFile>) -> String {
    let mut hasher = DefaultHasher::new();
    for (key, file) in files.iter() {
        key.hash(&mut hasher);
        file.name.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

// Handler to generate presentation HTML
async fn generate_html_endpoint(State(run_conf): State<RunningConf>) -> impl IntoResponse {
    let presentations_dir = run_conf.0.lock().expect("").presentations_dir.clone();
    let path = Path::new(&presentations_dir);

    // Check if directory exists
    if !path.exists() {
        return (
            [("Content-Type", "text/html; charset=utf-8")],
            generate_no_folder_html(),
        )
            .into_response();
    }

    // Get presentation files from directory
    match get_presentation_files(&presentations_dir) {
        Ok(files) => {
            // Check if folder is empty (no valid media files)
            let has_valid_files = files.values().any(|f| f.is_image() || f.is_video());

            if !has_valid_files {
                // Update version even for empty folder
                let mut conf = run_conf.0.lock().expect("Failed to lock config");
                conf.presentation_version = String::new();
                drop(conf);

                return (
                    [("Content-Type", "text/html; charset=utf-8")],
                    generate_empty_folder_html(),
                )
                    .into_response();
            }

            // Compute version and check if changed
            let version = compute_version(&files);
            let mut conf = run_conf.0.lock().expect("Failed to lock config");

            if !conf.presentation_version.is_empty() && conf.presentation_version != version {
                // Files changed - notify all connected clients
                let _ = conf.reload_tx.send(());
            }
            conf.presentation_version = version;
            drop(conf);

            let html_content = generate_html(files);
            ([("Content-Type", "text/html; charset=utf-8")], html_content).into_response()
        }
        Err(e) => {
            eprintln!("Error generating presentation HTML: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)).into_response()
        }
    }
}

// WebSocket handler for live reload
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(run_conf): State<RunningConf>,
) -> impl IntoResponse {
    let reload_rx = run_conf.0.lock().expect("").reload_tx.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, reload_rx))
}

async fn handle_socket(
    mut socket: axum::extract::ws::WebSocket,
    mut reload_rx: broadcast::Receiver<()>,
) {
    use axum::extract::ws::Message;

    loop {
        tokio::select! {
            // Wait for reload signal
            result = reload_rx.recv() => {
                match result {
                    Ok(()) => {
                        // Send reload message to client
                        if socket.send(Message::Text("reload".into())).await.is_err() {
                            break; // Client disconnected
                        }
                    }
                    Err(_) => break, // Channel closed
                }
            }
            // Check if client disconnected
            msg = socket.recv() => {
                if msg.is_none() {
                    break; // Client disconnected
                }
                // Ignore incoming messages (client doesn't send any)
            }
        }
    }
}

// Helper function to get presentation files for HTML generation
fn get_presentation_files(
    dir: &str,
) -> Result<OrderMap<String, PresentationFile>, Box<dyn std::error::Error>> {
    let mut files = OrderMap::new();
    let path = Path::new(dir);

    if !path.exists() {
        return Err(format!("Directory '{}' not found", dir).into());
    }

    // Get entries and sort by filename
    let mut entries: Vec<_> = fs::read_dir(path)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_string());

    // Filter and process files
    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files and non-media files
        if file_name.starts_with('.') || !is_media_file(&file_name) {
            continue;
        }

        // Skip directories
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            continue;
        }

        let f_lower = file_name.to_lowercase();

        // Handle PDF conversion
        if f_lower.ends_with(".pdf") {
            let pdf_path = entry.path();
            let converted_files = convert_pdf_to_images(&pdf_path)?;
            files.extend(converted_files);
        } else if f_lower.ends_with(".pptx") | f_lower.ends_with(".ppt") {
            let pptx_path = entry.path();
            let converted_files = convert_pptx_to_svgs(&pptx_path)?;
            files.extend(converted_files);
        } else {
            // Add valid media file
            files.insert(file_name.clone(), PresentationFile { name: file_name });
        }
    }

    Ok(files)
}

fn get_pdf_pages(pdf_path: &Path) -> Result<u32, Box<dyn std::error::Error>> {
    let output = app("pdfinfo").unwrap().arg(pdf_path).output()?;
    let info_str = str::from_utf8(&output.stdout)?;

    let page_count = info_str
        .lines()
        .find(|line| line.starts_with("Pages:"))
        .and_then(|line| line.split_whitespace().last())
        .and_then(|count| count.parse::<u32>().ok())
        .ok_or("Could not determine page count")?;

    println!("Found {} slides. Converting to SVG...", page_count);
    Ok(page_count)
}

fn convert_pptx_to_pdf(pptx_path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let status = app("flatpak")
        .unwrap()
        .args([
            "run",
            "org.libreoffice.LibreOffice",
            "--headless",
            "--convert-to",
            "pdf",
        ])
        .arg(pptx_path)
        .status()?;

    if !status.success() {
        return Err("Failed to convert PPTX to PDF".into());
    }

    Ok(pptx_path.with_extension("pdf"))
}

fn convert_pptx_to_svgs(
    pptx_path: &Path,
) -> Result<OrderMap<String, PresentationFile>, Box<dyn std::error::Error>> {
    if !pptx_path
        .with_file_name(format!(
            "{}_1.svg",
            pptx_path.file_stem().expect("").to_str().expect("")
        ))
        .exists()
    {
        let pdf_path = convert_pptx_to_pdf(pptx_path)?;
        let result = convert_pdf_to_images(&pdf_path)?;
        let _ = fs::remove_file(&pdf_path);
        Ok(result)
    } else {
        // If they already exist will be picked as part of the FS pass
        Ok(OrderMap::new())
    }
}

fn convert_pdf_to_images(
    pdf_path: &Path,
) -> Result<OrderMap<String, PresentationFile>, Box<dyn std::error::Error>> {
    if pdf_path
        .with_file_name(format!(
            "{}_1.svg",
            pdf_path.file_stem().expect("").to_str().expect("")
        ))
        .exists()
    {
        // They already exist, will be picked as part of the FS pass
        return Ok(OrderMap::new());
    }

    let page_count = get_pdf_pages(pdf_path)?;

    let mut result = OrderMap::new();
    for i in 1..=page_count {
        let output_svg = format!(
            "{}_{}.svg",
            pdf_path.file_stem().expect("").to_str().expect(""),
            i
        );
        let svg_status = app("pdf2svg")
            .unwrap()
            .arg(pdf_path)
            .arg(&output_svg)
            .arg(i.to_string())
            .status()?;

        if svg_status.success() {
            println!("Generated: {}", output_svg);
            result.insert(output_svg.clone(), PresentationFile { name: output_svg });
        }
    }

    Ok(result)
}

// Helper function to check if a file is a media file
fn is_media_file(filename: &str) -> bool {
    let extension = filename.split('.').last().unwrap_or("").to_lowercase();

    matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "svg"
            | "webp"
            | "mp4"
            | "mov"
            | "avi"
            | "webm"
            | "ogg"
            | "mkv"
            | "pdf"
            | "ppt"
            | "pptx"
    )
}

/// Get current volume percentage using wpctl (PipeWire/WirePlumber) or pactl (PulseAudio/PipeWire compat)
/// Returns None if no volume control tool is available
fn get_current_volume() -> Option<u8> {
    // Try wpctl first (PipeWire native)
    if app("wpctl").is_some() {
        if let Some(mut cmd) = app("wpctl") {
            cmd.args(["get-volume", "@DEFAULT_AUDIO_SINK@"]);
            if let Ok(output) = cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse "Volume: 0.75" format
                if let Some(vol) = stdout
                    .lines()
                    .find(|l| l.contains("Volume:"))
                    .and_then(|l| {
                        l.split_whitespace()
                            .last()
                            .and_then(|v| v.parse::<f32>().ok())
                    })
                    .map(|v| (v * 100.0) as u8)
                {
                    return Some(vol);
                }
            }
        }
    }

    // Fallback to pactl (PulseAudio/PipeWire compatibility)
    if app("pactl").is_some() {
        if let Some(mut cmd) = app("pactl") {
            cmd.args(["list", "sinks"]);
            if let Ok(output) = cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Find the default sink and extract volume percentage
                for line in stdout.lines() {
                    if line.contains("Volume:") && line.contains('%') {
                        // Parse "Volume: front-left: 65536 / 100% / 0.00 dB,..."
                        if let Some(percent_start) = line.find('/') {
                            let after_slash = &line[percent_start + 1..];
                            if let Some(percent_end) = after_slash.find('%') {
                                if let Ok(vol) = after_slash[..percent_end].trim().parse::<u8>() {
                                    return Some(vol);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None // No volume control available
}

/// Set volume up or down by 5%
fn change_volume(direction: &str) {
    let (wpctl_change, pactl_change) = match direction {
        "up" => ("5%+", "+5%"),
        "down" => ("5%-", "-5%"),
        _ => return,
    };

    // Try wpctl first (PipeWire native)
    if let Some(mut cmd) = app("wpctl") {
        cmd.args(["set-volume", "@DEFAULT_AUDIO_SINK@", wpctl_change]);
        if cmd.status().map(|s| s.success()).unwrap_or(false) {
            return;
        }
    }

    // Fallback to pactl (PulseAudio/PipeWire compatibility)
    if let Some(mut cmd) = app("pactl") {
        cmd.args(["set-sink-volume", "@DEFAULT_SINK@", pactl_change]);
        let _ = cmd.status();
    }
}

#[derive(Deserialize)]
struct LimiterQuery {
    enable: Option<String>,
}

async fn limiter_handler(
    State(config): State<RunningConf>,
    Form(query): Form<LimiterQuery>,
) -> impl IntoResponse {
    if query.enable.as_deref() == Some("on") {
        config.0.lock().expect("").limiter_on = true;
        try_run(cmd::app("flatpak").map(|mut a| {
            a.args(["run", "com.github.wwmm.easyeffects", "-b", "2"]);
            a
        }));
    } else {
        config.0.lock().expect("").limiter_on = false;
        try_run(cmd::app("flatpak").map(|mut a| {
            a.args(["run", "com.github.wwmm.easyeffects", "-b", "1"]);
            a
        }));
    }
    Redirect::to("/")
}

#[derive(Deserialize)]
struct VolumeQuery {
    direction: String,
}

async fn volume_handler(Form(query): Form<VolumeQuery>) -> impl IntoResponse {
    // Only change volume if control is available
    if volume_control_available() {
        match query.direction.as_str() {
            "up" => change_volume("up"),
            "down" => change_volume("down"),
            _ => {}
        }
    }
    Redirect::to("/")
}

// Helper function to create presentations directory
fn create_presentations_dir_if_needed(dir: &str) {
    let path = Path::new(dir);
    if !path.exists() {
        match fs::create_dir_all(path) {
            Ok(_) => println!("Created presentations directory: {}", dir),
            Err(e) => eprintln!("Failed to create presentations directory: {}", e),
        }
    }
}
