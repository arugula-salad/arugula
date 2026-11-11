// Whether a host in the fleet is the machine a page's own client is attached
// to. The fleet's clients are summary connections of their own, so comparing
// clients never matches: the address each one was made with does.

/** `clientBase` is `client.base`; `hostBase` makes the host's (`directory.base(name)`, which throws for a host that is gone). */
export function isThisMachine(clientBase: string, hostBase: () => string): boolean {
  try {
    return clientBase === hostBase();
  } catch {
    return false;
  }
}
