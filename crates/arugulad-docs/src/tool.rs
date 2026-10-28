//! The `doc` MCP tool (#963): a Doc made, listed, opened, filled, given a
//! picture and shared, by an agent. Registered into the base's tool list
//! after `show` (`arugulad_base::mcp::registry`), where it was.

use arugula_proto::{
    BlockType,
    api::{DocOpenRequest, OpenRequest},
    op::ops::DocsList,
};
use arugulad_base::mcp::{
    ops::{call_of, kind_of},
    registry,
    tools::{Args, Call, Def, Kind, Needs, Out, PaneArg, confine, done, line, parse},
};
use futures_util::future::BoxFuture;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Docs, mux::Api};

#[derive(Deserialize, JsonSchema)]
pub struct DocShareArgs {
    /// The Doc block (from doc new).
    pub block: PaneArg,
}

/// One section of a Doc's outline.
#[derive(Deserialize, JsonSchema)]
pub struct DocSection {
    /// The section's heading.
    pub heading: String,
    /// One line on what the section will hold: the user reads it while the section is pending.
    #[serde(default)]
    pub intent: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocNewArgs {
    /// The Doc's title.
    pub title: String,
    /// Every section, in order (at most 40), so the user sees the whole outline at once.
    pub sections: Vec<DocSection>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocOpenArgs {
    /// The Doc's id (doc list has them).
    pub doc: String,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocFillArgs {
    /// The Doc block (from doc new).
    pub block: PaneArg,
    /// The section's id (s1, s2, … from doc new).
    pub section: String,
    /// The section's body, in Markdown (at most 64 KB). Filling a section again replaces it.
    pub markdown: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocAssetArgs {
    /// The Doc block (from doc new).
    pub block: PaneArg,
    /// The picture's name in the Doc: letters, digits, _ . and -, ending in .png, .jpg, .jpeg, .gif or .webp (the bytes must be that type). A name already used is replaced.
    pub name: String,
    /// A PNG, JPEG, GIF or WebP file you can read, as an absolute path (at most 2 MB). An agent on a machine of its own sends data instead.
    #[serde(default)]
    pub path: Option<String>,
    /// Or the file itself, base64 (at most 2 MB).
    #[serde(default)]
    pub data: Option<String>,
}

/// `doc`'s kinds.
pub static DOC: &[Kind] = &[
    Kind {
        name: "new",
        description: "A Doc: a block beside your pane that holds a report, plan or write-up, with its outline (every section's heading and intent) shown at once. Returns the block and the section ids (s1, s2, …); fill them next, in order.",
        schema: schema_for_type::<DocNewArgs>,
        flag: None,
    },
    kind_of::<DocsList, Docs>(),
    Kind {
        name: "open",
        description: "Open a Doc from doc list in a block beside your pane, to read or to go on with: returns the block, and you can doc fill it, as after doc new (the sections are the outline's, s1, s2, …; its state has them). A Doc already open in a block is that block, not a second one: the user may have it open beside another pane.",
        schema: schema_for_type::<DocOpenArgs>,
        flag: None,
    },
    Kind {
        name: "fill",
        description: r##"Write one section of a Doc (its block, the section id, its Markdown): headings, paragraphs, lists, **bold**, *italic*, `code`, links and tables. No HTML. A picture is ![alt](assets/<name>) after doc asset has put it there. A section is at most 64 KB, a Doc 40 sections and 1 MiB; filling a section again replaces it.

A callout is a quote whose first line is [!NOTE], [!TIP], [!IMPORTANT], [!WARNING] or [!CAUTION], as on GitHub: "> [!TIP]" then "> Run it twice." Keep them for what the reader mustn't miss.

Three kinds of fenced block draw as more than code, in the section that talks about them.
Stat tiles: a ```stats fence holding a list of 1 to 6 tiles; label and value are text, delta and its tone (up, down or flat) are optional. For example:
```stats
[{"label": "Sessions", "value": "1,284", "delta": "+12%", "tone": "up"}, {"label": "Errors", "value": "3"}]
```
A chart: a ```chart fence holding a JSON spec. type is "bar" or "line"; title is optional; x is the key of each row that labels it; y is a key, or a list of up to 4 keys, whose values are numbers (not strings); rows is 1 to 200 objects. A line chart's x must go up: numbers, ISO dates (2026-01-31), or labels listed in order with "order": [...]. For example:
```chart
{"type": "bar", "title": "Sales by month", "x": "month", "y": ["sales"], "rows": [{"month": "Jan", "sales": 12}, {"month": "Feb", "sales": 19}]}
```
A drawing: a ```svg fence holding one <svg> element, with xmlns="http://www.w3.org/2000/svg" on it and a viewBox. Any text after svg on the fence line is its alt text. It is shown as a picture, so scripts in it don't run.
Colours are named, so a Doc reads in light and dark: leaf, pepper, sky, plum, sun, stone, ink (text) and paper (the page), each with a soft fill (leaf-soft, stone-soft, …). In a drawing, fill, stroke, stop-color and color take a name; a chart's optional "colors" names one of the six hues for each y series ("colors": ["leaf", "stone"]).
Grey first (stone and ink), one accent per figure. Text stays ink. For example:
```svg Idea to app
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 40" font-size="11" text-anchor="middle"><rect x="2" y="8" width="60" height="24" rx="4" fill="stone-soft" stroke="stone"/><text x="32" y="24" fill="ink">Idea</text><path d="M62 20 H98" stroke="ink"/><rect x="98" y="8" width="60" height="24" rx="4" fill="leaf-soft" stroke="leaf"/><text x="128" y="24" fill="ink">App</text></svg>
```
Every fence is checked before anything is written: a bad chart, tile or drawing is refused with one line saying what to fix, and the section stays as it was."##,
        schema: schema_for_type::<DocFillArgs>,
        flag: None,
    },
    Kind {
        name: "asset",
        description: r##"Put a picture in a Doc (its block, a name, and one of path or data): a PNG, JPEG, GIF or WebP, at most 2 MB, at most 20 per Doc. path is an absolute path to a file you saved; an agent on a machine of its own sends data (base64) instead. What the bytes are decides the type, so the name has to end in the right extension (.png, .jpg, .jpeg, .gif, .webp); an SVG file is refused: draw it with a ```svg fence. Returns assets/<name>; show it in a section with ![what it shows](assets/<name>). Put the asset before the doc fill that names it: a fill naming an asset the Doc doesn't have is refused. Only assets/ images show; an http:// or ../ one is refused. A name already used is replaced. For example: {"kind": "asset", "block": 7, "name": "flow.png", "path": "/tmp/flow.png"}."##,
        schema: schema_for_type::<DocAssetArgs>,
        flag: None,
    },
    Kind {
        name: "share",
        description: r##"Ask the user to share a finished Doc (its block) with a link anyone can open, without an account. A card asks them on the Doc and nothing is published until they tap Share; this returns {status: "waiting"} at once. Call share again on the same block to learn the outcome: shared (with the url, which you can give the user), declined, lapsed (a card nobody answered in a day) or failed (with why). A Doc already shared answers shared. Asking again after a decline raises a new card. Don't ask more than once for a Doc unless the user wants it."##,
        schema: schema_for_type::<DocShareArgs>,
        flag: None,
    },
];

/// The `doc` tool's row.
fn def() -> Def {
    Def {
        name: "doc",
        title: "Build a document for the user",
        description: "Build a Doc, a report, plan or write-up longer than a screen, in a block beside your pane that the user (and anyone the session is shared with) watches fill in. Make the outline first with new, so they see every section at once; then fill each section in order and say one sentence about it in the pane. Docs are kept: to go on with an earlier one, doc list and then doc open. Never write the Doc's files by hand.",
        args: Args::Kinds { kinds: DOC.iter().collect(), default: None },
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: false,
        flag: None,
        output: None,
        human: line(
            "write a Doc you watch fill in: a report, a plan, a comparison",
            "Write me a Doc comparing the two approaches, with a chart.",
            Needs::Nothing,
        ),
    }
}

/// A call to `doc`, by its kind.
fn run<'a>(call: &'a Call<'a>, kind: Option<&'static str>, args: Value) -> BoxFuture<'a, Out> {
    Box::pin(async move {
        match kind {
            Some("new") => match parse(args) {
                Ok(a) => doc_new(call, a).await,
                Err(e) => Err(e),
            },
            Some("list") => call_of::<DocsList, Docs>(call, args).await,
            Some("open") => match parse(args) {
                Ok(a) => doc_open(call, a).await,
                Err(e) => Err(e),
            },
            Some("fill") => match parse(args) {
                Ok(a) => doc_fill(call, a).await,
                Err(e) => Err(e),
            },
            Some("asset") => match parse(args) {
                Ok(a) => doc_asset(call, a).await,
                Err(e) => Err(e),
            },
            Some("share") => match parse(args) {
                Ok(a) => doc_share(call, a).await,
                Err(e) => Err(e),
            },
            _ => Err(format!("doc kind {} isn't wired up", kind.unwrap_or("?"))),
        }
    })
}

/// Lists `doc` after `show`.
pub fn register() {
    registry::tool(registry::Tool { after: Some("show"), def, run });
}

/// `doc new`: a Doc in the library, its block beside the caller's pane,
/// then its outline.
async fn doc_new(call: &Call<'_>, a: DocNewArgs) -> Out {
    let state = call.app().control.state_dir().to_owned();
    let (beside, host) = call.beside(a.beside.as_ref()).await?;
    // Two doc new make two Docs: each is made here, before its block.
    let by = call.by();
    let title = a.title.clone();
    let meta = {
        let state = state.clone();
        crate::task::spawn_blocking(move || crate::docs::library::make(&state, &title, &by))
            .await
            .map_err(|e| e.to_string())??
    };
    // A Doc with no outline is nothing to keep: undone if it fails.
    let undo = || {
        let (state, id) = (state.clone(), meta.id.clone());
        let _ = crate::docs::library::remove(&state, &id);
    };
    let req = OpenRequest {
        kind: BlockType::Doc,
        config: json!({ "doc": meta.id, "title": a.title }),
        session: None,
        split: beside,
        from_pane: beside,
        vm: false,
        image: None,
        host,
        local: host.is_none(),
    };
    // Through `open`, which records who started it: an agent block may
    // drive only what it started, so it could not fill its own Doc
    // otherwise.
    let block = match call.open(req).await {
        Ok(b) => b,
        Err(e) => {
            undo();
            return Err(e);
        }
    };
    let sections: Vec<Value> = a.sections.iter().map(|s| json!({ "heading": s.heading, "intent": s.intent })).collect();
    let args = json!({ "title": a.title, "sections": sections });
    let outlined = match call.app().mux.api(|r| Api::Block(block, r)).await.flatten() {
        Some(b) => b.call_by("doc_outline", args, Some(&call.by())).await,
        None => Err("the Doc's block closed".into()),
    };
    let outline = match outlined {
        Ok(o) => o,
        Err(e) => {
            let _ = call.app().mux.api(|r| Api::Close(block, r)).await;
            undo();
            return Err(e);
        }
    };
    let sections = outline["sections"].clone();
    let ids: Vec<&str> = sections.as_array().into_iter().flatten().filter_map(|s| s["id"].as_str()).collect();
    done(
        format!(
            "Doc block %{block} with {} sections ({}): fill them in order with doc fill",
            ids.len(),
            ids.join(", ")
        ),
        json!({ "block": block, "doc": meta.id, "sections": sections }),
    )
}

/// `doc open`: a Doc from the library in a block beside the caller's
/// pane, or the block that has it. The caller drives a block it made;
/// one the user (or another agent) had open stays theirs unless nobody
/// started it, which the caller then does.
async fn doc_open(call: &Call<'_>, a: DocOpenArgs) -> Out {
    let (beside, host) = call.beside(a.beside.as_ref()).await?;
    confine(call.on_machine().await?, host.is_some())?;
    let req = DocOpenRequest { id: Some(a.doc.clone()), link: None, session: None, split: beside };
    let (block, made) = crate::doc_ops::open_doc(call.app(), None, req).await.map_err(crate::ops::OpError::sentence)?;
    let unclaimed = match call.app().mux.api(Api::Panes).await {
        Some(panes) => panes.iter().find(|p| p.info.id == block).is_some_and(|p| p.info.started_by.is_none()),
        None => false,
    };
    if made || unclaimed {
        call.started(block).await;
    }
    let b = call.app().mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| format!("no block %{block}"))?;
    let state = b.state();
    let sections: Vec<&str> =
        state["entries"].as_array().into_iter().flatten().filter_map(|e| e["id"].as_str()).collect();
    let what = if made { "Opened" } else { "It was already open:" };
    done(
        format!("{what} Doc {} in block %{block} ({} sections); doc fill goes on with it", a.doc, sections.len()),
        json!({ "block": block, "doc": a.doc, "title": state["title"], "sections": sections }),
    )
}

/// `doc fill`: one section of a Doc this caller may drive.
async fn doc_fill(call: &Call<'_>, a: DocFillArgs) -> Out {
    let block = a.block.id()?;
    let p = call.drivable(block).await?;
    if p.info.kind != BlockType::Doc {
        return Err(format!("%{block} isn't a Doc: doc new makes one"));
    }
    let b = call.app().mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| format!("no block %{block}"))?;
    let args = json!({ "section": a.section, "markdown": a.markdown });
    let out = b.call_by("doc_fill", args, Some(&call.by())).await?;
    done(
        format!("Filled {} of Doc %{block} ({} bytes)", a.section, out["bytes"]),
        json!({ "block": block, "section": a.section, "bytes": out["bytes"] }),
    )
}

