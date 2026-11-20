// Push (#1260): the app subscribes this device at APNs with keys of its own,
// as a browser subscribes at its push service, and a tap on a notification
// opens what it's about. The Notification Service Extension (ArugulaNotify)
// decrypts what arrives with those keys (Shared/WebPush.swift).

import SwiftUI
import UIKit
import UserNotifications

/// What a tapped notification opens, from what it said: a pane on a machine
/// (an agent's question, a terminal that wants you), a chat room, an issue.
enum PushRoute: Equatable {
    case pane(machine: String, pane: UInt32)
    case room(String)
    case issue(String)

    init?(said: [String: Any]) {
        if let machine = said["daemon"] as? String, let pane = said["pane"] as? Int, pane > 0 {
            self = .pane(machine: machine, pane: UInt32(pane))
        } else if let chat = said["chat"] as? [String: Any], let room = chat["room"] as? String {
            self = .room(room)
        } else if let inbox = said["inbox"] as? [String: Any] {
            if let item = inbox["item"] as? String, item.hasPrefix("iss_") {
                self = .issue(item)
            } else if let room = inbox["room"] as? String {
                self = .room(room)
            } else {
                return nil
            }
        } else {
            return nil
        }
    }
}

/// This device's subscription, and where a tap goes.
@MainActor
final class PushCenter: ObservableObject {
    static let shared = PushCenter()

    /// Where the last tap goes, until the view it opens takes it.
    @Published var route: PushRoute?
    private var control: Control?
    private var token: String?
    private static let subscribed = "arugula.push-endpoint"

    /// APNs' host for this build: a development build's device token is
    /// the sandbox's. A build signed for TestFlight or the App Store has no
    /// embedded profile, and is production's.
    static let host: String = {
        guard let url = Bundle.main.url(forResource: "embedded", withExtension: "mobileprovision"),
              let raw = try? Data(contentsOf: url), let text = String(data: raw, encoding: .isoLatin1),
              let start = text.range(of: "<?xml"), let end = text.range(of: "</plist>"),
              let plist = try? PropertyListSerialization.propertyList(from: Data(text[start.lowerBound..<end.upperBound].utf8), format: nil) as? [String: Any],
              let entitlements = plist["Entitlements"] as? [String: Any],
              entitlements["aps-environment"] as? String == "development"
        else { return "api.push.apple.com" }
        return "api.sandbox.push.apple.com"
    }()

    /// Signed in: ask to notify, once, and have APNs hand this device's
    /// token (`registered`), which is subscribed with control.
    func start(_ control: Control) {
        self.control = control
        Task {
            let center = UNUserNotificationCenter.current()
            guard (try? await center.requestAuthorization(options: [.alert, .sound, .badge])) == true else { return }
            UIApplication.shared.registerForRemoteNotifications()
            if token != nil { await subscribe() }
        }
    }

    /// APNs' token for this device.
    func registered(_ token: Data) {
        self.token = token.map { String(format: "%02x", $0) }.joined()
        Task { await subscribe() }
    }

    /// Signing out: this device hears nothing more for the account.
    func stop(_ control: Control) async {
        self.control = nil
        guard let endpoint = UserDefaults.standard.string(forKey: Self.subscribed) else { return }
        _ = try? await control.call("POST", "/api/push/unsubscribe", body: ["endpoint": endpoint])
        UserDefaults.standard.removeObject(forKey: Self.subscribed)
    }

    /// Subscribe the device's endpoint with control, with this install's
    /// keys. Again on each start: control keeps one per endpoint.
    private func subscribe() async {
        guard let control, let token else { return }
        let endpoint = "https://\(Self.host)/3/device/\(token)"
        do {
            let keys = try PushKeys.loadOrMake()
            let me = try await control.get("/api/me", as: Me.self)
            let sub: [String: Any] = [
                "v": 1, "account": me.account, "device": "", "endpoint": endpoint,
                "p256dh": keys.p256dh, "auth": keys.authText, "at": Int(Date().timeIntervalSince1970 * 1000), "sig": "",
            ]
            _ = try await control.call("POST", "/api/push/subscribe", body: ["sub": sub])
            UserDefaults.standard.set(endpoint, forKey: Self.subscribed)
        } catch {
            // Control without APNs (`apns_off`), or offline: the next start
            // tries again.
            print("push: not subscribed: \(describe(error))")
        }
    }

    /// A notification was tapped.
    func open(_ info: [AnyHashable: Any]) {
        guard let text = info["said"] as? String,
              let said = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] else { return }
        route = PushRoute(said: said)
    }

    private struct Me: Decodable { var account: String }
}

/// The app's delegate, for what only UIKit hands over: the device token and
/// notifications as they're shown and tapped.
final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    func application(_ app: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        return true
    }

    func application(_ app: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken token: Data) {
        MainActor.assumeIsolated { PushCenter.shared.registered(token) }
    }

    func application(_ app: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
        print("push: APNs gave no token: \(error.localizedDescription)")
    }

    /// Shown while the app is open too.
    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent n: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .list, .sound]
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        await MainActor.run { PushCenter.shared.open(info) }
    }
}
