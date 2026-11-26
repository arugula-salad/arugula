// Push (#1260): the app subscribes this device at APNs with keys of its own,
// as a browser subscribes at its push service, and a tap on a notification
// opens what it's about. The Notification Service Extension (ArugulaNotify)
// decrypts what arrives with those keys (Shared/WebPush.swift). Each push
// about an Inbox item is tagged with its id; when items clear, control sends
// a quiet update (`inbox-clear`, Arugula #1432) that names them, and their
// notifications go. The icon's badge is the Inbox's count, as APNs' `badge`
// sets it.

import SwiftUI
import UIKit
import UserNotifications

/// What a tapped notification opens, from what it said: a pane on a machine
/// (an agent's question, a terminal that wants you), a chat room, an issue,
/// a deploy waiting for approval (Arugula #604), or an Inbox item that isn't
/// an issue or a thread (a PR, a run: Arugula #1365), which opens the Inbox at it
/// (by its item's id, or the whole Inbox when that's empty).
enum PushRoute: Equatable {
    case pane(machine: String, pane: UInt32)
    case room(String)
    case issue(String)
    case deploy(String)
    case inbox(String)

    init?(said: [String: Any]) {
        if let deploy = said["deploy"] as? String, deploy.hasPrefix("dep_") {
            self = .deploy(deploy)
        } else if let machine = said["daemon"] as? String, let pane = said["pane"] as? Int, pane > 0 {
            self = .pane(machine: machine, pane: UInt32(pane))
        } else if let chat = said["chat"] as? [String: Any], let room = chat["room"] as? String {
            self = .room(room)
        } else if let inbox = said["inbox"] as? [String: Any] {
            if let item = inbox["item"] as? String, item.hasPrefix("iss_") {
                self = .issue(item)
            } else if let room = inbox["room"] as? String {
                self = .room(room)
            } else if let item = (inbox["ids"] as? [String])?.first ?? inbox["item"] as? String {
                self = .inbox(item)
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
    /// An Inbox item a tap opened before the app had control: marked seen
    /// once it does.
    private var unmarked: String?
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
        if let item = unmarked {
            unmarked = nil
            markSeen(item)
        }
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
        // A push about an Inbox item (Arugula #1365): it's seen once tapped,
        // as opening it in the Inbox marks it.
        if let item = (said["inbox"] as? [String: Any])?["item"] as? String { markSeen(item) }
    }

    /// A link into the app (`arugula://inbox`, `arugula://inbox/<item>`), as
    /// Android takes it: the Inbox, at the item, which is marked seen.
    func open(link url: URL) -> Bool {
        guard url.scheme == "arugula", url.host() == "inbox" else { return false }
        let item = url.pathComponents.dropFirst().first ?? ""
        route = .inbox(item)
        if !item.isEmpty { markSeen(item) }
        return true
    }

    /// A quiet update (`content-available`): the items it names cleared, so
    /// their notifications go, and the Inbox is read again.
    func quiet(_ e: String?) async {
        guard let e, let sealed = Data(base64Encoded: e),
              let keys = PushKeys.load(), let plain = try? WebPush.decrypt(sealed, keys: keys),
              let said = (try? JSONSerialization.jsonObject(with: plain)) as? [String: Any],
              said["tag"] as? String == "inbox-clear"
        else { return }
        await Self.withdrawn((said["clear"] as? [String]) ?? [])
        if let control { await InboxModel.shared.load(control, project: InboxModel.shared.project) }
    }

    /// Take away the notifications of items that cleared.
    nonisolated static func withdraw(_ items: [String]) {
        Task { await withdrawn(items) }
    }

    /// A notification is an item's when it was tagged with its id, or names
    /// it as its Inbox item.
    nonisolated static func withdrawn(_ items: [String]) async {
        let gone = Set(items)
        guard !gone.isEmpty else { return }
        let center = UNUserNotificationCenter.current()
        let shown = await center.deliveredNotifications()
        let ids = shown.filter { n in
            let info = n.request.content.userInfo
            guard let text = info["said"] as? String,
                  let said = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any]
            else { return gone.contains(n.request.content.threadIdentifier) }
            let inbox = said["inbox"] as? [String: Any]
            let names = [said["tag"] as? String, inbox?["item"] as? String].compactMap { $0 } + (inbox?["ids"] as? [String] ?? [])
            return names.contains(where: gone.contains)
        }.map(\.request.identifier)
        center.removeDeliveredNotifications(withIdentifiers: ids)
    }

    /// Mark all of an item's Inbox entries seen, and count again.
    private func markSeen(_ item: String) {
        guard let control else {
            unmarked = item
            return
        }
        Task {
            let id = item.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? item
            _ = try? await control.call("POST", "/api/inbox/\(id)", body: ["seen": true])
            await InboxModel.shared.load(control, project: InboxModel.shared.project)
        }
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

    /// A quiet update: items cleared. APNs has set the badge already.
    nonisolated func application(_ app: UIApplication, didReceiveRemoteNotification info: [AnyHashable: Any]) async -> UIBackgroundFetchResult {
        let e = info["e"] as? String
        await PushCenter.shared.quiet(e)
        return .newData
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        await MainActor.run { PushCenter.shared.open(info) }
    }
}
