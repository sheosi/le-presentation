use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router};
use serde::{Deserialize, Serialize};
use std::{env, fs, net::SocketAddr, path::Path};
use tokio::join;
use tower_http::cors::CorsLayer;

mod cmd;
mod dashboard;
mod html_generator;
use html_generator::generate_html;

use crate::cmd::{open_link, try_run};

#[derive(Serialize, Deserialize)]
struct PresentationFile {
    name: String,
    path: String,
    size: u64,
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
        println!("PRESENTATIONS_DIR not set, using default 'presentations' directory");
        "presentations".to_string()
    });

    // Create presentations directory if it doesn't exist
    create_presentations_dir_if_needed(&presentations_dir);

    let config = Config {};

    // Build our application with a route
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/presentation", get(generate_html_endpoint))
        .layer(CorsLayer::permissive())
        .with_state(config);

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
async fn dashboard_handler(_config: State<Config>) -> impl IntoResponse {
    let html = dashboard::main_dashboard();
    ([("Content-Type", "text/html; charset=utf-8")], html).into_response()
}

// Handler to generate presentation HTML
async fn generate_html_endpoint() -> impl IntoResponse {
    // Get presentations directory from environment variable
    let presentations_dir =
        env::var("PRESENTATIONS_DIR").unwrap_or_else(|_| "presentations".to_string());

    // Get presentation files from directory
    match get_presentation_files(&presentations_dir) {
        Ok(files) => {
            // Convert to the right type for the HTML generator
            let html_files = files
                .into_iter()
                .map(|f| html_generator::PresentationFile {
                    name: f.name,
                    path: f.path,
                    size: f.size,
                })
                .collect();

            let html_content = generate_html(html_files);
            ([("Content-Type", "text/html; charset=utf-8")], html_content).into_response()
        }
        Err(e) => {
            eprintln!("Error generating presentation HTML: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)).into_response()
        }
    }
}

// Helper function to get presentation files for HTML generation
fn get_presentation_files(dir: &str) -> Result<Vec<PresentationFile>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
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

        // Add valid media file
        files.push(PresentationFile {
            name: file_name,
            path: entry.path().to_string_lossy().to_string(),
            size: metadata.len(),
        });
    }

    Ok(files)
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
    )
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
