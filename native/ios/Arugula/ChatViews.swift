// Chat: the channels and DMs, and one room. Issues', panes' and sessions'
// threads aren't listed here. No live stream yet, so an open room looks
// again every few seconds.

import SwiftUI

struct ChatRoomsView: View {
    let control: Control
    @State private var picked: ChatRoom?
    @State private var columns = NavigationSplitViewVisibility.all

    var body: some View {
        NavigationSplitView(columnVisibility: $columns) {
            // Beside an open room, the list keeps its counts current.
            Loaded(load: { try await control.get("/api/chat", as: ChatRooms.self) }, every: .seconds(10)) { r in
                let rooms = r.rooms.filter { $0.isConversation && !$0.archived }.sorted { ($0.unread > 0 ? 1 : 0, $0.last) > ($1.unread > 0 ? 1 : 0, $1.last) }
                let channels = rooms.filter { $0.kind == "channel" }
                let dms = rooms.filter { $0.kind == "dm" }
                List(selection: $picked) {
                    if !channels.isEmpty {
                        Section("Channels") { ForEach(channels) { RoomRow(room: $0, open: $0 == picked) } }
                    }
                    if !dms.isEmpty {
                        Section("Direct messages") { ForEach(dms) { RoomRow(room: $0, open: $0 == picked) } }
                    }
                }
                .overlay { if rooms.isEmpty { Text("No rooms yet").foregroundStyle(.secondary) } }
            }
            .navigationTitle("Chat")
        } detail: {
            NavigationStack {
                if let picked {
                    ChatRoomView(control: control, room: picked)
                } else {
                    ContentUnavailableView("Pick a room", systemImage: "bubble.left.and.bubble.right")
                }
            }
            .id(picked?.id)
        }
        .navigationSplitViewStyle(.balanced)
    }
}

/// A room in the list: its name, and how many messages wait in it.
struct RoomRow: View {
    let room: ChatRoom
    let open: Bool

    var body: some View {
        NavigationLink(value: room) {
            HStack {
                Text(room.title).lineLimit(1)
                Spacer()
                if room.unread > 0 && !open {
                    Text("\(room.unread)")
                        .font(.caption2.weight(.semibold))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(room.mentions > 0 ? Color.red : Color.secondary, in: Capsule())
                        .foregroundStyle(.white)
                }
            }
        }
        .accessibilityIdentifier("room-\(room.name)")
    }
}

struct ChatRoomView: View {
    let control: Control
    let room: ChatRoom
    @State private var messages: [ChatMessage] = []
    @State private var draft = ""
    @State private var problem: String?
    @State private var sending = false

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    let shown = messages.filter { $0.deleted == nil }
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(shown.enumerated()), id: \.element.seq) { i, m in
                            MessageView(message: m, continues: i > 0 && continues(shown[i - 1], m))
                                .id(m.seq)
                        }
                    }
                    // A readable line on an iPad's width.
                    .frame(maxWidth: 760, alignment: .leading)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal)
                    .padding(.vertical, 8)
                }
                .onChange(of: messages.last?.seq) { _, last in
                    if let last { withAnimation { proxy.scrollTo(last, anchor: .bottom) } }
                }
            }
            if let problem {
                Text(problem).font(.footnote).foregroundStyle(.red).padding(.horizontal)
            }
            if room.writable {
                HStack {
                    TextField("Message", text: $draft, axis: .vertical)
                        .lineLimit(1...4)
                        .textFieldStyle(.roundedBorder)
                        .accessibilityIdentifier("chat-draft")
                    Button("Send") { Task { await send() } }
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || sending)
                        .accessibilityIdentifier("chat-send")
                }
                .padding(8)
                .background(.bar)
            }
        }
        .detailTitle(room.title)
        .task {
            while !Task.isCancelled {
                await load()
                try? await Task.sleep(for: .seconds(4))
            }
        }
    }

    /// The same person (or agent) again within five minutes: no name and
    /// time over it, as in the web's room.
    private func continues(_ a: ChatMessage, _ b: ChatMessage) -> Bool {
        a.from == b.from && a.author == b.author && b.at - a.at < 5 * 60 * 1000
    }

    private func load() async {
        do {
            let page = try await control.get("/api/chat/rooms/\(room.id)/messages", as: ChatMessages.self)
            messages = page.messages.sorted { $0.seq < $1.seq }
            problem = nil
            if let last = messages.last?.seq {
                _ = try? await control.call("POST", "/api/chat/rooms/\(room.id)/read", body: ["seq": last])
            }
        } catch is CancellationError {
        } catch {
            problem = describe(error)
        }
    }

    private func send() async {
        sending = true
        defer { sending = false }
        do {
            _ = try await control.call("POST", "/api/chat/rooms/\(room.id)/messages", body: ["text": draft])
            draft = ""
            await load()
        } catch {
            problem = describe(error)
        }
    }
}

