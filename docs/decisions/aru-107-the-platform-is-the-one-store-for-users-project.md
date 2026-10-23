---
schema: 1
id: "aru-107"
title: "The platform is the one store for users' project secrets; Infisical stays the operator's"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "Where does a project's secret live, when arugula, planter, redone, studio and beds each keep their own?"
options:
  - id: "a"
    label: "The platform, for users' project secrets only; Infisical stays the operator's store and the Kubernetes layer, written by the platform only as a projection"
    how: "A secret a user sets for a project (a provider key, an app's egress secret, a deploy token) is a platform project secret: sealed per project, shown by name and never read back. Infisical keeps the operator's own secrets and stays the layer that materializes secrets into Kubernetes. Where a project's secret has to reach a cluster, the platform writes it to Infisical as a projection, through redone's writer identity, and the platform's copy is the truth. Users' secrets in planter, redone, studio and beds move to the platform, and docs/secrets-inventory.md lists every store by name and owner."
    tradeoff: "Two stores still exist, so the line between a user's secret and the operator's is drawn store by store, in the inventory."
  - id: "b"
    label: "Infisical for everything"
    how: "Users' project secrets go into an Infisical path per project."
    tradeoff: "Infisical's access is the operator's, not a project's members', and every user secret would need an operator-made path."
  - id: "c"
    label: "The platform for everything, Infisical retired"
    how: "The operator's secrets move to the platform too, and it writes Kubernetes Secrets itself."
    tradeoff: "The platform would hold the keys that run it, and Kubernetes would lose the layer that already gets it its secrets."
  - id: "d"
    label: "Each product keeps its own store"
    how: "As today."
    tradeoff: "One project's secrets stay in up to five places, each with its own idea of who may read them."
choice:
  option: "a"
  reason: "A user's secret belongs to a project, whose members the platform knows; the operator's secrets belong to the operator, whose store already works (Jake, 2026-10-22, platform step 8, including studio's egress secrets on Alex's behalf, pending Alex's review)."
rejected:
  - option: "b"
    why: "Who reads a project's secret follows project membership, which Infisical doesn't know."
  - option: "c"
    why: "Infisical stays the operator's store and the Kubernetes layer."
  - option: "d"
    why: "One store for users' project secrets."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "Arugula #25"
    url: "https://app.arugula.io/issues/iss_29cbf1829e2a97cac4abe6d0070e6c3e"
  - title: "Arugula #33"
    url: "https://app.arugula.io/issues/iss_fafeedc287db9ef4e5e9bfe05ee12ade"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:crates/platform/src/secrets.rs"
  - "path:crates/platform/src/projects.rs"
---

# The platform is the one store for users' project secrets; Infisical stays the operator's

A project's secrets live in one place, the platform (Jake, 2026-10-22,
platform step 8, #1337).

- **Users' project secrets only.** A secret a person sets for a project (a
  provider key, an app's egress secret, a deploy token) is a platform
  project secret: sealed per project, shown by name and never read back, as
  "Hosted inference: strangers bring their own key" keeps a project's own
  key. The users' secrets in planter, redone, studio and beds move here.
- **Infisical stays the operator's store,** for the secrets that run the
  org, and stays the layer that materializes secrets into Kubernetes.
- **The platform writes Infisical only as a projection.** Where a project's
  secret has to reach a cluster, the platform writes it through redone's
  writer identity, and the platform's copy is the truth. Taking that
  identity over is Jake's to say (M235).
- **The inventory** (`docs/secrets-inventory.md`, M227) lists every store
  and key across the org by name and owner, never values, and says which
  move here, which stay the operator's, and which go with their product.

studio's egress secrets coming from the platform is Alex's part, decided by
Jake on Alex's behalf, pending Alex's review of this record.
Where: `crates/platform/src/secrets.rs`, `crates/platform/src/projects.rs`, the inventory (Arugula #25, M227, native), redone's Infisical writer (Arugula #33, M235, native).
From: #1337 (platform step 8, tracked natively in Arugula #22).
