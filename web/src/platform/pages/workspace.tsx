// `/p/:project/workspaces`, `/p/:project/workspaces/:id[/:tab]` and
// `/p/:project/past/:id`: Cloud workspaces (M153), the views the shell
// draws (../cloud/workspaces), here at paths (../cloud-routes): the list
// and New Cloud workspace, a workspace's tabs, + Agent, Rebuild and Delete,
// its home with its settings and changes, a tab's transcript with its
// approvals and questions, and a past run's or closed tab's, read only.

import { useCloudPanes } from "../../shared/cloud";
import { RunDetails, PastTabDetails } from "../cloud/cloud";
import { RebuildCloudWorkspace } from "../cloud/workspace-rebuild";
import { CloudWorkspaceCrumbs, CloudWorkspaceMain, CloudWorkspaceTabStrip, DeleteCloudWorkspace } from "../cloud/workspaces";

export function WorkspacesPage({ workspace }: { workspace?: string }) {
  // Re-render on the store and the route.
  useCloudPanes();
  return (
    <section class="platform-workspace" data-platform-workspace={workspace ?? ""}>
      <nav class="platform-ws-head" aria-label="Where you are">
        <CloudWorkspaceCrumbs />
        <span class="bar-fill" />
        {workspace && <RebuildCloudWorkspace place="header" />}
        {workspace && <DeleteCloudWorkspace />}
      </nav>
      {workspace && (
        <div class="ws-tabs">
          <CloudWorkspaceTabStrip />
        </div>
      )}
      <CloudWorkspaceMain />
    </section>
  );
}

export function PastPage({ id }: { id: string }) {
  return (
    <section class="platform-workspace" data-platform-past={id}>
      {id.startsWith("cwc_") ? <PastTabDetails key={id} id={id} /> : <RunDetails key={id} id={id} />}
    </section>
  );
}
