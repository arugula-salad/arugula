// The app as a person takes it, on a simulator against a real control
// (native/ios/test-sim.sh): pair, projects, chat, a terminal. The script approves
// the pairing from outside, as a signed-in browser would.

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

    func testPairProjectsChatAndATerminal() throws {
        continueAfterFailure = false
        let env = ProcessInfo.processInfo.environment
        let app = XCUIApplication()
        app.launchEnvironment["ARUGULA_RESET"] = "1"
        app.launchEnvironment["ARUGULA_CONTROL_URL"] = env["ARUGULA_CONTROL_URL"] ?? ""
        app.launch()

        // Pair: the code goes to the script, which approves it.
        app.buttons["pair-start"].tap()
        let code = app.staticTexts["pair-code"]
        XCTAssertTrue(code.waitForExistence(timeout: 30), "no pairing code")
        shot(app, "1-pairing")
        try FileManager.default.createDirectory(at: handoff, withIntermediateDirectories: true)
        try code.label.write(to: handoff.appendingPathComponent("code"), atomically: true, encoding: .utf8)

        // In a project, as the signed-in account: the personal one at first.
        let paired = app.buttons["project-switcher"].waitForExistence(timeout: 120)
        if !paired { shot(app, "1-never-paired") }
        XCTAssertTrue(paired, "never paired: \(app.staticTexts["pair-problem"].exists ? app.staticTexts["pair-problem"].label : "still waiting")")
        let switcher = app.buttons["project-switcher"]
        let deadline0 = Date().addingTimeInterval(30)
        while !switcher.label.lowercased().contains("personal") && Date() < deadline0 { sleep(1) }
        XCTAssertTrue(switcher.label.lowercased().contains("personal"), "not in the Personal project: \(switcher.label)")
        shot(app, "2-projects")

        // Chat: post to the room the script made, and see it come back.
        tab(app, "Chat").tap()
        let room = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'room-'")).firstMatch
        XCTAssertTrue(room.waitForExistence(timeout: 30), "no chat room")
        room.tap()
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
        tab(app, "Machines").tap()
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
