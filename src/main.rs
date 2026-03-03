use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    response::Redirect,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde::Serialize;
use std::{env, fs, net::SocketAddr, path::Path};
use tokio::join;
use tower_http::{cors::CorsLayer, services::ServeDir};

mod cmd;
mod dashboard;
mod html_generator;
use html_generator::generate_html;
use html_generator::PresentationFile;

use crate::cmd::{app, open_link, try_run};

#[derive(Clone)]
struct RunningConf {
    presentations_dir: String,
    limiter_on: bool,
    config: Config,
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

    // Build our application with a route
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/presentation", get(generate_html_endpoint))
        .route("/settings/limiter", post(limiter_handler))
        .layer(CorsLayer::permissive())
        .with_state(RunningConf {
            presentations_dir: presentations_dir.clone(),
            limiter_on: true,
            config,
        })
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
async fn dashboard_handler(_config: State<RunningConf>) -> impl IntoResponse {
    let html = dashboard::main_dashboard();
    ([("Content-Type", "text/html; charset=utf-8")], html).into_response()
}

// Handler to generate presentation HTML
async fn generate_html_endpoint(run_conf: State<RunningConf>) -> impl IntoResponse {
    // Get presentation files from directory
    match get_presentation_files(&run_conf.presentations_dir) {
        Ok(files) => {
            // Convert to the right type for the HTML generator
            let html_files = files
                .into_iter()
                .map(|f| html_generator::PresentationFile { name: f.name })
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

        // Handle PDF conversion
        if file_name.to_lowercase().ends_with(".pdf") {
            let pdf_path = entry.path();
            let converted_files = convert_pdf_to_images(&pdf_path)?;
            files.extend(converted_files);
        } else {
            // Add valid media file
            files.push(PresentationFile { name: file_name });
        }
    }

    Ok(files)
}

fn get_pdf_pages(pdf_path: &Path) -> Result<u32, Box<dyn std::error::Error>> {
    let output = app("pdfinfo").arg(temp_pdf).output()?;
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
    let status = app("libreoffice")
        .args(["--headless", "--convert-to", "pdf", input_pptx])
        .status()?;

    if !status.success() {
        return Err("Failed to convert PPTX to PDF".into());
    }
}

fn convert_pdf_to_images(
    pdf_path: &Path,
) -> Result<Vec<PresentationFile>, Box<dyn std::error::Error>> {
    let pdf_name = pdf_path.file_stem().unwrap().to_string_lossy();

    // 2. Get the number of pages using pdfinfo
    let page_count = get_pdf_pages(pdf_path)?;

    let mut result = Vec::with_capacity(page_count as usize);
    // 3. Loop through and run pdf2svg for each page
    for i in 1..=page_count {
        let output_svg = format!("slide_{}.svg", i);
        let svg_status = app("pdf2svg")
            .args([temp_pdf, &output_svg, &i.to_string()])
            .status()?;

        if svg_status.success() {
            result.push(PresentationFile { name: output_svg });
            println!("Generated: {}", output_svg);
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
    )
}

#[derive(Deserialize)]
struct LimiterQuery {
    enable: bool,
}

async fn limiter_handler(
    State(_config): State<RunningConf>,
    Query(query): Query<LimiterQuery>,
) -> impl IntoResponse {
    if query.enable {
        try_run(cmd::app("flatpak").map(|mut a| {
            a.args(["run", "com.github.wwmm.easyeffects", "-b", "2"]);
            a
        }));
    } else {
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
