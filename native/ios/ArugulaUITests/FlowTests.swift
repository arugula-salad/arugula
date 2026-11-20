// The app as a person takes it, on a simulator against a real control
// (native/ios/test-sim.sh): sign in, home, a board's thread and its lanes,
// an agent's permission prompt and question answered from home, projects,
// chat, a terminal. The script
// allows the sign-in from outside, as a signed-in browser would, and hands back
// the address the Allow was sent to, which a web sheet would have caught.

import XCTest

final class FlowTests: XCTestCase {
    /// Where this test and the script hand things to each other.
    private var handoff: URL { URL(fileURLWithPath: ProcessInfo.processInfo.environment["ARUGULA_HANDOFF"] ?? "/tmp/arugula-ui") }

    private func shot(_ app: XCUIApplication, _ name: String) {
        try? app.screenshot().pngRepresentation.write(to: handoff.appendingPathComponent("\(name).png"))
    }

    /// A tab: in the tab bar on an iPhone, and wherever iPadOS puts it on an iPad.
    private func tab(_ app: XCUIApplication, _ name: String) -> XCUIElement {
        let bar = app.tabBars.buttons[name]
        return bar.exists ? bar : app.buttons[name].firstMatch
    }

    private var iPad: Bool { UIDevice.current.userInterfaceIdiom == .pad }

    /// iOS's own alert asking to send notifications (#1260) comes over the
    /// app whenever the app asks, and takes the next tap: answer it first.
    private func clearAlerts() {
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        for name in ["Allow", "Don’t Allow"] where springboard.alerts.buttons[name].exists {
            springboard.alerts.buttons[name].tap()
            return
        }
    }

    /// Tap a text field until the keyboard is up for it: a tap as the view
    /// settles (or as an alert goes) can land without giving it focus.
    private func focus(_ app: XCUIApplication, _ field: XCUIElement) {
        for _ in 0..<4 {
            clearAlerts()
            field.tap()
            if app.keyboards.firstMatch.shows(within: 3) { return }
        }
    }

    /// A tab, from home's menu or from the tab bar once one is open.
    private func go(_ app: XCUIApplication, _ name: String) {
        clearAlerts()
        let menu = app.buttons["home-menu"]
        if menu.exists {
            // Once more if the menu closed as home moved under it.
            let item = app.buttons[name].firstMatch
            for _ in 0..<3 where !item.exists {
                menu.tap()
                _ = item.shows(within: 5)
            }
            if !item.exists { shot(app, "fail-menu-\(name)") }
            XCTAssertTrue(item.exists, "no \(name) in home's menu: \(app.debugDescription)")
            item.tap()
        } else {
            tab(app, name).tap()
        }
    }

    /// Back one screen: the bar's back button (on an iPad the sidebar's
    /// button comes first, so by its name).
    private func goBack(_ app: XCUIApplication) {
        clearAlerts()
        let named = app.navigationBars.buttons["Back"].firstMatch
        (named.exists ? named : app.navigationBars.buttons.element(boundBy: 0)).tap()
    }

