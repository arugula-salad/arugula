// A chief of staff's session is named by its project's id (Arugula #1457),
// "Chief of staff · prj_…": that name routes its person's staff posts to it,
// and stays put when the project is renamed or another project shares its
// name. Where a view can mix projects, people read "Chief of staff · <the
// project's name>", else "Chief of staff", never the id; a project's own
// pages say "Chief of staff" alone (shownSession, Arugula #1561). Only a
// chief of staff's: every other session shows as it's named. Display only:
// match and route by the name itself.

const CHIEF = /^Chief of staff · (prj_[A-Za-z0-9]+)$/;

/** The project a chief of staff's session name is by, or null for any
 * other session's name. */
export function chiefProject(session: string | null | undefined): string | null {
  return (session && CHIEF.exec(session)?.[1]) || null;
}

// Its name, its block's title by the project's name and their slugs
// (`chief-of-staff-prj-…`, `chief-of-staff-arugula`).
const CHIEF_SHOWN = /^(?:Chief of staff · .+|chief-of-staff(?:-.+)?)$/;

/** A session's name (or its slug, or its title) where the page is already
 * the project's (Arugula #1561): a chief of staff's is "Chief of staff",
 * any other as it is. */
export function shownSession(name: string): string {
  return CHIEF_SHOWN.test(name) ? "Chief of staff" : name;
}

/** A session's name as people read it where projects mix: a chief of
 * staff's by its project's name, when `projectName` knows it, else "Chief
 * of staff"; any other as it is. */
export function sessionLabel(session: string, projectName?: (id: string) => string | undefined): string {
  const project = chiefProject(session);
  if (!project) return session;
  const name = projectName?.(project)?.trim();
  return name ? `Chief of staff · ${name}` : "Chief of staff";
}
