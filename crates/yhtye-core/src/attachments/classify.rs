//! What an attachment is (its kind and MIME type) and how it should be
//! delivered. Pure: the file name, the client's MIME hint and the content.

use super::{AttachmentDelivery, AttachmentKind, PromptCaps};

/// The MIME type used when nothing better is known.
const GENERIC_MIME: &str = "application/octet-stream";

/// The images an agent can be sent as an image block.
const NATIVE_IMAGES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Extensions of text files (lower case).
const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "text",
    "md",
    "markdown",
    "rst",
    "adoc",
    "org",
    "log",
    "csv",
    "tsv",
    "json",
    "jsonc",
    "json5",
    "jsonl",
    "ndjson",
    "xml",
    "xsl",
    "xsd",
    "plist",
    "html",
    "htm",
    "xhtml",
    "css",
    "scss",
    "sass",
    "less",
    "svg",
    "js",
    "mjs",
    "cjs",
    "jsx",
    "ts",
    "mts",
    "cts",
    "tsx",
    "vue",
    "svelte",
    "astro",
    "py",
    "pyi",
    "rb",
    "rs",
    "go",
    "java",
    "kt",
    "kts",
    "scala",
    "groovy",
    "gradle",
    "swift",
    "m",
    "mm",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "hh",
    "cs",
    "fs",
    "fsx",
    "vb",
    "php",
    "pl",
    "pm",
    "lua",
    "r",
    "jl",
    "dart",
    "ex",
    "exs",
    "erl",
    "hs",
    "ml",
    "mli",
    "clj",
    "cljs",
    "elm",
    "nim",
    "zig",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "bat",
    "cmd",
    "sql",
    "graphql",
    "gql",
    "proto",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "config",
    "env",
    "properties",
    "editorconfig",
    "gitignore",
    "gitattributes",
    "dockerignore",
    "lock",
    "diff",
    "patch",
    "tex",
    "bib",
    "srt",
    "vtt",
    "tf",
    "tfvars",
    "hcl",
    "nix",
    "cmake",
    "mk",
    "make",
    "ipynb",
    "rtf",
];

/// File names (lower case) that are text without an extension.
const TEXT_NAMES: &[&str] = &[
    "makefile",
    "dockerfile",
    "license",
    "licence",
    "readme",
    "changelog",
    "authors",
    "codeowners",
    "gemfile",
    "rakefile",
    "procfile",
    "jenkinsfile",
    "vagrantfile",
];

