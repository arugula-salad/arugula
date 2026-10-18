# Arugula app 0.28.0

- **Crash reports, opt-in** (#989): with usage reports turned on for the
  machine, a crash in the app is saved and sent with the daemon's next
  report. A report holds only where the crash happened in our code, never
  its message. The app asks once, with the same card as the web client.
  [telemetry.md](../telemetry.md) has the details.

It carries arugulad 0.43.0.
