# @arugula/presence-react

arugula.stream for React: thin components over `@arugula/presence-elements`
(`connect()`, `mount`, `avatar` and the camera controls), React as a peer
dependency. The surface: `../sdk/API.md`.

```jsx
<PresenceRoom room="design-team" token={getToken}>
  <CameraControls />
  <People />
  <Avatar user={id} size="head" />
</PresenceRoom>
const { members, wave, camera } = usePresence();
```

- `PresenceRoom`: `room`, `token` (a room token your server signed, or an async function returning a fresh one), `relay`, `mode`, `look`.
- `People`: `size` (`bust`|`head`), `self` (`first`|`hide`). `Avatar`: `user`, `size`; 36×36 unless styled. `CameraControls`: `controls` (`full`|`button`).
- `usePresence()`: `{ room, members, live, error, me, waves, wave, answer, setLook, camera: { on, start, stop } }`.

`pnpm typecheck` checks it against React's types.
