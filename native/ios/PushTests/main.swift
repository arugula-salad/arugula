// Shared/WebPush.swift against RFC 8291's example (Appendix A), the one the
// daemon's own encrypting is checked against byte for byte
// (crates/daemon/src/push.rs, rfc8291_example), and a notification as a
// daemon sends one. Built and run on a Mac by run.sh.

import CryptoKit
import Foundation

var failed = 0
func check(_ ok: Bool, _ what: String) {
    print(ok ? "ok   \(what)" : "FAIL \(what)")
    if !ok { failed += 1 }
}

func b64(_ s: String) -> Data { Data(base64URL: s)! }

// RFC 8291 Appendix A: the receiver's keys, and what the sender sent.
let uaPrivate = b64("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94")
let uaPublic = "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"
let auth = b64("BTBZMqHH6r4Tts7J_aSIgg")
let body = b64(
    "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
)
let keys = PushKeys.Keys(ecdh: try P256.KeyAgreement.PrivateKey(rawRepresentation: uaPrivate), auth: auth)
check(keys.p256dh == uaPublic, "the subscription's p256dh is the receiver's public key")
check(Data(base64URL: keys.authText) == auth, "and auth is its secret, base64url")

let plain = try WebPush.decrypt(body, keys: keys)
check(String(decoding: plain, as: UTF8.self) == "When I grow up, I want to be a watermelon", "RFC 8291's example decrypts")

// Anything changed fails: the tag, another subscription's secret.
var bad = [UInt8](body)
bad[bad.count - 1] ^= 1
check((try? WebPush.decrypt(Data(bad), keys: keys)) == nil, "a changed byte doesn't decrypt")
let other = PushKeys.Keys(ecdh: keys.ecdh, auth: Data(repeating: 7, count: 16))
check((try? WebPush.decrypt(body, keys: other)) == nil, "another secret doesn't decrypt")
check((try? WebPush.decrypt(body.prefix(40), keys: keys)) == nil, "a short body doesn't")

// A daemon's notification, encrypted by arugula_e2e::push::encrypt for these
// keys with the RFC's sender key and salt (crates/e2e/src/push.rs,
// the_iphone_apps_example, checks it's what the daemon makes).
let fixture = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_e3c9cFr7_NBuenescvlYf5Grx9_cXCMaB6jpGw4jlRbTLVj1jFGvJQhKtjFONkFoXCqdCsknxnqpyt5HaaijivSrqnYoAV6nZYqUtFGYK-r132x169JHllw6vmltNz-cPpCqU"
let said = try WebPush.decrypt(b64(fixture), keys: keys)
let v = try JSONSerialization.jsonObject(with: said) as? [String: Any]
check(v?["title"] as? String == "Needs you", "a daemon's notification decrypts: \(String(decoding: said, as: UTF8.self))")
check(v?["pane"] as? Int == 3 && v?["daemon"] as? String == "d1", "with its pane and machine")

if failed > 0 {
    print("\(failed) failed")
    exit(1)
}
print("push: every check passed")
