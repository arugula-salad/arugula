// Pairing by code: control takes the request, a signed-in browser approves it.

import CoreImage.CIFilterBuiltins
import SwiftUI
import UIKit

struct PairView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.openURL) private var openURL
    @State private var url = AppModel.defaultURL
    @State private var pairing: Pairing?
    @State private var problem: String?
    @State private var busy = false
    @State private var showAddress = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 20) {
                    if let pairing {
                        waiting(pairing)
                    } else {
                        start
                    }
                    if let problem {
                        Text(problem)
                            .font(.footnote)
                            .foregroundStyle(.red)
                            .multilineTextAlignment(.center)
                            .accessibilityIdentifier("pair-problem")
                    }
                }
                .padding()
            }
            .navigationTitle("Arugula")
        }
        .task(id: pairing?.code) { await waitForApproval() }
    }

    private var start: some View {
        VStack(spacing: 16) {
            Text("Make this iPhone one of your devices. You'll approve it in a browser that's signed in to Arugula.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button {
                Task { await begin() }
            } label: {
                Text(busy ? "Asking…" : "Pair this iPhone").frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(busy)
            .accessibilityIdentifier("pair-start")
            DisclosureGroup("Address", isExpanded: $showAddress) {
                TextField("https://control.arugula.io", text: $url)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("pair-url")
            }
            .font(.footnote)
        }
    }

    private func waiting(_ p: Pairing) -> some View {
        VStack(spacing: 16) {
            Text("Approve this iPhone").font(.headline)
            Text("Open the link in a browser that's signed in to Arugula. On another device, scan the code.")
                .font(.subheadline)
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button("Open in Safari") { openURL(p.link) }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .accessibilityIdentifier("pair-open")
            if let qr = qrCode(p.link.absoluteString) {
                Image(uiImage: qr)
                    .interpolation(.none)
                    .resizable()
                    .frame(width: 180, height: 180)
                    .accessibilityLabel("QR code of the link")
            }
            Text(p.code)
                .font(.system(.footnote, design: .monospaced))
                .textSelection(.enabled)
                .multilineTextAlignment(.center)
                .accessibilityIdentifier("pair-code")
            Button("Copy the code") { UIPasteboard.general.string = p.code }
                .font(.footnote)
            HStack(spacing: 8) {
                ProgressView()
                Text("Waiting for approval…").font(.footnote).foregroundStyle(.secondary)
            }
            Button("Start over") { pairing = nil }
                .font(.footnote)
        }
    }

    private func begin() async {
        busy = true
        problem = nil
        defer { busy = false }
        do {
            pairing = try await Pairing(url: url, name: "Arugula on \(UIDevice.current.name)")
        } catch {
            let said = error.localizedDescription
            problem = said.contains("a daemon certificate")
                ? "This Arugula isn't taking the iPhone app yet. Its operator has to turn native clients on."
                : said
        }
    }

    private func waitForApproval() async {
        guard let p = pairing else { return }
        while !Task.isCancelled {
            do {
                if let stored = try await p.poll() {
                    try model.paired(stored)
                    return
                }
            } catch {
                problem = error.localizedDescription
                pairing = nil
                return
            }
            try? await Task.sleep(for: .seconds(2))
        }
    }

    private func qrCode(_ text: String) -> UIImage? {
        let f = CIFilter.qrCodeGenerator()
        f.message = Data(text.utf8)
        guard let out = f.outputImage?.transformed(by: .init(scaleX: 6, y: 6)),
            let cg = CIContext().createCGImage(out, from: out.extent)
        else { return nil }
        return UIImage(cgImage: cg)
    }
}
