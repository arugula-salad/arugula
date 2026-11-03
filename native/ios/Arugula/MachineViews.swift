// Machines: the account's, reached through control's relay over an encrypted
// channel, and their terminals.

import SwiftUI

/// An open channel to one machine. Its socket's thread tells this object when
/// there's something to look at; everything else happens on the main thread.
@MainActor
final class MachineSession: ObservableObject {
    let control: Control
    let machine: Machine
    @Published var panes: [PaneRow] = []
    @Published var problem: String?
    @Published var connected = false
    private var link: OpaquePointer?
    /// The terminal on screen, which takes that pane's bytes.
    weak var screen: TerminalUIView?

    init(control: Control, machine: Machine) {
        self.control = control
        self.machine = machine
    }

    /// Handed to the core's callback. It can fire once more after the link is
    /// freed, so it isn't tied to this object's life, and it's a few bytes a
    /// connection.
    private final class Notifier { weak var session: MachineSession? }

    func connect() async {
        guard link == nil else { return }
        problem = nil
        let notifier = Notifier()
        notifier.session = self
        let ctx = UInt(bitPattern: Unmanaged.passRetained(notifier).toOpaque())
        let control = self.control
        let id = machine.id
        do {
            let p: UInt = try await Task.detached(priority: .userInitiated) {
                var err: UnsafeMutablePointer<CChar>?
                let got = aru_link_open(control.ptr, id, { ctx in
                    guard let ctx else { return }
                    let n = Unmanaged<Notifier>.fromOpaque(ctx).takeUnretainedValue()
                    DispatchQueue.main.async { MainActor.assumeIsolated { n.session?.pump() } }
                }, UnsafeMutableRawPointer(bitPattern: ctx), &err)
                guard let got else { throw err.map(CoreError.init(taking:)) ?? CoreError("no answer") }
                return UInt(bitPattern: got)
            }.value
            link = OpaquePointer(bitPattern: p)
            connected = true
            screen?.linkOpened()
            pump()
        } catch {
            problem = describe(error)
        }
    }

    var linkPointer: OpaquePointer? { link }

    func disconnect() {
        guard let l = link else { return }
        link = nil
        connected = false
        aru_link_free(l)
    }

    /// Take what's queued.
    func pump() {
        guard let l = link else { return }
        let flags = aru_link_feed(l, screen?.term, screen?.pane ?? 0)
        if flags & 2 != 0, let s = takeString(aru_link_panes(l)),
            let list = try? JSONDecoder().decode(PaneList.self, from: Data(s.utf8)) {
            panes = list.panes
        }
        if flags & 1 != 0 { screen?.render() }
        if flags & 8 != 0, let said = takeString(aru_link_said(l)) { problem = said }
        if flags & 4 != 0 {
            let why = takeString(aru_link_closed(l))
            disconnect()
            if why != "closed" { problem = why }
        }
    }

    /// A new shell on the machine: its pane.
    func newTerminal() async throws -> UInt32 {
        guard let l = link else { throw CoreError("not connected") }
        let p = UInt(bitPattern: l)
        let s: String = try await Task.detached(priority: .userInitiated) {
            var err: UnsafeMutablePointer<CChar>?
            let got = aru_link_request(OpaquePointer(bitPattern: p), "POST", "/api/run", "{}", &err)
            guard let s = takeString(got) else { throw err.map(CoreError.init(taking:)) ?? CoreError("no answer") }
            return s
        }.value
        let v = try JSONSerialization.jsonObject(with: Data(s.utf8)) as? [String: Any]
        guard let body = v?["body"] as? [String: Any], let pane = body["pane"] as? Int else { throw CoreError("the machine sent no pane") }
        return UInt32(pane)
    }
}

struct MachinesView: View {
    let control: Control

    var body: some View {
        NavigationStack {
            Loaded(load: { try await control.get("/api/directory", as: Directory.self) }) { d in
                List(d.daemons) { m in
                    NavigationLink(value: m) {
                        HStack {
                            Circle().fill(m.online == true ? Color.green : Color.secondary).frame(width: 8, height: 8)
                            Text(m.name)
                        }
                    }
                    .accessibilityIdentifier("machine-\(m.name)")
                }
                .overlay { if d.daemons.isEmpty { Text("No machines yet").foregroundStyle(.secondary) } }
            }
            .navigationTitle("Machines")
            .navigationDestination(for: Machine.self) { MachineView(control: control, machine: $0) }
        }
    }
}

struct MachineView: View {
    @StateObject private var session: MachineSession
    @State private var opened: PaneRow?
    @State private var pendingPane: UInt32?
    @State private var problem: String?
    @Environment(\.scenePhase) private var phase

    init(control: Control, machine: Machine) {
        _session = StateObject(wrappedValue: MachineSession(control: control, machine: machine))
    }

    var body: some View {
        List {
            if let p = session.problem ?? problem {
                Text(p).font(.footnote).foregroundStyle(.red).accessibilityIdentifier("machine-problem")
            }
            Button {
                Task {
                    do {
                        pendingPane = try await session.newTerminal()
                        openPending()
                    } catch {
                        problem = describe(error)
                    }
                }
            } label: {
                Label("New terminal", systemImage: "plus")
            }
            .disabled(!session.connected)
            .accessibilityIdentifier("new-terminal")
            ForEach(session.panes) { pane in
                Button { opened = pane } label: {
                    VStack(alignment: .leading) {
                        Text(pane.command ?? pane.tab_name ?? "Terminal \(pane.id)").foregroundStyle(.primary)
                        if let cwd = pane.cwd { Text(cwd).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head) }
                    }
                }
            }
        }
        .overlay { if !session.connected && session.problem == nil { ProgressView("Connecting…") } }
        .navigationTitle(session.machine.name)
        .navigationBarTitleDisplayMode(.inline)
        .task { await session.connect() }
        .onChange(of: session.panes) { _, _ in openPending() }
        .onChange(of: phase) { _, now in
            if now == .active { Task { await session.connect() } }
        }
        .onDisappear { if opened == nil { session.disconnect() } }
        .fullScreenCover(item: $opened) { pane in
            TerminalScreen(session: session, pane: pane)
        }
    }

    /// The new pane's row, once the machine's state has it.
    private func openPending() {
        guard let want = pendingPane, let row = session.panes.first(where: { $0.id == want }) else { return }
        pendingPane = nil
        opened = row
    }
}
