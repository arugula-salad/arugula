// Whether a person's words look like a secret, checked as they type a
// to-do's note (Arugula #1468), so it's caught before it's sent. Control
// checks the note again and refuses one (#1462): this is a guess that
// errs toward saying so, never the gate. It says what the text looks like,
// for the field's words, or null.

/** Known tokens' shapes: their prefix, then what follows it. */
const TOKENS: [RegExp, string][] = [
  [/-----BEGIN [A-Z ]*PRIVATE KEY-----/, "a private key"],
  [/\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}/, "a GitHub token"],
  [/\bgithub_pat_[A-Za-z0-9_]{20,}/, "a GitHub token"],
  [/\bglpat-[A-Za-z0-9_-]{20,}/, "a GitLab token"],
  [/\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}/, "an API key"],
  [/\bxox[abposr]-[A-Za-z0-9-]{10,}/, "a Slack token"],
  [/\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/, "an AWS key"],
  [/\bAIza[A-Za-z0-9_-]{35}\b/, "a Google key"],
  [/\bnpm_[A-Za-z0-9]{36}\b/, "an npm token"],
  [/\b(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{16,}/, "a Stripe key"],
  [/\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/, "a token"],
  [/\b[a-z][a-z0-9+.-]*:\/\/[^\s/:@]+:[^\s/@]+@/i, "a password in a link"],
  [/\b(?:password|passwd|pwd|secret|token|api[_-]?key)\s*[:=]\s*\S{6,}/i, "a password"],
];

/** A run of 32 or more characters with no break a word or a path has. */
const RANDOM = /[A-Za-z0-9+_=-]{32,}/g;

export function looksSecret(text: string): string | null {
  for (const [re, what] of TOKENS) if (re.test(text)) return what;
  for (const run of text.match(RANDOM) ?? []) if (random(run)) return "a key";
  return null;
}

/** Lower, upper and digits mixed, as a key is; or hex with both letters
 * and digits, but a commit's hash (40 hex) is said aloud often enough to
 * let pass. */
function random(run: string): boolean {
  if (/[a-z]/.test(run) && /[A-Z]/.test(run) && /[0-9]/.test(run)) return true;
  return /^[0-9a-fA-F]+$/.test(run) && /[0-9]/.test(run) && /[a-fA-F]/.test(run) && run.length !== 40;
}
