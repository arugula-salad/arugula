// The app as a person takes it, on a simulator against a real control
// (native/ios/test-sim.sh): sign in, the tab bar, home, the project's issues,
// the zoom to a board and a card's work and back, by taps and by address,
// a board's thread and its lanes,
// an agent's permission prompt and question answered from home, projects,
// a Doc in a room, chat, a terminal, a deploy approved and a project's runs
// (Arugula #604). The script
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

    /// A tab, from the tab bar (always there, Arugula #1262).
    private func go(_ app: XCUIApplication, _ name: String) {
        clearAlerts()
        let t = tab(app, name)
        XCTAssertTrue(t.shows(within: 10), "no \(name) tab: \(app.debugDescription)")
        // A tap as a screen slides back can be lost: again until it's picked.
        for _ in 0..<3 {
            t.tap()
            // Once a second: a quicker poll floods XCTest's log on an iPad,
            // which then takes the app down.
            for _ in 0..<3 where !t.isSelected { sleep(1) }
            if t.isSelected { return }
            clearAlerts()
        }
    }

    /// Send what's in the room's draft. A tap while the on-screen keyboard is
    /// still coming up can land where the button was: tap again until the
    /// draft is empty.
    private func send(_ app: XCUIApplication) {
        let draft = app.textFields["chat-draft"]
        for _ in 0..<3 {
            app.buttons["chat-send"].tap()
            let by = Date().addingTimeInterval(3)
            while Date() < by {
                let v = draft.value as? String ?? ""
                if v.isEmpty || v == draft.placeholderValue { return }
                usleep(250_000)
            }
        }
    }

    /// Back one screen: the bar's back button (on an iPad the sidebar's
    /// button comes first, so by its name; or home's own, beside the tabs).
    private func goBack(_ app: XCUIApplication) {
        clearAlerts()
        // On an iPad the tabs float over the detail's back button: home's
        // detail has its own, on the other side.
        let beside = app.buttons["detail-back"].firstMatch
        let named = app.navigationBars.buttons["Back"].firstMatch
        (beside.exists ? beside : named.exists ? named : app.navigationBars.buttons.element(boundBy: 0)).tap()
    }

    /// Back until `there` shows: a tap as a screen settles can be lost.
    private func goBack(_ app: XCUIApplication, until there: XCUIElement) {
        for _ in 0..<3 where !there.exists {
            goBack(app)
            _ = there.shows(within: 8)
        }
    }

    /// Pick a project from the switcher heading the tab that's open.
    private func pick(_ app: XCUIApplication, _ name: String) {
        let switcher = app.buttons["project-switcher"].firstMatch
        XCTAssertTrue(switcher.shows(within: 15), "no project switcher")
        for _ in 0..<3 where !switcher.label.lowercased().contains(name.lowercased()) {
            clearAlerts()
            switcher.tap()
            let item = app.buttons[name].firstMatch
            if item.shows(within: 10) { item.tap() }
            let by = Date().addingTimeInterval(10)
            while !switcher.label.lowercased().contains(name.lowercased()) && Date() < by { usleep(300_000) }
        }
        XCTAssertTrue(switcher.label.lowercased().contains(name.lowercased()), "not in \(name): \(switcher.label)")
    }

    /// Open an address in the app, as a link from elsewhere would: iOS asks
    /// once whether to open it in Arugula.
    private func openAddress(_ app: XCUIApplication, _ address: String) {
        XCUIDevice.shared.system.open(URL(string: address)!)
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let open = springboard.buttons["Open"]
        if open.waitForExistence(timeout: 5) { open.tap() }
        _ = app.wait(for: .runningForeground, timeout: 10)
    }

    /// Peek at the lead over the open thread: its session's last lines say
    /// the stand-in is ready. Then close the peek.
    private func peekAtLead(_ app: XCUIApplication, _ name: String) {
        let peek = app.buttons["lead-peek"]
        XCTAssertTrue(peek.shows(within: 30), "no peek at the lead: \(app.debugDescription)")
        let lines = app.descendants(matching: .any)["peek-lines"]
        // A tap as the room settles can open nothing: again.
        for _ in 0..<3 where !lines.exists {
            _ = peek.waitFor(hittable: true, timeout: 5)
            clearAlerts()
            peek.tap()
            _ = lines.shows(within: 10)
        }
        XCTAssertTrue(lines.exists, "no peek")
        let by = Date().addingTimeInterval(20)
        while !lines.label.contains("cos ready") && Date() < by { sleep(1) }
        shot(app, name)
        XCTAssertTrue(lines.label.contains("cos ready"), "the lead's peek shows: \(lines.label)")
        // Who it is, and that a reply goes to it (Arugula #1305).
        let who = app.staticTexts["who-line"].firstMatch
        XCTAssertTrue(who.label.contains("straight to this agent"), "the peek's who-line: \(who.label)")
        app.buttons["peek-close"].tap()
        let closedBy = Date().addingTimeInterval(10)
        while lines.exists && Date() < closedBy { usleep(200_000) }
    }

    func testSignInProjectsChatAndATerminal() throws {
        continueAfterFailure = false
        let env = ProcessInfo.processInfo.environment
        let app = XCUIApplication()
        app.launchEnvironment["ARUGULA_RESET"] = "1"
        app.launchEnvironment["ARUGULA_SIGNIN_BY_HAND"] = "1"
        app.launchEnvironment["ARUGULA_APPROVE_BY_HAND"] = "1"
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

        // The tab bar is there from the start, Home first (Arugula #1262), and
        // the project's issues open from its menu.
        for name in ["Home", "Boards", "Chat", "Machines", "You"] {
            XCTAssertTrue(tab(app, name).shows(within: 10), "no \(name) on the tab bar")
        }
        XCTAssertFalse(tab(app, "Issues").exists, "Issues is still a tab")
        switcher.tap()
        let issuesItem = app.buttons["project-issues"]
        XCTAssertTrue(issuesItem.shows(within: 10), "no Issues in the project's menu")
        issuesItem.tap()
        XCTAssertTrue(app.buttons["issues-done"].shows(within: 15), "the project's issues didn't open")
        shot(app, "2-issues")
        app.buttons["issues-done"].tap()
        XCTAssertTrue(switcher.shows(within: 10), "not back home from the issues")

        // What a passkey approves (Arugula #604). A deploy waiting on this
        // account is first under Needs you, and opens over the app; Approve
        // would open control's dialog in a web sheet, where the passkey is:
        // here the app shows that address, the script approves the deploy
        // with the account's passkey as the dialog would, and Done stands in
        // for the sheet coming back. The app reads it again: approved. Then
        // the project's Runs, from its menu, opens its grant's dialog the
        // same way.
        if let deploy = env["ARUGULA_DEPLOY"], !deploy.isEmpty {
            let row = app.buttons["need-deploy-\(deploy)"]
            XCTAssertTrue(row.shows(within: 30), "the waiting deploy isn't under Needs you: \(app.debugDescription)")
            shot(app, "2-deploy-need")
            row.tap()
            let state = app.staticTexts["deploy-state"]
            XCTAssertTrue(state.shows(within: 20), "the deploy didn't open")
            XCTAssertEqual(state.value as? String, "pending")
            let approve = app.buttons["deploy-approve"]
            XCTAssertTrue(approve.shows(within: 10), "no Approve on a deploy waiting for this owner")
            shot(app, "2-deploy-sheet")
            approve.tap()
            let link = app.staticTexts["approve-link"]
            XCTAssertTrue(link.shows(within: 10), "Approve opened nothing")
            XCTAssertTrue(link.label.hasSuffix("/#deploy=\(deploy)&app"), "Approve opens \(link.label)")
            let approved = handoff.appendingPathComponent("approved")
            try? FileManager.default.removeItem(at: approved)
            try link.label.write(to: handoff.appendingPathComponent("approve"), atomically: true, encoding: .utf8)
            let approveBy = Date().addingTimeInterval(120)
            while !FileManager.default.fileExists(atPath: approved.path) && Date() < approveBy { sleep(1) }
            XCTAssertTrue(FileManager.default.fileExists(atPath: approved.path), "nobody approved the deploy")
            app.buttons["approve-done"].tap()
            let decidedBy = Date().addingTimeInterval(20)
            while state.value as? String == "pending" && Date() < decidedBy { sleep(1) }
            shot(app, "2-deploy-approved")
            // Approved, and maybe gone on from there (testnet's driver makes
            // its box, or fails to): read again, not waiting any more.
            let after = state.value as? String ?? ""
            XCTAssertTrue(!["pending", "denied", ""].contains(after), "the deploy reads \(after) after")
            XCTAssertFalse(app.buttons["deploy-approve"].exists, "an approved deploy still offers Approve")
            app.buttons["deploy-close"].tap()
            let goneBy = Date().addingTimeInterval(15)
            while row.exists && Date() < goneBy { sleep(1) }
            XCTAssertFalse(row.exists, "the approved deploy is still under Needs you")

            switcher.tap()
            let runs = app.buttons["project-runs"]
            XCTAssertTrue(runs.shows(within: 10), "no Runs in the project's menu")
            runs.tap()
            let grant = app.staticTexts["runs-grant"]
            XCTAssertTrue(grant.shows(within: 20), "the project's runs didn't open")
            shot(app, "2-runs")
            app.buttons["runs-change"].tap()
            XCTAssertTrue(link.shows(within: 10), "Allow runs opened nothing")
            XCTAssertNotNil(link.label.range(of: #"/#run-grant=prj_[0-9a-f]{32}&app$"#, options: .regularExpression), "Allow runs opens \(link.label)")
            app.buttons["approve-done"].tap()
            app.buttons["runs-close"].tap()
            XCTAssertTrue(switcher.shows(within: 10), "not back home from the runs")
        }

        // A Doc in a conversation (Arugula #1259): its card opens the Doc's page
        // in a web view, signed in as the app; on an iPad in the right-hand
        // column, beside the room, and on a phone over it. A comment made in
        // the page saves, and its prototype runs.
        if let doc = env["ARUGULA_DOC"], !doc.isEmpty, let roomName = env["ARUGULA_DOC_ROOM"], !roomName.isEmpty {
            let row = app.buttons["home-room-\(roomName)"]
            XCTAssertTrue(row.waitForExistence(timeout: 45), "home has no room \(roomName)")
            row.tap()
            let card = app.buttons["doc-card-\(doc)"]
            XCTAssertTrue(card.waitForExistence(timeout: 30), "the room has no card for \(doc)")
            card.tap()
            let web = app.webViews.firstMatch
            XCTAssertTrue(web.waitForExistence(timeout: 30), "no web view for the Doc")
            let para = env["ARUGULA_DOC_PARA"] ?? ""
            let paragraph = web.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", para)).firstMatch
            let drew = paragraph.waitForExistence(timeout: 45)
            shot(app, "2-doc")
            XCTAssertTrue(drew, "the Doc's page never drew its paragraph: \(app.debugDescription)")
            if iPad {
                XCTAssertTrue(app.textFields["chat-draft"].exists, "the room isn't beside the Doc")
            }
            paragraph.tap()
            let box = web.textViews.firstMatch
            XCTAssertTrue(box.waitForExistence(timeout: 15), "no comment box")
            box.tap()
            let said = "from-the-app-\(Int.random(in: 1000...9999))"
            box.typeText(said)
            web.buttons["Save"].tap()
            let saved = web.staticTexts[said].waitForExistence(timeout: 20)
            shot(app, "2-doc-comment")
            XCTAssertTrue(saved, "the comment didn't come back into the page")
            try said.write(to: handoff.appendingPathComponent("comment"), atomically: true, encoding: .utf8)
            if let proto = env["ARUGULA_DOC_PROTO"], !proto.isEmpty {
                let run = web.buttons[proto]
                XCTAssertTrue(run.waitForExistence(timeout: 30), "the prototype never showed its button")
                // A tap into the frame can land before the prototype has
                // wired its button (the runner writes it in): tap again, a
                // few times, until it says so.
                var ran = false
                for _ in 0..<4 where !ran {
                    run.tap()
                    ran = web.staticTexts["prototype ran"].waitForExistence(timeout: 5)
                }
                shot(app, "2-doc-prototype")
                XCTAssertTrue(ran, "the prototype didn't run")
            }
            // Close the Doc: on an iPad home's list and the room are back; on a
            // phone, back out of the room to home.
            app.buttons["doc-close"].tap()
            if iPad {
                XCTAssertTrue(app.buttons["home-room-\(roomName)"].waitForExistence(timeout: 10), "home's list didn't come back")
            } else {
                goBack(app)
            }
        }

        // The chief of staff (Arugula #1279), in the team's project the
        // script made: the staff room first on home, led by the script's
        // stand-in session; ask it something there and peek at its session.
        // Then Boards: the board it leads, whose lead opens the board's
        // thread, and a peek from there. Then back to Default.
        if let team = env["ARUGULA_STAFF_PROJECT"], !team.isEmpty, let led = env["ARUGULA_STAFF_BOARD"], !led.isEmpty {
            let lead = env["ARUGULA_STAFF_LEAD"] ?? "cos"
            go(app, "Home")
            pick(app, team)
            // Home and Boards in the team's project as they look, before
            // anything is asked of them: on main's app, the Before shots.
            let boardShown = app.buttons["board-\(led)"]
            _ = app.buttons["home-board-\(led)"].shows(within: 45)
            sleep(2)
            shot(app, "5-team-home")
            go(app, "Boards")
            _ = boardShown.shows(within: 45)
            sleep(2)
            shot(app, "5-team-boards")
            go(app, "Home")
            // A lead's name on home opens that agent, not its board's thread
            // (Arugula #1305); the thread is a button of its own.
            let homeLead = app.buttons["home-lead-\(led)"]
            XCTAssertTrue(homeLead.shows(within: 45), "home's board has no lead to tap: \(app.debugDescription)")
            let work = app.descendants(matching: .any)["work-level"]
            for _ in 0..<3 where !work.exists {
                _ = homeLead.waitFor(hittable: true, timeout: 5)
                clearAlerts()
                homeLead.tap()
                _ = work.shows(within: 10)
            }
            XCTAssertTrue(work.exists, "the lead's name didn't open the agent: \(app.debugDescription)")
            let leadNamed = app.staticTexts.matching(NSPredicate(format: "label == %@", lead)).firstMatch
            XCTAssertTrue(leadNamed.shows(within: 30), "the lead's session isn't on its Work level: \(app.debugDescription)")
            XCTAssertFalse(app.descendants(matching: .any)["board-lead"].exists, "the lead's name opened the board's thread")
            shot(app, "5-home-lead")
            app.buttons["zoom-path-overview"].tap()
            // Its Board level: the lead's card names it, and Talk to the
            // lead opens it over the board (Arugula #1306).
            let ledRow = app.buttons["home-board-\(led)"]
            XCTAssertTrue(ledRow.shows(within: 15), "not back home")
            let leadCard = app.staticTexts["lead-card-name"]
            for _ in 0..<3 where !leadCard.exists {
                _ = ledRow.waitFor(hittable: true, timeout: 5)
                clearAlerts()
                ledRow.tap()
                _ = leadCard.shows(within: 10)
            }
            XCTAssertTrue(leadCard.exists, "no lead card on the led board: \(app.debugDescription)")
            XCTAssertTrue(leadCard.label.contains(lead), "the lead card says: \(leadCard.label)")
            shot(app, "5-board-level-led")
            app.buttons["lead-talk"].tap()
            XCTAssertTrue(leadNamed.shows(within: 30), "Talk to the lead didn't open the lead: \(app.debugDescription)")
            shot(app, "5-lead-talk")
            goBack(app)
            if !leadCard.shows(within: 15) { shot(app, "fail-led-board") }
            XCTAssertTrue(leadCard.exists, "not back on the led board: \(app.debugDescription)")
            let homeThread = app.buttons["home-thread-\(led)"]
            // A tap as the board settles can be lost: again until home is there.
            for _ in 0..<3 where !homeThread.exists {
                clearAlerts()
                if app.buttons["zoom-path-overview"].exists { app.buttons["zoom-path-overview"].tap() }
                _ = homeThread.shows(within: 10)
            }
            XCTAssertTrue(homeThread.exists, "no Board thread on home's row: \(app.debugDescription)")
            // Home slides back in: a tap as it settles opens nothing, so again.
            let homeThreadLead = app.descendants(matching: .any)["board-lead"]
            for _ in 0..<3 where !homeThreadLead.exists {
                _ = homeThread.waitFor(hittable: true, timeout: 5)
                clearAlerts()
                homeThread.tap()
                _ = homeThreadLead.shows(within: 10)
            }
            if !homeThreadLead.exists { shot(app, "fail-home-thread") }
            XCTAssertTrue(homeThreadLead.exists, "Board thread didn't open the thread: \(app.debugDescription)")
            let threadWho = app.staticTexts["who-line"].firstMatch
            XCTAssertTrue(threadWho.label.contains("board's thread"), "the thread's who-line: \(threadWho.label)")
            shot(app, "5-home-thread")
            if !iPad { goBack(app, until: app.buttons["home-staff"]) }
            let staff = app.buttons["home-staff"]
            XCTAssertTrue(staff.shows(within: 45), "home has no chief of staff in \(team): \(app.debugDescription)")
            let ledBy = Date().addingTimeInterval(45)
            while !staff.label.contains("Led by \(lead)") && Date() < ledBy { sleep(1) }
            clearAlerts()
            shot(app, "5-staff-home")
            XCTAssertTrue(staff.label.contains("Led by \(lead)"), "home's chief of staff says: \(staff.label)")
            // A tap as home's list redraws can open nothing: again.
            let room = app.descendants(matching: .any)["staff-lead"]
            for _ in 0..<3 where !room.exists {
                _ = staff.waitFor(hittable: true, timeout: 5)
                clearAlerts()
                staff.tap()
                _ = room.shows(within: 10)
            }
            XCTAssertTrue(room.exists, "the staff room didn't open: \(app.debugDescription)")
            let morning = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "One board moved overnight")).firstMatch
            XCTAssertTrue(morning.shows(within: 20), "the chief of staff's message isn't in the room")
            let ask = "what-moved-\(Int.random(in: 1000...9999))"
            let draft = app.textFields["chat-draft"]
            XCTAssertTrue(draft.shows(within: 15))
            focus(app, draft)
            draft.typeText(ask)
            send(app)
            XCTAssertTrue(app.staticTexts[ask].shows(within: 20), "the staff room didn't take the ask")
            try ask.write(to: handoff.appendingPathComponent("staff"), atomically: true, encoding: .utf8)
            shot(app, "5-staff-room")
            peekAtLead(app, "5-staff-peek")
            if !iPad { goBack(app) }

            go(app, "Boards")
            let chip = app.buttons["board-lead-\(led)"]
            XCTAssertTrue(chip.shows(within: 45), "Boards has no lead on \(led): \(app.debugDescription)")
            XCTAssertTrue(chip.label.contains("Led by \(lead)"), "the board's lead says: \(chip.label)")
            shot(app, "5-boards")
            // The lead's name opens that agent (Arugula #1305). Boards
            // reloads its list as it comes back: a tap as it does opens
            // nothing, so tap again until the agent is there.
            for _ in 0..<3 where !work.exists {
                _ = chip.waitFor(hittable: true, timeout: 5)
                clearAlerts()
                chip.tap()
                _ = work.shows(within: 10)
            }
            XCTAssertTrue(work.exists, "the lead's name on Boards didn't open the agent: \(app.debugDescription)")
            XCTAssertTrue(leadNamed.shows(within: 30), "the lead's session isn't on its Work level")
            shot(app, "5-boards-lead")
            goBack(app)
            // Board thread, beside it, opens the board's thread.
            let threadButton = app.buttons["board-thread-\(led)"]
            let thread = app.descendants(matching: .any)["board-lead"]
            var threadShown = false
            for _ in 0..<3 where !threadShown {
                guard threadButton.shows(within: 15) else { break }
                _ = threadButton.waitFor(hittable: true, timeout: 5)
                clearAlerts()
                threadButton.tap()
                threadShown = thread.shows(within: 10)
            }
            if !threadShown { shot(app, "fail-lead-thread") }
            XCTAssertTrue(threadShown, "Board thread didn't open the board's thread: \(app.debugDescription)")
            XCTAssertTrue(thread.label.contains(lead), "the thread's lead says: \(thread.label)")
            let toLead = "to-the-lead-\(Int.random(in: 1000...9999))"
            let leadDraft = app.textFields["chat-draft"]
            XCTAssertTrue(leadDraft.shows(within: 15))
            focus(app, leadDraft)
            leadDraft.typeText(toLead)
            send(app)
            XCTAssertTrue(app.staticTexts[toLead].shows(within: 20), "the board's thread didn't take the post")
            shot(app, "5-lead-thread")
            peekAtLead(app, "5-lead-peek")
            // Back as the peek's sheet goes can land on nothing: again.
            for _ in 0..<3 where !chip.exists {
                goBack(app)
                _ = chip.shows(within: 5)
            }
            XCTAssertTrue(chip.shows(within: 10), "not back on Boards: \(app.debugDescription)")
            pick(app, "Default")
            go(app, "Home")
        }

        // Home (Arugula #1255) and the zoom (Arugula #1304): the board the
        // script made, its row's question under Needs you. Its row zooms to
        // the Board level, a ticket there to its Work level, and the path bar
        // zooms back out. The board's thread and lanes are a tap away from
        // the Board level. The Work level's address, opened as a link from
        // home, lands there again.
        if let title = env["ARUGULA_BOARD"], !title.isEmpty {
            let board = app.buttons["home-board-\(title)"]
            XCTAssertTrue(board.shows(within: 45), "home has no board \(title): \(app.debugDescription)")
            if let ask = env["ARUGULA_ASK"], !ask.isEmpty {
                let asked = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", ask)).firstMatch
                XCTAssertTrue(asked.shows(within: 30), "Needs you hasn't the board's question")
            }
            clearAlerts()
            shot(app, "2-home")
            // Home reloads its list after the project switch above: a tap as
            // it does opens nothing, so tap again until the board is there.
            let level = app.descendants(matching: .any)["board-level"]
            var zoomed = false
            for _ in 0..<3 where !zoomed {
                guard board.shows(within: 15) else { break }
                _ = board.waitFor(hittable: true, timeout: 5)
                clearAlerts()
                board.tap()
                zoomed = level.shows(within: 10)
            }
            if !zoomed { shot(app, "fail-board-level") }
            XCTAssertTrue(zoomed, "the board's row didn't zoom to its Board level: \(app.debugDescription)")
            let path = app.descendants(matching: .any)["zoom-path"]
            if !path.shows(within: 10) { shot(app, "fail-zoom-path") }
            XCTAssertTrue(path.exists, "no path bar over the Board level: \(app.debugDescription)")
            XCTAssertTrue((path.value as? String ?? "").contains("/board/brd_"), "the Board level's address: \(path.value ?? "none")")
            let ticket = app.buttons.matching(NSPredicate(format: "identifier MATCHES 'ticket-([0-9]+|row_.*)'")).firstMatch
            XCTAssertTrue(ticket.shows(within: 30), "the Board level lists no ticket: \(app.debugDescription)")
            // The Board level (Arugula #1306): a lead card (no one leads this
            // board) with Board thread beside, and the ticket with its word
            // (its question is the reader's) and the stand-in agent on it,
            // whose name opens it over the board.
            XCTAssertTrue(app.descendants(matching: .any)["lead-card"].shows(within: 10), "no lead card: \(app.debugDescription)")
            XCTAssertEqual(app.staticTexts["lead-card-name"].label, "No lead")
            XCTAssertTrue(ticket.label.contains("asks you"), "the ticket's word: \(ticket.label)")
            if env["ARUGULA_CARD_AGENT"] == "1" {
                let agentChip = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'ticket-agent-'")).firstMatch
                XCTAssertTrue(agentChip.shows(within: 45), "the ticket has no agent on it: \(app.debugDescription)")
                shot(app, "2-board-level")
                clearAlerts()
                agentChip.tap()
                let agentWork = app.descendants(matching: .any)["work-level"]
                XCTAssertTrue(agentWork.shows(within: 15), "the agent's name didn't open it: \(app.debugDescription)")
                shot(app, "2-board-agent")
                goBack(app)
                XCTAssertTrue(ticket.shows(within: 15), "not back on the Board level")
            } else {
                shot(app, "2-board-level")
            }
            clearAlerts()
            ticket.tap()
            let work = app.descendants(matching: .any)["work-level"]
            XCTAssertTrue(work.shows(within: 15), "the ticket didn't zoom to its Work level: \(app.debugDescription)")
            var address = ""
            let addressBy = Date().addingTimeInterval(10)
            while !address.contains("/card/row_") && Date() < addressBy {
                address = path.value as? String ?? ""
                if !address.contains("/card/row_") { sleep(1) }
            }
            XCTAssertTrue(address.contains("/card/row_"), "the Work level's address: \(address)")
            shot(app, "2-work-level")
            // The Work level (Arugula #1307): the stand-in's terminal, live
            // at its own size; its permission prompt over it, answered
            // there; and the terminal typed into where it is.
            if env["ARUGULA_CARD_AGENT"] == "1" {
                let term = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'work-terminal-'")).firstMatch
                XCTAssertTrue(term.shows(within: 30), "no live terminal on the Work level: \(app.debugDescription)")
                var seen = ""
                let seenBy = Date().addingTimeInterval(30)
                while !seen.contains("Read src/main.rs") && Date() < seenBy {
                    seen = term.value as? String ?? ""
                    if !seen.contains("Read src/main.rs") { sleep(1) }
                }
                XCTAssertTrue(seen.contains("Read src/main.rs"), "the Work level's terminal shows: \(seen)")
                // The ask over the panes: on an iPad home's Needs you, to its
                // left, has its own Allow.
                let allows = app.buttons.matching(identifier: "answer-Allow")
                XCTAssertTrue(allows.firstMatch.shows(within: 60), "no permission prompt over the panes: \(app.debugDescription)")
                let allowHere = allows.allElementsBoundByIndex.max { $0.frame.minX < $1.frame.minX }!
                shot(app, "2-work-ask")
                clearAlerts()
                allowHere.tap()
                let allowGone = Date().addingTimeInterval(20)
                while allowHere.exists && Date() < allowGone { sleep(1) }
                XCTAssertFalse(allowHere.exists, "the permission over the panes didn't fold away")
                clearAlerts()
                term.tap()
                _ = app.keyboards.firstMatch.shows(within: 5)
                term.typeText("zoom-typed")
                var typed = ""
                let typedBy = Date().addingTimeInterval(20)
                while !typed.contains("zoom-typed") && Date() < typedBy {
                    typed = term.value as? String ?? ""
                    if !typed.contains("zoom-typed") { sleep(1) }
                }
                shot(app, "2-work-typed")
                XCTAssertTrue(typed.contains("zoom-typed"), "what was typed at the Work level didn't show: \(typed)")
            }
            // Out to the board by the path bar, then its thread and its lanes.
            app.buttons["zoom-path-board"].tap()
            XCTAssertTrue(level.shows(within: 10), "the path bar didn't zoom out to the board")
            XCTAssertTrue(app.buttons["board-thread"].shows(within: 10), "no Thread on the Board level")
            app.buttons["board-thread"].tap()
            XCTAssertTrue(app.descendants(matching: .any)["board-lead"].shows(within: 15), "no board thread: \(app.debugDescription)")
            let said = "to-the-board-\(Int.random(in: 1000...9999))"
            let draft = app.textFields["chat-draft"]
            XCTAssertTrue(draft.shows(within: 15))
            focus(app, draft)
            draft.typeText(said)
            send(app)
            XCTAssertTrue(app.staticTexts[said].shows(within: 20), "the board's thread didn't take the post")
            shot(app, "2-board-thread")
            clearAlerts()
            app.buttons["open-board"].tap()
            XCTAssertTrue(app.staticTexts["NEXT"].shows(within: 30), "no lanes")
            shot(app, "2-board-lanes")
            goBack(app)
            XCTAssertTrue(app.descendants(matching: .any)["board-lead"].shows(within: 15), "not back on the thread")
            goBack(app)
            XCTAssertTrue(level.shows(within: 15), "not back on the Board level")
            // Out to home by the path bar.
            app.buttons["zoom-path-overview"].tap()
            if !iPad { XCTAssertTrue(board.shows(within: 15), "the path bar didn't zoom out to home") }
            // The Work level's address, as a link.
            openAddress(app, address)
            let opened = work.shows(within: 20)
            if !opened { shot(app, "fail-address") }
            XCTAssertTrue(opened, "\(address) didn't open the Work level: \(app.debugDescription)")
            var landed = ""
            let landedBy = Date().addingTimeInterval(10)
            while landed != address && Date() < landedBy {
                landed = path.value as? String ?? ""
                if landed != address { sleep(1) }
            }
            XCTAssertEqual(landed, address, "the link landed elsewhere")
            shot(app, "2-work-by-address")
            app.buttons["zoom-path-overview"].tap()
            if !iPad { XCTAssertTrue(board.shows(within: 15), "not back home") }
        }

        // Answered in the row (Arugula #1256): the stand-in agent's permission
        // prompt (after a peek at it) and its question, then the board's ask.
        // The script checks that the agent went on with the answers.
        // With the agent on the board's card, its permission was answered at
        // the Work level (Arugula #1307): home answers its question.
        if env["ARUGULA_AGENT"] == "1" {
            if env["ARUGULA_CARD_AGENT"] != "1" {
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
            }
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
            XCTAssertTrue(app.descendants(matching: .any)["board-lead"].shows(within: 20), "telling the board didn't open its thread")
            let told = "told-the-board-\(Int.random(in: 1000...9999))"
            let draft = app.textFields["chat-draft"]
            XCTAssertTrue(draft.shows(within: 15))
            focus(app, draft)
            draft.typeText(told)
            send(app)
            XCTAssertTrue(app.staticTexts[told].shows(within: 20), "the board's thread didn't take it")
            shot(app, "4-told")
            if !iPad { goBack(app) }

            XCTAssertTrue(new.shows(within: 15), "not back home")
            // Home slides back in: a tap before it settles opens nothing, so
            // wait for New to take taps, and tap again if its menu didn't come.
            for _ in 0..<3 where !app.buttons["new-idea"].exists {
                _ = new.waitFor(hittable: true, timeout: 5)
                new.tap()
                _ = app.buttons["new-idea"].shows(within: 5)
            }
            XCTAssertTrue(app.buttons["new-idea"].exists, "no New idea")
            app.buttons["new-idea"].tap()
            // Which kind of field it is is known once the sheet is up.
            XCTAssertTrue(app.descendants(matching: .any)["idea-words"].shows(within: 10), "no words for the idea")
            let words = app.textViews["idea-words"].exists ? app.textViews["idea-words"] : app.textFields["idea-words"]
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
        send(app)
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
        // Done again if the sheet stayed (a tap as the keyboard settles can miss).
        for _ in 0..<3 where app.buttons["terminal-done"].exists {
            clearAlerts()
            app.buttons["terminal-done"].tap()
            let goneBy = Date().addingTimeInterval(4)
            while app.buttons["terminal-done"].exists && Date() < goneBy { usleep(300_000) }
        }
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
    func waitFor(hittable: Bool, timeout: TimeInterval) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            if exists && isHittable == hittable { return true }
            usleep(1_000_000)
        }
        return false
    }

    // Once a second: each look snapshots the app's accessibility tree, and
    // looking faster on an iPad, with a live terminal beside home's list,
    // floods the app's log until XCTest's log hook takes it down.

    /// As `waitForExistence`, answering iOS's notification alert (#1260)
    /// while it waits: it comes over the app when the app asks, whenever
    /// that is, and takes the next tap.
    func shows(within timeout: TimeInterval) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        repeat {
            if springboard.alerts.buttons["Allow"].exists { springboard.alerts.buttons["Allow"].tap() }
            if exists { return true }
            usleep(1_000_000)
        } while Date() < end
        return exists
    }

    func waitFor(enabled: Bool, timeout: TimeInterval) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            if exists && isEnabled == enabled { return true }
            usleep(1_000_000)
        }
        return false
    }
}
