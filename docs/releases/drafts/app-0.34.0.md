# Arugula app 0.34.0

The desktop app for Arugula 0.71. It installs the daemon on a machine that has none, and signing it in is all it takes: there's no device to approve.

- **Sign-in says what happens now** (#2652). The app's sign-in page no longer says signing in makes the app one of your devices: Allow signs it in, and the machine is reached wherever you sign in. A machine that asks to join again says to add it in Arugula cloud with its code, not to approve the code on a device.

- **The camera prompt** (#2664). macOS asks for the camera only to scan a QR code, without the old mention of approving a device.

It carries arugulad 0.71.0.
