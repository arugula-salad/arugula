// The Rust core (native/core), as Swift calls it, and where its device is kept.

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

/// The device: its keys, certificate and account root. In the keychain, on this
/// device only and never synced (aru-131).
enum DeviceStore {
    private static let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "io.arugula.ios",
        kSecAttrAccount as String: "device",
    ]

    static func load() -> String? {
        var q = query
        q[kSecReturnData as String] = true
        var out: AnyObject?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let d = out as? Data else { return nil }
        return String(decoding: d, as: UTF8.self)
    }

    static func save(_ stored: String) throws {
        forget()
        var q = query
        q[kSecValueData as String] = Data(stored.utf8)
        q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let s = SecItemAdd(q as CFDictionary, nil)
        guard s == errSecSuccess else { throw CoreError("couldn't save this device's keys (\(s))") }
    }

    static func forget() { SecItemDelete(query as CFDictionary) }
}

/// A paired device's requests to control. The calls block, so they run off the
/// main thread.
final class Control: @unchecked Sendable {
    let ptr: OpaquePointer

    init(stored: String) throws {
        var err: UnsafeMutablePointer<CChar>?
        guard let p = aru_control_open(stored, &err) else { throw err.map(CoreError.init(taking:)) ?? CoreError("no device") }
        ptr = p
    }

    deinit { aru_control_free(ptr) }

    func call(_ method: String, _ path: String, body: [String: Any]? = nil) async throws -> Data {
        let json = try body.map { String(decoding: try JSONSerialization.data(withJSONObject: $0), as: UTF8.self) }
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
    }

    func get<T: Decodable>(_ path: String, as: T.Type = T.self) async throws -> T {
        try JSONDecoder().decode(T.self, from: try await call("GET", path))
    }
}

/// Asking to join: control has the request, and a signed-in browser approves it.
final class Pairing: @unchecked Sendable {
    let ptr: OpaquePointer
    let code: String
    let link: URL

    init(url: String, name: String) async throws {
        ptr = try await Task.detached(priority: .userInitiated) {
            var err: UnsafeMutablePointer<CChar>?
            guard let p = aru_pairing_start(url, name, &err) else { throw err.map(CoreError.init(taking:)) ?? CoreError("no answer") }
            return p
        }.value
        let info = takeString(aru_pairing_info(ptr)) ?? "{}"
        let v = (try? JSONSerialization.jsonObject(with: Data(info.utf8))) as? [String: Any]
        code = v?["code"] as? String ?? ""
        guard let link = URL(string: v?["link"] as? String ?? "") else { throw CoreError("control sent no link") }
        self.link = link
    }

    deinit { aru_pairing_free(ptr) }

    /// What to keep, once approved; nil while still waiting.
    func poll() async throws -> String? {
        try await Task.detached(priority: .userInitiated) { [self] in
            var err: UnsafeMutablePointer<CChar>?
            if let s = takeString(aru_pairing_poll(ptr, &err)) { return s }
            if let err { throw CoreError(taking: err) }
            return nil
        }.value
    }
}
