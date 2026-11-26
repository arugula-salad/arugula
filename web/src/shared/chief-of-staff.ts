// A chief of staff's session is named by its project's id (Arugula #1457),
// "Chief of staff · prj_…": that name routes its person's staff posts to it,
// and stays put when the project is renamed or another project shares its
// name. People read "Chief of staff · <the project's name>" instead,
// wherever the page knows the project, else the name as it is. Only a chief
// of staff's: every other session shows as it's named. Display only: match
// and route by the name itself.

const CHIEF = /^Chief of staff · (prj_[A-Za-z0-9]+)$/;

/** The project a chief of staff's session name is by, or null for any
 * other session's name. */
export function chiefProject(session: string | null | undefined): string | null {
  return (session && CHIEF.exec(session)?.[1]) || null;
}

/** A session's name as people read it: a chief of staff's by its project's
 * name, when `projectName` knows it; any other as it is. */
export function sessionLabel(session: string, projectName?: (id: string) => string | undefined): string {
  const project = chiefProject(session);
  const name = project ? projectName?.(project)?.trim() : "";
  return name ? `Chief of staff · ${name}` : session;
}
