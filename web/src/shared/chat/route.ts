// What the chat page shows: a room (at a message), Activity or search.
// The shell reads it from `#chat…` (../../shell/ui/places), the platform
// page from its path (M150).

export interface ChatRoute {
  /** M74: a message to show (a copied link). */
  msg?: number;
  /** M75: Activity (your mentions), or search results for `q`. */
  view?: "activity" | "saved" | "search";
  q?: string;
  /** M123: a room in control's chat (`#chat=<room>`). */
  room?: string;
}
