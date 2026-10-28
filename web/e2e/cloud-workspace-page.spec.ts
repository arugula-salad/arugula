// Discovery 10: a cloud workspace's page does what `arugula cloud` does to
// it, through the same routes: Edit (`cloud edit`) changes its spec, and it
// then wants a rebuild; Rebuild (`cloud rebuild`) asks for one; a secret
// (`cloud secret`) is set and removed; the project's inference key (`cloud
// key`) is set by its owner; and previews say they aren't set up here. A
// local control (team-fixture.ts) with hosted workspaces on for everyone
// against an E2B nobody answers and a secrets key, and a workspace written
// into its database: no box runs here.

import { DatabaseSync } from "node:sqlite";
import { randomBytes } from "node:crypto";
import { expect, test } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("cloud-page");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const hex = () => randomBytes(16).toString("hex");

function sql<T>(f: (db: DatabaseSync) => T): T {
  const db = new DatabaseSync(t.db);
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    return f(db);
  } finally {
    db.close();
  }
}

test("a cloud workspace's page edits, rebuilds, keeps secrets and the project's key", async ({ browser }) => {
  test.setTimeout(120_000);
  await t.start([
    ...["--platform-host", "app.localhost", "--hosted-accounts", "*", "--e2b-api-key", "e2e-nobody", "--e2b-url", "http://127.0.0.1:9"],
    ...["--cloud-secret-key", hex() + hex()],
  ]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));

  await alice.goto(`${app}/`);
  await expect(alice.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];
  const me = await alice.evaluate(async () => ((await (await fetch("/api/projects")).json()) as { me: { account: string } }).me.account);

  // A workspace whose box was built from its spec and enrolled; its lease
  // is far off, so the hosted worker leaves it be.
  const ws = `hw_${hex()}`;
  const now = Date.now();
  sql((db) =>
    db
      .prepare(
        `INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, built_hash, state, machine, lease_until, created_by, created_at, updated_at)
         VALUES (?, ?, 'dev', '{}', 'h', 'h', 'ready', ?, ?, ?, ?, ?)`,
      )
      .run(ws, project, `m${hex()}`, now + 86_400_000, me, now, now),
  );
  await alice.goto(`${app}/p/${project}/workspaces/${ws}`);
  const page = alice.locator(`[data-hosted-workspace="${ws}"]`);
  await expect(page).toBeVisible({ timeout: 15_000 });

  // Edit: a repo and the large size; then it wants a rebuild.
  await page.locator("[data-hosted-edit]").click();
  const form = alice.locator("[data-hosted-edit-form]");
  await form.locator("[data-hosted-edit-repos]").fill("https://github.com/acme/app");
  await form.locator("[data-hosted-edit-env]").fill("NPM_TOKEN");
  await form.locator('[data-hosted-edit-size="large"]').check();
  await form.locator("[data-hosted-edit-save]").click();
  await expect(form).toBeHidden();
  await expect(page.locator("[data-hosted-rebuild]")).toContainText("Wants a rebuild");
  await expect(page.locator("[data-hosted-size]")).toHaveAttribute("data-hosted-size", "large");
  const spec = JSON.parse(sql((db) => (db.prepare("SELECT spec FROM hosted_workspaces WHERE id = ?").get(ws) as { spec: string }).spec));
  expect(spec).toMatchObject({ repos: ["https://github.com/acme/app"], env: ["NPM_TOKEN"], size: "large" });

  // A secret its spec names: listed by name, then removed.
  const secrets = page.locator("[data-hosted-secrets]");
  await expect(secrets).toContainText("Its spec names NPM_TOKEN, not set yet.");
  await secrets.locator("[data-hosted-secret-name]").fill("NPM_TOKEN");
  await secrets.locator("[data-hosted-secret-value]").fill("npm_s3cret");
  await secrets.locator("[data-hosted-secret-set]").click();
  await expect(secrets.locator('[data-hosted-secret="NPM_TOKEN"]')).toBeVisible();
  await expect(secrets).not.toContainText("npm_s3cret");
  await secrets.locator('[data-hosted-secret-remove="NPM_TOKEN"]').click();
  await alice.locator("[data-confirm-remove]").click();
  await expect(secrets.locator("[data-hosted-secret]")).toHaveCount(0);

  // The project's key, by its owner: set, never shown.
  const keys = page.locator("[data-hosted-keys]");
  await expect(keys.locator('[data-hosted-key="anthropic"]')).toHaveAttribute("data-set", "no");
  await keys.locator('[data-hosted-key-set="anthropic"]').click();
  await keys.locator("[data-hosted-key-value]").fill("sk-ant-e2e-0123456789");
  await keys.locator('[data-hosted-key="anthropic"] button[type=submit]').click();
  await expect(keys.locator('[data-hosted-key="anthropic"]')).toHaveAttribute("data-set", "yes");
  await expect(keys.locator('[data-hosted-key="anthropic"]')).toContainText("set by alice");

  // Previews need a preview domain, which this control has none of.
  await expect(page.locator("[data-hosted-previews-off]")).toBeVisible();

  // Rebuild: it says what this page can't check, and asks.
  await page.locator("[data-hosted-rebuild-open]").click();
  await expect(alice.locator("[data-confirm-dialog]")).toContainText("arugula cloud rebuild dev");
  await alice.locator("[data-confirm-remove]").click();
  await expect(alice.locator("[data-confirm-dialog]")).toBeHidden();
  await expect.poll(() => sql((db) => (db.prepare("SELECT asked FROM hosted_workspaces WHERE id = ?").get(ws) as { asked: string | null }).asked)).toBe("rebuild");
});
