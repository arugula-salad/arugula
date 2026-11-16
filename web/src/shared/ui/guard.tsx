// What draws in place of something that threw while drawing (Arugula #615).
// Without one, a page that throws stays on whatever it last drew and says
// nothing: 0.66.12's Docs page sat on "Reading this machine's Docs…" on
// every older daemon. Each page sits in a PageGuard, on control's page, the
// daemon's and the app's, so only the failed page is replaced; the rail
// and the rest go on working.

import { Component, type ComponentChildren } from "preact";
import { countError } from "../telemetry";
import { BlockError } from "./block-states";

/** Draws its children, or `failed` once they throw while drawing; `retry`
 * draws them again from scratch. A new `at` (another page, another
 * address) tries again on its own. */
export class Guard extends Component<
  { children: ComponentChildren; failed: (retry: () => void) => ComponentChildren; onError?: (e: unknown) => void; at?: string },
  { bad: boolean; at?: string }
> {
  state = { bad: false, at: this.props.at };
  static getDerivedStateFromProps(props: { at?: string }, state: { at?: string }) {
    return props.at !== state.at ? { bad: false, at: props.at } : null;
  }
  componentDidCatch(e: unknown) {
    this.props.onError?.(e);
    this.setState({ bad: true });
  }
  retry = () => this.setState({ bad: false });
  render() {
    return this.state.bad ? this.props.failed(this.retry) : this.props.children;
  }
}

/** Said of a page that threw: to the console with its error, and counted
 * among the web errors if the person said yes to usage counts (#989). */
function report(e: unknown) {
  console.error("A page failed to draw", e);
  countError();
}

/** A page, or its failure in its place: says so, with Try again. `frame`
 * draws the note where the page would have been, for a page that draws its
 * own frame (an overlay, a page with its own rail). */
export function PageGuard({ at, frame, children }: { at?: string; frame?: (note: ComponentChildren) => ComponentChildren; children: ComponentChildren }) {
  const failed = (retry: () => void) => {
    const note = (
      <BlockError title="Can't show this page" detail="Something in it went wrong while drawing it. The rest of Arugula still works." retry={retry} data-page-failed />
    );
    return frame ? frame(note) : note;
  };
  return (
    <Guard at={at} failed={failed} onError={report}>
      {children}
    </Guard>
  );
}
