// The Notification Service Extension (#1260): APNs brings an alert that says
// only that something came, with the notification itself encrypted for this
// device's subscription (`e`, RFC 8291). This decrypts it with the keys the
// app keeps in the shared keychain group and shows its real title and body.
// What it said goes on to the app as `said`, for a tap to open what it's about.

import UserNotifications

final class NotificationService: UNNotificationServiceExtension {
    private var deliver: ((UNNotificationContent) -> Void)?
    private var content: UNMutableNotificationContent?

    override func didReceive(_ request: UNNotificationRequest, withContentHandler deliver: @escaping (UNNotificationContent) -> Void) {
        let content = (request.content.mutableCopy() as? UNMutableNotificationContent) ?? UNMutableNotificationContent()
        self.deliver = deliver
        self.content = content
        // Anything that doesn't decrypt is shown as APNs brought it.
        guard let e = request.content.userInfo["e"] as? String, let sealed = Data(base64Encoded: e),
              let keys = PushKeys.load(), let plain = try? WebPush.decrypt(sealed, keys: keys),
              let said = (try? JSONSerialization.jsonObject(with: plain)) as? [String: Any]
        else {
            finish()
            return
        }
        if let title = said["title"] as? String, !title.isEmpty { content.title = title }
        if let body = said["body"] as? String { content.body = body }
        var info = content.userInfo
        info.removeValue(forKey: "e")
        info["said"] = String(decoding: plain, as: UTF8.self)
        content.userInfo = info
        // One per tag (a pane's, a room's), as a browser shows them: the
        // newest replaces what's there.
        guard let tag = said["tag"] as? String, !tag.isEmpty else {
            finish()
            return
        }
        let key = [said["daemon"] as? String, tag].compactMap { $0 }.joined(separator: "/")
        content.threadIdentifier = key
        let center = UNUserNotificationCenter.current()
        center.getDeliveredNotifications { shown in
            let same = shown.filter { $0.request.content.threadIdentifier == key }.map(\.request.identifier)
            center.removeDeliveredNotifications(withIdentifiers: same)
            self.finish()
        }
    }

    override func serviceExtensionTimeWillExpire() {
        finish()
    }

    private func finish() {
        guard let deliver, let content else { return }
        self.deliver = nil
        deliver(content)
    }
}
