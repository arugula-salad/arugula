// `/t/:team/settings`: a team's own settings (Arugula #895), under its team
// page's header (./team). For now, what was the team's in a project's
// settings: Team chat. Arugula #897 lays this page out.

import { useSubscribe } from "../../shared/ui/hooks";
import { SectionHead } from "../../shared/ui/section-head";
import { chat } from "../../shared/chat-store";
import type { Owner } from "../owners";
import { TeamChat } from "../settings/chat-alerts";

export function OwnerSettings({ owner }: { owner: Owner }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const team = owner.kind === "team" ? { id: owner.key, name: owner.name, role: owner.role } : null;
  return (
    <div class="settings-body" data-owner-settings={owner.key}>
      <SectionHead title="Settings" />
      <TeamChat team={team} />
      {(chat.on !== true || team?.role !== "owner") && <p class="dim">Nothing to set here yet.</p>}
    </div>
  );
}
