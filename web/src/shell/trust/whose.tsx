// Whose a machine in a team is (Arugula #1182): its owner's, also used for
// the team (the default: the team watches its projects' sessions, and the
// owner's own stay private), or the team's, a shared or work machine (its
// sessions go in the team's project, and the team uses it by its roles).
// Picked when a machine is approved into a team, and changed on Machines.

/** The two answers, as the picker shows them. */
export const WHOSE_MINE = "Mine, also used for the team";
export const WHOSE_TEAMS = "The team's: a shared or work machine";

/** The picker: `teamBox` is whether it's the team's. */
export function WhosePicker({
  teamBox,
  onPick,
  disabled = false,
  hook,
}: {
  teamBox: boolean;
  onPick: (teamBox: boolean) => void;
  disabled?: boolean;
  /** The `data-` hook it carries: `data-join-whose` or `data-whose`. */
  hook: "join" | "machine";
}) {
  const attrs = hook === "join" ? { "data-join-whose": "" } : { "data-whose": "" };
  return (
    <label class="dim control-move-label">
      Whose is it?{" "}
      <select
        class="field"
        {...attrs}
        value={teamBox ? "team" : "mine"}
        disabled={disabled}
        onChange={(e) => onPick((e.target as HTMLSelectElement).value === "team")}
      >
        <option value="mine">{WHOSE_MINE}</option>
        <option value="team">{WHOSE_TEAMS}</option>
      </select>
    </label>
  );
}
