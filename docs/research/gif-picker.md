# A GIF picker in chat

Research for [#1626](https://github.com/arugula-salad/arugula/issues/1626),
checked 2026-10-31. These are implementation recommendations, not an
accepted decision or permission from either provider. No production key,
provider agreement or live API integration was tested.

Implementation follow-up: [#1935](https://github.com/arugula-salad/arugula/issues/1935).

## Recommendation

Pursue **KLIPY with ads disabled**, subject to written approval for
platform-side search and media delivery. Keep the picker disabled until
that approval and production access exist. GIPHY is an alternative only
after an explicit agreement permits the same architecture and establishes
its price. Do not quietly switch to browser-direct requests if approval
is refused: that changes what users disclose.

Neither provider's standard integration meets the proposed privacy model.
GIPHY's [integration rules](https://developers.giphy.com/docs/api/)
prohibit API/media proxying and caching responses, media URLs or assets.
KLIPY's [integration requirements](https://docs.klipy.com/integration-requirements)
require written approval for server requests, intermediaries and alternate
media delivery. Approved caches must support revalidation and removal.
GIPHY's endpoint notes mentioning proxies do not override its explicit
prohibition; obtain written clarification.

Tenor is not a candidate: [Google's shutdown notice](https://support.google.com/tenor/answer/10455265?hl=en)
sets June 30, 2026 as the API decommission date.

## 1. Provider, cost, attribution and ads

| Question | KLIPY | GIPHY |
|---|---|---|
| Cost and quota | Its [published API collection](https://www.postman.com/klipy-api/klipy-api-services/overview) advertises a free API. Its [developer FAQ](https://klipy.com/developers) limits testing to 100 calls/hour and advertises unlimited requests after production approval. Confirm pricing and operational limits for our custom architecture; these are not a negotiated service guarantee. | [API quickstart](https://developers.giphy.com/docs/api/): beta keys allow 100 calls/hour; production requires an application and pricing discussion. No production dollar amount is established here. |
| Attribution | [API terms](https://klipy.com/support/api-terms) require visible branding and available creator/source attribution. [UI guidance](https://docs.klipy.com/attribution) requires the search placeholder **Search KLIPY** but calls the watermark and Powered by mark optional. Use visible branding as well and resolve the wording difference during approval. | [API terms](https://support.giphy.com/hc/en-us/articles/360028134111-GIPHY-API-Terms-of-Service) require the Powered by GIPHY text/logo and available user/source attribution. |
| Ads in the picker | [Developer FAQ](https://klipy.com/developers): ads are optional and controlled per key. Request an ad-free key; do not enable the Ads API for v1. | The [SDK agreement](https://support.giphy.com/hc/en-us/articles/360027858552-Giphy-SDK-License-Agreement) allows advertising. That does not establish the ad behavior of a bespoke REST contract. Ask for written confirmation; do not promise an ad-free GIPHY picker. |

Keep one provider's results in their own grid and preserve their order and
URLs. Both providers restrict mixing results; do not build a merged search
or silently remove promoted results. Resolve unexpected ads by disabling
the integration and correcting its configuration, not by stripping them.

## 2. Privacy and where requests run

Recommended flow after approval:

```text
room member -> authenticated platform search -> approved provider
room reader -> authenticated platform media route -> approved provider CDN
```

Proxying search alone is insufficient: direct thumbnails, videos, posters
or tracking URLs still disclose readers' network information. The
[KLIPY privacy policy](https://klipy.com/support/privacy-policy) describes
search terms, IP-associated viewing history and tracking technologies.
The [GIPHY privacy policy](https://support.giphy.com/hc/en-us/articles/360032872931-GIPHY-Privacy-Policy)
describes queries, IP addresses, identifiers and cookies. Ads off does not
mean tracking off.

For the approved design, create outbound requests from an explicit header
allowlist. Send neither browser cookies, authorization, user IP forwarding
headers nor private room/project identifiers. Use no provider SDK, browser
preconnect or external tracking pixel. Agree on whether a pseudonymous
customer identifier or any usage callbacks are required before implementing
them; do not send the Arugula account id as a shortcut. Redact keys and
queries from logs and traces, including upstream URLs.

The picker should explain, before its first search, that the named provider
receives search terms and selected-content requests. The privacy notice
must describe the approved data flow, provider policy, identifiers/events,
retention, and any advertising. State that the platform processes searches;
do not describe the feature as anonymous. A different direct-client design
would need a separate decision and notice covering reader IPs and cookies.

No provider contact, account creation or agreement was made in this research.

## 3. Rating policy

Recommend a team-level default, project restrictions for project rooms,
and room restrictions that can only tighten their parent's policy. DMs
and machine threads without a project inherit the team default. Begin
with a single strict setting; add looser choices only after their mappings
are reviewed. An account preference may be stricter, never bypass the room.

For a strict request, use KLIPY's documented `content_filter=high` or
GIPHY's `rating=g`. KLIPY's [GIF search reference](https://docs.klipy.com/gifs-api/gifs-search-api)
lists `off`, `low`, `medium`, `high`; GIPHY's
[rating guide](https://developers.giphy.com/docs/optional-settings/)
defines its own categories. They are not established as equivalent safety
guarantees. Always set the filter explicitly. Enforce the effective policy
on the platform, including trending, search and selection resolution;
never trust a client-provided rating or arbitrary pasted provider URL.
Keep KLIPY's dashboard restrictions consistent with these requests and
confirm allowed filtering during approval.

## 4. What gets posted and retained

Post a typed provider reference, with creator/source attribution, rather
than treating a selection as an ordinary uploaded file. Resolve the
reference through the approved media path when displaying it. The agreement
must explicitly permit keeping the identifiers and metadata needed for
historical messages; do not assume a URL is a durable license.

KLIPY's [API terms](https://klipy.com/support/api-terms) prohibit media
storage except client search thumbnails. Consequently, copying a selected
GIF into our upload store to survive removal is not the default option.
If approved caching is used, follow the provider's expiry, revalidation
and purge rules. Show **GIF unavailable** for removed content; provider
removal must not be defeated by a permanent copy. Provider changes affect
new searches; old references remain associated with their original
provider and become unavailable if its agreement ends.

Prefer provider-supplied small MP4 previews and a larger MP4 for selected
opaque animations; use animated WebP where appropriate, especially for
transparency. GIPHY's [rendition guide](https://developers.giphy.com/docs/optional-settings/)
supports that choice; KLIPY's GIF search reference offers MP4/WebP format
filters. Select existing renditions rather than transcoding by default.
Provide still previews for reduced motion, explicit playback, alt text,
and a working unavailable state. Bound actual transferred bytes and decode
dimensions; provider size metadata alone is insufficient.

## 5. Configuration and fit in this repository

Use platform operator settings for the provider and server-side key, plus
team opt-in, a rating ceiling, ads disabled and an emergency off switch.
Treat these as proposed settings, not existing option names. Keep keys out
of web bundles and logs. An operator may configure only providers whose
delivery/privacy agreement is approved. V1 needs one adapter; an interface
for capabilities avoids pretending the providers have interchangeable
contracts. Do not implement automatic cross-provider fallback.

The implementation belongs to the platform chat feature:

- [`RoomCompose`](../../web/src/shared/chat/room.tsx) already hosts the emoji
  picker; add the adjacent GIF control there, including thread replies.
- [`arugula-platform-chat`](../../crates/platform-chat/README.md) owns chat's
  routes. Reuse its room membership/posting checks for search, selection
  and media reads. Losing room access must revoke GIF access too.
- [`files.rs`](../../crates/platform-chat/src/chat/files.rs) serves retained
  uploads from the platform database. Its storage semantics differ from
  revocable provider media; a picker must not call the upload path to
  bypass that difference.
- [`link_fetch.rs`](../../crates/platform-chat/src/chat/link_fetch.rs) has
  DNS/redirect validation, pinned public addresses, time and byte limits.
  Reuse those protections in a provider-specific bounded media fetcher,
  with an approved HTTPS host allowlist. Its generic 1 MiB preview fetch
  limit is not a ready-made video delivery policy.

Chat stays in the platform while machines sleep, as
[ARU-051](../decisions/aru-051-chat-lives-in-control-in-plain-text-terminal-content-doesn-t.md)
requires. This proposal does not change that decision. A separate build
issue requires provider approval, migrations/wire types, privacy
copy, revocation, and phone/keyboard tests before the feature is enabled.
