# Infisical's answers, for the projection's fakes (M235)

`answers.json` is what the fake Infisical in `crates/platform-agents/src/infisical_tests.rs`
and the testnet's (`testnet/infisical-fake.mjs`) answer. Nothing in it is invented:

- **The requests** the platform sends are redone's (`backends.mjs` `infisicalClient`,
  arugula-salad/redone at a4b1b49), which writes through the `planter-apps-secret-writer`
  identity in production today: `POST /api/v1/auth/universal-auth/login`,
  `GET /api/v1/folders?workspaceId&environment&path=/`, `POST /api/v1/folders`,
  `POST` then (on "already exist") `PATCH /api/v3/secrets/raw/<KEY>`.
- **The success shapes** are Infisical's API reference (infisical.com/docs/api-reference,
  read 2026-10-23): universal-auth login's `{accessToken, expiresIn, accessTokenMaxTTL,
  tokenType}`, a folder's fields, and a secret's `{secret: {id, _id, workspace,
  environment, version, type, secretKey, secretValue, secretComment, …}}`.
- **The error shape** is the reference's `{reqId, statusCode, message, error}`, with the
  messages redone's tests (`backends.test.mjs`) say the real service sends when a folder or a
  secret is already there: "Folder already exists", "Secret already exist".

The ids and times are placeholders; `{name}`, `{project}`, `{environment}`, `{key}` and
`{value}` are filled from the request, as Infisical echoes them.