    func testSignInProjectsChatAndATerminal() throws {
        continueAfterFailure = false
        let env = ProcessInfo.processInfo.environment
        let app = XCUIApplication()
        app.launchEnvironment["ARUGULA_RESET"] = "1"
        app.launchEnvironment["ARUGULA_SIGNIN_BY_HAND"] = "1"
        app.launchEnvironment["ARUGULA_CONTROL_URL"] = env["ARUGULA_CONTROL_URL"] ?? ""
        app.launch()

        // Sign in: the ticket goes to the script, which allows it and hands back
        // the address the page was sent to.
        XCTAssertTrue(app.buttons["signin-start"].shows(within: 30), "no sign-in")
        shot(app, "1-signin")
        app.buttons["signin-start"].tap()
        let ticket = app.staticTexts["signin-ticket"]
        let asked = ticket.shows(within: 30)
        if !asked { shot(app, "1-no-ticket") }
        XCTAssertTrue(asked, "no ticket: \(app.staticTexts["signin-problem"].exists ? app.staticTexts["signin-problem"].label : "")")
        try FileManager.default.createDirectory(at: handoff, withIntermediateDirectories: true)
        let back = handoff.appendingPathComponent("callback")
        try? FileManager.default.removeItem(at: back)
        try ticket.label.write(to: handoff.appendingPathComponent("ticket"), atomically: true, encoding: .utf8)
        var callback = ""
        let allowBy = Date().addingTimeInterval(120)
        while callback.isEmpty && Date() < allowBy {
            callback = (try? String(contentsOf: back, encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            if callback.isEmpty { sleep(1) }
        }
        XCTAssertFalse(callback.isEmpty, "nobody allowed the sign-in")
        let field = app.textFields["signin-callback"]
        field.tap()
        field.typeText(callback)
        app.buttons["signin-finish"].tap()

        // In a project, as the signed-in account: the personal one at first, named
        // Default since the cloud repo's #148.
        let signedIn = app.buttons["project-switcher"].shows(within: 60)
        if !signedIn { shot(app, "1-never-signed-in") }
        XCTAssertTrue(signedIn, "never signed in: \(app.staticTexts["signin-problem"].exists ? app.staticTexts["signin-problem"].label : "still waiting")")
        let switcher = app.buttons["project-switcher"]
        let deadline0 = Date().addingTimeInterval(30)
        while !switcher.label.lowercased().contains("default") && Date() < deadline0 { sleep(1) }
        XCTAssertTrue(switcher.label.lowercased().contains("default"), "not in the Default project: \(switcher.label)")
        shot(app, "2-projects")

        // Home (Arugula #1255): the board the script made, its row's question
        // under Needs you, its thread and its lanes, and back.
        if let title = env["ARUGULA_BOARD"], !title.isEmpty {
            let board = app.buttons["home-board-\(title)"]
            XCTAssertTrue(board.shows(within: 45), "home has no board \(title): \(app.debugDescription)")
            if let ask = env["ARUGULA_ASK"], !ask.isEmpty {
                let asked = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", ask)).firstMatch
                XCTAssertTrue(asked.shows(within: 30), "Needs you hasn't the board's question")
            }
            clearAlerts()
            shot(app, "2-home")
            board.tap()
            XCTAssertTrue(app.staticTexts["board-lead"].shows(within: 30), "no board thread")
            let said = "to-the-board-\(Int.random(in: 1000...9999))"
            let draft = app.textFields["chat-draft"]
            XCTAssertTrue(draft.shows(within: 15))
            focus(app, draft)
            draft.typeText(said)
            app.buttons["chat-send"].tap()
            XCTAssertTrue(app.staticTexts[said].shows(within: 20), "the board's thread didn't take the post")
            shot(app, "2-board-thread")
            clearAlerts()
            app.buttons["open-board"].tap()
            XCTAssertTrue(app.staticTexts["NEXT"].shows(within: 30), "no lanes")
            shot(app, "2-board-lanes")
            goBack(app)
            XCTAssertTrue(app.staticTexts["board-lead"].shows(within: 15), "not back on the thread")
            if !iPad {
                goBack(app)
                XCTAssertTrue(board.shows(within: 15), "not back home")
            }
        }

        // Answered in the row (Arugula #1256): the stand-in agent's permission
        // prompt (after a peek at it) and its question, then the board's ask.
        // The script checks that the agent went on with the answers.
        if env["ARUGULA_AGENT"] == "1" {
            let asking = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", "phone-allowed")).firstMatch
            XCTAssertTrue(asking.shows(within: 60), "no permission prompt on home: \(app.debugDescription)")
            clearAlerts()
            asking.tap()
            let lines = app.descendants(matching: .any)["peek-lines"]
            XCTAssertTrue(lines.shows(within: 20), "no peek")
            let peekBy = Date().addingTimeInterval(20)
            while !lines.label.contains("Read src/main.rs") && Date() < peekBy { sleep(1) }
            XCTAssertTrue(lines.label.contains("Read src/main.rs"), "the peek shows: \(lines.label)")
            shot(app, "3-peek")
            app.buttons["peek-close"].tap()
            // The sheet goes before its row is tapped.
            let closedBy = Date().addingTimeInterval(10)
            while lines.exists && Date() < closedBy { usleep(200_000) }
            let allow = app.buttons["answer-Allow"]
            XCTAssertTrue(allow.shows(within: 15), "no Allow in the row")
            clearAlerts()
            allow.tap()
            XCTAssertTrue(app.staticTexts["toast"].shows(within: 15) || !allow.exists, "Allow wasn't taken")
            let green = app.buttons["answer-Green"]
            XCTAssertTrue(green.shows(within: 45), "the agent's question didn't come after the Allow")
            clearAlerts()
            shot(app, "3-question")
            green.tap()
            let goneBy = Date().addingTimeInterval(30)
            while green.exists && Date() < goneBy { sleep(1) }
            XCTAssertFalse(green.exists, "the question's row didn't fold away")
            let yes = app.buttons["answer-Yes"]
            if yes.shows(within: 10) {
                clearAlerts()
                yes.tap()
                let yesBy = Date().addingTimeInterval(30)
                while yes.exists && Date() < yesBy { sleep(1) }
                if yes.exists { shot(app, "fail-board-ask") }
                XCTAssertFalse(yes.exists, "the board's ask didn't fold away: \(app.debugDescription)")
            }
            shot(app, "3-answered")
        }

        // New (Arugula #1257): tell the board, which opens its thread; then
        // a New idea, filed as an issue and handed to an agent (on testnet,
        // which has no hosted sessions, a machine's: box-systemd's), whose
        // conversation opens and is listed under New ideas.
        if let title = env["ARUGULA_BOARD"], !title.isEmpty {
            let new = app.buttons["home-new"]
            XCTAssertTrue(new.shows(within: 15), "no New on home")
            new.tap()
            XCTAssertTrue(app.buttons["new-board"].shows(within: 10), "no Tell a board")
            app.buttons["new-board"].tap()
            let pick = app.buttons["tell-\(title)"]
            XCTAssertTrue(pick.shows(within: 10), "the board isn't offered")
            pick.tap()
            XCTAssertTrue(app.staticTexts["board-lead"].shows(within: 20), "telling the board didn't open its thread")
            let told = "told-the-board-\(Int.random(in: 1000...9999))"
            let draft = app.textFields["chat-draft"]
            XCTAssertTrue(draft.shows(within: 15))
            focus(app, draft)
            draft.typeText(told)
            app.buttons["chat-send"].tap()
            XCTAssertTrue(app.staticTexts[told].shows(within: 20), "the board's thread didn't take it")
            shot(app, "4-told")
            if !iPad { goBack(app) }

            XCTAssertTrue(new.shows(within: 15), "not back home")
            new.tap()
            XCTAssertTrue(app.buttons["new-idea"].shows(within: 10), "no New idea")
            app.buttons["new-idea"].tap()
            let words = app.textViews["idea-words"].exists ? app.textViews["idea-words"] : app.textFields["idea-words"]
            XCTAssertTrue(words.shows(within: 10), "no words for the idea")
            focus(app, words)
            let idea = "Chat that keeps working on the train \(Int.random(in: 1000...9999))"
            words.typeText(idea)
            shot(app, "4-idea")
            app.buttons["idea-start"].tap()
            // Hosted, it opens at once; else a machine to run it on.
            let machine = app.buttons[env["ARUGULA_MACHINE"].map { "idea-on-\($0)" } ?? "idea-on-box-systemd"]
            if machine.shows(within: 20) {
                shot(app, "4-idea-machine")
                machine.tap()
            }
            let reply = app.textFields["chat-draft"]
            XCTAssertTrue(reply.shows(within: 30), "the idea's conversation didn't open: \(app.debugDescription)")
            shot(app, "4-idea-thread")
            if !iPad { goBack(app) }
            let listed = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'home-idea-' AND label CONTAINS %@", idea)).firstMatch
            XCTAssertTrue(listed.shows(within: 45), "the idea isn't under New ideas: \(app.debugDescription)")
            shot(app, "4-ideas")
        }

        // Chat: post to the room the script made, and see it come back.
        go(app, "Chat")
        let room = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'room-'")).firstMatch
        XCTAssertTrue(room.shows(within: 30), "no chat room")
        room.tap()
        // The room is live: chat's stream is open.
        let liveMark = app.staticTexts["chat-live"]
        let liveBy = Date().addingTimeInterval(30)
        while !(liveMark.exists && liveMark.label == "LIVE") && Date() < liveBy { sleep(1) }
        XCTAssertEqual(liveMark.exists ? liveMark.label : "missing", "LIVE", "chat's stream never opened")
        let draft = app.textFields["chat-draft"]
        XCTAssertTrue(draft.shows(within: 15))
        focus(app, draft)
        let marker = "from-the-simulator-\(Int.random(in: 1000...9999))"
        draft.typeText(marker)
        app.buttons["chat-send"].tap()
        XCTAssertTrue(app.staticTexts[marker].shows(within: 20), "the message didn't come back")
        shot(app, "3-chat")

        // A terminal on a machine, through the relay. On an iPhone, back out of the
        // room first: its keyboard covers the tab bar. On an iPad the room sits
        // beside the list, so the tabs stay in reach.
        if !iPad { app.navigationBars.buttons.firstMatch.tap() }
        go(app, "Machines")
        let machine = app.buttons[env["ARUGULA_MACHINE"].map { "machine-\($0)" } ?? "machine-box-systemd"]
        XCTAssertTrue(machine.shows(within: 30), "machine not listed")
        machine.tap()
        let new = app.buttons["new-terminal"]
        XCTAssertTrue(new.shows(within: 30))
        XCTAssertTrue(new.waitFor(enabled: true, timeout: 45), "never connected: \(app.staticTexts["machine-problem"].label)")
        new.tap()
        let terminal = app.descendants(matching: .any)["terminal-view"]
        let opened = terminal.shows(within: 30)
        shot(app, "4-after-new-terminal")
        XCTAssertTrue(opened, "no terminal: \(app.debugDescription)")
        sleep(2)
        terminal.typeText("echo SIM-$((6*7))\n")
        let deadline = Date().addingTimeInterval(25)
        var seen = ""
        while Date() < deadline {
            seen = terminal.value as? String ?? ""
            if seen.contains("SIM-42") { break }
            sleep(1)
        }
        shot(app, "5-terminal")
        XCTAssertTrue(seen.contains("SIM-42"), "the terminal showed: \(seen)")

        // Back from driving it: the machine's screen (on an iPad, its tab whole).
        app.buttons["terminal-done"].tap()
        sleep(3)
        shot(app, "6-machine")
        XCTAssertTrue(app.buttons["new-terminal"].shows(within: 10), "not back on the machine")

        // Signing out ends it here: the sign-in again.
        go(app, "You")
        let signOut = app.buttons["sign-out"]
        XCTAssertTrue(signOut.shows(within: 30), "no sign-out")
        shot(app, "7-you")
        signOut.tap()
        XCTAssertTrue(app.buttons["signin-start"].shows(within: 15), "still signed in")
    }
}

private extension XCUIElement {
    /// As `waitForExistence`, answering iOS's notification alert (#1260)
    /// while it waits: it comes over the app when the app asks, whenever
    /// that is, and takes the next tap.
    func shows(within timeout: TimeInterval) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        repeat {
            if springboard.alerts.buttons["Allow"].exists { springboard.alerts.buttons["Allow"].tap() }
            if exists { return true }
            usleep(300_000)
        } while Date() < end
        return exists
    }

    func waitFor(enabled: Bool, timeout: TimeInterval) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            if exists && isEnabled == enabled { return true }
            usleep(300_000)
        }
        return false
    }
}
