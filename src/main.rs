use axum::{
    extract::{Form, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    response::Redirect,
    routing::{get, post},
    Router,
};
use ordermap::OrderMap;
use serde::Deserialize;
use serde::Serialize;
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
use tower_http::{cors::CorsLayer, services::ServeDir};

mod cmd;
mod dashboard;
mod html_generator;
use html_generator::generate_html;
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

    // Build our application with a route
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/presentation", get(generate_html_endpoint))
        .route("/presentation/ws", get(websocket_handler))
        .route("/settings/limiter", post(limiter_handler))
        .layer(CorsLayer::permissive())
        .with_state(RunningConf(Arc::new(Mutex::new(RunningConfInner {
            presentations_dir: presentations_dir.clone(),
            limiter_on: true,
            config,
            presentation_version: String::new(),
            reload_tx: reload_tx.clone(),
        }))))
        .fallback_service(ServeDir::new(&presentations_dir));

    // Run the server
    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("Server listening on {}", addr);
    println!("Presentations directory: {}", presentations_dir);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    open_link(&format!("http://localhost:{}/presentation", port));
    let (server_res, _) = join!(axum::serve(listener, app), load_easy_effects());
    server_res.unwrap();
}

// Handler for dashboard
async fn dashboard_handler(State(config): State<RunningConf>) -> impl IntoResponse {
    let html = dashboard::main_dashboard(config.0.lock().expect("").limiter_on);
    ([("Content-Type", "text/html; charset=utf-8")], html).into_response()
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
    // Get presentation files from directory
    match get_presentation_files(&run_conf.0.lock().expect("").presentations_dir) {
        Ok(files) => {
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
