//! What the Docs list shows of a Doc without opening it (native #588): each
//! section's heading and length, how many figures it has, and the Doc's
//! opening line. [`library`](super::library) keeps this in `meta.json`, so a
//! list doesn't read every section of every Doc.
//!
//! A fenced block counts as a figure, not as words: a prototype's code would
//! otherwise read as a long essay.

use std::{path::Path, sync::OnceLock};

use arugula_proto::api::DocSectionShape;
use regex::Regex;
use serde::{Deserialize, Serialize};

const OUTLINE: &str = "doc.json";
/// How long a Doc's opening line may be on its row.
const SUMMARY_MAX: usize = 200;
/// How long one paragraph of a preview may be.
const PARAGRAPH_MAX: usize = 600;
/// The fences that draw as a picture ([`super::embeds`]).
const FIGURES: [&str; 4] = ["chart", "svg", "stats", "prototype"];

/// A Doc as its row shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    /// Its first paragraph, as plain text. Empty until it has one.
    pub summary: String,
    pub sections: Vec<DocSectionShape>,
}

#[derive(Default)]
struct Scan {
    words: u32,
    figures: u32,
    paragraphs: Vec<String>,
}

/// Is this line something other than a paragraph's text?
fn structural(line: &str) -> bool {
    line.starts_with(['#', '|', '>'])
        || line.starts_with("![")
        || line.starts_with("- ")
        || line.starts_with("* ")
        || line.starts_with("+ ")
        || line.chars().all(|c| matches!(c, '-' | '*' | '_' | ' '))
        || line.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Markdown's marks off a paragraph. `_` stays, or `snake_case` loses it.
fn plain(text: &str) -> String {
    static IMAGE: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    let image = IMAGE.get_or_init(|| Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap());
    let link = LINK.get_or_init(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").unwrap());
    let text = image.replace_all(text, "");
    let text = link.replace_all(&text, "$1");
    text.chars().filter(|c| !matches!(c, '*' | '`')).collect::<String>().trim().to_owned()
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max - 1).collect();
    format!("{}…", kept.trim_end())
}

fn scan(md: &str) -> Scan {
    let mut out = Scan::default();
    let mut fenced = false;
    let mut para: Vec<&str> = Vec::new();
    let flush = |para: &mut Vec<&str>, out: &mut Scan| {
        if !para.is_empty() {
            let text = plain(&para.join(" "));
            if !text.is_empty() {
                out.paragraphs.push(text);
            }
            para.clear();
        }
    };
    for line in md.lines() {
        let line = line.trim();
        if let Some(info) = line.strip_prefix("```") {
            if !fenced && FIGURES.contains(&info.split_whitespace().next().unwrap_or("")) {
                out.figures += 1;
            }
            fenced = !fenced;
            flush(&mut para, &mut out);
            continue;
        }
        if fenced {
            continue;
        }
        out.figures += line.matches("![").count() as u32;
        if !line.starts_with("![") {
            out.words += line.split_whitespace().count() as u32;
        }
        if line.is_empty() || structural(line) {
            flush(&mut para, &mut out);
        } else {
            para.push(line);
        }
    }
    flush(&mut para, &mut out);
    out
}

/// The sections of `doc.json`, each with its body, or `None` for one not
/// filled yet. A Doc with no outline has none.
fn bodies(dir: &Path) -> Vec<(String, Option<String>)> {
    #[derive(Deserialize)]
    struct Section {
        id: String,
        heading: String,
    }
    #[derive(Deserialize)]
    struct Outline {
        sections: Vec<Section>,
    }
    let Some(outline) = std::fs::read(dir.join(OUTLINE)).ok().and_then(|b| serde_json::from_slice::<Outline>(&b).ok())
    else {
        return vec![];
    };
    outline
        .sections
        .into_iter()
        .map(|s| {
            // `sN` and nothing else names a file.
            let named = s.id.strip_prefix('s').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            let body = named.then(|| std::fs::read_to_string(dir.join(format!("{}.md", s.id))).ok()).flatten();
            (s.heading, body)
        })
        .collect()
}

/// The Doc's shape, from its files.
pub fn of(dir: &Path) -> Shape {
    let mut shape = Shape::default();
    for (heading, body) in bodies(dir) {
        let s = scan(body.as_deref().unwrap_or(""));
        if shape.summary.is_empty()
            && let Some(first) = s.paragraphs.first()
        {
            shape.summary = cut(first, SUMMARY_MAX);
        }
        shape.sections.push(DocSectionShape { heading, words: s.words, figures: s.figures });
    }
    shape
}

/// The Doc's first `n` paragraphs, as plain text, for a preview.
pub fn opening(dir: &Path, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for (_, body) in bodies(dir) {
        for p in scan(body.as_deref().unwrap_or("")).paragraphs {
            if out.len() == n {
                return out;
            }
            out.push(cut(&p, PARAGRAPH_MAX));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(name: &str, sections: &[(&str, Option<&str>)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("arugula-doc-shape-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let outline: Vec<_> = sections
            .iter()
            .enumerate()
            .map(|(i, (h, _))| serde_json::json!({"id": format!("s{}", i + 1), "heading": h, "intent": ""}))
            .collect();
        std::fs::write(dir.join(OUTLINE), serde_json::json!({"title": "T", "sections": outline}).to_string()).unwrap();
        for (i, (_, body)) in sections.iter().enumerate() {
            if let Some(b) = body {
                std::fs::write(dir.join(format!("s{}.md", i + 1)), b).unwrap();
            }
        }
        dir
    }

    /// The trap: a prototype's code counted as prose makes a one-line Doc
    /// read as a 27-minute one.
    #[test]
    fn a_fence_is_a_figure_and_its_code_is_not_words() {
        let dir = doc(
            "fence",
            &[(
                "Try it",
                Some(
                    "Two words.\n\n```prototype Tune it\n<div>one two three four five six</div>\n```\n\n![a shot](assets/a.png)\n\n```rust\nlet not_a_figure = 1;\n```\n",
                ),
            )],
        );
        let s = of(&dir);
        assert_eq!(s.sections, [DocSectionShape { heading: "Try it".into(), words: 2, figures: 2 }]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_summary_is_the_first_paragraph_as_plain_text() {
        let dir = doc(
            "summary",
            &[
                ("Empty so far", None),
                (
                    "Tables first",
                    Some(
                        "## A heading\n\n| a | b |\n| - | - |\n\n> [!NOTE]\n> A callout.\n\n- a list\n\n1. a step\n\n---\n\n**Read** the `snake_case` [plan](https://x.test/y)\nacross two lines.\n\nThe second one.",
                    ),
                ),
            ],
        );
        let s = of(&dir);
        assert_eq!(s.summary, "Read the snake_case plan across two lines.");
        assert_eq!(s.sections.len(), 2);
        assert_eq!((s.sections[0].words, s.sections[0].figures), (0, 0), "a section not filled yet");
        assert_eq!(opening(&dir, 1), ["Read the snake_case plan across two lines."]);
        assert_eq!(opening(&dir, 9), ["Read the snake_case plan across two lines.", "The second one."]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_long_opening_is_cut_and_a_doc_with_no_outline_has_no_shape() {
        let long = "word ".repeat(100);
        let dir = doc("long", &[("One", Some(&long))]);
        let s = of(&dir);
        assert!(s.summary.ends_with('…') && s.summary.chars().count() <= SUMMARY_MAX, "{}", s.summary);
        std::fs::remove_file(dir.join(OUTLINE)).unwrap();
        assert_eq!(of(&dir), Shape::default());
        assert!(opening(&dir, 3).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// `doc.json` names the files: only `sN` may.
    #[test]
    fn an_outline_cant_name_a_file_outside_the_doc() {
        let dir = doc("path", &[("One", None)]);
        std::fs::write(dir.parent().unwrap().join("arugula-doc-shape-secret.md"), "secret words").unwrap();
        std::fs::write(
            dir.join(OUTLINE),
            serde_json::json!({"sections": [{"id": "../arugula-doc-shape-secret", "heading": "H"}]}).to_string(),
        )
        .unwrap();
        assert_eq!(of(&dir).sections[0].words, 0);
        std::fs::remove_file(dir.parent().unwrap().join("arugula-doc-shape-secret.md")).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
