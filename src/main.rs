use axum::{
    extract::{State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use html_generator::PresentationFile;
use notify::Config as NotifyConfig;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use ordermap::OrderMap;
use serde::Deserialize;
use serde::Serialize;
use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    env,
    error::Error,
    fs,
    hash::{Hash, Hasher},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use sysinfo::Disks;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;
use tokio::time::sleep;
use tower_http::{cors::CorsLayer, services::ServeDir};

mod cmd;
mod dashboard;
mod html_generator;
mod pptx_parser;
use html_generator::generate_empty_folder_html;
use html_generator::generate_html;
use html_generator::generate_no_folder_html;
use pptx_parser::PptxParser;

#[cfg(not(feature = "embedded-device"))]
use crate::cmd::open_link;
use crate::{
    cmd::{app, try_run},
    html_generator::{RevealTransition, RevealTransitionKind, RevealTransitionSpeed},
};

#[derive(Clone)]
struct RunningConf(Arc<Mutex<RunningConfInner>>);

struct RunningConfInner {
    presentations_dir: String,
    limiter_on: bool,
    config: Config,
    presentation_version: String,
    reload_tx: broadcast::Sender<()>,
    converting: bool,
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

    load_easy_effects().await;

    // Get presentations directory
    // In embedded-device mode, default to /media/usb-kiosk for USB auto-mount
    #[cfg(feature = "embedded-device")]
    let default_dir = "/media/usb-kiosk".to_string();
    #[cfg(not(feature = "embedded-device"))]
    let default_dir = std::env::current_dir()
        .expect("Failed to get current dir")
        .to_string_lossy()
        .to_string();

    let presentations_dir = env::var("PRESENTATIONS_DIR").unwrap_or_else(|_| {
        #[cfg(feature = "embedded-device")]
        println!("PRESENTATIONS_DIR not set, using /media/usb-kiosk (embedded-device mode)");
        #[cfg(not(feature = "embedded-device"))]
        println!("PRESENTATIONS_DIR not set, using current directory");
        default_dir
    });

    // Create presentations directory if it doesn't exist
    create_presentations_dir_if_needed(&presentations_dir);

    let config = Config {};

    // Create broadcast channel for reload messages
    let (reload_tx, _) = broadcast::channel(16);

    // Create shutdown channel for graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);

    // Create shared state
    let run_conf = RunningConf(Arc::new(Mutex::new(RunningConfInner {
        presentations_dir: presentations_dir.clone(),
        limiter_on: true,
        config,
        presentation_version: String::new(),
        reload_tx: reload_tx.clone(),
        converting: false,
    })));

    // Build our application with a route
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/presentation", get(generate_html_endpoint))
        .route("/presentation/ws", get(websocket_handler))
        .route(
            "/settings/limiter",
            post(dashboard::sys_integration::limiter_handler),
        )
        .route(
            "/settings/volume",
            post(dashboard::sys_integration::volume_handler),
        )
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

    // Only open browser automatically in non-embedded mode
    // In embedded-device mode, Cage is already handling the display
    #[cfg(not(feature = "embedded-device"))]
    open_link(&format!("http://localhost:{}/presentation", port));

    // Spawn file watcher task with shutdown channel
    tokio::spawn(watch_presentations_dir(
        run_conf.clone(),
        shutdown_tx.subscribe(),
    ));

    // Set up signal handlers for graceful shutdown
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to create SIGINT handler");

    // Run server with graceful shutdown
    tokio::select! {
        // Run the server
        result = axum::serve(listener, app) => {
            if let Err(e) = result {
                eprintln!("Server error: {}", e);
            }
        }
        // Wait for shutdown signal
        _ = sigterm.recv() => {
            println!("Received SIGTERM, shutting down gracefully...");
        }
        _ = sigint.recv() => {
            println!("Received SIGINT, shutting down gracefully...");
        }
        // Wait for shutdown from file watcher or other components
        _ = shutdown_rx.recv() => {
            println!("Received shutdown signal...");
        }
    }

    // Send shutdown signal to all components
    let _ = shutdown_tx.send(());

    println!("Shutdown complete");
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
    match get_presentation_files(&run_conf) {
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

// Handler for dashboard
async fn dashboard_handler(State(config): State<RunningConf>) -> impl IntoResponse {
    let limiter_on = config.0.lock().expect("").limiter_on;
    let html = dashboard::main_dashboard(limiter_on);
    ([("Content-Type", "text/html; charset=utf-8")], html).into_response()
}

/// Watch the presentations directory for changes and broadcast reloads
async fn watch_presentations_dir(run_conf: RunningConf, mut shutdown: broadcast::Receiver<()>) {
    let presentations_dir = run_conf.0.lock().expect("").presentations_dir.clone();

    // Wait for directory to exist
    let path = Path::new(&presentations_dir);
    while !path.exists() {
        // Check for shutdown while waiting
        tokio::select! {
            _ = shutdown.recv() => {
                println!("File watcher received shutdown signal during startup");
                return;
            }
            _ = sleep(Duration::from_secs(2)) => {}
        }
    }

    // Now set up the watcher
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    let tx_clone = tx.clone();
    let run_conf_watcher = run_conf.clone();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                // Only care about file creation/removal/modification
                match event.kind {
                    notify::EventKind::Create(_)
                    | notify::EventKind::Remove(_)
                    | notify::EventKind::Modify(_) => {
                        // Skip events while converting to avoid reload loops
                        let is_converting = run_conf_watcher
                            .0
                            .lock()
                            .map(|conf| conf.converting)
                            .unwrap_or(false);
                        if !is_converting {
                            let _ = tx_clone.try_send(());
                        }
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

    // Track previous state to detect folder creation/deletion/mount/unmount
    let mut folder_accessible = true;

    // Helper to check if directory is accessible (not just exists)
    let is_accessible = |path: &Path| path.exists() && fs::read_dir(path).is_ok();

    loop {
        tokio::select! {
            // Wait for file system events
            _ = rx.recv() => {
                pending_reload = true;
            }
            // Check for shutdown signal
            _ = shutdown.recv() => {
                println!("File watcher shutting down...");
                return;
            }
            // Periodic poll every 3 seconds to detect mount/unmount using sysinfo
            _ = sleep(Duration::from_secs(3)) => {
                // Check if the mount point has a filesystem using sysinfo
                let disks = Disks::new_with_refreshed_list();
                let path_str = presentations_dir.as_str();
                let is_mounted = disks.iter().any(|disk| {
                    disk.mount_point().to_string_lossy() == path_str
                });

                if is_mounted != folder_accessible {
                    // Check if we're currently converting (avoid reload loops)
                    let is_converting = run_conf
                        .0
                        .lock()
                        .map(|conf| conf.converting)
                        .unwrap_or(false);

                    if !is_converting {
                        if is_mounted {
                            println!("Sysinfo detected mount at {}, broadcasting reload...", presentations_dir);
                        } else {
                            println!("Sysinfo detected unmount at {}, broadcasting reload...", presentations_dir);
                        }
                        let _ = run_conf.0.lock().expect("").reload_tx.send(());
                    }
                    folder_accessible = is_mounted;
                }
            }
            // Debounce timer
            _ = sleep(debounce_duration), if pending_reload => {
                pending_reload = false;

                let path = Path::new(&presentations_dir);
                let accessible_now = is_accessible(path);

                // Detect folder becoming accessible (created or mounted)
                if accessible_now && !folder_accessible {
                    // Check if we're currently converting (avoid reload loops)
                    let is_converting = run_conf
                        .0
                        .lock()
                        .map(|conf| conf.converting)
                        .unwrap_or(false);

                    if !is_converting {
                        println!("Presentations folder became accessible (mounted), broadcasting reload...");
                        let _ = run_conf.0.lock().expect("").reload_tx.send(());
                    }
                    folder_accessible = true;
                    continue;
                }

                // Detect folder becoming inaccessible (deleted or unmounted)
                if !accessible_now && folder_accessible {
                    // Check if we're currently converting (avoid reload loops)
                    let is_converting = run_conf
                        .0
                        .lock()
                        .map(|conf| conf.converting)
                        .unwrap_or(false);

                    if !is_converting {
                        println!("Presentations folder became inaccessible (unmounted), broadcasting reload...");
                        let _ = run_conf.0.lock().expect("").reload_tx.send(());
                        let mut conf = run_conf.0.lock().expect("");
                        conf.presentation_version = String::new();
                    }
                    folder_accessible = false;
                    continue;
                }

                folder_accessible = accessible_now;

                if !accessible_now {
                    continue;
                }

                // Re-scan files and check if version changed
                match get_presentation_files(&run_conf) {
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

struct GeneratorPresentationFile {
    pub file: PresentationFile,
    /// For use inside generators, keep track of the slide inside ppt, pptx and
    /// pdfs for things like merging differente kinds of media
    pub internal_slide: u32,
}

fn strip_generator_data(
    input: OrderMap<String, GeneratorPresentationFile>,
) -> OrderMap<String, PresentationFile> {
    input.into_iter().map(|(s, f)| (s, f.file)).collect()
}

// Helper function to get presentation files for HTML generation
fn get_presentation_files(
    run_conf: &RunningConf,
) -> Result<OrderMap<String, PresentationFile>, Box<dyn std::error::Error>> {
    // Get presentations_dir and set converting flag
    let presentations_dir = {
        let mut conf = run_conf.0.lock().expect("");
        conf.converting = true;
        conf.presentations_dir.clone()
    };

    let mut files = OrderMap::new();
    let path = Path::new(&presentations_dir);

    if !path.exists() {
        // Clear converting flag on error
        let mut conf = run_conf.0.lock().expect("");
        conf.converting = false;
        return Err(format!("Directory '{}' not found", presentations_dir).into());
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

        // TODO: Extract animations

        // Handle PDF conversion
        if f_lower.ends_with(".pdf") {
            let pdf_path = entry.path();
            let converted_files = strip_generator_data(convert_pdf_to_images(
                &pdf_path,
                HashSet::new(),
                HashMap::new(),
            )?);
            files.extend(converted_files);
        } else if f_lower.ends_with(".pptx") {
            let pptx_path = entry.path();
            let converted_files = strip_generator_data(convert_pptx_to_svgs(&pptx_path)?);
            files.extend(converted_files);
        } else if f_lower.ends_with(".ppt") {
            // For PPT files, convert to PPTX first, then extract videos and convert
            let ppt_path = entry.path();
            println!("Converting PPT to PPTX: {}", file_name);

            // Convert PPT to PPTX using LibreOffice
            let parent_dir = ppt_path.parent().unwrap_or(Path::new("."));
            let pptx_path = ppt_path.with_extension("pptx");

            if !pptx_path.exists() {
                let status = std::process::Command::new("flatpak")
                    .args([
                        "run",
                        "--filesystem=host",
                        "org.libreoffice.LibreOffice",
                        "--headless",
                        "--convert-to",
                        "pptx",
                        "--outdir",
                        parent_dir.to_str().unwrap(),
                        ppt_path.to_str().unwrap(),
                    ])
                    .status();

                match status {
                    Ok(s) if s.success() && pptx_path.exists() => {
                        // Process the converted PPTX
                        let converted_files =
                            strip_generator_data(convert_pptx_to_svgs(&pptx_path)?);

                        files.extend(converted_files);

                        // Clean up the temporary PPTX file
                        let _ = std::fs::remove_file(&pptx_path);
                    }
                    _ => {
                        eprintln!("Failed to convert PPT to PPTX: {}", file_name);
                    }
                }
            } else {
                let converted_files = strip_generator_data(convert_pptx_to_svgs(&pptx_path)?);

                files.extend(converted_files);

                // Clean up the temporary PPTX file
                let _ = std::fs::remove_file(&pptx_path);
            }
        } else {
            // Add valid media file
            if !files.contains_key(&file_name) {
                files.insert(
                    file_name.clone(),
                    PresentationFile {
                        name: file_name,
                        transition: RevealTransition::default(),
                    },
                );
            }
        }
    }

    // Clear converting flag before returning
    let mut conf = run_conf.0.lock().expect("");
    conf.converting = false;

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
) -> Result<OrderMap<String, GeneratorPresentationFile>, Box<dyn std::error::Error>> {
    // First, extract any embedded videos from the PPTX
    let mut parse_result = extract_videos_from_pptx(pptx_path).unwrap_or_else(|e| {
        println!("Warning: Failed to extract videos from PPTX: {}", e);
        ExtractedVideos {
            extracted_videos: OrderMap::new(),
            slides_with_videos: HashSet::new(),
            slides_with_transitions: HashMap::new(),
            slides_count: 0,
        }
    });

    let mut result = if !pptx_path
        .with_file_name(format!(
            "{}_1.svg",
            pptx_path.file_stem().expect("").to_str().expect("")
        ))
        .exists()
    {
        let pdf_path = convert_pptx_to_pdf(pptx_path)?;
        let svg_result = convert_pdf_to_images(
            &pdf_path,
            parse_result.slides_with_videos,
            parse_result.slides_with_transitions,
        )?;
        let _ = fs::remove_file(&pdf_path);

        // Filter out SVGs for slides that have videos (keep only videos)
        let mut result = OrderMap::new();

        for (svg_name, presentation_file) in svg_result {
            result.insert(svg_name, presentation_file);
        }

        result
    } else {
        let res = (1..=parse_result.slides_count)
            .filter_map(|i| {
                if parse_result.slides_with_videos.contains(&i) {
                    return None;
                }

                let transition = parse_result
                    .slides_with_transitions
                    .remove(&i)
                    .unwrap_or_else(RevealTransition::default);

                let output_svg = format!(
                    "{}_{}.svg",
                    pptx_path.file_stem().expect("").to_str().expect(""),
                    i
                );

                Some((
                    output_svg.clone(),
                    GeneratorPresentationFile {
                        file: PresentationFile {
                            name: output_svg,
                            transition,
                        },
                        internal_slide: i,
                    },
                ))
            })
            .collect();

        res
    };

    // Add extracted videos to result (already PresentationFile objects)
    for (video_name, presentation_file) in parse_result.extracted_videos.into_iter() {
        result.insert(video_name.clone(), presentation_file);
    }

    let result = result
        .sorted_by(|_, f1, _, f2| f1.internal_slide.cmp(&f2.internal_slide))
        .collect();

    Ok(result)
}

fn convert_pdf_to_images(
    pdf_path: &Path,
    skip: HashSet<u32>,
    mut transitions: HashMap<u32, RevealTransition>,
) -> Result<OrderMap<String, GeneratorPresentationFile>, Box<dyn std::error::Error>> {
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
        if skip.contains(&i) {
            continue;
        }

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

            let transition = transitions
                .remove(&i)
                .unwrap_or_else(RevealTransition::default);

            result.insert(
                output_svg.clone(),
                GeneratorPresentationFile {
                    file: PresentationFile {
                        name: output_svg,
                        transition,
                    },
                    internal_slide: i,
                },
            );
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
            | "apng"
            | "jpg"
            | "jpeg"
            | "jfif"
            | "pjpeg"
            | "pjp"
            | "bmp"
            | "ico"
            | "cur"
            | "tif"
            | "tiff"
            | "gif"
            | "svg"
            | "webp"
            | "abif"
            | "mp4"
            | "m4v"
            | "m4p"
            | "mov"
            | "avi"
            | "webm"
            | "mkv"
            | "av1"
            | "3gp"
            | "mpg"
            | "mpeg"
            | "pdf"
            | "ppt"
            | "pptx"
    )
}

struct ExtractedVideos {
    extracted_videos: OrderMap<String, GeneratorPresentationFile>,
    slides_with_videos: HashSet<u32>,
    slides_with_transitions: HashMap<u32, RevealTransition>,
    slides_count: u32,
}

/// Extract embedded videos from a PPTX file and save them to the presentations directory
/// Returns a map of video file names to PresentationFile (already sorted by slide number)
fn extract_videos_from_pptx(
    pptx_path: &Path,
) -> Result<ExtractedVideos, Box<dyn std::error::Error>> {
    let mut extracted_videos: OrderMap<u32, Vec<String>> = OrderMap::new();

    // Parse the PPTX to find embedded media
    let pptx_info = PptxParser::parse(pptx_path)?;

    // Get the base name for video naming
    let base_name = pptx_path
        .file_stem()
        .unwrap_or_default()
        .to_str()
        .unwrap_or("presentation");

    // Open the PPTX as a ZIP to extract media files
    let file = std::fs::File::open(pptx_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // Track video index per slide for naming
    let mut slide_video_counts: std::collections::HashMap<u32, u32> =
        std::collections::HashMap::new();

    let mut slide_transitions = HashMap::new();

    // Process each slide's embedded media
    for slide in &pptx_info.slides {
        let slide_number = slide.slide_number;

        // Track already-extracted media to avoid duplicates (key: source media path)
        let mut extracted_media: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for media in &slide.embedded_media {
            // Only process video files
            if !matches!(media.media_type, pptx_parser::MediaType::Video) {
                continue;
            }

            // Count videos for this slide
            let video_count = slide_video_counts.entry(slide_number).or_insert(0);
            *video_count += 1;

            // Skip if we've already extracted this media
            if !extracted_media.insert(media.filename.clone()) {
                continue;
            }

            // Check if this is an external file reference (file://)
            if media.filename.starts_with("file://") {
                // Parse the external file path
                let source_path = &media.filename[7..]; // Strip "file://" prefix
                let source = Path::new(source_path);

                if source.exists() {
                    // Get the file extension
                    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("mp4");

                    // Create output filename: {base_name}_video_{slide_number}_{video_count}.{ext}
                    let output_name = format!(
                        "{}-{}_video_{}.{}",
                        base_name, slide_number, video_count, ext
                    );
                    let output_path = pptx_path.with_file_name(&output_name);

                    // Ignore a file that already is there
                    if !output_path.exists() {
                        // Copy the external file to presentations directory
                        match std::fs::copy(source, &output_path) {
                            Ok(_) => {
                                extracted_videos
                                    .entry(slide_number)
                                    .or_default()
                                    .push(output_name);
                            }
                            Err(e) => {
                                eprintln!(
                                    "Failed to copy external video from slide {}: {} - {}",
                                    slide_number,
                                    source.display(),
                                    e
                                );
                            }
                        }
                    } else {
                        extracted_videos
                            .entry(slide_number)
                            .or_default()
                            .push(output_name);
                    }
                } else {
                    eprintln!(
                        "External video not found on slide {}: {}",
                        slide_number,
                        source.display()
                    );
                }
                continue;
            }

            // Handle embedded media (inside PPTX ZIP)
            // Media paths in relationships are like "../media/video1.mp4"
            // We need to convert to the path inside the ZIP: "ppt/media/video1.mp4"
            let media_path_in_zip = if media.filename.contains("/") {
                // Try to extract just the filename
                Path::new(&media.filename)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| format!("ppt/media/{}", n))
            } else {
                Some(format!("ppt/media/{}", media.filename))
            };

            let media_path_in_zip = match media_path_in_zip {
                Some(p) => p,
                None => continue,
            };

            // Try to find and extract the file from the ZIP
            if let Ok(mut media_file) = archive.by_name(&media_path_in_zip) {
                // Get the file extension
                let ext = Path::new(&media_path_in_zip)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("mp4");

                // Create output filename: {base_name}_video_{slide_number}_{video_count}.{ext}
                let output_name = format!(
                    "{}-{}__video_{}.{}",
                    base_name, slide_number, video_count, ext
                );
                let output_path = pptx_path.with_file_name(&output_name);

                // Extract the file
                let mut output_file = std::fs::File::create(&output_path)?;
                std::io::copy(&mut media_file, &mut output_file)?;

                println!(
                    "Extracted embedded video from slide {}: {}",
                    slide_number, output_name
                );

                // Map this slide to the extracted video
                extracted_videos
                    .entry(slide_number)
                    .or_default()
                    .push(output_name);
            } else {
                // Media referenced but not found in ZIP
                println!(
                    "Warning: Video referenced on slide {} but not found in PPTX: {}",
                    slide_number, media_path_in_zip
                );
            }

            let transition = if let Some(ref transition) = slide.transition {
                let tr_kind = match transition.transition_type {
                    pptx_parser::TransitionType::None => RevealTransitionKind::None,
                    pptx_parser::TransitionType::Fade => RevealTransitionKind::Fade,
                    pptx_parser::TransitionType::Cut => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::RandomBars => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Newsflash => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Vortex => RevealTransitionKind::Concave,
                    pptx_parser::TransitionType::Shred => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Switch => RevealTransitionKind::Convex,
                    pptx_parser::TransitionType::Flip => RevealTransitionKind::Convex,
                    pptx_parser::TransitionType::Gallery => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Ripple => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Honeycomb => RevealTransitionKind::Fade,
                    pptx_parser::TransitionType::Cube => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Box => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Accordion => RevealTransitionKind::Concave,
                    pptx_parser::TransitionType::Frame => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Glitter => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Airplane => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::FerrisWheel => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::ConveyorBelt => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Clock => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Wheel => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Comb => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Morph => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::ZoomCenter => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Rotate => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Push(_) => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Cover(_) => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Uncover(_) => RevealTransitionKind::Fade,
                    pptx_parser::TransitionType::PeelOff(_) => RevealTransitionKind::Convex,
                    pptx_parser::TransitionType::PageCurl(_) => RevealTransitionKind::Concave,
                    pptx_parser::TransitionType::Wipe(_) => RevealTransitionKind::Fade,
                    pptx_parser::TransitionType::Split(_) => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Reveal(_) => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Doors(_) => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Window(_) => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Pan(_) => RevealTransitionKind::Slide,
                    pptx_parser::TransitionType::Zoom(_) => RevealTransitionKind::Zoom,
                    pptx_parser::TransitionType::Other(_) => RevealTransitionKind::default(),
                };

                match transition.duration_ms {
                    0 => RevealTransition {
                        kind: RevealTransitionKind::None,
                        speed: RevealTransitionSpeed::Default,
                    },
                    1..=300 => RevealTransition {
                        kind: tr_kind,
                        speed: RevealTransitionSpeed::Fast,
                    },
                    301..=500 => RevealTransition {
                        kind: tr_kind,
                        speed: RevealTransitionSpeed::Default,
                    },
                    _ => RevealTransition {
                        kind: tr_kind,
                        speed: RevealTransitionSpeed::Slow,
                    },
                }
            } else {
                RevealTransition::default()
            };

            if !transition.is_default() {
                slide_transitions.insert(slide_number, transition);
            }
        }
    }

    // Convert to OrderMap<String, PresentationFile> with proper ordering
    // The OrderMap is already sorted by slide_number (u32), and within each slide,
    // videos are in extraction order (video_count order)
    let mut result = OrderMap::new();
    let mut slides_with_videos = HashSet::new();
    for (slide_number, video_names) in extracted_videos {
        for video_name in video_names {
            let transition = slide_transitions
                .remove(&slide_number)
                .unwrap_or_else(RevealTransition::default);

            result.insert(
                video_name.clone(),
                GeneratorPresentationFile {
                    file: PresentationFile {
                        name: video_name,
                        transition,
                    },
                    internal_slide: slide_number,
                },
            );
        }
        slides_with_videos.insert(slide_number);
    }

    Ok(ExtractedVideos {
        extracted_videos: result,
        slides_with_videos,
        slides_with_transitions: slide_transitions,
        slides_count: pptx_info.slide_count,
    })
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
