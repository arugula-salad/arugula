# Arugula app 0.27.5

- **Huddles can use the microphone again on macOS** (#177). 0.27.4, the first
  notarized app, was signed without the microphone entitlement, so macOS
  refused it the mic. The release now checks the entitlement is there.
- **The daemon's launch agent runs from wherever the app is** (#177). It
  used to need `/Applications/Arugula.app`; now `~/Applications` works too,
  and the agent finds the app again after it moves.

It carries arugulad 0.38.1.
