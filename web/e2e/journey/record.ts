// A journey's steps as a person takes them (#551), and what each cost.
//
// Every step names the prompt on screen that told the person to take it.
// A step whose prompt isn't on screen is unguided: the journey still takes
// it (as someone who already knew would), so the report shows the whole
// path, and the run fails at the end. A step that can't be done at all
// fails the run there.

import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import type { Locator, Page, TestInfo } from "@playwright/test";
import { graph, report, totals } from "./report";

/** Where reports go: local output, never committed (web/.gitignore). */
export const REPORTS = resolve(
  process.env.JOURNEY_REPORT_DIR ?? "journey-reports",
);

export type Surface = "app" | "browser" | "terminal" | "message";

export type Step = {
  id: string;
  title: string;
  actor: string;
  surface: Surface;
  /** The visible text that led here, or why there was none. */
  prompt: string;
  result: "ok" | "unguided" | "failed";
  note?: string;
  start: number;
  ms: number;
  /** Codes and fingerprints read on one screen and checked on another. */
  compared: string[];
  /** Commands typed or pasted into a shell. */
  typed: string[];
  /** Links the app opened in the person's browser. */
  opened: string[];
  shot?: string;
};

export type Spec = {
  id: string;
  title: string;
  actor: string;
  surface: Surface;
  /** Where to screenshot, after the step (a function when the step
   * opens the window it ends on). */
  page?: Page | (() => Page);
  /** What on screen tells the person to take this step; `null` when
   * nothing does (say why in `why`). */
  prompt: Locator | null;
  why?: string;
  /** A step the person starts on their own (opening the app): no prompt
   * needed, and this says what it is. */
  own?: string;
};

/** Thrown where a journey can't go on: the report is written first. */
export class Stopped extends Error {}

export class Journey {
  readonly steps: Step[] = [];
  private t0 = Date.now();
  private current?: Step;

  constructor(
    readonly name: string,
    readonly title: string,
    private info: TestInfo,
    readonly actors: string[],
  ) {}

  /** Inside a step: a code or fingerprint the person compared. */
  compared(what: string) {
    this.current?.compared.push(what);
  }

  /** Inside a step: a link the app opened in the browser. */
  opened(url: string) {
    (this.current ?? this.steps.at(-1))?.opened.push(url);
  }

  /** Inside a step: a command the person typed. */
  typed(cmd: string) {
    this.current?.typed.push(cmd);
  }

  async step<T>(spec: Spec, act: () => Promise<T>): Promise<T> {
    const s: Step = {
      id: spec.id,
      title: spec.title,
      actor: spec.actor,
      surface: spec.surface,
      prompt: "",
      result: "ok",
      start: Date.now() - this.t0,
      ms: 0,
      compared: [],
      typed: [],
      opened: [],
    };
    this.steps.push(s);
    this.current = s;
    const began = Date.now();
    if (spec.own) {
      s.prompt = `the person's own doing: ${spec.own}`;
    } else if (spec.prompt === null) {
      s.result = "unguided";
      s.prompt = spec.why ?? "nothing on screen says to do this";
    } else {
      try {
        await spec.prompt
          .first()
          .waitFor({ state: "visible", timeout: 15_000 });
        s.prompt = (await spec.prompt.first().innerText())
          .replace(/\s+/g, " ")
          .trim()
          .slice(0, 400);
      } catch {
        s.result = "unguided";
        s.prompt = `expected on screen, not there: ${spec.prompt}`;
      }
    }
    try {
      const out = await act();
      s.ms = Date.now() - began;
      await this.shoot(s, spec.page);
      return out;
    } catch (e) {
      s.ms = Date.now() - began;
      s.result = "failed";
      s.note = (e as Error).message.split("\n").slice(0, 6).join("\n");
      await this.shoot(s, spec.page);
      this.write();
      throw new Stopped(
        `${this.name} stopped at ${s.id} (${s.title}): ${s.note}`,
      );
    } finally {
      this.current = undefined;
    }
  }

  private async shoot(s: Step, at?: Page | (() => Page)) {
    let page: Page | undefined;
    try {
      page = typeof at === "function" ? at() : at;
    } catch {
      return;
    }
    if (!page || page.isClosed()) return;
    try {
      const buf = await page.screenshot({
        type: "jpeg",
        quality: 60,
        timeout: 5_000,
      });
      s.shot = buf.toString("base64");
    } catch {
      // a page mid-navigation: no picture
    }
  }

  /** The report, the graph and the data: the newest run in REPORTS, each
   * run also under REPORTS/history, and beside the test's output. */
  write() {
    const data = {
      name: this.name,
      title: this.title,
      actors: this.actors,
      at: new Date().toISOString(),
      steps: this.steps,
    };
    const files = {
      html: report(data),
      svg: graph(data),
      json: JSON.stringify(data, null, 1),
    };
    const stamp = data.at.replace(/[:.]/g, "-");
    mkdirSync(join(REPORTS, "history"), { recursive: true });
    for (const [ext, body] of Object.entries(files)) {
      writeFileSync(join(REPORTS, `${this.name}.${ext}`), body);
      writeFileSync(
        join(REPORTS, "history", `${stamp}-${this.name}.${ext}`),
        body,
      );
      writeFileSync(this.info.outputPath(`${this.name}.${ext}`), body);
    }
    return {
      json: this.info.outputPath(`${this.name}.json`),
      html: this.info.outputPath(`${this.name}.html`),
    };
  }

  /** Write the report and attach it to the test. */
  async attach() {
    const { json, html } = this.write();
    await this.info.attach(`${this.name} report`, {
      path: html,
      contentType: "text/html",
    });
    await this.info.attach(`${this.name} data`, {
      path: json,
      contentType: "application/json",
    });
    const t = totals(this.steps);
    const verdict = t.failed
      ? "stopped"
      : t.unguided
        ? `${t.unguided} unguided step(s)`
        : "led all the way";
    console.log(
      `${this.name}: ${verdict}; ${t.steps} steps, ${t.switches} switches, ${t.compared} compared. Graph: ${join(REPORTS, `${this.name}.html`)}`,
    );
  }

  /** Attach the report, and fail if any step was unguided. */
  async finish() {
    await this.attach();
    const unguided = this.steps.filter((s) => s.result === "unguided");
    if (unguided.length)
      throw new Stopped(
        `${this.name}: ${unguided.length} step(s) a newcomer isn't led to: ${unguided.map((s) => `${s.id} (${s.prompt})`).join("; ")}`,
      );
  }
}
