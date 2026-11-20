// The app: sign in, then one navigation over projects, chat and machines.

import SwiftUI

@main
struct ArugulaApp: App {
    @StateObject private var model = AppModel()
    @Environment(\.scenePhase) private var phase

    init() {
        Face.register()
        applyBarAppearance()
    }

    var body: some Scene {
        WindowGroup {
            Group {
                if model.updateNeeded {
                    UpdateAppView().environmentObject(model)
                } else if let control = model.control {
                    HomeView(control: control).environmentObject(model)
                } else {
                    SignInView().environmentObject(model)
                }
            }
            .tint(Palette.accent)
            .font(Face.ui(16))
            .foregroundStyle(Palette.fg)
            .preferredColorScheme(.dark)
            .onChange(of: phase, initial: true) { _, now in
                if now == .active { Task { await model.checkApp() } }
            }
        }
    }
}

@MainActor
final class AppModel: ObservableObject {
    @Published var control: Control?
    /// Why the app is asking for a sign-in again, if it's again.
    @Published var notice: String?
    /// Control no longer serves this build (#603): the app says to update it.
    @Published var updateNeeded = false
    private var signedOut: NSObjectProtocol?
    private var checked: Date?
    /// Where the session is, else where a sign-in would go.
    private var url = AppModel.defaultURL

    /// Where control is. A test or a dev build can point elsewhere.
    static var defaultURL: String {
        ProcessInfo.processInfo.environment["ARUGULA_CONTROL_URL"] ?? "https://control.arugula.io"
    }

    init() {
        if ProcessInfo.processInfo.environment["ARUGULA_RESET"] == "1" { SessionStore.forget() }
        if SessionStore.forgetPairedDevice() {
            notice = "Arugula now signs this \(device) in with your account. Sign in once more."
        }
        signedOut = NotificationCenter.default.addObserver(forName: .arugulaSignedOut, object: nil, queue: .main) { [weak self] n in
            MainActor.assumeIsolated {
                // Only the session in use: an old one's late answer ends nothing.
                guard let self, let c = n.object as? Control, c === self.control else { return }
                SessionStore.forget()
                self.control = nil
                self.notice = "Your sign-in has ended. Sign in again."
            }
        }
        guard let stored = SessionStore.load() else { return }
        url = Self.url(of: stored) ?? url
        do {
            control = try Control(stored: stored)
        } catch {
            SessionStore.forget()
            notice = "Sign in again: \(error.localizedDescription)"
        }
    }

    func signedIn(_ stored: String) throws {
        try SessionStore.save(stored)
        url = Self.url(of: stored) ?? url
        notice = nil
        control = try Control(stored: stored)
    }

    /// Ask control whether it still serves this build: as the app opens or
    /// comes back, at most every ten minutes, or now with `force`. Offline
    /// isn't an answer, so it leaves things as they are.
    func checkApp(force: Bool = false) async {
        if !force, !updateNeeded, let checked, Date().timeIntervalSince(checked) < 600 { return }
        checked = Date()
        if let served = await appServed(at: url) { updateNeeded = !served }
    }

    /// A sign-in was refused because control needs a newer app.
    func mustUpdate(at url: String) {
        self.url = url
        updateNeeded = true
    }

    private static func url(of stored: String) -> String? {
        let v = (try? JSONSerialization.jsonObject(with: Data(stored.utf8))) as? [String: Any]
        return v?["url"] as? String
    }

    func signOut() {
        let c = control
        SessionStore.forget()
        control = nil
        if let c { Task { await c.signOut() } }
    }
}

/// Control no longer serves this build of the app (#603). Testers get builds
/// from TestFlight, so that's where it sends them.
struct UpdateAppView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.openURL) private var openURL

    var body: some View {
        ContentUnavailableView {
            Label("Update Arugula", systemImage: "arrow.down.app")
        } description: {
            Text("Arugula cloud has moved on from this version of the app. Update it to keep going.")
        } actions: {
            Button("Open TestFlight") { openURL(URL(string: "itms-beta://")!) }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("update-app")
            Button("Try again") { Task { await model.checkApp(force: true) } }
        }
        .background(Palette.void)
    }
}

/// Says what went wrong, with a way to try again.
struct Problem: View {
    var message: String
    var retry: (() -> Void)?

    var body: some View {
        ContentUnavailableView {
            Label("Can't load this", systemImage: "exclamationmark.triangle").foregroundStyle(Palette.danger)
        } description: {
            Text(message).font(Face.mono(12)).foregroundStyle(Palette.subtle)
        } actions: {
            if let retry { Button("Try again", action: retry) }
        }
    }
}

/// What control says when chat isn't on, and so on, in its own words.
func describe(_ e: Error) -> String { e.localizedDescription }
