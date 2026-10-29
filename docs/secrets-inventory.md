# Secrets inventory

Every secret store and every key across the arugula-salad org, by **name and
owner only**. No value, digest or fingerprint is written here, and none may
be. Taken 2026-10-23 for step 8 of the platform roadmap (M227, Arugula #25).

Step 8 leaves one store for a project's secrets: the platform's. Infisical
stays the operator's store and the layer that puts secrets into Kubernetes;
the platform writes it only as a projection. So each key below has one of
three fates:

- **Platform:** moves into a project's secrets on the platform, sealed with
  that project's own key (M209; [control-ops.md](control-ops.md#secrets-keys-m209)).
- **Operator:** stays the operator's (Jake's), in Infisical, with whatever
  runtime copy it has today (a Fly secret, a GitHub secret, a Kubernetes
  Secret).
- **Deleted:** goes with its product, at the issue named; or **now**, when
  nothing reads it.

A few keys belong to a box or a person rather than the org; they say so.
Non-secret settings kept in a secret store are marked *config*: they are
listed because they're there, and can move to plain environment later.

How it was taken: `fly secrets list` (names only), `gh secret list` and
`gh variable list`, Infisical's folder listing and key names, and reading
each product's code and manifests for the names it reads. What couldn't be
listed is at the end.

## The stores, and who can read them

