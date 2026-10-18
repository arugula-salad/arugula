# Arugula app 0.29.0

- **Huddles are removed** (#999): the app no longer runs its own voice
  call on Linux, and drops WebRTC, Opus and ALSA from its build. On macOS
  it still asks for the microphone, for a lent device's `record_audio`.

It carries arugulad 0.44.0.
