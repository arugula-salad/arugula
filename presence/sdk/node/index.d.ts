export interface RoomTokenOptions {
  /** Your app's id on the relay. */
  app: string;
  /** Your app's secret (keep it on the server). */
  secret: string;
  /** The room, 1–128 of A–Z a–z 0–9 _ . : - */
  room: string;
  /** Who: your id for them (1–128 of A–Z a–z 0–9 _ . : @ -) and what to call them (1–80 characters). */
  user: { id: string; name: string };
  /** A starting character and colour (each 0–255). */
  look?: { character: number; colour: number };
  /** Seconds it lives, 1–86400 (default 3600). */
  ttl?: number;
  /** Unix seconds to count from (tests); default now. */
  now?: number;
}

/** A room token (HS256 JWT) for `user` in `room`. Throws on bad input. */
export function createRoomToken(o: RoomTokenOptions): string;
