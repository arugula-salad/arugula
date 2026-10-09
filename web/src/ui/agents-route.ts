// The Agents page's route (#403), in the main bundle so a menu can open it
// without loading Labs: `#agents` (the recipes) and `#agents=team` (the
// team's catalog, tasks waiting for you, grants). The page itself is in the
// Labs chunk (ui/agents-page.tsx).

export type AgentsTab = "recipes" | "team";

export function agentsRoute(): AgentsTab | null {
  if (location.hash === "#agents") return "recipes";
  if (location.hash === "#agents=team") return "team";
  return null;
}

export function openAgentsPage(tab: AgentsTab = "recipes") {
  const hash = tab === "team" ? "agents=team" : "agents";
  if (location.hash !== `#${hash}`) location.hash = hash;
}

export function closeAgentsPage() {
  history.replaceState(null, "", location.pathname + location.search);
  window.dispatchEvent(new HashChangeEvent("hashchange"));
}
