// Verifying someone you share with (#763): their account and the root this
// device pinned for them, as eight words to compare or a QR code to scan.
// Both sides work it out from what they hold: the person from their own
// pinned root, the sharer from the one they pinned. Control isn't asked.

import { sha256 } from "./noise.ts";

// 256 words, one per byte: short, concrete, none alike to the ear.
const WORDS = [
  "acid", "acorn", "actor", "adult", "agent", "alarm", "album", "alien", "alley", "amber", "angle", "ankle", "apple", "apron", "arena", "armor",
  "arrow", "atlas", "attic", "audio", "award", "bacon", "badge", "bagel", "baker", "banjo", "barn", "basil", "basin", "beach", "beard", "berry",
  "bison", "blade", "blank", "blaze", "bloom", "board", "bonus", "boost", "boot", "brain", "brass", "bread", "brick", "bride", "brook", "broom",
  "brush", "bucket", "bugle", "cabin", "cable", "cactus", "camel", "canal", "candy", "canoe", "canyon", "cargo", "carpet", "cedar", "chalk", "chart",
  "cherry", "chess", "chief", "cider", "circus", "civic", "clamp", "cliff", "clock", "cloud", "clover", "coach", "cobra", "cocoa", "comet", "coral",
  "cotton", "couch", "crane", "crater", "crayon", "cream", "creek", "crown", "cube", "daisy", "dancer", "delta", "denim", "desert", "dingo", "disco",
  "dolphin", "donkey", "dragon", "drum", "eagle", "easel", "echo", "elbow", "elder", "ember", "engine", "falcon", "fern", "ferry", "fiddle", "flame",
  "flute", "forest", "fossil", "fox", "frost", "galaxy", "garden", "garlic", "gecko", "giant", "ginger", "globe", "gravel", "guitar", "hammer", "harbor",
  "harp", "hazel", "helmet", "heron", "hippo", "honey", "hotel", "husky", "igloo", "index", "iris", "island", "ivory", "jacket", "jaguar", "jelly",
  "jungle", "kayak", "kettle", "kiwi", "koala", "ladder", "lagoon", "lamp", "laser", "lemon", "lever", "lilac", "lily", "lizard", "locket", "lotus",
  "magnet", "mango", "maple", "marble", "meadow", "melon", "mitten", "monkey", "moose", "motor", "muffin", "mural", "napkin", "nectar", "needle", "nickel",
  "noodle", "oasis", "ocean", "olive", "onion", "opal", "orbit", "orchid", "otter", "oven", "owl", "paddle", "panda", "parrot", "pasta", "peach",
  "peanut", "pebble", "pepper", "piano", "pickle", "pilot", "pirate", "planet", "plum", "pony", "poppy", "prism", "pumpkin", "puzzle", "quartz", "quilt",
  "rabbit", "radar", "radish", "raven", "rhino", "ribbon", "river", "robot", "rocket", "rodeo", "saddle", "salmon", "satin", "scarf", "shovel", "silver",
  "skate", "sketch", "sloth", "snail", "sofa", "spider", "spoon", "squid", "stable", "statue", "stereo", "summit", "sunset", "tablet", "tango", "teapot",
  "tiger", "toast", "tomato", "topaz", "torch", "tulip", "tuna", "turtle", "unicorn", "valley", "velvet", "violin", "waffle", "walnut", "walrus", "wagon",];

const enc = new TextEncoder();

/** Eight words (64 bits of SHA-256) for an account and its root. */
export async function verifyWords(account: string, root: string): Promise<string[]> {
  // Frozen: both sides must make the same words.
  const h = await sha256(enc.encode(`arugula verify v1\naccount ${account}\nroot ${root}\n`));
  return Array.from(h.subarray(0, 8), (b) => WORDS[b]);
}

const PREFIX = "arugula-verify:";

/** What a person's QR code holds: their account and root. */
export const verifyCode = (account: string, root: string) => `${PREFIX}${account}.${root}`;

/** A scanned verify code, or null for anything else. */
export function readVerifyCode(text: string): { account: string; root: string } | null {
  if (!text.startsWith(PREFIX)) return null;
  const rest = text.slice(PREFIX.length);
  const dot = rest.lastIndexOf(".");
  if (dot <= 0 || dot === rest.length - 1) return null;
  return { account: rest.slice(0, dot), root: rest.slice(dot + 1) };
}
