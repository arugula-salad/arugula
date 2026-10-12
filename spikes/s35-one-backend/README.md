# S35: one crypto backend, and tower for the hyper proxies (#463)

A verdict, not a spike with code: nothing here ships, and the throwaway patches used to measure were
not committed. Run 2026-10-11 on geek, from main at 41bdea32. Part of the architecture tracker (#387).

## Answers

- **Crypto: no-go, in both directions.**
  - Moving our RustCrypto calls to aws-lc-rs wouldn't change the wire or stored keys. But it wouldn't
    leave one backend either: russh and ssh-key keep the RustCrypto curve stack in both binaries. Our
    own calls are 89 kB and about 3 s of CPU.
  - Dropping aws-lc-rs instead would save 2.5 MB per binary and 62 s of CPU, but almost no wall time,
    and it costs TLS's post-quantum key exchange and a safe RSA signer.
- **hyper: no-go.** All four proxies use things tower and axum don't have: an HTTP client over a
  stream that isn't TCP, upgrades other than WebSocket, and a per-connection server. The direct
  dependency costs nothing, because axum, reqwest, instant-acme and rmcp already pull in hyper and
  hyper-util with the same features.

No follow-up is needed. Two optional small ones are listed at the end.

## The numbers

One clean release build of `-p arugulad -p arugula-control` together (as `just static` builds them),
with `--timings`, `nice -n 19`, `-j 6`, on glibc. The other agents' builds were running at the same
time (load average 9–11 on 32 threads), so the times are from one noisy run. The CPU times per crate
are solid. The wall time is only a rough figure. `web/dist` was a one-line placeholder, so the
binaries don't include the web client.

- **Clean build:** 175 s of wall time. The binaries are 61.4 MB (arugulad) and 24.3 MB
  (arugula-control). Built on its own, arugula-control is 21.7 MB: built together, the two share one
  feature set.
- **Bytes per library:** from relinking each binary with an lld `-Map`. This undercounts generic
  code, which is counted in the crate that instantiates it.

| | arugulad | arugula-control | build, CPU (when) |
|---|---|---|---|
| aws-lc (C) + aws-lc-rs | 2,548 kB | 2,547 kB | 62.4 s build script (5.7–68 s) + 2 s |
| rustls + webpki (need a provider) | 949 kB | 760 kB | 10.4 s |
| **our RustCrypto calls** (p256, ed25519-dalek, aes-gcm, hkdf, hmac) | 89 kB | 90 kB | 3.1 s (3–23 s) |
| RustCrypto that russh/ssh-key keep anyway (p384, p521, curve25519-dalek, sha2, aes, …) | 489 kB | 418 kB | — |
| russh + ssh-key | 788 kB | 789 kB | 21 s |
| snow + its older RustCrypto set (0.10 generation) | 109 kB | 2 kB | 5.4 s (10–27 s) |
| hyper + hyper-util | 263 kB | 239 kB | 4.9 s |

**Crates in the tree** (`cargo tree -e normal`; changed manifests, reverted afterwards):

| experiment | arugulad | control | duplicate versions (both) |
|---|---|---|---|
| main today | 325 | 280 | 63 |
| drop our p256/ed25519-dalek/aes-gcm/hkdf/hmac **and** snow (move all of it to aws-lc-rs) | 308 | 263 | 35 |
| also drop russh (not a proposal: it shows what holds RustCrypto in) | 238 | 187 | 29 |

The 17 crates the second row drops are all snow's: snow and its 0.10-generation RustCrypto set
(aes-gcm 0.10, aes 0.8, sha2 0.10, curve25519-dalek 4, chacha20poly1305, blake2 0.10, and their
helper crates). Our direct p256, ed25519-dalek, aes-gcm, hkdf, hmac and sha2 all stay in the tree
through russh, ssh-key and elliptic-curve.

**aws-lc on the critical path.** aws-lc-sys's build script runs from 5.7 s to 68 s, and rustls,
russh and reqwest wait for it. But libghostty-vt-sys (25–72 s) and libsqlite3-sys (41–77 s) finish
about then anyway, so removing aws-lc would cut its 62 s of CPU but only a few seconds of wall time
at `-j 6`. Swapping it for ring would put ring's own C build back.

## Crypto: what moves, and what it buys

**Could the primitives move to aws-lc-rs byte for byte? Yes.** Each is a standard primitive with
fixed outputs or standard encodings, and browsers do the same with WebCrypto:

| use | today | aws-lc-rs equivalent | same bytes? |
|---|---|---|---|
| Noise IK channel (`e2e/channel.rs`, browser `e2e/noise.ts`): `Noise_IK_25519_AESGCM_SHA256` | snow, default resolver | a custom snow `CryptoResolver`: `agreement::PrivateKey::from_private_key(&X25519, …)`, `aead::AES_256_GCM`, `digest::SHA256` | yes, the protocol is fixed |
| Device signing keys (`e2e/keys.rs`, raw 32-byte seed in the key file; browsers use PKCS#8 Ed25519 with WebCrypto) | ed25519-dalek | `Ed25519KeyPair::from_seed_unchecked` | yes: Ed25519 signatures are deterministic |
| Web Push encryption (`e2e/push.rs`, RFC 8291) | p256 ECDH + hkdf + aes-gcm (AES-128) | `agreement::ECDH_P256`, `hkdf::HKDF_SHA256`, `aead::AES_128_GCM` | yes |
| VAPID JWTs (`daemon/push.rs`, `control/push.rs`; raw 32-byte scalar on disk / in the db) | p256 ECDSA | `EcdsaKeyPair::from_private_key_and_public_key` (derive the public point first), `ECDSA_P256_SHA256_FIXED_SIGNING` | the key yes; signatures differ (p256 uses RFC 6979, aws-lc randomises), and both are valid |
| Sealed synced history (`daemon/seal.rs`) | hkdf + AES-256-GCM | `hkdf` + `aead::AES_256_GCM` with the same nonce and AAD layout | yes |
| Passkeys ES256/Ed25519 (`control/passkey.rs`) | p256, ed25519-dalek | `UnparsedPublicKey` with `ECDSA_P256_SHA256_ASN1` / `ED25519` (RS256 is already aws-lc-rs) | verification only |
| Cert/MCP-token HMACs (`e2e/cert.rs`, `daemon/mcp/tokens.rs`) | hmac + sha2 | `hmac::HMAC_SHA256` | yes |

The frozen-bytes tests (`e2e/src/frozen.rs`, #504) and the web interop tests would catch any slip.
aws-lc-rs's HKDF API is awkward for arbitrary output lengths (you need a `KeyType`), and the snow
resolver means about 120 lines of our own crypto glue on the channel browsers talk to.

**Why it's still a no-go.** The goal was one backend, and this doesn't get there. Both binaries
link russh (control always; the daemon through the `labs` default for GitLab/Forgejo SSH). Even with
russh's `aws-lc-rs` feature, russh and ssh-key depend on p256, p384, p521, ed25519-dalek,
curve25519-dalek, aes, hmac and sha2. So the move saves 89 kB and 3 s of CPU that runs off the
critical path. The only real reduction is snow's older RustCrypto set (17 crates, 109 kB, 5.4 s of
CPU, also off the critical path), and getting it means writing our own crypto resolver for the E2E
channel. The risk is in the most sensitive code we have, for a benefit you can barely measure.

**The reverse (drop aws-lc-rs): no-go.**
- rustls, tokio-rustls, reqwest, instant-acme, rcgen and russh all need a provider.
  - ring is the only mature alternative, and it's a C/asm build of its own.
  - ring has no static X25519 keys, so snow would stay on RustCrypto.
  - ring has no `X25519MLKEM768`, which aws-lc-rs gives rustls by default (`prefer-post-quantum`).
    Dropping it changes the TLS handshake every client negotiates with us.
- rustls-rustcrypto isn't production-ready.
- Control signs GitHub App JWTs with RSA (`control/forge.rs`). The pure-Rust `rsa` crate still has
  the unfixed Marvin timing advisory (RUSTSEC-2023-0071).
- The savings are 2.5 MB and 62 s of CPU, but only a few seconds of wall time, because aws-lc builds
  while libghostty-vt-sys and libsqlite3-sys do.

**When to look again:** a snow release on the 0.11-generation RustCrypto crates (that would remove
the 17 duplicates with no code from us), or russh dropping RustCrypto under its `aws-lc-rs` feature.

## hyper: what each proxy needs

| file | what it does with hyper | tower or axum equivalent? |
|---|---|---|
| `dial.rs` (`forward`) | `client::conn::http1::handshake` over a mux stream inside a WebSocket, sends the request, splices a `101` with `upgrade::on` on both sides | none: tower and axum have no HTTP client; reqwest can't run over an arbitrary `AsyncRead + AsyncWrite` |
| `labs/provider_tunnel.rs` (`tunnel`) | the same, over the provider's connection to a sandbox | none, as above |
| `sites.rs` (`serve`, `Upstream`, `handle`) | its own listener with TLS and a `tailscale whois` check per connection; `server::conn::http1` with upgrades; **one kept upstream `SendRequest` per browser connection**, with `try_send_request` to retry an unsent request | `axum::serve` gives peer addresses, but not TLS, not state per connection, and not upstream affinity per connection |
| `editor/link.rs` (`connect`) | a `101` to the editor's own line protocol (not WebSocket), then `upgrade::on` for raw IO | axum's only upgrade is `WebSocketUpgrade` |

The direct dependency costs nothing. hyper 1.11 and hyper-util 0.1 are already in the tree through
axum, reqwest, instant-acme and rmcp, and other crates turn on the same `client`, `server` and
`http1` features. Removing the direct dependency would remove no crate and no feature.

## Follow-ups

None required. Two optional ones, each small:

1. **One proxy helper for the three hyper clients.** `dial.rs`, `provider_tunnel.rs` and `sites.rs`
   each repeat the same code: handshake, spawn the connection with upgrades, send, and on `101`
   splice both upgrades with `copy_bidirectional`. That's about 25 lines in each. A shared
   `proxy::send_over(io, req)` and `proxy::splice(client, res)` would keep hyper and remove the
   duplication. Cleanup only.
2. **snow without blake2.** Listing snow's features as `use-aes-gcm`, `use-chacha20poly1305`,
   `use-sha2`, `use-curve25519` and `use-getrandom` drops blake2 0.10 (one crate, about 14 kB),
   with no change on the wire. Dropping chacha20poly1305 too doesn't compile in snow 0.10.0: with
   `use-aes-gcm` alone, `Aes256Gcm::new` doesn't resolve. That's upstream, and a report to snow
   would be outward-facing, so it's Jake's call.
