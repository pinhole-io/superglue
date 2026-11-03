# Operations: throttled downloads

## Status

`http::download::download_throttled` is shipped.
It writes an HTTP body to a file at a maximum byte rate.
The limiter is a `governor` token bucket, the same family as provider QPS limits.

The function is a library API. The CLI has no download subcommand.

## Call

```rust
use std::path::Path;

use superglue::http::download::{DownloadConfig, download_throttled};
use superglue::http::{ClientConfig, HttpClient};

let http = HttpClient::new(ClientConfig::default())?;
let bytes = download_throttled(
    &http,
    "https://example.com/model.bin",
    Path::new("/tmp/model.bin"),
    DownloadConfig {
        max_bytes_per_sec: 1024 * 1024,
        on_progress: Some(Box::new(|downloaded, total| {
            eprintln!("{downloaded} of {total:?}");
        })),
    },
)
.await?;
```

## Rules

- `max_bytes_per_sec` of `0` means no limit. That is the default.
- `on_progress` receives `(bytes_written, content_length)`.
- `content_length` is `None` when the server omits `Content-Length`.
- Reads and writes are async. The limiter does not sleep the whole runtime.

## See also

- [Ecosystem reference](10-ecosystem-reference.md)
- [Roadmap and phasing](09-roadmap-and-phasing.md)
