// `/p/:project/issues/:issue` (M201): one native issue, drawn by the
// issue view the shell's issue blocks draw too (../../shared/issues/view,
// M203), as the reader, with the way back to the project's list.

import { IssueView } from "../../shared/issues/view";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { mayWrite, membersOf, useIssuesOn } from "./issues";
import { Missing } from "./project";

export function IssuePage({ project, issue, msg }: { project: string; issue: string; msg?: number }) {
  const on = useIssuesOn(project);
  if ("show" in on) return on.show;
  const p = on.p;
  const list = pathOf({ page: "issues", project: p.id }, names);
  const back = { href: list, label: "Issues", onClick: (e: MouseEvent) => follow(e, list), data: "issue-back" };
  return <IssueView key={issue} class="platform-section sw-page" place={{ project: p.id, members: membersOf(p), writer: mayWrite(p) }} id={issue} msg={msg} back={back} Missing={Missing} />;
}
