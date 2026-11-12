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
                List(rooms, selection: $picked) { room in
                    NavigationLink(value: room) {
                        HStack {
                            Text(room.title).lineLimit(1)
                            Spacer()
                            if room.unread > 0 && room != picked {
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
                    LazyVStack(alignment: .leading, spacing: 10) {
                        ForEach(messages.filter { $0.deleted == nil }) { m in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(m.name).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                                Text(m.text).textSelection(.enabled)
                            }
                            .id(m.seq)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding()
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
