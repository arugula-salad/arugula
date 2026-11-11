// How a Doc is laid out (Arugula #827, #829), apart from drawing it so
// a spec can run these in plain node.

export type Layout = "doc" | "tabs" | "prototype";
/** How a tab goes in the address; see `DocBlock`'s `link`. */
export type Link = "page" | "share";

/** The layout a state names: a missing one, or one a newer machine made that
 * this build doesn't know, is `doc`. */
export const docLayout = (name: string | null | undefined): Layout => (name === "tabs" || name === "prototype" ? name : "doc");
