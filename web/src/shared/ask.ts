// Questions and forms from agents (M6c), as the wire has them: Claude's
// AskUserQuestion, any other form with its JSON Schema, a sign-in link, or
// a permission. The kit's NeedRow (./kit/need-row.tsx) draws and answers
// one, in an agent block, beside a terminal running Claude Code (through
// its hook), on Swarm's rail and in Needs you. Answers are the form's
// fields: `question_<n>` (a label, or a list of them) and
// `question_<n>_custom` for questions.

export interface AskOption {
  label: string;
  description?: string;
  preview?: string;
}

export interface Question {
  question: string;
  header?: string;
  multiSelect?: boolean;
  options: AskOption[];
}

export interface Ask {
  id: string;
  kind: "questions" | "form" | "url" | "permission";
  message: string;
  questions?: Question[];
  schema?: Schema;
  url?: string;
  accepted?: boolean;
  tool_call_id?: string;
  /** `agent`, `hook`, or what raised it on a block (M35: `hud`). */
  source: string;
  /** Who asks, when that isn't the terminal's own agent (M35: "hud"). */
  agent?: string;
  at_ms: number;
  /** `permission` (M29): the tool Claude Code asks to use, its input and
   * its own "always allow" suggestions. */
  tool?: string;
  input?: Record<string, unknown>;
  suggestions?: Suggestion[];
  session?: string;
}

/** One of Claude Code's "always allow" suggestions, as its hook gives it. */
export interface Suggestion {
  type: string;
  rules?: { toolName: string; ruleContent?: string }[];
  directories?: string[];
  mode?: string;
  destination?: string;
}

/** Who answered a card, and how (M29). */
export interface Answered {
  id: string;
  how: string;
  who: string;
  name: string;
  at_ms: number;
  headline: string;
}

/** The JSON Schema subset forms use (MCP's, Codex's, Claude's). */
export interface Schema {
  type?: string;
  title?: string;
  description?: string;
  properties?: Record<string, Schema>;
  required?: string[];
  enum?: (string | number)[];
  enumNames?: string[];
  oneOf?: { const: string | number; title?: string; description?: string }[];
  anyOf?: { const: string | number; title?: string; description?: string }[];
  items?: Schema;
  minimum?: number;
  maximum?: number;
  maxLength?: number;
  /** `markdown` (M36: a forge draft's text): a multi-line box. */
  format?: string;
  default?: unknown;
}

export interface AskActions {
  answer(content: Record<string, unknown>): void;
  decline(): void;
  /** Hook questions: let Claude Code show its own picker. */
  terminal?(): void;
  /** Agent blocks: stop the turn (the agent withdraws the question). */
  stop?(): void;
}

/** What a list or a notification says about it. */
export function headline(a: Ask): string {
  return a.questions?.[0]?.question ?? a.message;
}
