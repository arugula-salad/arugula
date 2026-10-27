# Design: control sends email

Status: proposed, for review. Issue #1609, epic #1599. Needed first by the
reset (aru-119, docs/design/self-service-reset.md): when a reset of an
account's devices starts, its email hears about it.

## Today

- Control stores no email address
- GitHub sign-in asks for no scopes (`auth.rs`: `/login/oauth/authorize`
  with no `scope`), so GitHub gives control a public email only if the
  person made one public
- Passkey-only accounts have no address anywhere
- Nothing sends mail: no provider, no DNS for it, no template

## What it's for

Security notices only, to start:

- a reset of your devices started, was cancelled, or finished
- later, maybe: a new device approved, a recovery code used

Not marketing, not chat, not digests. Each is short, says what happened,
when and from where, and how to stop it if it wasn't you.

## The address

Options:

| | How | For | Against |
|---|---|---|---|
| A | Ask GitHub for `user:email` at sign-in; keep the primary verified one | No typing; GitHub verified it | A new scope on the consent screen for everyone; changes when they change GitHub's |
| B | The person adds one in Account settings; control sends a link to verify it | Works for passkey accounts; they choose | Nobody has one until they add it, and the stranded person is exactly who didn't |
| C | A, and B to change it or for passkey accounts | Both | Two paths to build |

Recommendation: **C**, with A asked only once, when someone links or signs
in with GitHub after this ships (`prompt` stays as it is). An account with
no address says so in Account settings, next to the recovery codes: "Add an
email so a reset of your devices reaches you."

## What control keeps

- `accounts.email` (text, null), `accounts.email_verified_at` (ms, null),
  `accounts.email_from` (`github` or `typed`)
- `tables.toml`: `accounts` gains `plaintext` (an address is personal data);
  deleted with the account
- `email_sends` (account, kind, at, provider_id), for "did it go out" and
  rate limits; kept 90 days
- Never: an email's body after it's sent

## The provider

- One transactional provider behind a small trait (`Mailer`), so tests use
  a fake and self-hosted controls can use SMTP
- Hosted: Postmark or Resend; the key is a Fly secret (`docs/control-ops.md`
  *Secrets*)
- DNS for `arugula.io`: SPF, DKIM (the provider's records) and DMARC; from
  `security@arugula.io`, reply-to a monitored address
- Off when unset: a control without a mailer sends nothing and says so in
  the account's settings

## Sending

- From the hub, after the transaction that made the event commits
- Best-effort: a send that fails is logged and retried a few times; the
  reset's wait doesn't depend on it
- At most 5 security emails per account per day

## Privacy and the third party

- The privacy notice (on the site) names the provider and says what it
  sees: the address and the notice
- `tables.toml`'s `[parties]` gains the provider, `seen_by` on `email_sends`
- PostHog never gets the address (as today: `docs/control-ops.md`,
  *Product analytics*)

## Tests

- Control: a reset start sends one email to a verified address and none to
  an unverified one; a failing mailer doesn't fail the reset; the daily
  limit; deleting the account deletes its address and sends
- The verify link: used once, expires in a day, only for the account that
  asked

## Open questions

1. **Postmark or Resend?** Either does the job; pick by deliverability and
   price at our volume.
2. **The GitHub scope.** Adding `user:email` changes the consent screen for
   every new sign-in. Worth it, or typed addresses only?
3. **Self-hosted.** SMTP settings as flags, or no email off the hosted one?
4. **Does self-service wait for this?** A reset can start without email
   (notices and pushes reach the devices); the email is for when no
   device is left to see them, which is the stranded case.
