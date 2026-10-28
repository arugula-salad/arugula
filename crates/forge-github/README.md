# arugula-forge-github

The GitHub reader: GitHub's REST API onto Arugula's forge model. The daemon's
forge blocks read and write GitHub through it, and the platform will read
linked repositories through it (#1204).

Start at `src/lib.rs`. Then:

- `src/client.rs`: `Github`, a client for one API base. Conditional GETs
  with ETags (`EtagCache`), GitHub's rate limits, the timeline's last page,
  and the writes (comment, review, merge, rerun, a new issue).
- `src/token.rs`: `TokenSource`, where `Github` gets a token for a
  repository and a `Purpose` (read or write). The daemon's is the person's
  `gh` login (`crates/arugulad-forge/src/github.rs`). The platform's will be an
  installation token scoped to one repository, or a one-use user token.
- `src/parse.rs`: GitHub's JSON onto the model, and GitHub's links.
- `src/model.rs`: the normalized model (pull requests, issues, checks,
  reviews, events) and the attention rules. Every forge maps onto it: the
  daemon's Forgejo and GitLab adapters use it too. The types themselves are
  `arugula_proto::forge`'s, which the web client's are made from.
- `src/util.rs`: what every forge adapter shares: timestamps, a short HTTP
  error, a URL's host.

It knows nothing of blocks, `layout.json`, the daemon or the platform. It has
no TLS of its own: the caller hands it a `reqwest::Client`.

Its tests read the recorded `cli/cli` pull requests in
`crates/daemon/tests/fixtures/github/` (S23), which the daemon's integration
test and the web client's e2e serve too.
