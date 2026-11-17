// chant (0.124.70) reads GitHub's SSH host keys for the Homebrew tap from
// `GET {api}/meta` without a token, so the request shares the runner's IP's
// unauthenticated rate limit, and a 403 there failed v0.70.10's and
// v0.70.20's publish ("gave no SSH host keys ... it answered 403"). Loaded
// into `chant release publish` with NODE_OPTIONS=--import (release.yml's
// publish job), this sends that one request with the job's GITHUB_TOKEN, to
// the API it already talks to. Every other request goes out untouched.
// Drop it once chant sends a token there itself.
const token = process.env.GITHUB_TOKEN;
const meta = `${(process.env.GITHUB_API_URL || "https://api.github.com").replace(/\/+$/, "")}/meta`;

if (token && typeof globalThis.fetch === "function") {
  const fetch = globalThis.fetch;
  globalThis.fetch = (input, init) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input?.url;
    if (url !== meta) return fetch(input, init);
    const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
    if (!headers.has("authorization")) headers.set("authorization", `Bearer ${token}`);
    return fetch(input, { ...init, headers });
  };
}
