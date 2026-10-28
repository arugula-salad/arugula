//! A Doc's pictures (#965): PNG, JPEG, GIF and WebP files the agent saved,
//! kept beside the Doc in `doc/assets/<name>` and shown by the section that
//! says `![alt](assets/<name>)`.
//!
//! What a file is, is what its bytes say (`agent::images::sniff`), never
//! what its name says; the name has to agree with the bytes, so a section
//! can't name a `.png` that is something else. SVG files are not pictures
//! here: a drawing is a ```` ```svg ```` fence (#964). The state holds
//! `{name, mime, size, rev}` for each and never the bytes; a client asks
//! for one by name (`doc_asset`) when a section shows it.

use std::path::Path;

use serde::Serialize;

use crate::agent::images::sniff;

/// One picture, in bytes.
pub const ASSET_MAX: usize = 2 * 1024 * 1024;
/// The pictures one Doc has.
pub const ASSETS_MAX: usize = 20;

/// The folder inside the Doc's.
pub const FOLDER: &str = "assets";

/// What the state says of a picture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Asset {
    pub name: String,
    pub mime: &'static str,
    pub size: u64,
    /// Changes whenever the name is written again (a time in ms that only
    /// goes up, so it is not reused after a restart either): clients cache
    /// a picture by `name/rev`.
    pub rev: u64,
}

/// The extensions a picture's name can end in, and the type each stands for.
const EXTENSIONS: [(&str, &str); 5] =
    [("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"), ("gif", "image/gif"), ("webp", "image/webp")];

/// `.png .jpg .jpeg .gif .webp`, for a refusal.
pub fn extensions() -> String {
    EXTENSIONS.iter().map(|(e, _)| format!(".{e}")).collect::<Vec<_>>().join(" ")
}

/// A plain file name: what the web client's `assets/<name>` also takes.
pub fn is_name(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=100).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

/// The type the name's extension stands for (lowercase only: the page
/// matches the same).
pub fn named_mime(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1;
    EXTENSIONS.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// A name `doc_asset_put` takes, or why not.
pub fn check_name(name: &str) -> Result<(), String> {
    if !is_name(name) {
        return Err(format!(
            "asset name {name:?}: use 1 to 100 letters, digits, _ . or -, starting with a letter or digit (no slash)"
        ));
    }
    // `write_atomic` writes `<name without its extension>.tmp` and renames it.
    if name.to_ascii_lowercase().ends_with(".tmp") {
        return Err(format!("asset name {name:?} can't end in .tmp"));
    }
    if named_mime(name).is_none() {
        return Err(format!("asset name {name:?} must end in {} (lowercase)", extensions()));
    }
    Ok(())
}

/// Why `data` isn't a picture we keep: one line.
pub fn not_a_picture(name: &str, data: &str) -> String {
    use base64::Engine;
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data.trim()) else {
        return format!("{name}: data isn't base64");
    };
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_lowercase();
    let head = head.trim_start();
    if head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg")) {
        return format!(
            "{name} is an SVG file, which a Doc doesn't keep: draw it with a ```svg fence in the section, or send a PNG, JPEG, GIF or WebP"
        );
    }
    format!("{name} isn't a PNG, JPEG, GIF or WebP image (the bytes say what a file is, not its name)")
}

/// The type of `bytes` if `name` agrees with it, else why not.
pub fn kind_of(name: &str, bytes: &[u8]) -> Result<&'static str, String> {
    let mime = sniff(bytes).ok_or_else(|| format!("{name} isn't a PNG, JPEG, GIF or WebP image"))?;
    if named_mime(name) != Some(mime) {
        let right = EXTENSIONS.iter().find(|(_, m)| *m == mime).map_or("png", |(e, _)| e);
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        return Err(format!("{name} holds a {} image: name it {stem}.{right}", mime.trim_start_matches("image/")));
    }
    Ok(mime)
}

/// The pictures in `dir` (the Doc's `assets/`), as a restart finds them: the
/// ones we could have written. A `.tmp` left by a crash is not one.
pub fn load(dir: &Path) -> Vec<Asset> {
    use std::io::Read;
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(name) = e.file_name().into_string() else { continue };
        if check_name(&name).is_err() {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        let mut head = [0u8; 12];
        let Ok(n) = std::fs::File::open(e.path()).and_then(|mut f| f.read(&mut head)) else { continue };
        let Some(mime) = sniff(&head[..n]) else { continue };
        if meta.is_file() && named_mime(&name) == Some(mime) {
            let rev = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            out.push(Asset { name, mime, size: meta.len(), rev: rev.map_or(1, |d| d.as_millis() as u64) });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_plain_files_with_a_picture_extension() {
        for good in ["a.png", "Chart-1_final.jpeg", "x.webp", "0.gif", "a.b.png"] {
            assert_eq!(check_name(good), Ok(()), "{good}");
        }
        let long = format!("{}.png", "a".repeat(96));
        assert_eq!(check_name(&long), Ok(()), "100 characters");
        for (bad, says) in [
            ("", "1 to 100"),
            ("../a.png", "1 to 100"),
            ("a/b.png", "1 to 100"),
            (".hidden.png", "1 to 100"),
            ("-a.png", "1 to 100"),
            ("a b.png", "1 to 100"),
            ("é.png", "1 to 100"),
            ("a.tmp", "can't end in .tmp"),
            ("a.png.TMP", "can't end in .tmp"),
            ("a.svg", "must end in .png"),
            ("a.PNG", "must end in .png"),
            ("a", "must end in .png"),
        ] {
            let e = check_name(bad).expect_err(bad);
            assert!(e.contains(says) && !e.contains('\n'), "{bad:?}: {e}");
        }
        let e = check_name(&format!("{}.png", "a".repeat(97))).unwrap_err();
        assert!(e.contains("1 to 100"), "101 characters: {e}");
    }

    #[test]
    fn the_bytes_decide_and_the_name_has_to_agree() {
        let png = b"\x89PNG\r\n\x1a\n rest";
        assert_eq!(kind_of("a.png", png), Ok("image/png"));
        assert_eq!(kind_of("a.jpg", b"\xff\xd8\xff\xe0 jfif"), Ok("image/jpeg"));
        assert_eq!(kind_of("a.jpeg", b"\xff\xd8\xff\xe0 jfif"), Ok("image/jpeg"));
        assert_eq!(kind_of("a.webp", b"RIFF\0\0\0\0WEBPVP8 "), Ok("image/webp"));
        let e = kind_of("a.jpg", png).unwrap_err();
        assert!(e.contains("holds a png image: name it a.png"), "{e}");
        assert!(kind_of("a.png", b"<html></html>").is_err());
    }

    #[test]
    fn a_svg_file_is_pointed_at_the_fence() {
        use base64::Engine;
        let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
        for svg in ["<svg xmlns=\"http://www.w3.org/2000/svg\"/>", "  \n<?xml version=\"1.0\"?><svg/>"] {
            assert!(not_a_picture("d.png", &b64(svg)).contains("```svg fence"), "{svg}");
        }
        assert!(not_a_picture("d.png", &b64("<html>")).contains("isn't a PNG"));
        assert!(not_a_picture("d.png", "!!").contains("isn't base64"));
    }
}
