// The app: sign in, then one navigation over projects, chat and machines.

import SwiftUI

@main
struct ArugulaApp: App {
    @StateObject private var model = AppModel()

    init() {
        Face.register()
        applyBarAppearance()
    }

    var body: some Scene {
        WindowGroup {
            Group {
                if let control = model.control {
                    HomeView(control: control).environmentObject(model)
                } else {
                    SignInView().environmentObject(model)
                }
            }
            .tint(Palette.accent)
            .font(Face.ui(16))
            .foregroundStyle(Palette.fg)
            .preferredColorScheme(.dark)
        }
    }
}

@MainActor
final class AppModel: ObservableObject {
    @Published var control: Control?
    /// Why the app is asking for a sign-in again, if it's again.
    @Published var notice: String?
    private var signedOut: NSObjectProtocol?

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
        do {
            control = try Control(stored: stored)
        } catch {
            SessionStore.forget()
            notice = "Sign in again: \(error.localizedDescription)"
        }
    }

    func signedIn(_ stored: String) throws {
        try SessionStore.save(stored)
        notice = nil
        control = try Control(stored: stored)
    }

    func signOut() {
        let c = control
        SessionStore.forget()
        control = nil
        if let c { Task { await c.signOut() } }
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
