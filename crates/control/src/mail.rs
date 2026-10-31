//! Control sends email (#1609, docs/design/control-email.md): security
//! notices only, to an account's verified address, and the link that
//! verifies a typed one. Over SMTP, so any provider (Postmark, Resend, a
//! self-hosted server) is a URL away: `--smtp-url
//! smtps://USER:PASS@HOST:465` (ARUGULA_CONTROL_SMTP_URL) and `--mail-from`.
//! Off when unset: nothing is sent, and the account's settings say so.
//!
//! Best-effort: a notice goes out after what it's about committed, in the
//! background, tried three times; nothing waits on it. At most
//! [`DAILY_LIMIT`] an account a day. Control keeps each send's kind and
//! time (`email_sends`), never its body. A reset's notices, and the one an
//! old address gets when it's changed, are outside the limit (DK10,
//! [`send_security`]); a passkey reset's start waits for its email to be
//! taken ([`notice_now`]).

use std::sync::{Arc, Mutex};

use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::Mailbox};
use tracing::{info, warn};

use crate::App;

/// Notices an account gets a day, at most.
pub const DAILY_LIMIT: u64 = 5;
const DAY_MS: u64 = 24 * 3600 * 1000;

/// One email, as the fake mailer keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// An SMTP server and the From address.
pub struct Smtp {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

/// How control sends: over SMTP, not at all, or (tests) into a list.
#[derive(Clone, Default)]
pub enum Mailer {
    #[default]
    Off,
    Smtp(Arc<Smtp>),
    Fake(Arc<Mutex<Vec<Sent>>>),
    /// (Tests.) On, and every send fails, as an SMTP server that's down.
    #[cfg(test)]
    Failing,
}

impl Mailer {
    /// From `--smtp-url` and `--mail-from`: both, or off.
    pub fn from_config(url: Option<&str>, from: Option<&str>) -> anyhow::Result<Self> {
        match (url, from) {
            (Some(url), Some(from)) => Ok(Mailer::Smtp(Arc::new(Smtp {
                transport: AsyncSmtpTransport::<Tokio1Executor>::from_url(url)?.build(),
                from: from.parse()?,
            }))),
            (None, None) => Ok(Mailer::Off),
            _ => anyhow::bail!("--smtp-url and --mail-from go together"),
        }
    }

    pub fn on(&self) -> bool {
        !matches!(self, Mailer::Off)
    }

