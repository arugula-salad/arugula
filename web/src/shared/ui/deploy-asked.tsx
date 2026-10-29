// When approval was asked for and how long it has waited (#333): "asked 3h
// ago · waiting 3h" on a deploy waiting for an owner, in the inbox's card
// and Control's prompt. Once it isn't waiting, only when it was asked.

import { waitedText } from "../deploys";
import { Time } from "./time";

export function DeployAsked({ at, waiting }: { at: number | null | undefined; waiting: boolean }) {
  if (!at) return null;
  return (
    <span class="deploy-asked" data-deploy-asked>
      asked <Time at={at} />
      {waiting ? ` · waiting ${waitedText(at)}` : ""}
    </span>
  );
}