/// MIME types for extensions (lower case), for the attachments that arrive
/// without one. Text extensions not listed are `text/plain`.
const EXTENSION_MIMES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("bmp", "image/bmp"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("heic", "image/heic"),
    ("heif", "image/heif"),
    ("avif", "image/avif"),
    ("ico", "image/x-icon"),
    ("svg", "image/svg+xml"),
    ("pdf", "application/pdf"),
    ("zip", "application/zip"),
    ("gz", "application/gzip"),
    ("tar", "application/x-tar"),
    ("7z", "application/x-7z-compressed"),
    ("doc", "application/msword"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("xls", "application/vnd.ms-excel"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("ppt", "application/vnd.ms-powerpoint"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
    ("mp4", "video/mp4"),
    ("mov", "video/quicktime"),
    ("webm", "video/webm"),
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("markdown", "text/markdown"),
    ("csv", "text/csv"),
    ("tsv", "text/tab-separated-values"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("mjs", "text/javascript"),
    ("cjs", "text/javascript"),
    ("json", "application/json"),
    ("jsonl", "application/x-ndjson"),
    ("ndjson", "application/x-ndjson"),
    ("xml", "application/xml"),
    ("yaml", "application/yaml"),
    ("yml", "application/yaml"),
    ("toml", "application/toml"),
];

/// The kind and MIME type of an attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub mime: String,
    pub kind: AttachmentKind,
}

/// Classifies a file by its content first, then its MIME hint and its name.
///
/// - An image is one whose content starts like a PNG, JPEG, GIF or WebP; its
///   MIME type is then what the content says, whatever the client claimed (an
///   agent's API refuses an image whose type does not match).
/// - Text is valid UTF-8 without NUL bytes, with a text MIME type or a known text
///   extension or file name.
/// - Everything else is a file; its MIME type is the client's, else the
///   extension's, else `application/octet-stream`.
#[must_use]
pub fn classify(name: &str, mime_hint: Option<&str>, bytes: &[u8]) -> Classified {
    if let Some(mime) = sniff_image(bytes) {
        return Classified {
            mime: mime.to_string(),
            kind: AttachmentKind::Image,
        };
    }
    let hint = mime_hint.and_then(normalize_mime);
    let by_name = extension_of(name).and_then(mime_of_extension);
    let textual = hint.as_deref().is_some_and(is_text_mime)
        || by_name.is_some_and(is_text_mime)
        || has_text_name(name);
    if textual && is_text(bytes) {
        let mime = hint
            .filter(|m| is_text_mime(m))
            .or_else(|| by_name.filter(|m| is_text_mime(m)).map(str::to_string))
            .unwrap_or_else(|| "text/plain".to_string());
        return Classified {
            mime,
            kind: AttachmentKind::Text,
        };
    }
    let mime = hint
        .or_else(|| by_name.map(str::to_string))
        .unwrap_or_else(|| GENERIC_MIME.to_string());
    Classified {
        mime,
        kind: AttachmentKind::File,
    }
}

/// How an attachment of `kind` is delivered to an agent that accepts `caps`.
#[must_use]
pub fn delivery_for(kind: AttachmentKind, caps: PromptCaps) -> AttachmentDelivery {
    match kind {
        AttachmentKind::Image if caps.image => AttachmentDelivery::Native,
        AttachmentKind::Text if caps.embedded_context => AttachmentDelivery::Native,
        _ => AttachmentDelivery::Path,
    }
}

/// The MIME type of an image whose content starts like one of
/// [`NATIVE_IMAGES`].
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return None;
    };
    debug_assert!(NATIVE_IMAGES.contains(&mime));
    Some(mime)
}

/// Valid UTF-8 without NUL bytes.
fn is_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// `hint` as `type/subtype` in lower case without parameters; `None` for
/// something that is not a MIME type or says nothing (`application/octet-stream`).
/// A few common aliases are corrected (`image/jpg`).
fn normalize_mime(hint: &str) -> Option<String> {
    let essence = hint.split(';').next()?.trim().to_ascii_lowercase();
    let (kind, subtype) = essence.split_once('/')?;
    let token = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "!#$&^_.+-".contains(c))
    };
    if !token(kind) || !token(subtype) || essence == GENERIC_MIME {
        return None;
    }
    Some(match essence.as_str() {
        "image/jpg" | "image/pjpeg" => "image/jpeg".to_string(),
        _ => essence,
    })
}

/// The lower-case extension of `name`, if it has one.
fn extension_of(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

fn mime_of_extension(ext: String) -> Option<&'static str> {
    if let Some((_, mime)) = EXTENSION_MIMES.iter().find(|(e, _)| *e == ext) {
        return Some(mime);
    }
    TEXT_EXTENSIONS
        .contains(&ext.as_str())
        .then_some("text/plain")
}

/// A file name that is text without an extension (`Makefile`, `LICENSE`).
fn has_text_name(name: &str) -> bool {
    TEXT_NAMES.contains(&name.to_ascii_lowercase().as_str())
}

