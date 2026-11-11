//! The project an `IDENT-N` with no `project` means (`forge_read`, `link`,
//! `tracker_write`, `board_write`'s `create_board`): the current project
//! when it holds a Plane connection with that identifier, else the one
//! project among those this machine's grants reach that does. Several
//! match: it says which. None does: it names where it looked and the
//! identifiers it could have used.

use arugula_control_wire::{
    boards::IssueRef,
    projects::{AGENT_TRACKER_HOMES, TrackerHome, TrackerHomes},
};
use std::future::Future;

use reqwest::Method;

use crate::mcp::tools::Call;

/// The identifiers of the `IDENT-N` references among `items`, in order.
pub(crate) fn idents<'a>(items: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for i in items {
        if let Some(IssueRef::Plane { key, .. }) = IssueRef::parse(i)
            && !out.contains(&key)
        {
            out.push(key);
        }
    }
    out
}

/// The project that holds `ident`: `Ok(None)` when it's `current` (keep it).
pub(crate) fn pick(current: (&str, &str), homes: &[TrackerHome], ident: &str) -> Result<Option<String>, String> {
    let holds = |h: &&TrackerHome| h.identifiers.iter().any(|i| i.eq_ignore_ascii_case(ident));
    if homes.iter().filter(holds).any(|h| h.id == current.0) {
        return Ok(None);
    }
    let found: Vec<&TrackerHome> = homes.iter().filter(holds).collect();
    match found.as_slice() {
        [one] => Ok(Some(one.id.clone())),
        [] => {
            let mut known: Vec<&str> = homes.iter().flat_map(|h| h.identifiers.iter().map(String::as_str)).collect();
            known.sort_unstable();
            known.dedup();
            let list = if known.is_empty() {
                "none of the projects this machine reaches has a Plane connection".to_owned()
            } else {
                format!("connected identifiers: {}", known.join(", "))
            };
            Err(format!(
                "no Plane connection with identifier {ident} in {} ({}), the current project, nor in the others this \
                 machine reaches ({list}); pass project (a prj_ id) to look in another",
                current.1, current.0
            ))
        }
        many => {
            let names: Vec<String> = many.iter().map(|h| format!("{} ({})", h.name, h.id)).collect();
            Err(format!("{ident} is connected in more than one project ({}): pass project", names.join(", ")))
        }
    }
}

/// The project for a call that names `items` and no `project`: the one that
/// holds their Plane identifiers, or `None` to keep the current one (no
/// `IDENT-N` among them, or it's the current project's).
pub(crate) async fn home(c: &Call<'_>, asked: Option<&str>, items: &[&str]) -> Result<Option<String>, String> {
    if asked.is_some_and(|p| !p.trim().is_empty()) {
        return Ok(None);
    }
    let want = idents(items.iter().copied());
    if want.is_empty() {
        return Ok(None);
    }
    let current = c.chosen_project(None).await?;
    let path = format!("{AGENT_TRACKER_HOMES}?also={}", current.id);
    let v = c.app().control.cloud(Method::GET, &path, None).await.map_err(|e| e.to_string())?;
    let homes: TrackerHomes =
        serde_json::from_value(v).map_err(|e| format!("Arugula control sent an answer I can't read: {e}"))?;
    let name = if current.name.is_empty() { current.id.as_str() } else { current.name.as_str() };
    let mut chosen: Option<String> = None;
    for ident in &want {
        if let Some(p) = pick((&current.id, name), &homes.projects, ident)? {
            if chosen.as_ref().is_some_and(|c| *c != p) {
                return Err(format!("{} are connected in different projects: pass project", want.join(" and ")));
            }
            chosen = Some(p);
        }
    }
    Ok(chosen)
}

/// `home`'s project, else the main rule's (`Call::chosen_project`): `home`
/// is `None` when the current project holds the identifier, or there is none.
pub(crate) async fn home_or_main(c: &Call<'_>, asked: Option<&str>, items: &[&str]) -> Result<String, String> {
    or_main(home(c, asked, items).await?, async { Ok(c.chosen_project(asked).await?.id) }).await
}

/// `home`, or what `main` resolves to; `main` only runs on a miss.
async fn or_main(home: Option<String>, main: impl Future<Output = Result<String, String>>) -> Result<String, String> {
    match home {
        Some(p) => Ok(p),
        None => main.await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Plane-home miss sends the main rule's project, not none; a hit
    /// never asks the main rule.
    #[tokio::test]
    async fn a_miss_falls_to_the_main_rule() {
        assert_eq!(or_main(None, async { Ok("prj_main".to_owned()) }).await, Ok("prj_main".to_owned()));
        assert_eq!(
            or_main(Some("prj_home".into()), async { Err("asked".to_owned()) }).await,
            Ok("prj_home".to_owned())
        );
        assert_eq!(or_main(None, async { Err("no project".to_owned()) }).await, Err("no project".to_owned()));
    }

    fn home(id: &str, name: &str, idents: &[&str]) -> TrackerHome {
        TrackerHome { id: id.into(), name: name.into(), identifiers: idents.iter().map(|s| s.to_string()).collect() }
    }

    /// A default project without the connection finds the one project that
    /// has it; the default's own is kept; several are named; none names
    /// where it looked and what it could have used.
    #[test]
    fn an_ident_finds_the_project_that_holds_it() {
        let homes = [home("prj_all", "Allgood", &["ALL"]), home("prj_two", "Two", &["ARUGU", "OPS"])];
        let default = ("prj_me", "Personal");
        assert_eq!(pick(default, &homes, "all").unwrap(), Some("prj_all".into()));
        assert_eq!(pick(("prj_all", "Allgood"), &homes, "ALL").unwrap(), None);
        let both = [home("prj_a", "A", &["ALL"]), home("prj_b", "B", &["ALL"])];
        let e = pick(default, &both, "ALL").unwrap_err();
        assert!(e.contains("A (prj_a)") && e.contains("B (prj_b)"), "{e}");
        let e = pick(default, &homes, "NOPE").unwrap_err();
        assert!(e.contains("Personal (prj_me)") && e.contains("ALL, ARUGU, OPS"), "{e}");
        assert!(pick(default, &[], "ALL").unwrap_err().contains("none of the projects"));
    }

    #[test]
    fn only_plane_refs_have_identifiers() {
        assert_eq!(idents(["#4", "all-7", "ALL-8", "o/r#1", "iss_1", "OPS-2"]), ["ALL", "OPS"]);
    }
}
