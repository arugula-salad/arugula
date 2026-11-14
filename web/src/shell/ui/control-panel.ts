// Opening one of control's dialogs by name (the `arugula:control-panel`
// event), which control's overlay (../trust/overlay.tsx) answers. Apart
// from the trust screens, so the shell's views can offer them without
// bringing those screens along (Arugula #989: the app holds no
// shell/trust/).

export type Panel = "devices" | "add" | "add-device" | "teams" | "account" | "shared-docs";

export const openPanel = (p: Panel) => () => dispatchEvent(new CustomEvent("arugula:control-panel", { detail: p }));
