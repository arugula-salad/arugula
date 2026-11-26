//! A chief of staff's session is named by its project's id (Arugula #1457),
//! "Chief of staff · prj_…": that name routes its person's staff posts to it,
//! and stays put when the project is renamed or another project shares its
//! name. People read "Chief of staff · <the project's name>" instead, as the
//! web's `sessionLabel` (web/src/shared/chief-of-staff.ts) shows it, once for
//! both apps. Only a chief of staff's: every other session shows as it's
//! named. Display only: match and route by the name itself.

const CHIEF: &str = "Chief of staff · ";

/// The project a chief of staff's session name is by, or None for any other
/// session's name.
pub fn chief_project(session: &str) -> Option<&str> {
    let id = session.strip_prefix(CHIEF)?;
    let hex = id.strip_prefix("prj_")?;
    (!hex.is_empty() && hex.bytes().all(|b| b.is_ascii_alphanumeric())).then_some(id)
}

/// A session's name as people read it: a chief of staff's by its project's
/// name when it's `project` (its id and name), any other as it is.
pub fn session_label(session: &str, project: Option<(&str, &str)>) -> String {
    match (chief_project(session), project) {
        (Some(id), Some((at, name))) if id == at && !name.trim().is_empty() => format!("{CHIEF}{}", name.trim()),
        _ => session.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chief_of_staff_reads_by_its_projects_name_and_falls_back_to_its_own() {
        let launch = Some(("prj_3e5d80aa", "Launch crew"));
        assert_eq!(chief_project("Chief of staff · prj_3e5d80aa"), Some("prj_3e5d80aa"));
        assert_eq!(session_label("Chief of staff · prj_3e5d80aa", launch), "Chief of staff · Launch crew");
        // Another project's, the project unknown, or a project with no name: as it is.
        assert_eq!(session_label("Chief of staff · prj_77", launch), "Chief of staff · prj_77");
        assert_eq!(session_label("Chief of staff · prj_3e5d80aa", None), "Chief of staff · prj_3e5d80aa");
        let blank = Some(("prj_3e5d80aa", "  "));
        assert_eq!(session_label("Chief of staff · prj_3e5d80aa", blank), "Chief of staff · prj_3e5d80aa");
    }

    #[test]
    fn no_other_sessions_name_changes() {
        let launch = Some(("prj_3e5d80aa", "Launch crew"));
        for other in [
            "Chief of staff",
            "Chief of staff · Launch crew",
            "My staff",
            "fix-1457",
            "Chief of staff · prj_3e5d80aa · extra",
            "chief of staff · prj_3e5d80aa",
            "Chief of staff · prj_",
        ] {
            assert_eq!(chief_project(other), None, "{other}");
            assert_eq!(session_label(other, launch), other);
        }
    }
}
