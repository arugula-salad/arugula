// The app as a person takes it, on a simulator against a real control
// (native/ios/test-sim.sh): sign in, home, a board's thread and its lanes,
// projects, chat, a terminal. The script
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

    /// A tab, from home's menu or from the tab bar once one is open.
    private func go(_ app: XCUIApplication, _ name: String) {
        let menu = app.buttons["home-menu"]
        if menu.exists {
            menu.tap()
            let item = app.buttons[name].firstMatch
            XCTAssertTrue(item.waitForExistence(timeout: 10), "no \(name) in home's menu")
            item.tap()
        } else {
            tab(app, name).tap()
        }
    }

    /// Back one screen: the bar's back button (on an iPad the sidebar's
    /// button comes first, so by its name).
    private func goBack(_ app: XCUIApplication) {
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
        XCTAssertTrue(app.buttons["signin-start"].waitForExistence(timeout: 30), "no sign-in")
        shot(app, "1-signin")
        app.buttons["signin-start"].tap()
        let ticket = app.staticTexts["signin-ticket"]
        let asked = ticket.waitForExistence(timeout: 30)
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
        let signedIn = app.buttons["project-switcher"].waitForExistence(timeout: 60)
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
            XCTAssertTrue(board.waitForExistence(timeout: 45), "home has no board \(title): \(app.debugDescription)")
            if let ask = env["ARUGULA_ASK"], !ask.isEmpty {
                let asked = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", ask)).firstMatch
                XCTAssertTrue(asked.waitForExistence(timeout: 30), "Needs you hasn't the board's question")
            }
            shot(app, "2-home")
            board.tap()
            XCTAssertTrue(app.staticTexts["board-lead"].waitForExistence(timeout: 30), "no board thread")
            let said = "to-the-board-\(Int.random(in: 1000...9999))"
            let draft = app.textFields["chat-draft"]
            XCTAssertTrue(draft.waitForExistence(timeout: 15))
            draft.tap()
            draft.typeText(said)
            app.buttons["chat-send"].tap()
            XCTAssertTrue(app.staticTexts[said].waitForExistence(timeout: 20), "the board's thread didn't take the post")
            shot(app, "2-board-thread")
            app.buttons["open-board"].tap()
            XCTAssertTrue(app.staticTexts["NEXT"].waitForExistence(timeout: 30), "no lanes")
            shot(app, "2-board-lanes")
            goBack(app)
            XCTAssertTrue(app.staticTexts["board-lead"].waitForExistence(timeout: 15), "not back on the thread")
            if !iPad {
                goBack(app)
                XCTAssertTrue(board.waitForExistence(timeout: 15), "not back home")
            }
        }

        // Chat: post to the room the script made, and see it come back.
        go(app, "Chat")
        let room = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'room-'")).firstMatch
        XCTAssertTrue(room.waitForExistence(timeout: 30), "no chat room")
        room.tap()
        // The room is live: chat's stream is open.
        let liveMark = app.staticTexts["chat-live"]
        let liveBy = Date().addingTimeInterval(30)
        while !(liveMark.exists && liveMark.label == "LIVE") && Date() < liveBy { sleep(1) }
        XCTAssertEqual(liveMark.exists ? liveMark.label : "missing", "LIVE", "chat's stream never opened")
        let draft = app.textFields["chat-draft"]
        XCTAssertTrue(draft.waitForExistence(timeout: 15))
        draft.tap()
        let marker = "from-the-simulator-\(Int.random(in: 1000...9999))"
        draft.typeText(marker)
        app.buttons["chat-send"].tap()
        XCTAssertTrue(app.staticTexts[marker].waitForExistence(timeout: 20), "the message didn't come back")
        shot(app, "3-chat")

        // A terminal on a machine, through the relay. On an iPhone, back out of the
        // room first: its keyboard covers the tab bar. On an iPad the room sits
        // beside the list, so the tabs stay in reach.
        if !iPad { app.navigationBars.buttons.firstMatch.tap() }
        go(app, "Machines")
        let machine = app.buttons[env["ARUGULA_MACHINE"].map { "machine-\($0)" } ?? "machine-box-systemd"]
        XCTAssertTrue(machine.waitForExistence(timeout: 30), "machine not listed")
        machine.tap()
        let new = app.buttons["new-terminal"]
        XCTAssertTrue(new.waitForExistence(timeout: 30))
        XCTAssertTrue(new.waitFor(enabled: true, timeout: 45), "never connected: \(app.staticTexts["machine-problem"].label)")
        new.tap()
        let terminal = app.descendants(matching: .any)["terminal-view"]
        let opened = terminal.waitForExistence(timeout: 30)
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
        XCTAssertTrue(app.buttons["new-terminal"].waitForExistence(timeout: 10), "not back on the machine")

        // Signing out ends it here: the sign-in again.
        go(app, "You")
        let signOut = app.buttons["sign-out"]
        XCTAssertTrue(signOut.waitForExistence(timeout: 30), "no sign-out")
        shot(app, "7-you")
        signOut.tap()
        XCTAssertTrue(app.buttons["signin-start"].waitForExistence(timeout: 15), "still signed in")
    }
}

private extension XCUIElement {
    func waitFor(enabled: Bool, timeout: TimeInterval) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            if exists && isEnabled == enabled { return true }
            usleep(300_000)
        }
        return false
    }
}
