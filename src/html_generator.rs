use serde::{Deserialize, Serialize};

/// Represents a presentation file for HTML generation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PresentationFile {
    pub name: String,
    pub path: String,
    pub size: u64,
}

impl PresentationFile {
    /// Check if this is a video file based on extension
    pub fn is_video(&self) -> bool {
        if let Some(ext) = self.name.split('.').last() {
            matches!(
                ext.to_lowercase().as_str(),
                "mp4" | "mov" | "avi" | "webm" | "ogg" | "mkv"
            )
        } else {
            false
        }
    }

    /// Check if this is an image file based on extension
    pub fn is_image(&self) -> bool {
        if let Some(ext) = self.name.split('.').last() {
            matches!(
                ext.to_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp"
            )
        } else {
            false
        }
    }
}

/// Generates Reveal.js HTML from a list of presentation files
pub fn generate_html(presentation_files: Vec<PresentationFile>) -> String {
    // Filter only show valid media files
    let mut slides: Vec<PresentationFile> = presentation_files
        .into_iter()
        .filter(|f| f.is_image() || f.is_video())
        .collect();

    // Sort slides by filename
    slides.sort_by(|a, b| a.name.cmp(&b.name));

    build_reveal_html(&slides)
}

/// Builds the Reveal.js HTML template
fn build_reveal_html(slides: &[PresentationFile]) -> String {
    let mut html = String::from(
        r#"<!doctype html>
<html lang="es">
    <head>
        <meta charset="utf-8" />
        <meta
            name="viewport"
            content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no"
        />

        <title>Presentación</title>

        <link
            rel="stylesheet"
            href="https://cdnjs.cloudflare.com/ajax/libs/reveal.js/4.5.0/reveal.min.css"
        />
        <link
            rel="stylesheet"
            href="https://cdnjs.cloudflare.com/ajax/libs/reveal.js/4.5.0/theme/black.min.css"
        />

        <style>
            :root {
            --r-background-color: #0b0b0b; /* Replace with your hex code */
            }
            /* Ensures videos and images fill the screen properly */
            .reveal .slides section {
                height: 100%;
            }
            /* This forces the video to stretch/scale to fill the entire viewport */
            .reveal .backgrounds video {
                width: 100% !important;
                height: 100% !important;
                object-fit: cover !important;
            }
        </style>
    </head>
    <body>
        <div class="reveal">
            <div class="slides">"#,
    );

    // Add slide sections
    for file in slides {
        html.push_str("\n                ");
        html.push_str(&generate_slide_section(&file));
    }

    html.push_str(r#"
            </div>
        </div>

        <script src="https://cdnjs.cloudflare.com/ajax/libs/reveal.js/4.5.0/reveal.js"></script>

        <script>
            Reveal.initialize({
                // The magic happens here:
                transition: "slide", // global transition style (none/fade/slide/convex/concave/zoom)
                transitionSpeed: "default", // default/fast/slow
                // This controls the movement of the background images/videos
                backgroundTransition: "slide",

                // Full screen configuration
                width: "100%",
                height: "100%",
                margin: 0,
                minScale: 1,
                maxScale: 1,

                // Media Autoplay
                autoPlayMedia: true,
                hash: true,
            });
        </script>
    </body>
</html>"#);

    html
}

/// Generates a slide section based on file type
fn generate_slide_section(file: &PresentationFile) -> String {
    if file.is_video() {
        format!(
            r#"<section
                    data-background-video="{}"
                    data-autoplay
                    data-background-size="cover"
                    data-background-video-loop
                ></section>"#,
            html_escape(&file.name)
        )
    } else if file.is_image() {
        format!(
            r#"<section
                    data-background-image="{}"
                    data-background-size="contain"
                ></section>"#,
            html_escape(&file.name)
        )
    } else {
        // Unsupported file type - return empty section
        String::from("<section></section>")
    }
}

fn html_escape(text: &str) -> String {
    text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_type_detection() {
        let video_file = PresentationFile {
            name: "PRESENTACION 1.1.mp4".to_string(),
            path: "/path/to/file.mp4".to_string(),
            size: 1024,
        };
        assert!(video_file.is_video());
        assert!(!video_file.is_image());

        let image_file = PresentationFile {
            name: "PRESENTACION 1.0.png".to_string(),
            path: "/path/to/file.png".to_string(),
            size: 2048,
        };
        assert!(image_file.is_image());
        assert!(!image_file.is_video());
    }

    #[test]
    fn test_html_generation() {
        let files = vec![
            PresentationFile {
                name: "PRESENTACION-1.0.png".to_string(),
                path: "PRESENTACION-1.0.png".to_string(),
                size: 1024,
            },
            PresentationFile {
                name: "PRESENTACION-1.1.mp4".to_string(),
                path: "PRESENTACION-1.1.mp4".to_string(),
                size: 2048,
            },
        ];

        let html = generate_html(files);
        assert!(html.contains(r#"data-background-image="PRESENTACION-1.0.png""#));
        assert!(html.contains(r#"data-background-size="contain""#));
        assert!(html.contains(r#"data-background-video="PRESENTACION-1.1.mp4""#));
        assert!(html.contains("data-autoplay"));
        assert!(html.contains(r#"data-background-size="cover""#));
        assert!(html.contains("Reveal.initialize"));
    }
}
