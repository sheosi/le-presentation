# Le Presentation Server

A presentation management server built with Rust and Axum.

## Features

- **List presentation files** from a configured directory
- **Generate HTML presentations** from images and videos using Reveal.js
- **Environment variable configuration** 
- **CORS enabled** for frontend integration
- **RESTful API** with automatic sorting
- **Automatic media type detection** (images vs videos with correct attributes)

## Environment Variables

- `PRESENTATIONS_DIR`: Directory where presentation files are stored (defaults to `presentations/`)
- `PORT`: Server port (defaults to 8080)

## API Endpoints

### GET /presentation

Returns a JSON list of all presentation files in the configured directory.

**Response:**
```json
{
  "files": [
    {
      "name": "PRESENTACION 1.0.png",
      "path": "presentations/PRESENTACION 1.0.png",
      "size": 2048
    }
  ],
  "count": 1,
  "presentations_directory": "presentations"
}
```

### GET /presentation.html

Generates a complete Reveal.js HTML presentation from all files in the directory. 

**Features:**
- Images: displays with `data-background-size="contain"`
- Videos: autoplay with `data-background-size="cover"` and loop
- Files are automatically sorted by name
- Media files are filtered (only images/videos processed)

**Response:** Complete HTML document ready to serve or save

## Development

**Run the server:**
```bash
export PRESENTATIONS_DIR=my_presentations  # optional
cargo run
```

**Test the APIs:**
```bash
# List presentation files
curl http://localhost:8080/presentation

# Generate HTML presentation
curl http://localhost:8080/presentation.html > presentation.html
```

## Configuration

The server expects presentation files in the `presentations/` directory by default. You can change this by setting the `PRESENTATIONS_DIR` environment variable.

## HTML Generation Features

The `/presentation.html` endpoint creates a complete Reveal.js presentation with:
- Automatic media type detection (images vs videos)
- Correct data attributes for each media type
- Autoplay for videos with loop
- Responsive styling and full-screen presentation
- Professional styling for professional presentations

## CORS

The server is configured with permissive CORS settings to allow frontend applications to connect from different origins.