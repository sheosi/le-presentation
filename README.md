   cargo build
   PRESENTATIONS_DIR=presentations cargo run
   ```

## API

### `GET /presentation`

Returns JSON with presentation files info:
```json
{
  "files": [
    {
      "name": "sample-presentation.html",
      "path": "presentations/sample-presentation.html",
      "size": 928
    }
  ],
  "count": 1,
  "presentations_directory": "presentations"
}
```

## Presentation Files

The server scans the `PRESENTATIONS_DIR` directory for all files (excluding subdirectories). Place your Reveal.js presentation HTML files here for server access.