/// One message: who and when over the first of a run, then its text with the
/// chat's Markdown, a quote from a pane, files and reactions.
struct MessageView: View {
    let message: ChatMessage
    let continues: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if !continues {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(message.from).font(.subheadline.weight(.semibold))
                    Text(stamp(message.date)).font(.caption).foregroundStyle(.secondary)
                }
                .padding(.top, 12)
            }
            if let quote = message.quote, !quote.isEmpty {
                CodeBlock(text: quote)
            }
            if !message.text.isEmpty {
                ChatText(text: message.text)
            }
            if message.edited != nil {
                Text("edited").font(.caption2).foregroundStyle(.tertiary)
            }
            if let files = message.files, !files.isEmpty {
                ForEach(Array(files.enumerated()), id: \.offset) { _, f in
                    Label(f.name, systemImage: "paperclip").font(.footnote).foregroundStyle(.secondary)
                }
            }
            if let reactions = message.reactions, !reactions.isEmpty {
                HStack(spacing: 6) {
                    ForEach(Array(reactions.enumerated()), id: \.offset) { _, r in
                        Text("\(r.emoji) \(r.count)")
                            .font(.caption)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(r.mine ? Color.accentColor.opacity(0.2) : Color.secondary.opacity(0.15), in: Capsule())
                    }
                }
            }
        }
        .padding(.top, continues ? 2 : 0)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func stamp(_ d: Date) -> String {
        Calendar.current.isDateInToday(d)
            ? d.formatted(date: .omitted, time: .shortened)
            : d.formatted(.dateTime.month(.abbreviated).day().hour().minute())
    }
}

/// A message's text with the chat's small Markdown, as the web shows it:
/// fenced code as a block, `code`, **bold**, *italic*, links; headings and
/// bullets as bold lines and dots. Nothing in a code block is formatted.
struct ChatText: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(ChatText.parts(text).enumerated()), id: \.offset) { _, part in
                switch part {
                case .code(let s): CodeBlock(text: s)
                case .prose(let s): Text(ChatText.inline(s)).textSelection(.enabled)
                }
            }
        }
    }

    enum Part { case prose(String), code(String) }

    static func parts(_ text: String) -> [Part] {
        var out: [Part] = []
        var prose: [Substring] = []
        var code: [Substring]?
        func flushProse() {
            let s = prose.joined(separator: "\n").trimmingCharacters(in: .newlines)
            if !s.isEmpty { out.append(.prose(s)) }
            prose = []
        }
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                if let c = code {
                    out.append(.code(c.joined(separator: "\n")))
                    code = nil
                } else {
                    flushProse()
                    code = []
                }
            } else if code != nil {
                code!.append(line)
            } else {
                prose.append(line)
            }
        }
        if let c = code { out.append(.code(c.joined(separator: "\n"))) }  // never closed
        flushProse()
        return out
    }

    /// Inline Markdown, keeping the lines; a heading becomes a bold line and
    /// a bullet a dot.
    static func inline(_ s: String) -> AttributedString {
        let lines = s.split(separator: "\n", omittingEmptySubsequences: false).map { raw -> String in
            let line = String(raw)
            let t = line.drop { $0 == " " }
            if t.hasPrefix("#") {
                let title = t.drop { $0 == "#" }.trimmingCharacters(in: .whitespaces)
                return title.isEmpty ? line : "**\(title)**"
            }
            for bullet in ["- ", "* ", "+ "] where t.hasPrefix(bullet) {
                let indent = String(repeating: " ", count: line.count - t.count)
                return indent + "• " + t.dropFirst(2)
            }
            return line
        }
        let joined = lines.joined(separator: "\n")
        return (try? AttributedString(markdown: joined, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(joined)
    }
}

/// Lines shown as they are: monospaced, scrolling sideways when long.
struct CodeBlock: View {
    let text: String

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Text(text)
                .font(.system(.footnote, design: .monospaced))
                .textSelection(.enabled)
                .fixedSize(horizontal: true, vertical: false)
                .padding(8)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.secondary.opacity(0.12), in: RoundedRectangle(cornerRadius: 6))
    }
}
