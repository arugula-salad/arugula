# Design: resetting your own devices

Status: built (#1582's PR). The table is `reset_requests`, one row per reset, which also counts starts a day. Builds what aru-119 decided ("An account can
reset its devices from a fresh GitHub sign-in, after a wait"). Issue #1582,
epic #1599. The support reset it shares a step with is
`arugula-control account-reset` (#1608).

## The person this is for

Signed in with GitHub, on *Approve this browser*, and nothing can approve:

- every device they approved is gone, or was approved at an address control
  left (#1581)
- no recovery code left

Today they're stuck. After this, the approval screen offers a way out that
takes two days and can be stopped by anything they still have.

## The flow

1. **Offered.** *Approve this browser* shows *Can't approve from another
   device?* under the recovery code form. It opens a page that says what a
   reset costs (below) before anything starts.
2. **Fresh sign-in.** Starting needs a GitHub sign-in from the last ten
   minutes (`REAUTH_MS`, as a first passkey takes, #1386). An older one is
   sent through GitHub again (`/auth/github?reauth=1`), then back here.
3. **Started.** Control records a pending reset with `due` = now + 48
   hours, and tells:
   - every device: a notice on control's page (`add_notice`), and a push to
     each device with push on
   - the account's email, once control sends email (#1609)
4. **Waiting.** The browser that started it shows the time left and *Cancel*.
   Signing in anywhere else on the account shows the notice with *This
   wasn't me: cancel*.
5. **Cancelled**, by any of:
   - a device the account trusts, signing the cancel (below)
   - a recovery code still good, signing it the same way
   - the session that started it (the person changed their mind)
6. **Done.** When `due` passes, the next request that looks at the account
   (its sign-in, `/api/devices`, the reset's own status) runs the same step
   as the support reset (`Db::reset_account_devices`, `by` = `self-service`).
   The waiting browser reloads, enrolls as the first device, and shows new
   recovery codes. No timer process: done lazily, inside the transaction
   that reads the account. Its passkeys are removed too, `by reset` in its
   passkey log (DK10, #393, for an account with GitHub linked): the one way
   to remove a passkey without another's say-so, and what lets a fresh
   GitHub sign-in add a first one again to an account that had one.

## What a reset costs, shown before it starts

- Every device, passkey and recovery code stops working
- Each machine is dropped and needs *Join again* on the machine
- Teammates see that your key changed, and check it again
- Anything end to end encrypted to the old devices can't be read

## Cancelling with a signature

The cancel has to come from something the account already trusts, not just
a signed-in session, or a stolen session could start a reset and also stop
the real person's cancel from counting.

- Body: `arugula reset cancel v1\n<account>\n<started_at>`
- Signed with the device's Ed25519 key (as approvals are), or a recovery
  code's (the page derives it, as `useRecoveryCode` does)
- Control checks the signer against the account's chain
  (`Trust::evaluate`), as it does an approval
- A cancelled reset can't be resumed; a new one starts a new wait

## Limits

- One pending reset per account
- One reset per account a week, self-service or support (`account_resets`,
  `AGAIN_AFTER_MS`)
- Starting is refused while the account has a device that has signed
  anything in the last 48 hours: then something can approve, and the page
  says to use it. (Control knows when a device last talked to it:
  `touch_device`, `devices_used`.)
- Rate limit on starting: 3 a day per account

## Control

| Route | Who | Does |
|---|---|---|
| `POST /api/me/reset` | page session, fresh GitHub | starts one; 409 if one's pending, a device was active, or a reset was this week |
| `GET /api/me/reset` | page session | `{ pending: { started_at, due } }` or `{ pending: null }`; finishes a due one first |
| `POST /api/me/reset/cancel` | page session plus a device or recovery signature, or the starting session | cancels |

Each goes in `routes.toml`, with its wire types in `arugula-control-wire`
and `just proto-ts`.

New table `account_reset_requests` (account, started_at, due, session_hash,
cancelled_at, cancelled_by), `plane = "machine"`, `holds = "metadata"`,
`with_account = "deleted"`. The archive stays `account_resets`.

## The page

- `web/src/shell/trust/gate.tsx`: the offer under the recovery form, the
  cost page, the waiting state with the time left
- `web/src/shell/trust/devices.tsx`: on a trusted device, the pending reset
  at the top of *Devices and machines* with *This wasn't me: cancel*
- The notice text says when it started and when it finishes

## Tests

- Control: start needs a fresh GitHub sign-in; a second start is refused;
  an active device refuses it; a cancel signed by a trusted device or a
  recovery code works, by a revoked one or a stranger doesn't; a due reset
  finishes on the next read; the week limit holds across support and
  self-service
- e2e: a browser with no devices starts it, a second browser of the account
  cancels it; with the clock moved past `due`, the browser enrolls as the
  first device

## Open questions

1. **48 hours.** Long enough for someone to notice, short enough to wait
   out. Shorter for an account with no other device at all?
2. **The active-device refusal.** It stops a stolen session from resetting
   an account someone is using, and stops nothing for an account that's
   really stranded. Is 48 hours of quiet the right test?
3. **Email first?** Ship with notices and pushes, add email when #1609
   lands, or hold self-service until email exists?
4. **Teams.** Should a team's owners hear when a member resets, since they
   see the key change anyway?
