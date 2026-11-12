// The app: pair, then one navigation over projects, chat and machines.

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
                    PairView().environmentObject(model)
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
    @Published var loadError: String?

    /// Where control is. A test or a dev build can point elsewhere.
    static var defaultURL: String {
        ProcessInfo.processInfo.environment["ARUGULA_CONTROL_URL"] ?? "https://control.arugula.io"
    }

    init() {
        if ProcessInfo.processInfo.environment["ARUGULA_RESET"] == "1" { DeviceStore.forget() }
        guard let stored = DeviceStore.load() else { return }
        do {
            control = try Control(stored: stored)
        } catch {
            loadError = error.localizedDescription
        }
    }

    func paired(_ stored: String) throws {
        try DeviceStore.save(stored)
        control = try Control(stored: stored)
    }

    func forget() {
        DeviceStore.forget()
        control = nil
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