    async fn send(&self, to: &str, subject: &str, body: &str) -> anyhow::Result<()> {
        match self {
            Mailer::Off => Ok(()),
            Mailer::Fake(sent) => {
                sent.lock().unwrap().push(Sent { to: to.into(), subject: subject.into(), body: body.into() });
                Ok(())
            }
            #[cfg(test)]
            Mailer::Failing => anyhow::bail!("the mail server is down"),
            Mailer::Smtp(s) => {
                let m = Message::builder()
                    .from(s.from.clone())
                    .to(to.parse()?)
                    .subject(subject)
                    .header(lettre::message::header::ContentType::TEXT_PLAIN)
                    .body(body.to_owned())?;
                s.transport.send(m).await?;
                Ok(())
            }
        }
    }
}

/// Whether `s` looks like an address worth sending a link to: one `@`,
/// something either side, a dot in the domain, no spaces, at most 254.
pub fn plausible(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else { return false };
    s.len() <= 254
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && !s.chars().any(char::is_whitespace)
        && s.parse::<Mailbox>().is_ok()
}

/// Send `subject` and `body` to `to` for `account`, as `kind`, unless the
/// mailer is off or the account had its [`DAILY_LIMIT`] today. In the
/// background; whether it was taken.
pub fn send(app: &App, account: &str, to: &str, kind: &'static str, subject: String, body: String) -> bool {
    if !app.mail.on() {
        return false;
    }
    let now = arugula_e2e::now_ms();
    match app.db.sends_since(account, now.saturating_sub(DAY_MS)) {
        Ok(n) if n < DAILY_LIMIT => {}
        Ok(_) => {
            warn!(account, kind, "not emailed: the account's daily limit");
            return false;
        }
        Err(e) => {
            warn!(account, kind, "not emailed: {e}");
            return false;
        }
    }
    if let Err(e) = app.db.record_send(account, kind, now, true) {
        warn!(account, kind, "not emailed: {e}");
        return false;
    }
    spawn_send(app, account, to, kind, subject, body);
    true
}

/// A notice that a reset, or a change of the address itself, happened
/// (DK10, #393): neither counted against [`DAILY_LIMIT`] nor stopped by
/// it, so a session can't use the limit up to keep one from going out.
/// Kept (`email_sends`, unlimited). In the background; whether it was
/// taken.
pub fn send_security(app: &App, account: &str, to: &str, kind: &'static str, subject: String, body: String) -> bool {
    if !app.mail.on() {
        return false;
    }
    if let Err(e) = app.db.record_send(account, kind, arugula_e2e::now_ms(), false) {
        warn!(account, kind, "not emailed: {e}");
        return false;
    }
    spawn_send(app, account, to, kind, subject, body);
    true
}

/// As [`send_security`], to the account's verified address, waiting for the
/// server to take it (three tries): what starting a passkey reset waits on,
/// so one never runs without its warning (DK10). Whether it went.
pub async fn notice_now(app: &Arc<App>, account: &str, kind: &'static str, subject: String, body: String) -> bool {
    if !app.mail.on() {
        return false;
    }
    let to = match app.db.verified_email(account) {
        Ok(Some(to)) => to,
        Ok(None) => return false,
        Err(e) => {
            warn!(account, kind, "not emailed: {e}");
            return false;
        }
    };
    if let Err(e) = app.db.record_send(account, kind, arugula_e2e::now_ms(), false) {
        warn!(account, kind, "not emailed: {e}");
        return false;
    }
    for attempt in 1..=3u32 {
        match app.mail.send(&to, &subject, &body).await {
            Ok(()) => {
                info!(account, kind, "emailed");
                return true;
            }
            Err(e) => {
                warn!(account, kind, attempt, "emailing failed: {e}");
                if attempt < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(500 * u64::from(attempt))).await;
                }
            }
        }
    }
    false
}

fn spawn_send(app: &App, account: &str, to: &str, kind: &'static str, subject: String, body: String) {
    let (mail, to, account) = (app.mail.clone(), to.to_owned(), account.to_owned());
    tokio::spawn(async move {
        for attempt in 1..=3u32 {
            match mail.send(&to, &subject, &body).await {
                Ok(()) => {
                    info!(account, kind, "emailed");
                    return;
                }
                Err(e) => {
                    warn!(account, kind, attempt, "emailing failed: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5 * u64::from(attempt))).await;
                }
            }
        }
    });
}

/// A security notice to the account's verified address, if it has one,
/// outside the daily limit ([`send_security`]): a reset's (DK10).
pub fn security_notice(app: &App, account: &str, kind: &'static str, subject: String, body: String) -> bool {
    match app.db.verified_email(account) {
        Ok(Some(to)) => send_security(app, account, &to, kind, subject, body),
        Ok(None) => false,
        Err(e) => {
            warn!(account, kind, "not emailed: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plausible_addresses() {
        for ok in ["a@b.co", "lex.00+arugula@example.com"] {
            assert!(plausible(ok), "{ok}");
        }
        for no in
            ["", "a", "a@b", "@b.co", "a@.co", "a@b.", "a b@c.co", "a@b@c.co", &format!("{}@b.co", "a".repeat(260))]
        {
            assert!(!plausible(no), "{no}");
        }
    }

    #[test]
    fn both_flags_or_neither() {
        assert!(!Mailer::from_config(None, None).unwrap().on());
        assert!(Mailer::from_config(Some("smtp://localhost:2525"), None).is_err());
        assert!(
            Mailer::from_config(Some("smtp://localhost:2525"), Some("Arugula <security@arugula.io>")).unwrap().on()
        );
    }
}
