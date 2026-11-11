// A link into another team's room still opens it, and the page is now in that
// room's project, as the address says (Arugula #882). This says so, and takes
// them back to the project they were in, until it's dismissed.

import { useSubscribe } from "../shared/ui/hooks";
import { CloseIcon } from "../shared/ui/icons";
import { arrival, dismissArrival, onCurrentProject } from "./current-project";
import { names } from "./links";
import { go, pathOf } from "./router";
import { session } from "./session";

export function ChatArrival() {
  useSubscribe(onCurrentProject);
  useSubscribe((fn) => session.subscribe(fn));
  const a = arrival();
  const to = a && session.project(a.to);
  const from = a && session.project(a.from);
  if (!a || !to || !from) return null;
  return (
    <div class="platform-arrival" role="status" data-chat-arrival>
      <span>Opened in {to.team ? `${to.team.name} / ${to.name}` : to.name}</span>
      <button type="button" class="link" data-arrival-back onClick={() => go(pathOf({ page: "project", project: a.from }, names))}>
        Back to {from.name}
      </button>
      <button type="button" class="link" aria-label="Dismiss" title="Dismiss" data-arrival-dismiss onClick={dismissArrival}>
        <CloseIcon />
      </button>
    </div>
  );
}
