//! The `doc` MCP tool (#963): a Doc made, listed, opened, filled, given a
//! picture and shared, by an agent. Registered into the base's tool list
//! after `show` (`arugulad_base::mcp::registry`), where it was.

use arugula_proto::{
    BlockType,
    api::{DocOpenRequest, OpenRequest},
    op::ops::{DocMove, DocsList},
};
use arugulad_base::mcp::{
    ops::{call_of, kind_of},
    registry,
    tools::{Args, Call, Def, Kind, Needs, NoArgs, Out, PaneArg, confine, done, line, parse},
};
use std::sync::Arc;

use futures_util::future::BoxFuture;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Docs,
    docs::team::{self, Cloud},
    mux::Api,
};
use arugulad_base::mcp::issues::Chosen;

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

/// How a Doc is laid out: doc, tabs or prototype.
#[derive(Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocLayoutArg {
    Doc,
    Tabs,
    Prototype,
}

impl DocLayoutArg {
    fn name(self) -> &'static str {
        match self {
            DocLayoutArg::Doc => "doc",
            DocLayoutArg::Tabs => "tabs",
            DocLayoutArg::Prototype => "prototype",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct DocNewArgs {
    /// The Doc's title.
    pub title: String,
    /// Every section, in order (at most 40), so the user sees the whole outline at once. A prototype Doc has one, made for you if you leave this out.
    #[serde(default)]
    pub sections: Vec<DocSection>,
    /// How it is laid out: doc (the default, sections one after another), tabs (sections shown one at a time) or prototype (one section holding one prototype that fills the screen).
    #[serde(default = "default_layout")]
    pub layout: DocLayoutArg,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

fn default_layout() -> DocLayoutArg {
    DocLayoutArg::Doc
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
    /// The section's body, in Markdown (at most 64 KB). Filling a section again replaces it. In a prototype Doc: one line, then one ```prototype fence.
    pub markdown: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocChecklistArgs {
    /// The Doc block (from doc new).
    pub block: PaneArg,
    /// The section that groups these tests (s1, s2, …). doc fill can add context to it without replacing results.
    pub section: String,
    /// The tests (at most 100 a Doc; bodies and checklists share a section's 64 KB). The person ticks passes or reports failures with notes and screenshots; each failure goes to the item's owner's prompt queue, at least once (a repeated report id is the same report). Calling again keeps results and can reassign future reports; a sent report keeps its recipient, and the person reports again to reach a new owner. An item with feedback can't be removed or have its id or text changed: use a new section. read_output on the Doc reads the results.
    pub items: Vec<crate::docs::checklist::Planned>,
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

/// What `doc fences` returns: the reference for the fences a section can
/// hold, kept out of `doc`'s description, which an agent sees only the
/// first 2048 characters of (#606).
const FENCES: &str = r##"A callout is a quote whose first line is [!NOTE], [!TIP], [!IMPORTANT], [!WARNING] or [!CAUTION], as on GitHub: "> [!TIP]" then "> Run it twice." Keep them for what the reader mustn't miss.

Four kinds of fenced block draw as more than code, in the section that talks about them.
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
An interactive or tunable prototype goes in a ```prototype fence (a ```html fence stays code). The fence line holds a title and height=N anywhere (120 to 2000 pixels, 480 by default), as in ```prototype Tuner height=480. It is one self-contained HTML page of at most 64 KB that runs in a sandboxed frame: inline <script> and <style>, no external src or href (so no <script src>, <link>, iframe, object, embed, base or meta http-equiv), no network, and fonts only as data: URLs. Pointer and touch work, and the viewer has a Full window button. Two calls reach out of the frame and return promises that reject with an error: arugula.copy(text) copies up to 16 KB, only from a click inside the frame; arugula.store.get(key), .set(key, value) and .all() keep settings per viewer (a key up to 128 characters, a JSON value up to 8 KB, 64 KB a section), in memory only on a shared link. Call store.all() on load to restore what the viewer set. Every prototype has a small kit in its page, so start from it: colors that follow the Doc's light or dark (--ar-bg, --ar-surface, --ar-raised, --ar-text, --ar-muted, --ar-border, --ar-accent, --ar-danger, --ar-ok, --ar-warn), sizes --ar-text-xs to --ar-text-lg, and the classes ar-btn (with ar-btn-primary, ar-btn-ghost), ar-chip (ar-chip-accent, -ok, -warn, -danger), ar-header, ar-row, ar-dialog, ar-menu with ar-menu-item, ar-empty and ar-toolbar. Fill the section again to update it: every open view changes, and a share gets the new version when the person taps Update.
Every fence is checked before anything is written: a bad chart, tile, drawing or prototype is refused with one line saying what to fix, and the section stays as it was."##;

/// `doc`'s kinds.
pub static DOC: &[Kind] = &[
    Kind {
        name: "new",
        description: "Make a Doc and its outline (each section's heading and intent). Returns the block and section ids to fill in order. Layout: tabs for many prototypes or long parts; prototype for a whole-screen one. When the current project (your block's, else your session's, else your clone's repo's, else the default) is a team's, a new Doc goes in it on Arugula; else it stays here.",
        schema: schema_for_type::<DocNewArgs>,
        flag: None,
    },
    kind_of::<DocsList, Docs>(),
    Kind {
        name: "open",
        description: "Open a Doc from list in a block beside your pane; fill it as after new. A Doc already open is that block.",
        schema: schema_for_type::<DocOpenArgs>,
        flag: None,
    },
    Kind {
        name: "fill",
        description: r##"Write one section in Markdown (no HTML). ![alt](assets/<name>) shows a picture put there by asset first. At most 64 KB a section, 40 sections, 1 MiB a Doc; filling again replaces. Callouts, ```stats, ```chart, ```svg and ```prototype (an interactive page with arugula.copy and arugula.store) draw as more than code: call doc fences for their format before writing one."##,
        schema: schema_for_type::<DocFillArgs>,
        flag: None,
    },
    Kind {
        name: "fences",
        description: "The format of callouts and fences: read it first.",
        schema: schema_for_type::<NoArgs>,
        flag: None,
    },
    Kind {
        name: "asset",
        description: r##"Put a picture in a Doc: PNG, JPEG, GIF or WebP, up to 2 MB. path is absolute; an agent on a machine of its own sends base64 data. The name ends in the file's real extension; SVG is refused. Put it before the fill that names it."##,
        schema: schema_for_type::<DocAssetArgs>,
        flag: None,
    },
    Kind {
        name: "checklist",
        description: "Tests in a section; failures go to each item's owner.",
        schema: schema_for_type::<DocChecklistArgs>,
        flag: None,
    },
    Kind {
        name: "share",
        description: r##"Ask the user to share a finished Doc by link. They decide on a card; this returns {status: "waiting"} at once. Call share again for the outcome: shared (with the url), declined, lapsed or failed (with why). Ask once."##,
        schema: schema_for_type::<DocShareArgs>,
        flag: None,
    },
    kind_of::<DocMove, Docs>(),
];

/// The `doc` tool's row.
fn def() -> Def {
    Def {
        name: "doc",
        title: "Build a document for the user",
        description: "Build a Doc, a report, plan or write-up longer than a screen, in a block beside your pane that the user watches fill in. Make the outline with new, then fill each section in order, saying one sentence about it in the pane. Never write its files by hand.",
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
            Some("list") => doc_list(call, args).await,
            Some("open") => match parse(args) {
                Ok(a) => doc_open(call, a).await,
                Err(e) => Err(e),
            },
            Some("fill") => match parse(args) {
                Ok(a) => doc_fill(call, a).await,
                Err(e) => Err(e),
            },
            Some("fences") => done(FENCES, json!({})),
            Some("checklist") => match parse(args) {
                Ok(a) => doc_checklist(call, a).await,
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
            Some("move") => call_of::<DocMove, Docs>(call, args).await,
            _ => Err(format!("doc kind {} isn't wired up", kind.unwrap_or("?"))),
        }
    })
}

/// Lists `doc` after `show`.
pub fn register() {
    registry::tool(registry::Tool { after: Some("show"), def, run });
}

/// Where a new Doc goes (aru-139): the team's project when the call's
/// context is a team's, else this machine's library. Nobody picks.
enum Home {
    Team(Chosen),
    /// On this machine; the reason is control's, when it couldn't say.
    Library(Option<String>),
    /// A team machine that can't tell its project: its Docs are the team's,
    /// so none is made (the sentence says why).
    Refused(&'static str),
}

/// Said when a team machine can't reach control to choose its project.
const TEAM_UNREACHABLE: &str =
    "Arugula cloud can't be reached, and a team machine's Docs live in the team's project: try again when it's back.";

/// The home a project choice means: a team's project is the team's, a
/// personal one is never a Doc's home (the machine is), and no answer is
/// the machine too, which says why. A team machine (known without the
/// network) never falls back to the library.
fn home_of(chosen: Result<Chosen, String>, team_machine: bool) -> Home {
    match chosen {
        Ok(c) if c.team => Home::Team(c),
        Ok(_) => Home::Library(None),
        Err(_) if team_machine => Home::Refused(TEAM_UNREACHABLE),
        Err(why) => Home::Library(Some(why)),
    }
}

/// `doc new`'s home for this call. Only the owner's own agents act as the
/// owner in a team's project.
async fn home(call: &Call<'_>) -> Home {
    if call.owners_own("a team's Doc").await.is_err() {
        return Home::Library(None);
    }
    home_of(call.chosen_project(None).await, call.app().control.is_team())
}

/// `doc new`: a Doc in the library, or the team's in its project; its block
/// beside the caller's pane, then its outline.
async fn doc_new(call: &Call<'_>, a: DocNewArgs) -> Out {
    let layout = a.layout;
    // A prototype Doc is one section: made here when none is given.
    let mut sections = a.sections;
    if layout == DocLayoutArg::Prototype {
        if sections.len() > 1 {
            return Err("a prototype Doc has one section".into());
        }
        if sections.is_empty() {
            sections.push(DocSection { heading: a.title.clone(), intent: "the prototype".into() });
        }
    }
    let (beside, host) = call.beside(a.beside.as_ref()).await?;
    let kept = match home(call).await {
        Home::Team(chosen) => return doc_new_team(call, a.title, sections, layout, beside, host, chosen).await,
        Home::Library(why) => why,
        Home::Refused(why) => return Err(why.into()),
    };
    let state = call.app().control.state_dir().to_owned();
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
    let sections: Vec<Value> = sections.iter().map(|s| json!({ "heading": s.heading, "intent": s.intent })).collect();
    let args = json!({ "title": a.title, "sections": sections, "layout": layout.name() });
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
    let home = match &kept {
        Some(why) => format!(" It is kept on this machine; control didn't name a team's project ({why})."),
        None => " It is kept on this machine.".to_owned(),
    };
    done(
        format!(
            "Doc block %{block} with {} sections ({}): fill them in order with doc fill.{home}",
            ids.len(),
            ids.join(", ")
        ),
        json!({ "block": block, "doc": meta.id, "sections": sections, "home": "machine" }),
    )
}

/// `doc new` in a team's project: made through control as the owner's agent,
/// then its block. A refusal (no `docs.write`) is control's own sentence:
/// the Doc is never kept on this machine instead.
async fn doc_new_team(
    call: &Call<'_>,
    title: String,
    sections: Vec<DocSection>,
    layout: DocLayoutArg,
    beside: Option<arugula_core::PaneId>,
    host: Option<u32>,
    chosen: Chosen,
) -> Out {
    team::title_ok(&title)?;
    if sections.is_empty() {
        return Err("a Doc needs at least one section".into());
    }
    let cloud: Arc<dyn Cloud> = call.app().control.clone();
    let plan: Vec<(String, String)> = sections.into_iter().map(|s| (s.heading, s.intent)).collect();
    let view = team::make(&*cloud, &chosen.id, &title, &plan, layout.name()).await?;
    let id = view.doc.id.clone();
    let req = OpenRequest {
        kind: BlockType::Doc,
        config: json!({ "doc": id, "project": chosen.id, "project_name": chosen.name, "title": title }),
        session: None,
        split: beside,
        from_pane: beside,
        vm: false,
        image: None,
        host,
        local: host.is_none(),
    };
    // An agent never deletes a team's Doc: if its block can't open, it is
    // there, and `doc open` shows it.
    let kept = |e: String| format!("{e}; the Doc {id} was made in {}: doc open {id} shows it", chosen.name);
    let block = call.open(req).await.map_err(&kept)?;
    let outlined = match call.app().mux.api(|r| Api::Block(block, r)).await.flatten() {
        Some(b) => b.call_by("doc_outline", json!({}), Some(&call.by())).await,
        None => Err("the Doc's block closed".into()),
    };
    let outline = match outlined {
        Ok(o) => o,
        Err(e) => {
            let _ = call.app().mux.api(|r| Api::Close(block, r)).await;
            return Err(kept(e));
        }
    };
    let sections = outline["sections"].clone();
    let ids: Vec<&str> = sections.as_array().into_iter().flatten().filter_map(|s| s["id"].as_str()).collect();
    done(
        format!(
            "{}. Doc block %{block} with {} sections ({}): fill them in order with doc fill",
            chosen.team_doc(),
            ids.len(),
            ids.join(", ")
        ),
        json!({ "block": block, "doc": id, "sections": sections, "home": "team", "project": chosen.id,
                "link": view.link }),
    )
}

/// `doc list`: the library, and the team Docs open in blocks on this
/// machine, marked with their project.
async fn doc_list(call: &Call<'_>, args: Value) -> Out {
    let mut out = call_of::<DocsList, Docs>(call, args).await?;
    let mut rows = Vec::new();
    for p in call.panes().await {
        if p.info.kind != BlockType::Doc || call.readable(p.info.id).await.is_err() {
            continue;
        }
        let Some(b) = call.app().mux.api(|r| Api::Block(p.info.id, r)).await.flatten() else { continue };
        let (config, state) = (b.config(), b.state());
        if config["project"].as_str().is_none_or(str::is_empty) {
            continue;
        }
        rows.push(json!({
            "id": config["doc"], "title": state["title"], "project": config["project"],
            "project_name": config["project_name"], "open": p.info.id, "updated": state["updated"],
            "layout": state["layout"], "link": state["team"]["link"],
        }));
    }
    if !rows.is_empty() {
        let was = out["summary"].as_str().unwrap_or_default().to_owned();
        out["summary"] = json!(format!("{was}; {} team Docs open here (team_docs, with their project)", rows.len()));
        out["team_docs"] = json!(rows);
    }
    Ok(out)
}

/// `doc open`: a Doc from the library in a block beside the caller's
/// pane, or the block that has it. The caller drives a block it made;
/// one the user (or another agent) had open stays theirs unless nobody
/// started it, which the caller then does.
async fn doc_open(call: &Call<'_>, a: DocOpenArgs) -> Out {
    let (beside, host) = call.beside(a.beside.as_ref()).await?;
    confine(call.on_machine().await?, host.is_some())?;
    let (block, made) = if a.doc.starts_with("pdc_") {
        open_team(call, &a.doc, beside, host).await?
    } else {
        // A Doc moved to a team opens as the team's, as the owner.
        let state = call.app().control.state_dir();
        if crate::docs::library::moved_to(state, &a.doc).is_some() {
            call.owners_own("a team's Doc").await?;
        }
        let req = DocOpenRequest { id: Some(a.doc.clone()), link: None, session: None, split: beside };
        crate::doc_ops::open_doc(call.app(), None, req).await.map_err(crate::ops::OpError::sentence)?
    };
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

/// A team Doc (`pdc_…`) in a block, or the block that has it: the Doc is in
/// the call's team project, read through control.
async fn open_team(
    call: &Call<'_>,
    doc: &str,
    beside: Option<arugula_core::PaneId>,
    host: Option<u32>,
) -> Result<(arugula_core::PaneId, bool), String> {
    if let Some(block) = crate::doc_ops::open_blocks(call.app()).await.get(doc) {
        return Ok((*block, false));
    }
    call.owners_own("a team's Doc").await?;
    let chosen = call.chosen_project(None).await?;
    if !chosen.team {
        return Err(format!(
            "no Doc {doc} in the library: a team Doc opens in its team's context (a repo or issue of a team project, or a team machine)"
        ));
    }
    let cloud: Arc<dyn Cloud> = call.app().control.clone();
    let view = team::read(&*cloud, &chosen.id, doc).await?;
    let req = OpenRequest {
        kind: BlockType::Doc,
        config: json!({ "doc": doc, "project": chosen.id, "project_name": chosen.name, "title": view.doc.title }),
        session: None,
        split: beside,
        from_pane: beside,
        vm: false,
        image: None,
        host,
        local: host.is_none(),
    };
    Ok((call.open(req).await?, true))
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

async fn doc_checklist(call: &Call<'_>, a: DocChecklistArgs) -> Out {
    let block = a.block.id()?;
    let p = call.drivable(block).await?;
    if p.info.kind != BlockType::Doc {
        return Err(format!("%{block} isn't a Doc"));
    }
    // Assigning a recipient creates a future route into its prompt queue.
    // Being allowed to edit the Doc alone does not grant that authority.
    for item in &a.items {
        call.drivable(item.owner.pane).await?;
    }
    let b = call.app().mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| format!("no block %{block}"))?;
    let out = b.call_by("doc_checklist", json!({ "section": a.section, "items": a.items }), Some(&call.by())).await?;
    done(format!("Checklist saved in {} of Doc %{block}; read_output reads the person's results", a.section), out)
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
        "shared" => match out["page"].as_str() {
            Some(page) => format!(
                "Doc %{block} is shared: {} (the team's page is {page})",
                out["url"].as_str().unwrap_or_default()
            ),
            None => format!("Doc %{block} is shared: {}", out["url"].as_str().unwrap_or_default()),
        },
        "team" => format!(
            "Doc %{block} is the team's: its page is {}; this control keeps no public links",
            out["url"].as_str().unwrap_or("not read yet")
        ),
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

    /// #256: `doc fences` names the Doc's colours, and its drawing, which
    /// uses them, passes the check.
    #[test]
    fn doc_fences_teaches_the_named_colours() {
        let fill = FENCES;
        for name in ["leaf", "pepper", "sky", "plum", "sun", "stone", "ink", "paper", "leaf-soft", "\"colors\""] {
            assert!(fill.contains(name), "{name}");
        }
        assert!(fill.contains("Grey first (stone and ink), one accent per figure") && fill.contains("Text stays ink"));
        let svg = fill.split("```svg Idea to app\n").nth(1).and_then(|s| s.split("\n```").next()).unwrap();
        assert_eq!(crate::docs::embeds::check_svg(svg), Ok(()));
        assert!(svg.contains("fill=\"leaf-soft\"") && !svg.contains('#'), "{svg}");
        for name in ["arugula.copy", "arugula.store", "```prototype", "```chart", "```stats"] {
            assert!(fill.contains(name), "{name}");
        }
    }

    fn chosen(team: bool, rule: arugulad_base::mcp::issues::Rule) -> Chosen {
        Chosen { id: "prj_1".into(), name: "Acme".into(), team, rule }
    }

    /// aru-139: a team's project is the Doc's home; a personal one never is
    /// (the machine is), and no answer from control is the machine too.
    #[test]
    fn a_team_projects_docs_are_the_teams_and_a_personal_projects_stay_on_the_machine() {
        use arugulad_base::mcp::issues::Rule;
        assert!(matches!(home_of(Ok(chosen(true, Rule::Personal)), false), Home::Team(_)));
        assert!(matches!(home_of(Ok(chosen(true, Rule::Linked("acme/app".into()))), false), Home::Team(_)));
        assert!(matches!(home_of(Ok(chosen(false, Rule::Personal)), false), Home::Library(None)));
        assert!(matches!(home_of(Ok(chosen(false, Rule::Linked("acme/app".into()))), false), Home::Library(None)));
    }

    /// A personal machine that can't reach control keeps its Docs on the
    /// machine, with the reason.
    #[test]
    fn a_personal_machine_without_control_keeps_the_library_and_says_why() {
        let Home::Library(Some(why)) = home_of(Err("can't reach control".into()), false) else { panic!() };
        assert_eq!(why, "can't reach control");
    }

    /// aru-139: a team machine's Docs are always the team's, so with control
    /// out of reach `new` is refused and nothing goes to the library.
    #[test]
    fn a_team_machine_without_control_refuses_new_and_never_uses_the_library() {
        let Home::Refused(why) = home_of(Err("can't reach control".into()), true) else { panic!("fell back") };
        assert_eq!(
            why,
            "Arugula cloud can't be reached, and a team machine's Docs live in the team's project: try again when it's back."
        );
    }

    /// The answer says which, in the rule's own words.
    #[test]
    fn the_answer_says_the_team_and_the_rule() {
        use arugulad_base::mcp::issues::Rule;
        assert_eq!(
            chosen(true, Rule::Linked("acme/app".into())).team_doc(),
            "A team Doc in Acme: this clone's repo acme/app is linked to it"
        );
        assert_eq!(
            chosen(true, Rule::Started("#9".into())).team_doc(),
            "A team Doc in Acme: you were started on its issue #9"
        );
        assert_eq!(
            chosen(true, Rule::Personal).team_doc(),
            "A team Doc in Acme: this is the team's machine, and no project was named"
        );
    }

    /// The tool says where Docs go, within the limit.
    #[test]
    fn the_tool_text_says_where_docs_go_and_fits() {
        let new = DOC.iter().find(|k| k.name == "new").unwrap().description;
        assert!(new.contains("When the current project (your block's, else your session's, else your clone's repo's, else the default) is a team's, a new Doc goes in it on Arugula; else it stays here."));
        for k in DOC {
            assert!(k.description.chars().count() <= 2048, "{}", k.name);
        }
    }

    /// #606: the description is cut at 2048 characters, so the fences' format
    /// is fetched with `doc fences`: `fill` has to keep naming it.
    #[test]
    fn doc_fill_points_to_the_fences() {
        let fill = DOC.iter().find(|k| k.name == "fill").unwrap().description;
        for name in ["fences", "prototype", "arugula.copy", "arugula.store"] {
            assert!(fill.contains(name), "{name}");
        }
        assert!(DOC.iter().any(|k| k.name == "fences"));
    }

    /// #828: `layout` is an enum of the three names in the generated schema,
    /// not a free string, and `sections` may be left out.
    #[test]
    fn doc_new_takes_a_layout_of_three_names() {
        let new = DOC.iter().find(|k| k.name == "new").unwrap();
        let schema = serde_json::to_value((new.schema)().as_ref()).unwrap().to_string();
        assert!(schema.contains(r#""enum":["doc","tabs","prototype"]"#), "{schema}");
        let a: DocNewArgs = serde_json::from_value(json!({ "title": "T", "layout": "prototype" })).unwrap();
        assert!(a.sections.is_empty() && a.layout == DocLayoutArg::Prototype);
        let a: DocNewArgs = serde_json::from_value(json!({ "title": "T", "sections": [] })).unwrap();
        assert!(a.layout == DocLayoutArg::Doc);
        let e = serde_json::from_value::<DocNewArgs>(json!({ "title": "T", "layout": "slides" }))
            .err()
            .unwrap()
            .to_string();
        assert!(e.contains("doc") && e.contains("tabs") && e.contains("prototype"), "{e}");
    }
}