| Store | What's in it | Who can read it |
| --- | --- | --- |
| Fly secrets, org `personal` | The hub (`illogical-control`) and the platform (`arugula-platform`); `arugula-deploy-test` has none | Jake, the Fly org's only member; and whoever holds a deploy token for the app, since a deploy token can run commands in its machines. The two deploy tokens are GitHub secrets on `arugula` (below) |
| GitHub Actions secrets | `arugula`, `studio`, `arcade`, `experimental`; no other repo has any, and no environment has its own | Nobody reads them back through the API. A workflow run reads them, so anyone who can push a branch with a workflow can: the org's three members, all admins (`jhgaylor`, `alecraso`, `lex00`). Pull requests from forks get none |
| Infisical (home-cloud's own instance) | Projects `arugula`, `arugula-flux`, `github-runners`, `planter`, `planter-apps`, `redone`, `beds`, `studio`, `arcade`, `loam`, `sandpit-substrate` | Jake's account. Each project has a viewer machine identity whose Kubernetes auth accepts one service account; the operator renders a Kubernetes Secret from it, which that namespace's pods can mount. Writers are named per project below |
| The platform's database | Projects' secrets, sealed with each project's data key; the data keys, wrapped by the master key | The platform process, holding the master key (`ARUGULA_CLOUD_SECRET_KEY`). Backups (Litestream, the nightly dump) hold ciphertext and never the key |
| The hub's database | Control's VAPID key pair and its hosted-machine signing key (`settings`) | The hub process; Litestream's copy |
| Apps' own volumes | planter's `/data`, redone's `state.json`, studio's and beds' lobby `/state` | The app's pod, and whoever can open a shell in its namespace. Sealed fields need the app's own key as well |
| E2B's secrets broker | The platform's inference keys for hosted boxes | Written with the platform's E2B key; a box's egress proxy uses them and never answers them back |
| Managoat (Fountain) vaults | Per-box secrets planter, hud and experimental wrote there | Holders of the Managoat key that wrote them, and the box |
| The operator's workstation | Copies in `~/.config` on Jake's machine, each `0600` | Jake's user, and agents that run as it |

## arugula: the hub (Fly `illogical-control`)

| Name | Copies | Fate |
| --- | --- | --- |
| `GITHUB_APP_ID`, `GITHUB_APP_SLUG` (*config*) | the platform; workstation | Operator |
| `GITHUB_APP_CLIENT_ID` (*config*), `GITHUB_APP_CLIENT_SECRET` | workstation only: **no Infisical copy** | Operator; escrow the secret in Infisical |
| `GITHUB_APP_PRIVATE_KEY` | the platform; Infisical `/illogical/control` as `GITHUB_APP_PRIVATE_KEY_2026_10_22` | Operator. The hub uses it only with the platform in process, so it's a rollback copy |
| `GITHUB_APP_WEBHOOK_SECRET` | the platform; workstation: **no Infisical copy** | Operator; rollback copy, as above; escrow it |
| `LITESTREAM_ACCESS_KEY_ID`, `LITESTREAM_SECRET_ACCESS_KEY`, `LITESTREAM_BUCKET` (*config*), `LITESTREAM_ENDPOINT` (*config*) | the platform; Infisical `/illogical/control` | Operator |
| `ARUGULA_PLATFORM_KEY` | the platform; Infisical `/illogical/control` | Operator |
| `POSTHOG_API_KEY` (a public project key) | the platform | Operator |
| `ARUGULA_PLATFORM`, `ARUGULA_PLATFORM_HOST`, `ARUGULA_PREVIEW_DOMAIN` (*config*) | | Operator |
| `ARUGULA_PRESENCE_APP`, `ARUGULA_PRESENCE_RELAY` (*config*; People blocks, **not set yet**) | the platform | Operator. The hub uses them for its CSP alone |
| The hub database's `settings`: VAPID key pair, hosted-machine signing key | Litestream only | Operator. M228's issuer keys will sit beside them; no escrow today |

Unset from the hub on 2026-10-24 with the platform's old copies (Arugula #71):
`ARUGULA_CLOUD_SECRET_KEY` (the platform's has the same digest),
`ARUGULA_CLOUD_ACCOUNTS` (a stale copy; the platform has its own) and the
retired run caps `ARUGULA_CLOUD_CONCURRENT_RUNS`, `ARUGULA_CLOUD_DAILY_RUNS`
and `ARUGULA_CLOUD_RUN_MINUTES`.

The hub's sign-in tables hold no secret it can use: sessions keep a hash of
their token, passkeys and devices keep public keys. They stay the hub's.

## arugula: the platform (Fly `arugula-platform`)

| Name | Copies | Fate |
| --- | --- | --- |
| `ARUGULA_CLOUD_SECRET_KEY` (the master key; `_NEXT` and `_PREVIOUS` only while rotating) | the hub; Infisical `/illogical/control` | Operator |
| `ARUGULA_PLATFORM_KEY` | the hub; Infisical | Operator |
| `ARUGULA_PLATFORM_DATABASE_URL` | Infisical `/illogical/control` | Operator |
| `LITESTREAM_*` (four, as on the hub) | the hub; Infisical | Operator |
| `GITHUB_APP_ID`, `GITHUB_APP_SLUG` (*config*), `GITHUB_APP_PRIVATE_KEY`, `GITHUB_APP_WEBHOOK_SECRET` | the hub; Infisical (the key only) | Operator |
| `ARUGULA_RUN_LOGS_ACCESS_KEY_ID`, `ARUGULA_RUN_LOGS_SECRET_ACCESS_KEY` | Infisical `/illogical/run-logs` | Operator |
| `ARUGULA_RUN_LOGS_BUCKET`, `ARUGULA_RUN_LOGS_ENDPOINT` (*config*) | | Operator |
| `ARUGULA_PRESENCE_SECRET` (People blocks: the arugula.stream app's server secret, which signs room tokens; **not set yet**), `ARUGULA_PRESENCE_APP`, `ARUGULA_PRESENCE_RELAY` (*config*) | Infisical, when set | Operator. arugula.stream shows a secret once: `rotate_secret` makes a new one |
| `ARUGULA_E2B_API_KEY` | Infisical `/illogical/control` (and `E2B_API_KEY` there) | Operator |
| `PLANTER_APP_ID`, `PLANTER_APP_SLUG` (*config*), `PLANTER_APP_PRIVATE_KEY` (M233: planter's App, the deploy App; **not set yet**) | Infisical project `planter` (as planter's `GITHUB_APP_ID`, `GITHUB_PRIVATE_KEY`) | Operator. Mints planter's drivers' tokens; planter's own copy goes with planter (M234) |
| `POSTHOG_API_KEY` | the hub | Operator |
| `ARUGULA_CLOUD_ACCOUNTS`, `ARUGULA_HOSTED_ACCOUNTS`, `ARUGULA_HOSTED_PLATFORM_KEYS`, `ARUGULA_PLATFORM_RELEASE_PROJECT`, `ARUGULA_PREVIEW_DOMAIN` (*config*) | | Operator |
| `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET` | read by code, set nowhere (billing is off) | Operator, when billing starts |

The nightly dump machine runs in the same app and reads the same secrets; it
encrypts to an age recipient. The private age identity,
`ARUGULA_PLATFORM_DUMP_AGE_KEY`, is only in Infisical `/illogical/control`
(with `ARUGULA_PLATFORM_DUMP_AGE_RECIPIENT`): Operator.

**Already on the platform**, sealed with each project's key: `project_keys`
(the data keys), `project_secrets` (a deploy environment's, M217),
`project_inference_keys` (a project's own Anthropic or OpenAI key, M185)
and `hosted_workspace_secrets`. Their names are each project's own business
and aren't listed. This is where every **Platform** row below goes. The
deploy environments code knows by name are in
[`crates/control/secrets.toml`](../crates/control/secrets.toml); today it has
one, `arugula-release`, with no secrets yet.

**E2B's secrets broker:** `arugula-platform-anthropic` and
`arugula-platform-openai`, the platform's own inference keys for hosted boxes
([control-ops.md](control-ops.md)). Infisical keeps the record as
`ARUGULA_HOSTED_ANTHROPIC_KEY`; there's no `ARUGULA_HOSTED_OPENAI_KEY`.
Operator.

## arugula: releases and CI

GitHub Actions on `arugula-salad/arugula`:

| Name | Used by | Copies | Fate |
| --- | --- | --- | --- |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`, `APPLE_PROVISIONING_PROFILE` (D2: the Developer ID provisioning profile, base64; not secret, kept beside the others) | `app-release.yml` | Infisical `/illogical/release`; the certificate's files on the workstation | Operator |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | `app-release.yml` | Infisical `/illogical/release` (with `TAURI_UPDATER_PUBKEY`, public) | Operator |
| `FLY_API_TOKEN` (deploys the hub), `FLY_PLATFORM_API_TOKEN` (deploys the platform) | `release.yml` | none; Fly mints new ones | Operator |
| `E2B_API_KEY` | `release.yml` (the hosted template) | Infisical | Operator |
| `HOMEBREW_TAP_KEY` | `release.yml` | none | Operator |
| `CI_GREEN_TOKEN` | `app-release.yml`, `chant-ci-green.yml` | none | Operator |
| `SCCACHE_WEBDAV_PASSWORD` | `check.yml`, `testnet-break.yml` | none | Operator |
| `ARUGULA_GH_APP_PRIVATE_KEY` (the forge-test App) | `forges-nightly.yml` | Infisical `/illogical/forge-tests` (with `ARUGULA_GH_APP_ID`) | Operator |
| `ARUGULA_GH_AUTHOR_TOKEN`, `ARUGULA_GH_REVIEWER_TOKEN` (the test bots') | `forges-nightly.yml` | none | Operator |

Variables (not secrets): `ARUGULA_GH_APP_ID`, `ARUGULA_GH_TEST_REPO`, and
`AZURE_CLIENT_ID`, `AZURE_TENANT_ID` for the Marketplace's OIDC sign-in. The
`marketplace` environment has no secrets: releases publish by OIDC.

In Infisical, outside `/illogical`, all Operator:

| Project / path | Names | Read by |
| --- | --- | --- |
| `arugula` `/ghcr` | `GHCR_DOCKERCONFIGJSON` | the `arugula-secrets` identity, into the image-pull Secret of home-cloud's `arugula` namespace (planter, redone, beds, salad pull with it) |
| `arugula` `/illogical/control-tests` | `ARUGULA_E2B_API_KEY`, `E2B_API_KEY` | people running control's tests by hand |
| `arugula-flux` `/` | `githubAppID`, `githubAppInstallationID`, `githubAppPrivateKey` | only `arugula-flux-operator`, for Flux to clone the org's private repos; kept apart so no app can read a key that clones the org |
| `github-runners` `/` | `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY`, `GITHUB_APP_SLUG` | `arc-operator`, for the self-hosted Linux runners; the Mac runners at deploy time |

And in `/illogical/control`, kept as the record of values unset from Fly:
`ILLOGICAL_SANDBOX_ACCOUNTS`, `ARUGULA_CLOUD_MODEL`, `ARUGULA_CLOUD_RUNTIME`.
Nothing reads any of them: Deleted **now**, once Jake agrees the record has
served. `ARUGULA_PLATFORM_DATABASE_URL_POOLED` is read by nothing either; it
can stay as the record of the pooled address.

## planter

Infisical `planter` → Secret `planter-secrets` in home-cloud's `arugula`
namespace, read by the `planter-operator` identity (viewer).

| Name | What for | Fate |
| --- | --- | --- |
| `SESSION_SECRET` | sessions; seals users' GitHub tokens and boxes' secrets | Deleted with planter (M234) |
| `GITHUB_APP_ID`, `GITHUB_APP_SLUG` (*config*), `GITHUB_PRIVATE_KEY`, `GITHUB_CLIENT_ID` (*config*), `GITHUB_CLIENT_SECRET` | planter's own GitHub App, which stays the deploy App | Operator, held for the planter driver (M233) |
| `GITHUB_WEBHOOK_SECRET` | never read | Deleted **now** |
| `FOUNTAIN_API_KEY` | planter's Managoat key, for its boxes | Deleted with planter (M234) |
| `BEDS_CALLBACK_TOKEN`, `BEDS_SERVICE_TOKEN` | read by nothing since boxes moved off beds | Deleted **now**; beds' half of each pair goes with beds |
| `PLANTER_FORGE_SIGNIN_KEY` | read when set; set nowhere, so off | Nothing to move. The hub's issuer keys (M228) take its job |

What planter keeps for its users, on its `/data` volume:

| Where | What | Fate |
| --- | --- | --- |
| `sessions.json` | users' GitHub tokens, sealed | Deleted with planter (M234) |
| `boxes.json` | each box's `BOX_SECRET` and `BOX_ENTER_SECRET`, sealed | Deleted with planter |
| `shipTokens.json` | ship tokens, as hashes only | Deleted after the announced revocation date (M232–M234) |
| Managoat, per box | `BOX_SECRET`, `BOX_ENTER_SECRET`, an hour-long `GITHUB_TOKEN` | Deleted with planter |

**`planter-apps`**, the shared Infisical project for planted apps: one
folder per app (`/<app>/`, with `registry`, `postgres`, `bucket`,
`conversations` and `analytics` beneath), plus `/ghcr`. Read by the
`planter-apps-operator` identity (viewer) into each app's Secrets in the
`planter-apps` namespace; written by `planter-apps-secret-writer` (redone),
`apps-pg-provisioner`, `apps-platform-provisioner`,
`apps-platform-conversations`, and home-cloud's admin identity. Fate: an
app's own keys are **Platform**, its project's secrets, which the platform
then writes back into the app's folder as a projection. The provisioners'
keys (`postgres`, `bucket`, `registry`) and `/ghcr` stay Operator. Planter
itself never writes here.

## redone

Infisical `redone` → Secret `redone-secrets` in the `arugula` namespace,
read by the `redone-operator` identity (viewer). Names from redone's code
and manifests: Jake's account can't list this project.

| Name | What for | Fate |
| --- | --- | --- |
| `PLANTER_SHIP_TOKEN` | ships and promotes through planter | Deleted with redone (M235) |
| `SESSION_SECRET` | the approver cookie | Deleted with redone |
| `LOAM_READ_KEY` | reads loam's `apps` project | Deleted with redone |
| `INFISICAL_CLIENT_ID`, `INFISICAL_CLIENT_SECRET` | the `planter-apps-secret-writer` identity: writes, can't read or delete | Operator. The platform takes this identity over for its projection (ask Jake first) |
| `INFISICAL_API`, `INFISICAL_PROJECT_ID` (*config*) | | Deleted with redone |
| `NTFY_URL`, `NTFY_TOKEN` | read, set nowhere | Nothing to move |

What redone keeps (`state.json` on its volume): each tenant's approver key
and each agent's `rdn_` token, as hashes only. Deleted with redone, after
their announced end date (M235); platform grants and device-signed deploy
approvals replace them.

## beds

beds is the studio lobby run a second time, from the studio repo. It
retires in step 8. Infisical `beds` → Secret `beds-secrets` in the `arugula`
namespace, read by the `beds-operator` identity.

| Name | What for | Fate |
| --- | --- | --- |
| `ANTHROPIC_API_KEY` | the house Claude key, metered per account | Deleted with beds (M236); the platform's inference key and metering replace it (M231) |
| `LOBBY_SECRET`, `STUDIO_SECRET` | beds' own sessions, vault and claim keys | Deleted with beds |
| `LOAM_KEY_BEDS` | makes beds' boxes | Deleted with beds |
| `STUDIO_SERVICE_TOKEN`, `PLANTER_CALLBACK_TOKEN` | calls between beds and planter, the other half of planter's `BEDS_*` pair; beds still reads them, planter no longer does | Deleted with beds |
| `SPRITE_TOKEN` | read by nothing | Deleted **now** |

beds' pod also requires `beds-wisp-token` (studio's `SANDPIT_KEY_BEDS`) to
start, though nothing reads it: it goes with beds. People's Claude keys
sealed in beds' `/state` are in the next section's terms.

## studio's lobby

Infisical `studio` → Secrets `studio-secrets`, `studio-loam-key`,
`studio-wisp-token`, `studio-posthog`, `studio-ghcr` in the `arugula`
namespace.

| Name | What for | Fate |
| --- | --- | --- |
| `LOBBY_SECRET` | sessions, the lobby's vault key, hud grants | Operator, as studio's own. Its sign-in and vault work leave when studio signs in with Arugula and takes secrets from the platform (M236) |
| `STUDIO_SECRET` | claim keys; every box holds it by design | Operator (studio's) |
| `LOAM_KEY` | studio's boxes | Operator |
| `POSTHOG_KEY`, `POSTHOG_APPS_KEY` | public analytics keys | Operator |
| `SANDPIT_KEY`, `SPRITE_TOKEN` | rollback only; the lobby reads neither | Deleted **now**; drop the `studio-wisp-token` reference first |
| `SANDPIT_KEY_BEDS` | beds' required, unread reference | Deleted with beds |
| `/ghcr` `GHCR_DOCKERCONFIGJSON` | the lobby image's pull | Operator |

Read by the lobby but set nowhere, so off in production:
`STUDIO_GITHUB_CLIENT_ID`, `STUDIO_GITHUB_CLIENT_SECRET` (replaced by "Sign
in with Arugula", M236), `STUDIO_FORGE_SIGNIN_KEY` (replaced by the hub's
issuer keys, M228), `STUDIO_TRIAGE_CLAUDE_TOKEN`, `LAYA_API_KEY`.

GitHub Actions on `studio` (and `arcade`): `HUD_DEPLOY_KEY`, the private half
of a read-only deploy key on `hud`, so CI can build hud into box and game
images. Operator.

What the lobby keeps for people, on its `/state` volume (beds' is the same
shape):

| Where | What | Fate |
| --- | --- | --- |
| `accounts.json`, each person's `key` | their Claude credential, sealed | **Platform**: the broker moves there (M231) |
| `accounts.json`, `passkeys` | public keys | Kept a quarter as the fallback sign-in, then Deleted |
| `accounts.json`, `tokens` | personal tokens, as hashes | Studio's own (M236): they list and open a person's studio apps, which stay studio's objects |
| `apps.json`, each app's `secrets` | an app's secrets, sealed (up to 20) | **Platform**: the app's project's own secrets, which its owners grant studio by name (M236; a value saved in studio still works beside it until its people move it) |
| `apps.json`, token hashes, `enter`, `nonce` | an app's own runtime | Studio's own |
| `usage.json` | Claude spend per person | Not a secret; the metering moves to the platform (M231) |

## hud

No org store holds a hud secret: no Infisical project, Kubernetes Secret or
Actions secret. What hud has belongs to each box.

| Name | Where | Fate |
| --- | --- | --- |
| `HUD_IDENTITY_JSON` (the box's signing secret, owner invite and roster) | a file on the box; a Managoat vault for boxes it made | The box's own. The roster comes from project members instead (M237) |
| `HUD_FORGE_SIGNIN_KEY` | a public key, set by studio or planter | Replaced by the hub's JWKS (M228, M237) |
| `HUD_BOX_USAGE_TOKEN`, `HUD_FEEDBACK_TOKEN`, `HUD_BROKER_TOKEN` | set on a box by studio | Follow studio's broker to the platform (M231) |
| `TYPESAFE_API_KEY` | box env, for the decider | The box's own |
| `CLAUDE_CODE_OAUTH_TOKEN` | Fly app `hud-box`, which is gone | Deleted (confirm the app is gone) |

The private half of forge sign-in's Ed25519 key would be studio's or
planter's (`STUDIO_FORGE_SIGNIN_KEY`, `PLANTER_FORGE_SIGNIN_KEY`); neither
is set.

## experimental

Archived in step 8 (M238). Everything here is **Deleted** with it.

| Name | Where |
| --- | --- |
| `FLY_API_TOKEN` (deploys `arugula-staging`), `FLY_TEST_TOKEN` (template releases to the Fly org `arugula-test`) | GitHub Actions on `experimental` |
| `SANDPIT_TOKEN`, `FOUNTAIN_KEY`, `FOUNTAIN_URL`, `FOUNTAIN_AGENT_ID`, `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, `ARUGULA_SECRET_KEY`, `ARUGULA_ADMINS` | Fly apps `arugula-staging` and `arugula-fountain`, from its `fly.toml` and code; in a Fly org Jake's account can't see |
| session hashes, a sealed `claude-token` per person | its SQLite volume |
| `CLAUDE_CODE_OAUTH_TOKEN` per account; `FOUNTAIN_API_KEY` in each runner's `runner.env` | Managoat |

Deleting its Fly apps and archiving the repo are asks for Jake (and Aaron).

## Elsewhere in the org

Not converging in step 8; listed so the inventory is whole. All Operator
unless marked.

| Product | Store | Names |
| --- | --- | --- |
| arcade (games) | Infisical `arcade` `/arugula` | `ARCADE_SECRET`, `LOBBY_SECRET`, `LOAM_KEY`; `SANDPIT_KEY` (a rollback script only) and `WISP_KEY` (read by nothing): Deleted **now** |
| arcade's retired site | Infisical `arcade` `/` | `ARCADE_SECRET`, `LOBBY_SECRET`, `SPRITE_TOKEN`: Deleted **now** |
| arcade | Infisical `arcade` `/ghcr`; Actions | `GHCR_DOCKERCONFIGJSON`; `HUD_DEPLOY_KEY` |
| loam | Infisical `loam` | `LOAM_ADMIN_TOKEN`, `LOAM_BROKER_CA_CRT`, `LOAM_BROKER_CA_KEY`, `LOAM_IDENTITY_KEY`, `LOAM_NODE_TOKEN`, `LOAM_PG_BACKUP_ACCESS_KEY_ID`, `LOAM_PG_BACKUP_SECRET_ACCESS_KEY`, `LOAM_SECRET_KEY`, `LOAM_STORE_ACCESS_KEY_ID`, `LOAM_STORE_SECRET_ACCESS_KEY` |
| sandpit | Infisical `sandpit-substrate` | `SANDPIT_TOKEN` |

## The operator's workstation

Copies in Jake's `~/.config`, each `0600`:

| File | Names | Fate |
| --- | --- | --- |
| `illogical/github-app-forge.env` | `GITHUB_APP_ID`, `GITHUB_APP_SLUG`, `GITHUB_APP_CLIENT_ID`, `GITHUB_APP_CLIENT_SECRET`, `GITHUB_APP_WEBHOOK_SECRET` | Operator: escrow the client and webhook secrets in Infisical, then delete the file |
| `illogical/github-app.env` | `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, the old sign-in-only App's | Deleted **now**: the hub signs in with the App's own client. Delete the old App on GitHub too, if it's unused |
| `arugula/apple-devid/` | the Developer ID key, certificate and `.p12` with its password | Operator (Infisical has the six release secrets) |
| `arugula/providers.env` | `E2B_API_KEY`, `VERCEL_TOKEN`, `VERCEL_TEAM_ID`, `VERCEL_PROJECT_ID` | Operator; the Vercel names aren't in any org store |
| `arugula/cloudflare-arugula-run-token`, `arugula/loam-key`, `illogical/claude-oauth-token`, `illogical/tailnet-authkey` | one token each | Operator; none has an Infisical copy |

## Duplicates and orphans

- **Read by nothing** (Deleted now): planter's `GITHUB_WEBHOOK_SECRET`, `BEDS_CALLBACK_TOKEN`, `BEDS_SERVICE_TOKEN`
  (beds still reads its half, `STUDIO_SERVICE_TOKEN` and `PLANTER_CALLBACK_TOKEN`);
  beds' `SPRITE_TOKEN`;
  studio's `SANDPIT_KEY`, `SPRITE_TOKEN`; arcade's `/` folder and `/arugula`'s
  `SANDPIT_KEY`, `WISP_KEY`; Infisical's `ILLOGICAL_SANDBOX_ACCOUNTS`,
  `ARUGULA_CLOUD_MODEL`, `ARUGULA_CLOUD_RUNTIME`; the workstation's
  `github-app.env`.
- **Rollback copies on the hub**: `GITHUB_APP_PRIVATE_KEY` and
  `GITHUB_APP_WEBHOOK_SECRET`. The hub reads them only with the platform in
  process. They stay until Jake decides; the cloud copies and run caps
  went on 2026-10-24 (Arugula #71).
- **Two names, one E2B key**: `E2B_API_KEY` and `ARUGULA_E2B_API_KEY` sit side
  by side in `/illogical/control` and again in `/illogical/control-tests`.
- **Same name, different keys**: `LOBBY_SECRET` (studio, beds, arcade twice),
  `STUDIO_SECRET` (studio, beds), `GHCR_DOCKERCONFIGJSON` (four folders) and
  `GITHUB_CLIENT_ID`/`_SECRET` (planter, experimental, the old hub App) are
  each a different product's own.
- **One identity, two client secrets**: `planter-apps-secret-writer` has one
  in redone's Infisical project and one in a Managoat vault. Rotating it
  means both.
- **No escrow**: `GITHUB_APP_CLIENT_SECRET` and `GITHUB_APP_WEBHOOK_SECRET`
  live only in Fly and on the workstation; the hub's VAPID and hosted keys
  only in its database. Fly can't read a secret back, so losing the
  workstation copy means rotating.
- **Config in secret stores**: redone's `INFISICAL_API`, `INFISICAL_PROJECT_ID`;
  the Fly settings marked *config*.

## Not listed

- **Org-level Actions secrets and variables**: listing them needs a token
  with `admin:org`, which the agent's doesn't have. Workflows here read
  `ARUGULA_RELEASE_SKIP_GREEN`, `ARUGULA_HUB_DEPLOY`, `ARUGULA_RED_MENTION`
  and `ARUGULA_RED_HOURS` as variables that no repo sets, so they may be
  there.
- **Infisical `redone`**: not visible to Jake's account; its names above
  come from redone's code.
- **experimental's and hud's Fly apps**: not in any Fly org Jake's account
  can see; names come from their code.
- **E2B's secrets broker and Managoat's vaults**: names come from docs and
  code, not from a listing.
- **The cloud Infisical bootstrap project** (home-cloud's own kernel): not
  arugula-salad's.
- **home-cloud's other Infisical projects**, the platform's per-project
  secret names (people's own), and Codespaces secrets.
