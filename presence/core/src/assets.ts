// Where the models and Wasm live (scripts/assets.sh puts them there): the
// page's own origin by default, `<base>mediapipe/` and `<base>depth/`. A
// page whose CSP takes nothing from elsewhere (Arugula control's) serves
// them itself; set the base before turning a camera on to serve them from
// somewhere else.

export const assets = { base: "/" };

export function setAssetBase(base: string) {
  assets.base = base.endsWith("/") ? base : `${base}/`;
}
