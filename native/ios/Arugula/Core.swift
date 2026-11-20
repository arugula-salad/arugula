// The Rust core (native/core), as Swift calls it, and where its sign-in is kept.

import Foundation
import Security

struct CoreError: LocalizedError {
    var status: Int?
    var error: String
    var reason: String?

    var errorDescription: String? { error }

    /// From the JSON the core sets in `*err`, freeing it.
    init(taking p: UnsafeMutablePointer<CChar>) {
        defer { aru_string_free(p) }
        let v = (try? JSONSerialization.jsonObject(with: Data(String(cString: p).utf8))) as? [String: Any]
        status = v?["status"] as? Int
        error = v?["error"] as? String ?? String(cString: p)
        reason = v?["reason"] as? String
    }

    init(_ message: String) { error = message }
}

/// A C string the core allocated, as Swift's, freeing it.
func takeString(_ p: UnsafeMutablePointer<CChar>?) -> String? {
    guard let p else { return nil }
    defer { aru_string_free(p) }
    return String(cString: p)
}

extension Notification.Name {
    /// Control said the session has ended: the app asks for a sign-in again.
    static let arugulaSignedOut = Notification.Name("arugulaSignedOut")
}

/// The sign-in: where, and the session control gave. In the keychain, on this
/// device only and never synced.
enum SessionStore {
    private static func query(_ account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "io.arugula.ios",
            kSecAttrAccount as String: account,
        ]
    }

    static func load() -> String? {
        var q = query("session")
        q[kSecReturnData as String] = true
        var out: AnyObject?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let d = out as? Data else { return nil }
        return String(decoding: d, as: UTF8.self)
    }

    static func save(_ stored: String) throws {
        forget()
        var q = query("session")
        q[kSecValueData as String] = Data(stored.utf8)
        q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let s = SecItemAdd(q as CFDictionary, nil)
        guard s == errSecSuccess else { throw CoreError("couldn't keep the sign-in (\(s))") }
    }

    static func forget() { SecItemDelete(query("session") as CFDictionary) }

    /// What an app from before signing in kept (#999): a paired device. Gone
    /// once read: the app signs in instead. True if there was one.
    static func forgetPairedDevice() -> Bool {
        SecItemDelete(query("device") as CFDictionary) == errSecSuccess
    }
}

/// The app's requests to control, with its session. The calls block, so they
/// run off the main thread.
final class Control: @unchecked Sendable {
    let ptr: OpaquePointer

    init(stored: String) throws {
        var err: UnsafeMutablePointer<CChar>?
        guard let p = aru_control_open(stored, &err) else { throw err.map(CoreError.init(taking:)) ?? CoreError("not signed in") }
        ptr = p
    }

    deinit { aru_control_free(ptr) }

    func call(_ method: String, _ path: String, body: [String: Any]? = nil) async throws -> Data {
        let json = try body.map { String(decoding: try JSONSerialization.data(withJSONObject: $0), as: UTF8.self) }
        do {
            return try await Task.detached(priority: .userInitiated) { [self] in
                var err: UnsafeMutablePointer<CChar>?
                let got: UnsafeMutablePointer<CChar>?
                if let json {
                    got = aru_control_call(ptr, method, path, json, &err)
                } else {
                    got = aru_control_call(ptr, method, path, nil, &err)
                }
                guard let s = takeString(got) else { throw err.map(CoreError.init(taking:)) ?? CoreError("no answer") }
                return Data(s.utf8)
            }.value
        } catch let e as CoreError where e.reason == "signed_out" {
            NotificationCenter.default.post(name: .arugulaSignedOut, object: self)
            throw e
        }
    }

    func get<T: Decodable>(_ path: String, as: T.Type = T.self) async throws -> T {
        try JSONDecoder().decode(T.self, from: try await call("GET", path))
    }

    /// End the session at control, as far as it can be reached.
    func signOut() async {
        await Task.detached(priority: .userInitiated) { [self] in aru_control_sign_out(ptr) }.value
    }
}

/// A sign-in waiting for someone signed in to Arugula to allow it in a
/// browser: control has the ticket, and the core keeps the verifier.
final class SignIn: @unchecked Sendable {
    let ptr: OpaquePointer
    /// Control's page, where they click Allow.
    let link: URL
    /// The scheme the browser's Allow sends the answer to.
    let callbackScheme: String
    let ticket: String

    init(url: String, name: String) async throws {
        ptr = try await Task.detached(priority: .userInitiated) {
            var err: UnsafeMutablePointer<CChar>?
            guard let p = aru_signin_start(url, name, &err) else { throw err.map(CoreError.init(taking:)) ?? CoreError("no answer") }
            return p
        }.value
        let info = takeString(aru_signin_info(ptr)) ?? "{}"
        let v = (try? JSONSerialization.jsonObject(with: Data(info.utf8))) as? [String: Any]
        callbackScheme = v?["callback_scheme"] as? String ?? "arugula"
        ticket = v?["ticket"] as? String ?? ""
        guard let link = URL(string: v?["link"] as? String ?? "") else { throw CoreError("control sent no link") }
        self.link = link
    }

    deinit { aru_signin_free(ptr) }

    /// What to keep, from the address the browser's Allow came back with.
    func finish(_ callback: URL) async throws -> String {
        try await Task.detached(priority: .userInitiated) { [self] in
            var err: UnsafeMutablePointer<CChar>?
            guard let s = takeString(aru_signin_finish(ptr, callback.absoluteString, &err)) else {
                throw err.map(CoreError.init(taking:)) ?? CoreError("no answer")
            }
            return s
        }.value
    }
}