/// `doc share`: ask the owner to share a Doc this caller may drive, or
/// say what became of the ask. It never publishes: the owner's tap does.
async fn doc_share(call: &Call<'_>, a: DocShareArgs) -> Out {
    let block = a.block.id()?;
    let p = call.drivable(block).await?;
    if p.info.kind != BlockType::Doc {
        return Err(format!("%{block} isn't a Doc: doc new makes one"));
    }
    let b = call.app().mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| format!("no block %{block}"))?;
    let out = b.call_by("doc_share_ask", json!({}), Some(&call.by())).await?;
    let status = out["status"].as_str().unwrap_or("waiting").to_owned();
    let summary = match status.as_str() {
        "shared" => format!("Doc %{block} is shared: {}", out["url"].as_str().unwrap_or_default()),
        "waiting" => format!("Asked the user to share Doc %{block}; call share again to see what they chose"),
        "failed" => {
            format!("Sharing Doc %{block} failed: {}", out["why"].as_str().unwrap_or("it didn't go through"))
        }
        "declined" => format!("The user chose not to share Doc %{block} now; sharing again asks again"),
        _ => format!("Nobody answered the ask to share Doc %{block} in time; sharing again asks again"),
    };
    done(summary, out)
}

/// `doc asset`: a picture into a Doc this caller may drive, from a file
/// of the caller's or from base64.
async fn doc_asset(call: &Call<'_>, a: DocAssetArgs) -> Out {
    use base64::Engine;
    let block = a.block.id()?;
    let p = call.drivable(block).await?;
    if p.info.kind != BlockType::Doc {
        return Err(format!("%{block} isn't a Doc: doc new makes one"));
    }
    let cap = crate::docs::ASSET_MAX as u64;
    let data = match (a.path, a.data) {
        (Some(path), None) => {
            // As `attach` reads one: its paths are on its machine, not here.
            if let Some(me) = call.me()
                && call.app().mux.api(|r| Api::MachineOf(me, r)).await.flatten().is_some()
            {
                return Err("this agent runs on a machine of its own: send the file as data".into());
            }
            // The daemon's working directory is not the agent's.
            if !std::path::Path::new(&path).is_absolute() {
                return Err(format!("{path}: give an absolute path, or send data"));
            }
            let size = tokio::fs::metadata(&path).await.map_err(|e| format!("{path}: {e}"))?.len();
            if size > cap {
                return Err(format!("{path} is {size} bytes; a picture is {cap} (2 MB) at most: shrink it"));
            }
            let bytes = tokio::fs::read(&path).await.map_err(|e| format!("{path}: {e}"))?;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }
        (None, Some(data)) => data,
        _ => return Err("give path or data".into()),
    };
    let b = call.app().mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| format!("no block %{block}"))?;
    let out = b.call_by("doc_asset_put", json!({ "name": a.name, "data": data }), Some(&call.by())).await?;
    let path = out["path"].as_str().unwrap_or_default().to_owned();
    done(
        format!("Saved {path} in Doc %{block}: show it with ![what it shows]({path}) in a section"),
        json!({ "block": block, "path": path, "mime": out["mime"], "size": out["size"] }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #256: `doc fill`'s guide names the Doc's colours, and its drawing,
    /// which uses them, passes the check.
    #[test]
    fn doc_fill_teaches_the_named_colours() {
        let fill = DOC.iter().find(|k| k.name == "fill").unwrap().description;
        for name in ["leaf", "pepper", "sky", "plum", "sun", "stone", "ink", "paper", "leaf-soft", "\"colors\""] {
            assert!(fill.contains(name), "{name}");
        }
        assert!(fill.contains("Grey first (stone and ink), one accent per figure") && fill.contains("Text stays ink"));
        let svg = fill.split("```svg Idea to app\n").nth(1).and_then(|s| s.split("\n```").next()).unwrap();
        assert_eq!(crate::docs::embeds::check_svg(svg), Ok(()));
        assert!(svg.contains("fill=\"leaf-soft\"") && !svg.contains('#'), "{svg}");
    }
}
