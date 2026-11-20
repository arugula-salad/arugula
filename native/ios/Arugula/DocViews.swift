// Docs and prototypes (Arugula #1259): the Doc's page itself, in a web view.
// Control serves a project Doc alone at `/embed/p/:project/docs/:doc` on
// its own site (web/src/shell/doc-embed.tsx), comments in its margin. The web
// view is signed in with the app's session: its cookie, put in the view's
// store, stays a native session there, which reads the Doc and its thread and
// posts comments, and nothing more. The page's links come back as intents on
// `window.webkit.messageHandlers.arugula` (`EmbedIntent` in that file): a
// board, an issue, a room or another Doc opens here, in the app; anything
// else in the browser. A Doc card in a thread opens it: over everything on a
// phone, in the right-hand column on an iPad. There's no native editing:
// comments are made in the page.

import SwiftUI
import WebKit

/// A project Doc, as a link to its page names it.
struct DocRef: Hashable, Identifiable {
    let project: String
    let doc: String
    /// What the link said, when it said more than its address.
    var label: String?
    var id: String { doc }
    static func == (a: DocRef, b: DocRef) -> Bool { a.doc == b.doc }
    func hash(into h: inout Hasher) { h.combine(doc) }

    // `/p/prj_…/docs/pdc_…`, on any host, a slug after either id.
    private static let path = #/\/p\/(prj_[0-9a-f]+)(?:-[^\/\s)#?]*)?\/docs\/(pdc_[0-9a-f]+)/#
    private static let labelled = #/\[([^\]\n]+)\]\((\S+?)\)/#

    /// Every Doc a message's text links to, each once, in order. A Markdown
    /// link's text is its label, unless it's a comment's own `On “…”`.
    static func all(in text: String) -> [DocRef] {
        var labels: [String: String] = [:]
        for m in text.matches(of: labelled) {
            if let p = String(m.2).firstMatch(of: path), !m.1.hasPrefix("On “") { labels[String(p.2)] = String(m.1) }
        }
        var seen = Set<String>()
        return text.matches(of: path).compactMap { m in
            let doc = String(m.2)
            guard seen.insert(doc).inserted else { return nil }
            return DocRef(project: String(m.1), doc: doc, label: labels[doc])
        }
    }
}

/// What the embed page tells the app (doc-embed.tsx's `EmbedIntent`).
struct EmbedIntent: Decodable {
    var kind: String
    var project: String?
    var id: String?
    var url: String?
    var title: String?
}

/// Where a link in a Doc goes, for whoever shows the Doc to open.
enum DocLink {
    case doc(DocRef)
    case board(project: String, id: String, url: URL?)
    case issue(project: String, id: String)
    case room(id: String, url: URL?)
    case web(URL)

    /// Its page on the web, for one the app can't open itself.
    var url: URL? {
        switch self {
        case .board(_, _, let u), .room(_, let u): u
        case .web(let u): u
        case .doc, .issue: nil
        }
    }
}

private struct OpenDocKey: EnvironmentKey {
    static let defaultValue: ((DocRef) -> Void)? = nil
}

extension EnvironmentValues {
    /// Open a Doc: beside the conversation on an iPad, over it on a phone.
    /// Unset, a Doc card opens it over everything itself.
    var openDoc: ((DocRef) -> Void)? {
        get { self[OpenDocKey.self] }
        set { self[OpenDocKey.self] = newValue }
    }
}

/// The app's sign-in as the web view needs it: control's address and the
/// session's cookie, from what the keychain keeps (the core's `Session`).
struct WebSession {
    let url: URL
    let cookie: HTTPCookie

