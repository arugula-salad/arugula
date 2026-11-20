// Push's keys and their decrypting (#1260), shared by the app and its
// Notification Service Extension.
//
// The app subscribes its device at APNs as a browser subscribes at its push
// service: with an ECDH key and a secret of its own (RFC 8291's p256dh and
// auth). Daemons and control encrypt each notification for those keys, and
// control hands APNs only the ciphertext, which the extension decrypts here
// before the notification is shown. The keys stay in a keychain group the app
// and the extension share, on this device only.

import CryptoKit
import Foundation
import Security

/// The subscription's keys, kept in the keychain group the app and its
/// extension share.
enum PushKeys {
    /// The shared group, as both targets' entitlements name it: the team's
    /// prefix and the app's bundle ID.
    static let group = "HNDPMQPF22.io.arugula.ios.shared"
    private static let service = "io.arugula.ios.push"

    struct Keys {
        let ecdh: P256.KeyAgreement.PrivateKey
        let auth: Data

        /// As a subscription carries them: base64url, with no padding.
        var p256dh: String { ecdh.publicKey.x963Representation.base64URL }
        var authText: String { auth.base64URL }
    }

    private static func query(_ account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrAccessGroup as String: group,
        ]
    }

    private static func read(_ account: String) -> Data? {
        var q = query(account)
        q[kSecReturnData as String] = true
        var out: AnyObject?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess else { return nil }
        return out as? Data
    }

    private static func write(_ account: String, _ data: Data) -> OSStatus {
        SecItemDelete(query(account) as CFDictionary)
        var q = query(account)
        q[kSecValueData as String] = data
        // The extension runs while the phone is locked, once it's been
        // unlocked since it started.
        q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        return SecItemAdd(q as CFDictionary, nil)
    }

    /// The keys, if the app has made them.
    static func load() -> Keys? {
        guard let raw = read("p256"), let auth = read("auth"), auth.count == 16,
              let key = try? P256.KeyAgreement.PrivateKey(rawRepresentation: raw) else { return nil }
        return Keys(ecdh: key, auth: auth)
    }

    /// The keys, made and kept the first time.
    static func loadOrMake() throws -> Keys {
        if let k = load() { return k }
        let key = P256.KeyAgreement.PrivateKey()
        var auth = Data(count: 16)
        let ok = auth.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, 16, $0.baseAddress!) }
        guard ok == errSecSuccess else { throw WebPush.Failure("no random bytes for push's keys") }
        for (name, data) in [("p256", key.rawRepresentation), ("auth", auth)] {
            let s = write(name, data)
            guard s == errSecSuccess else { throw WebPush.Failure("couldn't keep push's keys (\(s))") }
        }
        return Keys(ecdh: key, auth: auth)
    }
}

/// RFC 8291's aes128gcm, the receiving side: one record, as daemons and
/// control send them.
enum WebPush {
    struct Failure: LocalizedError {
        let errorDescription: String?
        init(_ s: String) { errorDescription = s }
    }

    /// The plaintext of `body` (the RFC 8188 header, then one record), for
    /// the subscription whose keys are `keys`.
    static func decrypt(_ body: Data, keys: PushKeys.Keys) throws -> Data {
        let b = [UInt8](body)
        guard b.count > 21 else { throw Failure("too short") }
        let salt = Data(b[0..<16])
        let rs = b[16..<20].reduce(0) { $0 << 8 | Int($1) }
        let idLen = Int(b[20])
        guard b.count >= 21 + idLen + 17 else { throw Failure("too short") }
        let asPublic = Data(b[21..<21 + idLen])
        let sealed = Data(b[(21 + idLen)...])
        guard sealed.count <= rs else { throw Failure("more than one record") }

        let sender = try P256.KeyAgreement.PublicKey(x963Representation: asPublic)
        let shared = try keys.ecdh.sharedSecretFromKeyAgreement(with: sender)
        // IKM = HKDF(auth, ECDH, "WebPush: info\0" || ua_public || as_public, 32)
        var info = Data("WebPush: info\0".utf8)
        info.append(keys.ecdh.publicKey.x963Representation)
        info.append(asPublic)
        let ikm = shared.hkdfDerivedSymmetricKey(using: SHA256.self, salt: keys.auth, sharedInfo: info, outputByteCount: 32)
        let cek = HKDF<SHA256>.deriveKey(inputKeyMaterial: ikm, salt: salt, info: Data("Content-Encoding: aes128gcm\0".utf8), outputByteCount: 16)
        let nonce = HKDF<SHA256>.deriveKey(inputKeyMaterial: ikm, salt: salt, info: Data("Content-Encoding: nonce\0".utf8), outputByteCount: 12)
        let box = try AES.GCM.SealedBox(
            nonce: AES.GCM.Nonce(data: nonce.withUnsafeBytes { Data($0) }),
            ciphertext: sealed.dropLast(16),
            tag: sealed.suffix(16)
        )
        let padded = [UInt8](try AES.GCM.open(box, using: cek))
        // The last record ends in 2, then any zeros of padding.
        guard let end = padded.lastIndex(where: { $0 != 0 }), padded[end] == 2 else { throw Failure("not the last record") }
        return Data(padded[..<end])
    }
}

extension Data {
    var base64URL: String {
        base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    init?(base64URL s: String) {
        var t = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        t += String(repeating: "=", count: (4 - t.count % 4) % 4)
        self.init(base64Encoded: t)
    }
}
