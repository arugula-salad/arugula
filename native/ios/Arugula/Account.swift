// The account, in every place's heading (Arugula #1361), now that You has
// left the tab bar: your picture opens your name, Settings, Send feedback,
// the app's version and Sign out, as the web's avatar menu; the bell beside
// it opens the Inbox (Inbox.swift). Settings is the web's page, in a web view
// here: control hands the app's sign-in to it (`POST /api/handoff`, Arugula
// #1420) for a web session of its own, which closing it ends. Send feedback
// goes to control's `POST /api/feedback` as the person signed in. Your
// picture is control's (`/api/chat/avatar/{account}`), as the web shows it.

import SwiftUI
import WebKit

/// Your picture (your initials in your tone until it comes, as chat shows
/// people), opening the account's menu.
struct AccountMenu: View {
    @EnvironmentObject private var scope: Scope
    @EnvironmentObject private var app: AppModel
    @State private var feedback = false
    @State private var settings = false
    @State private var picture: UIImage?

    var body: some View {
        Menu {
            Section {
                Button {} label: {
                    Text(scope.me?.name ?? "Signed in")
                    if let login = scope.me?.login { Text(login) }
                }
                .disabled(true)
            }
            Section {
                Button { settings = true } label: { Label("Settings", systemImage: "gearshape") }
                    .accessibilityIdentifier("account-settings")
                Button { feedback = true } label: { Label("Send feedback", systemImage: "exclamationmark.bubble") }
                    .accessibilityIdentifier("account-feedback")
            }
            Section {
                Text(verbatim: "Arugula \(Self.version)")
                Button("Sign out", role: .destructive) { app.signOut() }
                    .accessibilityIdentifier("sign-out")
            }
        } label: {
            Avatar(name: scope.me?.name ?? scope.me?.login ?? "?", size: 36, picture: picture)
                .clipShape(Circle())
        }
        .accessibilityLabel("You: \(scope.me?.name ?? "your account")")
        .accessibilityIdentifier("account")
        .sheet(isPresented: $feedback) {
            if let control = app.control { FeedbackSheet(control: control) { feedback = false } }
        }
        .fullScreenCover(isPresented: $settings) {
            if let control = app.control { SettingsScreen(control: control) { settings = false } }
        }
        .task(id: scope.me?.account) {
            guard let account = scope.me?.account, let control = app.control else { return }
            picture = (try? await control.bytes("/api/chat/avatar/\(account)")).flatMap(UIImage.init(data:))
        }
    }

    /// The app's version and build, as Settings › General › About says it.
    static var version: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String
        return build.map { "\(short) (\($0))" } ?? short
    }
}

/// The platform page's address, which `/control.json` names: where the
/// web's Settings and anything else the app can't open itself live.
enum Platform {
    static func origin(_ control: Control?) async -> URL? {
        guard let base = control?.url.flatMap({ URL(string: $0) }) else { return nil }
        struct Info: Decodable { var platform: String? }
        guard let (data, _) = try? await URLSession.shared.data(from: base.appending(path: "control.json")),
            let p = (try? JSONDecoder().decode(Info.self, from: data))?.platform
        else { return nil }
        return URL(string: p)
    }
}

/// Send feedback, as the web's sheet: what it is, a title and what happened.
struct FeedbackSheet: View {
    let control: Control
    let close: () -> Void
    @State private var kind = "bug"
    @State private var title = ""
    @State private var words = ""
    @State private var contact = true
    @State private var busy = false
    @State private var problem: String?
    @State private var sent: String?

    private static let kinds: [(String, String)] = [
        ("bug", "Something's broken"), ("missing", "Something's missing"), ("idea", "An idea"), ("praise", "Praise"), ("other", "Other"),
    ]

    var body: some View {
        NavigationStack {
            Form {
                Picker("What is it", selection: $kind) {
                    ForEach(Self.kinds, id: \.0) { Text($0.1).tag($0.0) }
                }
                TextField("Title", text: $title).accessibilityIdentifier("feedback-title")
                TextField("What happened", text: $words, axis: .vertical)
                    .lineLimit(4...12)
                    .accessibilityIdentifier("feedback-body")
                Toggle("You may contact me about it", isOn: $contact)
                if let sent {
                    Text("Sent as \(sent). Thank you.").foregroundStyle(Palette.ok)
                } else if let problem {
                    Text(problem).font(Face.mono(12)).foregroundStyle(Palette.danger)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Palette.void)
            .navigationTitle("Send feedback")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(sent == nil ? "Cancel" : "Done", action: close) }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") { Task { await send() } }
                        .disabled(busy || sent != nil || title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityIdentifier("feedback-send")
                }
            }
        }
    }

    private func send() async {
        busy = true
        defer { busy = false }
        let body: [String: Any] = [
            "kind": kind, "title": String(title.prefix(200)), "body": words, "contact_ok": contact,
            "source": "person", "surface": "other",
            "context": ["version": AccountMenu.version, "os": "other"],
        ]
        do {
            let got = try await control.call("POST", "/api/feedback", body: body)
            let shown = (try? JSONSerialization.jsonObject(with: got) as? [String: Any])?["shown"] as? String
            sent = shown.flatMap { $0.isEmpty ? nil : $0 } ?? "feedback"
            problem = nil
        } catch {
            problem = describe(error)
        }
    }
}

