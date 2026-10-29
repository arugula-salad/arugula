// @arugula/presence-elements: arugula.stream in a page (../../sdk/API.md).
// connect() for code, and the <arugula-room>, <arugula-camera>,
// <arugula-people> and <arugula-avatar> elements, defined on import. The
// same module, bundled, is https://arugula.stream/sdk/v0/presence.js.

import { defineElements } from "./elements.ts";

export { connect, Room, RELAY, httpOrigin, subOf, styled, type ConnectOptions, type RoomEvents } from "./room.ts";
export { mountPeople, mountAvatar, type PeopleOptions, type AvatarOptions } from "./view.ts";
export { mountCamera, type CameraOptions } from "./controls.ts";
export { ArugulaRoom, ArugulaPeople, ArugulaCamera, ArugulaAvatar, defineElements } from "./elements.ts";
export { CSS } from "./styles.ts";
export type { Member } from "@arugula/presence";
export type { Look } from "@arugula/presence-core/avatar/rig";

defineElements();
