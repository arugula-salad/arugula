// Whether the chat drawn here is on screen (#123). A page mounts chat as
// a block that stays mounted when its tab isn't shown, and a room nobody
// sees is neither read nor "open" (control pushes for it). The platform
// page and the drawer are always shown, the default.

import { createContext } from "preact";
import { useContext } from "preact/hooks";

export const ChatShown = createContext(true);

export const useChatShown = () => useContext(ChatShown);
