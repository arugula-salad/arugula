// What every block draws when it has nothing else to (UI audit #178) lives
// in shared/ui/block-states.tsx, so the Doc renderer (shared/docs/doc-render.tsx)
// draws it on the platform page too (aru-118). The blocks import it here.
export * from "../../shared/ui/block-states";
