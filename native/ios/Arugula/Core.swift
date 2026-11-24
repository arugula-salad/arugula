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

/// The project the app shows, as the core's rule reads it (Arugula #1355):
/// Machines and Home show only its sessions; one placed nowhere is yours, on
/// your own machine, in your personal project.
struct Shown: Equatable {
    var id: String
    /// The reader's personal project.
    var personal: Bool

    /// Whether a pane (its session's project and its card's) or a session
    /// (no card) on a machine that's `yours` or not shows in `shown`; with
    /// none, every one does.
    static func shows(_ shown: Shown?, project: String?, card: String? = nil, yours: Bool) -> Bool {
        aru_shown(shown?.id, shown?.personal == true ? 1 : 0, project, card, yours ? 1 : 0) != 0
    }
}

extension Projects.Project {
    var shown: Shown { Shown(id: id, personal: team == nil) }
}

/// One machine as the core's `aru_machines` reads it.
struct HostIn {
    var id: String
    var name: String
    var online: Bool
    /// Whose it is when it isn't yours.
    var owner: String?
    /// Its sessions, as `aru_link_panes` gave them.
    var panes: String?
}

/// What `aru_machines` answers: every machine in the phone's order, and
/// each one's sessions that show in the project.
struct Sections: Decodable {
    struct Section: Decodable {
        var id: String
        var sessions: [Card]
    }
    struct Card: Decodable {
        struct Tab: Decodable { var id: UInt32; var panes: [UInt32] }
        var id: UInt32
        var tabs: [Tab]
        var panes: Int
        var needs_you: Int
    }
    var order: [Section]
    var machines: [Section]

    /// From the core, for `hosts` in `shown`; `recent` is the machine used last.
    static func of(_ hosts: [HostIn], shown: Shown?, recent: String?) -> Sections? {
        let hs: [[String: Any]] = hosts.map { h in
            var o: [String: Any] = ["id": h.id, "name": h.name, "online": h.online]
            if let owner = h.owner { o["owner"] = owner }
            if let p = h.panes, let v = try? JSONSerialization.jsonObject(with: Data(p.utf8)) { o["panes"] = v }
            return o
        }
        var input: [String: Any] = ["hosts": hs]
        if let shown { input["at"] = ["id": shown.id, "personal": shown.personal] }
        if let recent { input["recent"] = recent }
        guard let data = try? JSONSerialization.data(withJSONObject: input) else { return nil }
        var err: UnsafeMutablePointer<CChar>?
        guard let s = takeString(aru_machines(String(decoding: data, as: UTF8.self), &err)) else {
            if let err { aru_string_free(err) }
            return nil
        }
        return try? JSONDecoder().decode(Sections.self, from: Data(s.utf8))
    }
}

extension Notification.Name {
    /// Control said the session has ended: the app asks for a sign-in again.
    static let arugulaSignedOut = Notification.Name("arugulaSignedOut")
}

/// Whether control at `url` serves this build of the app (#603): false when it
/// says to update the app, nil when it couldn't tell (offline, say).
func appServed(at url: String) async -> Bool? {
    await Task.detached(priority: .utility) {
        var err: UnsafeMutablePointer<CChar>?
        if aru_check_app(url, &err) == 1 { return true }
        let e = err.map(CoreError.init(taking:))
        return e?.reason == "update_app" ? false : nil
    }.value
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
    /// Where control is, as the session was kept: its page opens here
    /// (Approvals.swift).
    let url: String?

    init(stored: String) throws {
        var err: UnsafeMutablePointer<CChar>?
        guard let p = aru_control_open(stored, &err) else { throw err.map(CoreError.init(taking:)) ?? CoreError("not signed in") }
        ptr = p
        let v = (try? JSONSerialization.jsonObject(with: Data(stored.utf8))) as? [String: Any]
        url = v?["url"] as? String
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