/// Whether files of this MIME type are text.
fn is_text_mime(mime: &str) -> bool {
    if mime.starts_with("text/") {
        return true;
    }
    let (_, subtype) = mime.split_once('/').unwrap_or(("", mime));
    subtype.ends_with("+json")
        || subtype.ends_with("+xml")
        || subtype.ends_with("+yaml")
        || matches!(
            mime,
            "application/json"
                | "application/xml"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
                | "application/javascript"
                | "application/x-javascript"
                | "application/ecmascript"
                | "application/typescript"
                | "application/x-typescript"
                | "application/x-ndjson"
                | "application/x-sh"
                | "application/x-shellscript"
                | "application/sql"
                | "application/graphql"
                | "application/x-httpd-php"
                | "application/x-latex"
                | "application/x-tex"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n....";
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10];
    const BOTH: PromptCaps = PromptCaps {
        image: true,
        embedded_context: true,
    };
    const NONE: PromptCaps = PromptCaps {
        image: false,
        embedded_context: false,
    };

    fn kind_of(name: &str, hint: Option<&str>, bytes: &[u8]) -> (AttachmentKind, String) {
        let c = classify(name, hint, bytes);
        (c.kind, c.mime)
    }

    #[test]
    fn images_are_told_by_their_content_not_the_claimed_type() {
        for (bytes, mime) in [
            (PNG, "image/png"),
            (JPEG, "image/jpeg"),
            (b"GIF89a\x01\x00".as_slice(), "image/gif"),
            (b"RIFF\x24\0\0\0WEBPVP8 ".as_slice(), "image/webp"),
        ] {
            let name = "shot.bin";
            assert_eq!(
                kind_of(name, None, bytes),
                (AttachmentKind::Image, mime.to_string())
            );
            // The client's claim is corrected, not trusted.
            assert_eq!(
                kind_of(name, Some("image/png"), bytes).1,
                mime,
                "{mime} claimed as png"
            );
        }
        assert_eq!(
            kind_of("a.jpg", Some("image/jpg"), JPEG),
            (AttachmentKind::Image, "image/jpeg".into())
        );
    }

    #[test]
    fn an_image_type_without_image_content_or_of_another_format_is_a_plain_file() {
        // Claimed as a PNG but not one.
        assert_eq!(
            kind_of("x.png", Some("image/png"), b"not an image"),
            (AttachmentKind::File, "image/png".into())
        );
        // Formats the agents are not sent as images.
        assert_eq!(
            kind_of("x.bmp", None, b"BM\0\0\0\0"),
            (AttachmentKind::File, "image/bmp".into())
        );
        assert_eq!(
            kind_of("x.heic", None, b"\0\0\0\x18ftypheic"),
            (AttachmentKind::File, "image/heic".into())
        );
    }

    #[test]
    fn text_is_utf8_with_a_text_type_or_a_known_extension() {
        assert_eq!(
            kind_of("notes.txt", None, "héllo\n".as_bytes()),
            (AttachmentKind::Text, "text/plain".into())
        );
        assert_eq!(
            kind_of("main.rs", Some("application/octet-stream"), b"fn main() {}"),
            (AttachmentKind::Text, "text/plain".into())
        );
        assert_eq!(
            kind_of("data.json", None, b"{}"),
            (AttachmentKind::Text, "application/json".into())
        );
        assert_eq!(
            kind_of("README", None, b"hi"),
            (AttachmentKind::Text, "text/plain".into())
        );
        assert_eq!(
            kind_of("blob", Some("Text/Markdown; charset=utf-8"), b"# t"),
            (AttachmentKind::Text, "text/markdown".into())
        );
        assert_eq!(
            kind_of("pic.svg", None, b"<svg/>"),
            (AttachmentKind::Text, "image/svg+xml".into())
        );
        assert_eq!(
            kind_of("x", Some("application/vnd.api+json"), b"{}").0,
            AttachmentKind::Text
        );
    }

    #[test]
    fn text_that_is_not_utf8_or_has_nul_bytes_is_a_file() {
        assert_eq!(
            kind_of("sjis.txt", None, &[0x83, 0x65, 0x83, 0x58, 0x83, 0x67]).0,
            AttachmentKind::File
        );
        assert_eq!(kind_of("a.log", None, b"ok\0ok").0, AttachmentKind::File);
        assert_eq!(kind_of("empty.txt", None, b"").0, AttachmentKind::Text);
    }

    #[test]
    fn utf8_without_a_text_type_or_extension_is_a_file() {
        assert_eq!(
            kind_of("data.bin", None, b"plain ascii"),
            (AttachmentKind::File, GENERIC_MIME.into())
        );
        assert_eq!(
            kind_of("report.pdf", None, b"%PDF-1.7 ascii"),
            (AttachmentKind::File, "application/pdf".into())
        );
        assert_eq!(
            kind_of("x", Some("not a mime"), b"abc"),
            (AttachmentKind::File, GENERIC_MIME.into())
        );
        assert_eq!(
            kind_of("x", Some("video/mp4"), b"abc"),
            (AttachmentKind::File, "video/mp4".into())
        );
    }

    #[test]
    fn delivery_follows_what_the_agent_accepts() {
        use AttachmentDelivery::{Native, Path};
        use AttachmentKind::{File, Image, Text};
        for (kind, caps, want) in [
            (Image, BOTH, Native),
            (Image, NONE, Path),
            (
                Image,
                PromptCaps {
                    image: false,
                    embedded_context: true,
                },
                Path,
            ),
            (Text, BOTH, Native),
            (Text, NONE, Path),
            (
                Text,
                PromptCaps {
                    image: true,
                    embedded_context: false,
                },
                Path,
            ),
            (File, BOTH, Path),
            (File, NONE, Path),
        ] {
            assert_eq!(delivery_for(kind, caps), want, "{kind:?} {caps:?}");
        }
    }
}