/// Settings, the web's page, in a web view (Arugula #1420): the app asks
/// control for a one-time code with its session, and the web view posts it,
/// in the request's body, for a web session of its own and the page. Pages
/// off control's and the platform's hosts open in the browser. Closing it
/// ends that web session.
struct SettingsScreen: View {
    let control: Control
    let close: () -> Void
    @State private var start: SettingsWebView.Start?
    @State private var problem: String?
    @State private var loading = true
    @Environment(\.horizontalSizeClass) private var width

    var body: some View {
        NavigationStack {
            ZStack {
                if let start {
                    SettingsWebView(start: start, problem: $problem, loading: $loading)
                        .accessibilityIdentifier("settings-web")
                }
                if loading && problem == nil { ProgressView().tint(Palette.accent) }
                if let problem { Problem(message: problem) }
            }
            .background(Palette.void)
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: width == .regular ? .topBarTrailing : .topBarLeading) {
                    Button("Close", action: close).accessibilityIdentifier("settings-close")
                }
            }
        }
        .task { await begin() }
    }

    private func begin() async {
        guard let base = control.url.flatMap({ URL(string: $0.trimmingCharacters(in: CharacterSet(charactersIn: "/"))) }),
            let host = base.host()
        else {
            problem = "This app isn't signed in."
            return
        }
        async let platform = Platform.origin(control)
        do {
            let got = try await control.call("POST", "/api/handoff", body: [:])
            guard let code = (try? JSONSerialization.jsonObject(with: got) as? [String: Any])?["code"] as? String else {
                problem = "Control didn't hand the sign-in over."
                return
            }
            var request = URLRequest(url: base.appending(path: "auth/handoff"))
            request.httpMethod = "POST"
            request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
            // The code is hex: nothing in it to escape.
            request.httpBody = Data("code=\(code)&to=%2Fsettings".utf8)
            start = .init(request: request, control: host, hosts: Set([host, await platform?.host()].compactMap { $0 }))
        } catch {
            loading = false
            problem = describe(error)
        }
    }
}

/// The web's Settings in a WKWebView, from the hand-over's request.
struct SettingsWebView: UIViewRepresentable {
    struct Start {
        /// The hand-over: `POST /auth/handoff`, the code in its body.
        let request: URLRequest
        /// Control's host, where the web session's cookie is.
        let control: String
        /// Where the page may go in here: control's host and the platform's.
        let hosts: Set<String>
    }

    let start: Start
    @Binding var problem: String?
    @Binding var loading: Bool

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        // Its own store, gone with the view, as a Doc's is.
        config.websiteDataStore = .nonPersistent()
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        view.uiDelegate = context.coordinator
        view.isOpaque = false
        view.backgroundColor = UIColor(Palette.void)
        view.scrollView.backgroundColor = UIColor(Palette.void)
        #if DEBUG
            view.isInspectable = true
        #endif
        view.load(start.request)
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) { context.coordinator.parent = self }

    /// Closed: the web session ends at control, as signing out there would.
    static func dismantleUIView(_ view: WKWebView, coordinator: Coordinator) {
        let host = coordinator.parent.start.control
        view.configuration.websiteDataStore.httpCookieStore.getAllCookies { cookies in
            guard let stored = SessionStore.load(),
                var v = try? JSONSerialization.jsonObject(with: Data(stored.utf8)) as? [String: Any],
                let mine = v["cookie"] as? String, let name = mine.split(separator: "=").first.map(String.init),
                let web = cookies.first(where: { $0.name == name && ($0.domain == host || $0.domain == ".\(host)") }),
                "\(name)=\(web.value)" != mine
            else { return }
            v["cookie"] = "\(name)=\(web.value)"
            guard let data = try? JSONSerialization.data(withJSONObject: v),
                let c = try? Control(stored: String(decoding: data, as: UTF8.self))
            else { return }
            Task.detached { await c.signOut() }
        }
    }

    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate {
        var parent: SettingsWebView
        init(_ parent: SettingsWebView) { self.parent = parent }

        func webView(_ view: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            guard let to = action.request.url else { return .cancel }
            let main = action.targetFrame?.isMainFrame ?? true
            if !main || to.scheme == "about" || to.scheme == "blob" || to.scheme == "data" { return .allow }
            if let host = to.host(), parent.start.hosts.contains(host) { return .allow }
            await MainActor.run { UIApplication.shared.open(to) }
            return .cancel
        }

        func webView(_ view: WKWebView, didFinish _: WKNavigation!) { parent.loading = false }

        func webView(_ view: WKWebView, didFail _: WKNavigation!, withError e: Error) { failed(e) }
        func webView(_ view: WKWebView, didFailProvisionalNavigation _: WKNavigation!, withError e: Error) { failed(e) }

        private func failed(_ e: Error) {
            if (e as NSError).code == NSURLErrorCancelled { return }
            parent.loading = false
            parent.problem = e.localizedDescription
        }

        /// `window.open`: the browser.
        func webView(_ view: WKWebView, createWebViewWith _: WKWebViewConfiguration, for action: WKNavigationAction,
            windowFeatures _: WKWindowFeatures) -> WKWebView?
        {
            if let u = action.request.url { UIApplication.shared.open(u) }
            return nil
        }

        func webViewWebContentProcessDidTerminate(_ view: WKWebView) { view.reload() }
    }
}
