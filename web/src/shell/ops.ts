// The daemon's operations by name (`OPS` in proto.gen.ts, made from
// `arugula_proto::op`): a call site names the operation, and the route comes
// from the table the daemon's own router is checked against, instead of the
// path being spelled again here. Routes that aren't operations stay spelled
// where they are used.

import { OPS, type OpName } from "../shared/proto.gen";

export { OPS };
export type { OpName };

/** An operation's route, with each `{…}` filled from `params` (escaped as a
 * path segment). A hole `params` doesn't fill is a bug at the call site. */
export function opPath(name: OpName, params: Record<string, string | number> = {}): string {
  return OPS[name].path.replace(/\{(\w+)\}/g, (_, hole: string) => {
    const v = params[hole];
    if (v === undefined) throw new Error(`${name}: no value for {${hole}}`);
    return encodeURIComponent(String(v));
  });
}
