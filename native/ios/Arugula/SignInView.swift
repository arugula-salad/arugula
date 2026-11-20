// Signing in with the account (#999): control's page opens in a web sheet, where
// someone signed in to Arugula clicks Allow, and the sheet comes back to the app
// with a one-time grant that only this app can redeem.

import AuthenticationServices
import SwiftUI

struct SignInView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.webAuthenticationSession) private var webAuth
    @State private var url = AppModel.defaultURL
    @State private var problem: String?
    @State private var busy = false
    @State private var showAddress = false
    /// A UI test can't click Allow in a web sheet: with ARUGULA_SIGNIN_BY_HAND it
    /// reads the ticket here and types the address the Allow was sent to.
    @State private var byHand: SignIn?
    @State private var callback = ""

    private static let handTest = ProcessInfo.processInfo.environment["ARUGULA_SIGNIN_BY_HAND"] == "1"

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 20) {
                    VStack(spacing: 4) {
                        Text("ARUGULA").font(Face.display(56, "Black")).tracking(1)
                        Caps("Your machines, your agents, one place")
                    }
                    .padding(.top, 24)
                    if let notice = model.notice {
                        Text(notice)
                            .font(.footnote)
                            .multilineTextAlignment(.center)
                            .foregroundStyle(Palette.subtle)
                            .accessibilityIdentifier("signin-notice")
                    }
                    if let byHand {
                        typed(byHand)
                    } else {
                        start
                    }
                    if let problem {
                        Text(problem)
                            .font(.footnote)
                            .foregroundStyle(Palette.danger)
                            .multilineTextAlignment(.center)
                            .accessibilityIdentifier("signin-problem")
                    }
                }
                .padding()
                .frame(maxWidth: 520)
                .frame(maxWidth: .infinity)
            }
            .background(Palette.void)
        }
    }

    private var start: some View {
        VStack(spacing: 16) {
            Text("Sign in with your Arugula account. Arugula opens in a browser sheet: sign in there if you aren't already, and tap Allow.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button {
                Task { await signIn() }
            } label: {
                HStack(spacing: 8) {
                    if busy { ProgressView() }
                    Text(busy ? "Waiting for Allow…" : "Sign in")
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(busy)
            .accessibilityIdentifier("signin-start")
            DisclosureGroup("Address", isExpanded: $showAddress) {
                TextField("https://control.arugula.io", text: $url)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("signin-url")
            }
            .font(.footnote)
        }
    }

    /// The test's way in: the ticket to allow, and where to type the answer.
    private func typed(_ s: SignIn) -> some View {
        VStack(spacing: 12) {
            Text(s.ticket)
                .font(.system(.footnote, design: .monospaced))
                .accessibilityIdentifier("signin-ticket")
            TextField("arugula://signin?…", text: $callback)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("signin-callback")
            Button("Finish") { Task { await finish(s, callback) } }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("signin-finish")
        }
    }

    private func signIn() async {
        busy = true
        problem = nil
        defer { busy = false }
        do {
            let s = try await SignIn(url: url, name: "Arugula on \(UIDevice.current.name)")
            if Self.handTest {
                byHand = s
                return
            }
            let back = try await webAuth.authenticate(using: s.link, callbackURLScheme: s.callbackScheme, preferredBrowserSession: .shared)
            try model.signedIn(try await s.finish(back))
        } catch let e as ASWebAuthenticationSessionError where e.code == .canceledLogin {
            // Closed without allowing: nothing to say.
        } catch let e as CoreError where e.reason == "update_app" {
            model.mustUpdate(at: url)
        } catch let e as CoreError where e.reason == "older_control" {
            problem = "This Arugula isn't taking the \(device) app's sign-in yet. Its operator has to turn native clients on."
        } catch {
            problem = error.localizedDescription
        }
    }

    private func finish(_ s: SignIn, _ typed: String) async {
        guard let back = URL(string: typed.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            problem = "That isn't an address."
            return
        }
        do {
            try model.signedIn(try await s.finish(back))
        } catch {
            problem = error.localizedDescription
        }
    }
}