    static func current() -> WebSession? {
        guard let stored = SessionStore.load(),
            let v = try? JSONSerialization.jsonObject(with: Data(stored.utf8)) as? [String: Any],
            let base = (v["url"] as? String).flatMap({ URL(string: $0.trimmingCharacters(in: CharacterSet(charactersIn: "/"))) }),
            let host = base.host(),
            let pair = v["cookie"] as? String, let eq = pair.firstIndex(of: "=")
        else { return nil }
        var props: [HTTPCookiePropertyKey: Any] = [
            .name: String(pair[..<eq]), .value: String(pair[pair.index(after: eq)...]), .domain: host, .path: "/",
        ]
        if base.scheme == "https" { props[.secure] = "TRUE" }
        guard let cookie = HTTPCookie(properties: props) else { return nil }
        return WebSession(url: base, cookie: cookie)
    }

    func page(_ d: DocRef) -> URL? {
        URL(string: "\(url.absoluteString.trimmingCharacters(in: CharacterSet(charactersIn: "/")))/embed/p/\(d.project)/docs/\(d.doc)")
    }
}

/// A Doc's card in a message: it opens the Doc.
struct DocCard: View {
    let ref: DocRef
    @Environment(\.openDoc) private var openDoc
    @State private var over: DocRef?

    var body: some View {
        Button {
            if let openDoc { openDoc(ref) } else { over = ref }
        } label: {
            HStack(spacing: 10) {
                Image(systemName: "doc.richtext").foregroundStyle(Palette.accent)
                VStack(alignment: .leading, spacing: 2) {
                    Text(ref.label ?? "Doc").font(Face.ui(14, "Bold")).foregroundStyle(Palette.fg).lineLimit(2)
                    Text("Open the Doc").font(Face.mono(11)).foregroundStyle(Palette.dim)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right").font(.system(size: 12, weight: .semibold)).foregroundStyle(Palette.muted)
            }
            .padding(10)
            .frame(maxWidth: 420, alignment: .leading)
            .background(Palette.surface, in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.raised, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("doc-card-\(ref.doc)")
        .fullScreenCover(item: $over) { d in
            DocScreen(ref: d, close: { over = nil }) { link in
                if case .doc(let next) = link { over = next } else { link.openOutside() }
            }
        }
    }
}

extension DocLink {
    /// What can't open in the app opens in the browser, at the platform's page.
    func openOutside() {
        if let u = url { UIApplication.shared.open(u) }
    }
}

/// A Doc, its page in a web view, with its title and a way out.
struct DocScreen: View {
    let ref: DocRef
    /// Close: over everything on a phone, the right-hand column on an iPad.
    var close: (() -> Void)?
    let open: (DocLink) -> Void
    @State private var title: String?
    @State private var problem: String?
    @State private var loading = true
    @Environment(\.horizontalSizeClass) private var width

    var body: some View {
        NavigationStack {
            ZStack {
                if let session = WebSession.current(), let url = session.page(ref) {
                    DocWebView(url: url, cookie: session.cookie, session: session, title: $title, problem: $problem, loading: $loading, open: open)
                        .id(ref.doc)
                        .accessibilityIdentifier("doc-web")
                } else {
                    Problem(message: "This app isn't signed in.")
                }
                if loading && problem == nil { ProgressView().tint(Palette.accent) }
                if let problem { Problem(message: problem) }
            }
            .background(Palette.void)
            .navigationTitle(title ?? ref.label ?? "Doc")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                // At the trailing edge on an iPad: the leading one sits under
                // iPadOS's floating tab bar (as home's back buttons, #1262).
                if let close {
                    ToolbarItem(placement: width == .regular ? .topBarTrailing : .topBarLeading) {
                        Button("Close", action: close).accessibilityIdentifier("doc-close")
                    }
                }
            }
        }
    }
}

/// The embed page in a WKWebView, its cookie the app's session.
struct DocWebView: UIViewRepresentable {
    let url: URL
    let cookie: HTTPCookie
    let session: WebSession
    @Binding var title: String?
    @Binding var problem: String?
    @Binding var loading: Bool
    let open: (DocLink) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        // Its own store, gone with the view: the cookie is the app's, never
        // kept by WebKit too.
        config.websiteDataStore = .nonPersistent()
        config.userContentController.add(WeakHandler(context.coordinator), name: "arugula")
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        view.uiDelegate = context.coordinator
        view.isOpaque = false
        view.backgroundColor = UIColor(Palette.void)
        view.scrollView.backgroundColor = UIColor(Palette.void)
        view.allowsBackForwardNavigationGestures = false
        #if DEBUG
            view.isInspectable = true
        #endif
        config.websiteDataStore.httpCookieStore.setCookie(cookie) {
            view.load(URLRequest(url: url))
        }
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) { context.coordinator.parent = self }

    static func dismantleUIView(_ view: WKWebView, coordinator: Coordinator) {
        view.configuration.userContentController.removeScriptMessageHandler(forName: "arugula")
    }

    final class Coordinator: NSObject, WKScriptMessageHandler, WKNavigationDelegate, WKUIDelegate {
        var parent: DocWebView
        init(_ parent: DocWebView) { self.parent = parent }

        func userContentController(_ c: WKUserContentController, didReceive message: WKScriptMessage) {
            guard let body = message.body as? [String: Any],
                let data = try? JSONSerialization.data(withJSONObject: body),
                let intent = try? JSONDecoder().decode(EmbedIntent.self, from: data)
            else { return }
            switch intent.kind {
            case "ready":
                parent.title = intent.title
                parent.loading = false
            case "signed_out":
                parent.problem = "Control didn't take this app's sign-in for the Doc. Sign in again from You."
            case "doc":
                if let p = intent.project, let id = intent.id { parent.open(.doc(DocRef(project: p, doc: id))) }
            case "board":
                if let p = intent.project, let id = intent.id { parent.open(.board(project: p, id: id, url: intent.url.flatMap(URL.init(string:)))) }
            case "issue":
                if let p = intent.project, let id = intent.id { parent.open(.issue(project: p, id: id)) }
            case "chat", "dm":
                if let id = intent.id { parent.open(.room(id: id, url: intent.url.flatMap(URL.init(string:)))) }
            default:
                if let u = intent.url.flatMap(URL.init(string:)) { parent.open(.web(u)) }
            }
        }

        /// The Doc's own page loads here, and the frames its prototypes run
        /// in; any other page the view is sent to opens in the browser.
        func webView(_ view: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            guard let to = action.request.url else { return .cancel }
            let main = action.targetFrame?.isMainFrame ?? true
            if !main || to.scheme == "about" || to.scheme == "blob" || to.scheme == "data" { return .allow }
            if to.host() == parent.url.host() && to.path() == parent.url.path() { return .allow }
            await MainActor.run { parent.open(.web(to)) }
            return .cancel
        }

        func webView(_ view: WKWebView, didFinish _: WKNavigation!) {
            if parent.title == nil { parent.title = view.title.flatMap { $0.isEmpty ? nil : $0 } }
        }

        func webView(_ view: WKWebView, didFail _: WKNavigation!, withError e: Error) { failed(e) }
        func webView(_ view: WKWebView, didFailProvisionalNavigation _: WKNavigation!, withError e: Error) { failed(e) }

        private func failed(_ e: Error) {
            if (e as NSError).code == NSURLErrorCancelled { return }
            parent.loading = false
            parent.problem = e.localizedDescription
        }

        /// `window.open` (a link the page has no app for): the browser.
        func webView(_ view: WKWebView, createWebViewWith _: WKWebViewConfiguration, for action: WKNavigationAction,
            windowFeatures _: WKWindowFeatures) -> WKWebView?
        {
            if let u = action.request.url { parent.open(.web(u)) }
            return nil
        }

        /// The web view's content process died (memory): load it again.
        func webViewWebContentProcessDidTerminate(_ view: WKWebView) { view.reload() }
    }
}

/// The message handler, held weakly: the controller keeps its handlers, and
/// a coordinator held there would never go.
private final class WeakHandler: NSObject, WKScriptMessageHandler {
    weak var target: WKScriptMessageHandler?
    init(_ t: WKScriptMessageHandler) { target = t }
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        target?.userContentController(c, didReceive: m)
    }
}
