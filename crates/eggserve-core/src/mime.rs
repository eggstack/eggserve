use std::path::Path;

/// Returns the MIME type for a file based on its extension.
/// Falls back to `application/octet-stream` for unknown extensions.
pub fn mime_for_path(path: &Path) -> &'static str {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(|ext| MIME_MAP.get(ext.to_ascii_lowercase().as_str()))
        .copied()
        .unwrap_or("application/octet-stream")
}

/// Small embedded extension-to-MIME map covering common web-safe types.
/// Unknown types always fall back to application/octet-stream.
static MIME_MAP: phf::Map<&'static str, &'static str> = phf::phf_map! {
    // Documents
    "html" => "text/html; charset=utf-8",
    "htm" => "text/html; charset=utf-8",
    "css" => "text/css; charset=utf-8",
    "js" => "application/javascript; charset=utf-8",
    "mjs" => "application/javascript; charset=utf-8",
    "json" => "application/json; charset=utf-8",
    "xml" => "application/xml; charset=utf-8",
    "txt" => "text/plain; charset=utf-8",
    "csv" => "text/csv; charset=utf-8",
    "tsv" => "text/tab-separated-values; charset=utf-8",

    // Images
    "png" => "image/png",
    "jpg" => "image/jpeg",
    "jpeg" => "image/jpeg",
    "gif" => "image/gif",
    "svg" => "image/svg+xml",
    "ico" => "image/x-icon",
    "webp" => "image/webp",
    "avif" => "image/avif",
    "bmp" => "image/bmp",
    "tiff" => "image/tiff",
    "tif" => "image/tiff",

    // Fonts
    "woff" => "font/woff",
    "woff2" => "font/woff2",
    "ttf" => "font/ttf",
    "otf" => "font/otf",
    "eot" => "application/vnd.ms-fontobject",

    // Audio/Video
    "mp3" => "audio/mpeg",
    "mp4" => "video/mp4",
    "webm" => "video/webm",
    "ogg" => "audio/ogg",
    "wav" => "audio/wav",
    "flac" => "audio/flac",
    "aac" => "audio/aac",
    "m4a" => "audio/mp4",

    // Archives
    "zip" => "application/zip",
    "gz" => "application/gzip",
    "tar" => "application/x-tar",
    "bz2" => "application/x-bzip2",
    "xz" => "application/x-xz",
    "zst" => "application/zstd",

    // Documents
    "pdf" => "application/pdf",
    "doc" => "application/msword",
    "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "xls" => "application/vnd.ms-excel",
    "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "ppt" => "application/vnd.ms-powerpoint",
    "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",

    // Data
    "wasm" => "application/wasm",
    "manifest" => "text/cache-manifest",

    // Config/Script
    "yaml" => "text/yaml; charset=utf-8",
    "yml" => "text/yaml; charset=utf-8",
    "toml" => "text/plain; charset=utf-8",
    "ini" => "text/plain; charset=utf-8",
    "md" => "text/markdown; charset=utf-8",
    "rtf" => "application/rtf",
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn known_extension_returns_correct_type() {
        assert_eq!(
            mime_for_path(&PathBuf::from("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            mime_for_path(&PathBuf::from("style.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            mime_for_path(&PathBuf::from("app.js")),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(
            mime_for_path(&PathBuf::from("data.json")),
            "application/json; charset=utf-8"
        );
        assert_eq!(mime_for_path(&PathBuf::from("image.png")), "image/png");
        assert_eq!(mime_for_path(&PathBuf::from("image.jpg")), "image/jpeg");
        assert_eq!(mime_for_path(&PathBuf::from("font.woff2")), "font/woff2");
        assert_eq!(
            mime_for_path(&PathBuf::from("archive.zip")),
            "application/zip"
        );
    }

    #[test]
    fn unknown_extension_returns_octet_stream() {
        assert_eq!(
            mime_for_path(&PathBuf::from("file.xyz")),
            "application/octet-stream"
        );
        assert_eq!(
            mime_for_path(&PathBuf::from("file")),
            "application/octet-stream"
        );
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(
            mime_for_path(&PathBuf::from("FILE.HTML")),
            "text/html; charset=utf-8"
        );
        assert_eq!(mime_for_path(&PathBuf::from("image.PNG")), "image/png");
    }

    #[test]
    fn path_with_directories() {
        assert_eq!(
            mime_for_path(&PathBuf::from("/foo/bar/index.html")),
            "text/html; charset=utf-8"
        );
    }
}
