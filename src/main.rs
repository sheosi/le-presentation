use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use serde::{Deserialize, Serialize};
use std::{env, fs, net::SocketAddr, path::Path};
use tower_http::cors::CorsLayer;

#[derive(Serialize, Deserialize)]
struct PresentationFile {
    name: String,
    path: String,
    size: u64,
}

#[derive(Serialize, Deserialize)]
struct PresentationResponse {
    files: Vec<PresentationFile>,
    count: usize,
    presentations_directory: String,
}

#[tokio::main]
async fn main() {
    // Get presentations directory from environment variable
    let presentations_dir = env::var("PRESENTATIONS_DIR").unwrap_or_else(|_| {
        eprintln!("PRESENTATIONS_DIR not set, using default 'presentations' directory");
        "presentations".to_string()
    });

    // Ensure the presentations directory exists
    if !Path::new(&presentations_dir).exists() {
        if let Err(e) = fs::create_dir_all(&presentations_dir) {
            eprintln!("Failed to create presentations directory: {}", e);
        } else {
            println!("Created presentations directory: {}", presentations_dir);
        }
    }

    // Build our application with a route
    let app = Router::new()
        .route("/presentation", get(list_presentations))
        .layer(CorsLayer::permissive());

    // Run the server
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("Server listening on {}", addr);
    println!("Presentations directory: {}", presentations_dir);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// Handler function for listing presentation files
async fn list_presentations() -> impl IntoResponse {
    // Get presentations directory from environment variable
    let presentations_dir =
        env::var("PRESENTATIONS_DIR").unwrap_or_else(|_| "presentations".to_string());

    match scan_presentations_directory(&presentations_dir) {
        Ok(files) => {
            let response = PresentationResponse {
                count: files.len(),
                files,
                presentations_directory: presentations_dir,
            };
            Json(response).into_response()
        }
        Err(e) => {
            eprintln!("Error scanning presentations directory: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)).into_response()
        }
    }
}

// Helper function to scan the presentations directory
fn scan_presentations_directory(
    dir: &str,
) -> Result<Vec<PresentationFile>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    let path = Path::new(dir);

    if !path.exists() {
        return Err(format!("Directory '{}' not found", dir).into());
    }

    let entries = fs::read_dir(path)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Skip directories
        if path.is_dir() {
            continue;
        }

        // Get file metadata
        let metadata = entry.metadata()?;

        // Create PresentationFile struct
        let presentation_file = PresentationFile {
            name: entry.file_name().to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
            size: metadata.len(),
        };

        files.push(presentation_file);
    }

    Ok(files)
}
