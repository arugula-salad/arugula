// Download a Doc written on the platform as Markdown (Arugula #865): its
// title, then each section's heading and body, built here in the browser
// from the Doc as the page reads it. Its pictures are left out, and the file
// says so in a closing line.

import type { ProjectDocView } from "../proto";

/** The closing line of a Doc that had pictures. */
export const PICTURES_LEFT_OUT = "_This Doc's pictures are left out of this file._";

/** A picture of the Doc's own, as a section shows it. */
const PICTURE = /!\[[^\]]*\]\(assets\/[^)]*\)/g;

/** The Doc as Markdown. A pending section is its heading alone. */
export function docMarkdown(v: Pick<ProjectDocView, "doc" | "sections" | "assets">): string {
  const parts = [`# ${v.doc.title}`];
  let pictures = (v.assets?.length ?? 0) > 0;
  for (const s of v.sections) {
    parts.push(`## ${s.heading}`);
    if (s.body === null) continue;
    const body = s.body.replace(PICTURE, () => ((pictures = true), "")).trim();
    if (body) parts.push(body);
  }
  if (pictures) parts.push(PICTURES_LEFT_OUT);
  return `${parts.join("\n\n")}\n`;
}

/** A file name for the Doc: its title, without what a file name can't hold. */
export function fileName(title: string): string {
  const name = title.replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 100);
  return `${name || "Doc"}.md`;
}

/** Save `v` as a Markdown file, from the page. */
export function saveDoc(v: Pick<ProjectDocView, "doc" | "sections" | "assets">) {
  const url = URL.createObjectURL(new Blob([docMarkdown(v)], { type: "text/markdown;charset=utf-8" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = fileName(v.doc.title);
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
