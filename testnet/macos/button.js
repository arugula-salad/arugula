// Press the button titled LABEL in the app's window, as a person's click
// (testnet/macos/stale-daemon.sh's illogical claims, #661):
//   osascript -l JavaScript button.js LABEL
// The page's buttons are AXButtons inside the window's web area. Throws,
// listing the buttons there, when LABEL isn't one of them.
function run(argv) {
  const want = argv[0];
  const app = Application('System Events').processes.byName('arugula-desktop');
  const seen = [];
  for (const w of app.windows()) {
    for (const e of w.entireContents()) {
      let role, names;
      try {
        role = e.role();
        names = [e.name(), e.description(), e.title()];
      } catch (_) {
        continue;
      }
      if (role !== 'AXButton') continue;
      if (names.includes(want)) {
        e.actions.byName('AXPress').perform();
        return 'pressed ' + want;
      }
      seen.push(names.filter(Boolean).join('/'));
    }
  }
  throw new Error('no button "' + want + '"; there are: ' + seen.join(', '));
}
