use proptest::prelude::*;

use crate::{Edge, Effect, Intent, Mux, Node, Preset};

fn mux_with_session() -> Mux {
    let mut m = Mux::new();
    m.apply(Intent::NewSession { name: None, from_pane: None }).unwrap();
    m
}

#[test]
fn new_session_spawns_one_pane_in_one_tab() {
    let mut m = Mux::new();
    let fx = m.apply(Intent::NewSession { name: Some("work".into()), from_pane: None }).unwrap();
    assert_eq!(fx, vec![Effect::Spawn { pane: 1, cwd_from: None, cwd: None }]);
    assert_eq!(m.sessions[0].name, "work");
    assert_eq!(m.tab(1).unwrap().root, Node::pane(1));
}

#[test]
fn split_spawns_with_the_source_pane_cwd() {
    let mut m = mux_with_session();
    let fx = m.apply(Intent::Split { pane: 1, edge: Edge::Right, local: false, cwd: None }).unwrap();
    assert_eq!(fx, vec![Effect::Spawn { pane: 2, cwd_from: Some(1), cwd: None }]);
    assert_eq!(m.tab(1).unwrap().root.panes(), vec![1, 2]);
}

#[test]
fn closing_the_last_pane_closes_tab_and_session() {
    let mut m = mux_with_session();
    m.apply(Intent::NewTab { session: 1, from_pane: None, cwd: None }).unwrap();
    m.apply(Intent::ClosePane { pane: 2 }).unwrap();
    assert_eq!(m.sessions[0].tabs, vec![1]);
    let fx = m.apply(Intent::ClosePane { pane: 1 }).unwrap();
    assert_eq!(fx, vec![Effect::Kill { pane: 1 }]);
    assert!(m.sessions.is_empty() && m.tabs.is_empty());
}

#[test]
fn move_pane_across_tabs_and_break_it_out_again() {
    let mut m = mux_with_session();
    m.apply(Intent::NewTab { session: 1, from_pane: None, cwd: None }).unwrap();
    // Pane 2 (tab 2) docks left of pane 1; tab 2 is now empty and goes away.
    m.apply(Intent::MovePane { pane: 2, target: 1, edge: Edge::Left }).unwrap();
    assert_eq!(m.sessions[0].tabs, vec![1]);
    assert_eq!(m.tab(1).unwrap().root.panes(), vec![2, 1]);
    m.apply(Intent::BreakPane { pane: 2, session: 1, index: Some(0) }).unwrap();
    assert_eq!(m.sessions[0].tabs.len(), 2);
    let first = m.sessions[0].tabs[0];
    assert_eq!(m.tab(first).unwrap().root, Node::pane(2));
}

#[test]
fn swap_with_center() {
    let mut m = mux_with_session();
    m.apply(Intent::Split { pane: 1, edge: Edge::Right, local: false, cwd: None }).unwrap();
    m.apply(Intent::NewTab { session: 1, from_pane: None, cwd: None }).unwrap();
    m.apply(Intent::MovePane { pane: 3, target: 1, edge: Edge::Center }).unwrap();
    assert_eq!(m.tab(1).unwrap().root.panes(), vec![3, 2]);
    assert_eq!(m.tab(2).unwrap().root.panes(), vec![1]);
}

#[test]
fn dock_tab_merges_its_whole_layout() {
    let mut m = mux_with_session();
    m.apply(Intent::NewTab { session: 1, from_pane: None, cwd: None }).unwrap();
    m.apply(Intent::Split { pane: 2, edge: Edge::Bottom, local: false, cwd: None }).unwrap();
    m.apply(Intent::DockTab { tab: 2, target: 1, edge: Edge::Right }).unwrap();
    assert_eq!(m.sessions[0].tabs, vec![1]);
    assert_eq!(m.tab(1).unwrap().root.panes(), vec![1, 2, 3]);
    assert!(m.apply(Intent::DockTab { tab: 1, target: 1, edge: Edge::Left }).is_err());
}

#[test]
fn resize_split_renormalizes() {
    let mut m = mux_with_session();
    m.apply(Intent::Split { pane: 1, edge: Edge::Right, local: false, cwd: None }).unwrap();
    let split = match &m.tab(1).unwrap().root {
        Node::Split { id, .. } => *id,
        n => panic!("{n:?}"),
    };
    m.apply(Intent::ResizeSplit { split, weights: vec![3.0, 1.0] }).unwrap();
    let l = m.layout(1).unwrap();
    assert_eq!(l.panes[0].1.cols, 59);
    assert_eq!(l.panes[1].1.cols, 20);
    assert!(m.apply(Intent::ResizeSplit { split, weights: vec![1.0] }).is_err());
}

#[test]
fn view_ownership_and_zoom() {
    let mut m = mux_with_session();
    m.apply(Intent::Split { pane: 1, edge: Edge::Right, local: false, cwd: None }).unwrap();
    // First viewer sizes the tab even without claiming.
    assert!(m.view(7, 1, 120, 40, None, false).unwrap());
    // Another client's unclaimed view doesn't change it...
    assert!(!m.view(8, 1, 50, 30, Some(2), false).unwrap());
    // ...until it claims (a phone showing pane 2 alone).
    assert!(m.view(8, 1, 50, 30, Some(2), true).unwrap());
    let rects = m.pane_rects();
    assert_eq!(rects.get(&2).map(|r| (r.cols, r.rows)), Some((50, 30)));
    assert!(!rects.contains_key(&1), "hidden by zoom: keeps its size");
    // The phone leaves; the desktop's next view takes over without a claim.
    assert!(m.release(8));
    assert!(m.view(7, 1, 120, 40, None, false).unwrap());
    assert_eq!(m.pane_rects().len(), 2);
}

/// #333: two editors typing in one pane in turn, a second or so apart,
/// used to resize it at every handover. The size now stays with whoever
/// has it until they've left the keyboard for [`SIZE_HOLD`].
#[test]
fn editors_typing_in_turn_dont_fight_over_the_size() {
    use std::time::{Duration, Instant};

    use crate::{Claim, SIZE_HOLD, SizeHold};

    let mut m = mux_with_session();
    let mut hold = SizeHold::default();
    let t0 = Instant::now();
    let at = |s: f64| t0 + Duration::from_secs_f64(s);
    let (a, b) = (7, 8);
    let size = |m: &Mux| (m.tab(1).unwrap().cols, m.tab(1).unwrap().rows);
    // A opens the tab; B opens it too, later, and the size is B's.
    assert!(hold.view(&mut m, a, 1, (120, 40), None, Claim::Yes, at(0.0)).unwrap());
    assert!(hold.view(&mut m, b, 1, (90, 30), None, Claim::Yes, at(10.0)).unwrap());
    // Long after, A types: B left the keyboard, so A takes it.
    assert!(hold.view(&mut m, a, 1, (120, 40), None, Claim::Typed, at(20.0)).unwrap());
    hold.typed(&m, a, 1, at(20.0));
    // Now they take turns, 1.5 s apart, for a minute: nothing resizes.
    let mut resizes = 0;
    let mut t = 20.0;
    for turn in 0..40 {
        t += 1.5;
        let who = if turn % 2 == 0 { b } else { a };
        let mine = if who == a { (120, 40) } else { (90, 30) };
        let before = size(&m);
        hold.view(&mut m, who, 1, mine, None, Claim::Typed, at(t)).unwrap();
        hold.typed(&m, who, 1, at(t));
        resizes += usize::from(size(&m) != before);
    }
    assert_eq!(resizes, 0, "typing in turn resized the pane");
    assert_eq!(m.tab(1).unwrap().owner, Some(a));
    // A stops; B's keys take the size once A has been idle long enough.
    let last = t;
    assert!(!hold.view(&mut m, b, 1, (90, 30), None, Claim::Typed, at(last + 1.0)).unwrap());
    let idle = last + SIZE_HOLD.as_secs_f64() + 0.01;
    assert!(hold.view(&mut m, b, 1, (90, 30), None, Claim::Typed, at(idle)).unwrap());
    assert_eq!((size(&m), m.tab(1).unwrap().owner), ((90, 30), Some(b)));
    // Showing the tab or "use this size" still takes it at once.
    assert!(hold.view(&mut m, a, 1, (120, 40), None, Claim::Yes, at(idle + 0.1)).unwrap());
    // A plain view by someone else changes nothing.
    assert!(!hold.view(&mut m, b, 1, (91, 30), None, Claim::No, at(idle + 60.0)).unwrap());
    // The owner leaving frees the size for the next typist at once.
    m.release(a);
    assert!(hold.view(&mut m, b, 1, (90, 30), None, Claim::Typed, at(idle + 0.2)).unwrap());
}

/// #1472: while A presents, B's claims (switching to the tab, typing, "use
/// this size") don't take the size, even from a client that still sends
/// them. A's own window still sizes the tab.
#[test]
fn a_presenters_size_holds_against_other_clients() {
    use std::time::Instant;

    use crate::{Claim, SIZE_HOLD, SizeHold};

    let mut m = mux_with_session();
    let mut hold = SizeHold::default();
    let t0 = Instant::now();
    let (a, b) = (7, 8);
    let size = |m: &Mux| (m.tab(1).unwrap().cols, m.tab(1).unwrap().rows);
    assert!(hold.view(&mut m, b, 1, (90, 30), None, Claim::Yes, t0).unwrap());
    // A presents: its size is the tab's at once.
    assert!(m.present(a, 1, true, (120, 40), None).unwrap());
    assert_eq!((size(&m), m.tab(1).unwrap().presenter), ((120, 40), Some(a)));
    // Long after A's last move, B shows the tab, types, and asks for its size.
    let later = t0 + SIZE_HOLD * 20;
    for claim in [Claim::Yes, Claim::Typed, Claim::No] {
        assert!(!hold.view(&mut m, b, 1, (90, 30), None, claim, later).unwrap(), "{claim:?}");
    }
    assert!(!m.view(b, 1, 90, 30, None, true).unwrap());
    assert_eq!((size(&m), m.tab(1).unwrap().owner), ((120, 40), Some(a)));
    // A's window resizing still sizes the tab.
    assert!(hold.view(&mut m, a, 1, (130, 44), None, Claim::No, later).unwrap());
    assert_eq!(size(&m), (130, 44));
    // A leaving ends it, and the next claim works as before.
    assert!(m.release(a));
    assert_eq!(m.tab(1).unwrap().presenter, None);
    assert!(hold.view(&mut m, b, 1, (90, 30), None, Claim::Yes, later).unwrap());
    assert_eq!((size(&m), m.tab(1).unwrap().owner), ((90, 30), Some(b)));
}

/// #1472: taking over moves the pin and the size; stopping lets claims work.
#[test]
fn taking_over_a_presentation_and_stopping_it() {
    let mut m = mux_with_session();
    let (a, b) = (7, 8);
    m.present(a, 1, true, (120, 40), None).unwrap();
    assert!(m.present(b, 1, true, (90, 30), None).unwrap());
    let t = m.tab(1).unwrap();
    assert_eq!(((t.cols, t.rows), t.owner, t.presenter), ((90, 30), Some(b), Some(b)));
    assert!(!m.view(a, 1, 120, 40, None, true).unwrap());
    assert!(m.present(b, 1, false, (0, 0), None).unwrap());
    assert!(!m.present(b, 1, false, (0, 0), None).unwrap(), "already stopped");
    assert!(m.view(a, 1, 120, 40, None, true).unwrap());
}

/// #1472: a presentation is never saved, or after a restart (client ids
/// start over) a stranger could inherit the pin.
#[test]
fn a_presenter_is_not_saved() {
    let mut m = mux_with_session();
    m.present(7, 1, true, (120, 40), None).unwrap();
    let back: Mux = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.tab(1).unwrap().presenter, None);
}

#[test]
fn layout_serializes_for_clients() {
    let mut m = mux_with_session();
    m.apply(Intent::Split { pane: 1, edge: Edge::Bottom, local: false, cwd: None }).unwrap();
    let json = serde_json::to_value(&m.tab(1).unwrap().root).unwrap();
    assert_eq!(json["type"], "split");
    assert_eq!(json["dir"], "column");
    assert_eq!(json["children"][0]["node"], serde_json::json!({"type": "pane", "pane": 1}));
    let intent: Intent = serde_json::from_str(r#"{"op":"split","pane":1,"edge":"right"}"#).unwrap();
    assert_eq!(intent, Intent::Split { pane: 1, edge: Edge::Right, local: false, cwd: None });
}

fn preset() -> impl Strategy<Value = Preset> {
    prop_oneof![Just(Preset::Columns), Just(Preset::Grid), Just(Preset::MainStack)]
}

/// An intent built from small random numbers, aimed at IDs that may or may
/// not exist (so failures are exercised too).
fn arb_intent() -> impl Strategy<Value = Intent> {
    let id = 1u32..12;
    let edge =
        prop_oneof![Just(Edge::Left), Just(Edge::Right), Just(Edge::Top), Just(Edge::Bottom), Just(Edge::Center)];
    prop_oneof![
        Just(Intent::NewSession { name: None, from_pane: None }),
        (id.clone(), id.clone()).prop_map(|(s, p)| Intent::NewTab {
            session: s % 3 + 1,
            from_pane: Some(p),
            cwd: None
        }),
        (id.clone(), edge.clone()).prop_map(|(pane, edge)| Intent::Split { pane, edge, local: false, cwd: None }),
        id.clone().prop_map(|pane| Intent::ClosePane { pane }),
        id.clone().prop_map(|tab| Intent::CloseTab { tab }),
        id.clone().prop_map(|session| Intent::CloseSession { session: session % 3 + 1 }),
        (id.clone(), id.clone(), edge.clone()).prop_map(|(pane, target, edge)| Intent::MovePane { pane, target, edge }),
        (id.clone(), id.clone(), 0usize..4).prop_map(|(pane, s, i)| Intent::BreakPane {
            pane,
            session: s % 3 + 1,
            index: Some(i)
        }),
        (id.clone(), id.clone(), edge).prop_map(|(tab, target, edge)| Intent::DockTab { tab, target, edge }),
        (id.clone(), id.clone(), 0usize..4).prop_map(|(tab, s, index)| Intent::MoveTab {
            tab,
            session: s % 3 + 1,
            index
        }),
        (id.clone(), prop::collection::vec(-1.0f64..5.0, 1..4))
            .prop_map(|(split, weights)| Intent::ResizeSplit { split, weights }),
        (id.clone(), preset(), prop::option::of(id.clone())).prop_map(|(tab, preset, main)| Intent::Arrange {
            tab,
            preset,
            main
        }),
        (id.clone(), prop::option::of(id)).prop_map(|(tab, pane)| Intent::Focus { tab, pane }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Whatever clients send, the state stays valid, effects match what
    /// panes exist, and every visible pane gets at least one cell.
    #[test]
    fn random_intents_keep_the_mux_valid(
        intents in prop::collection::vec(arb_intent(), 1..60),
        views in prop::collection::vec((1u64..4, 1u32..12, 2u16..200, 1u16..80, any::<bool>()), 0..10),
    ) {
        let mut m = Mux::new();
        let mut live = std::collections::BTreeSet::new();
        for intent in intents {
            let before = m.clone();
            match m.apply(intent.clone()) {
                Ok(effects) => {
                    for e in effects {
                        match e {
                            Effect::Spawn { pane, .. } => prop_assert!(live.insert(pane), "spawned {pane} twice"),
                            Effect::Kill { pane } => prop_assert!(live.remove(&pane), "killed unknown {pane}"),
                        }
                    }
                }
                Err(_) => {
                    // A failed intent changes nothing except possibly nothing.
                    prop_assert_eq!(&m.sessions, &before.sessions, "failed {:?} changed sessions", intent);
                }
            }
            prop_assert_eq!(m.validate(), Ok(()), "after {:?}", intent);
            let panes: std::collections::BTreeSet<_> = m.panes().into_iter().collect();
            prop_assert_eq!(&panes, &live, "panes vs effects after {:?}", intent);
        }
        for (client, tab, cols, rows, claim) in views {
            let _ = m.view(client, tab, cols, rows, None, claim);
        }
        for (tab, t) in &m.tabs {
            for (_, r) in m.layout(*tab).unwrap().panes {
                prop_assert!(r.cols >= 1 && r.rows >= 1);
                // Never outside the tab, which is never smaller than its
                // tree (tmux refuses a layout that is).
                prop_assert!(r.x + r.cols <= t.cols && r.y + r.rows <= t.rows, "{:?} outside {}x{}", r, t.cols, t.rows);
            }
        }
    }
}

#[test]
fn options_are_kept_per_scope_and_go_with_their_owner() {
    use crate::OptionScope;
    let mut m = mux_with_session();
    let set = |scope, name: &str, value: Option<&str>| Intent::SetOption {
        scope,
        name: name.into(),
        value: value.map(str::to_owned),
    };
    m.apply(set(OptionScope::Global, "@g", Some("1"))).unwrap();
    m.apply(set(OptionScope::Session(1), "@iterm2_id", Some("UUID"))).unwrap();
    m.apply(set(OptionScope::Pane(1), "@uservars", Some("x"))).unwrap();
    assert!(m.apply(set(OptionScope::Pane(9), "@uservars", Some("x"))).is_err());
    assert_eq!(m.options.get(OptionScope::Session(1)).unwrap()["@iterm2_id"], "UUID");
    m.apply(set(OptionScope::Global, "@g", None)).unwrap();
    assert!(m.options.global.is_empty());
    // Saved with the layout.
    let back: Mux = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.options, m.options);
    // A pane's options go with it; the session's with the session.
    m.apply(Intent::Split { pane: 1, edge: Edge::Right, local: false, cwd: None }).unwrap();
    m.apply(Intent::ClosePane { pane: 1 }).unwrap();
    assert!(m.options.panes.is_empty());
    m.apply(Intent::CloseSession { session: 1 }).unwrap();
    assert!(m.options.sessions.is_empty());
}

#[test]
fn a_tab_is_never_smaller_than_its_tree() {
    let mut m = mux_with_session();
    for _ in 0..4 {
        let last = *m.tab(1).unwrap().root.panes().last().unwrap();
        m.apply(Intent::Split { pane: last, edge: Edge::Right, local: false, cwd: None }).unwrap();
    }
    // Five side by side need 9 columns.
    m.view(7, 1, 4, 2, None, true).unwrap();
    let t = m.tab(1).unwrap();
    assert_eq!((t.cols, t.rows), (9, 2));
    for (_, r) in m.layout(1).unwrap().panes {
        assert!(r.cols >= 1 && r.x + r.cols <= 9);
    }
}

#[test]
fn a_split_or_tab_can_name_its_directory() {
    let mut m = mux_with_session();
    let fx = m.apply(Intent::Split { pane: 1, edge: Edge::Bottom, local: false, cwd: Some("/tmp".into()) }).unwrap();
    assert_eq!(fx, vec![Effect::Spawn { pane: 2, cwd_from: Some(1), cwd: Some("/tmp".into()) }]);
    let fx = m.apply(Intent::NewTab { session: 1, from_pane: None, cwd: Some("/srv".into()) }).unwrap();
    assert_eq!(fx, vec![Effect::Spawn { pane: 3, cwd_from: None, cwd: Some("/srv".into()) }]);
}

/// A session whose first tab holds `n` panes, split every which way, in a
/// tab big enough for any preset.
fn tab_of(n: u32) -> Mux {
    let mut m = mux_with_session();
    m.view(1, 1, 200, 60, None, true).unwrap();
    let edges = [Edge::Right, Edge::Bottom, Edge::Left, Edge::Top];
    for i in 1..n {
        m.apply(Intent::Split { pane: i, edge: edges[i as usize % 4], local: false, cwd: None }).unwrap();
    }
    m
}

#[test]
fn every_preset_keeps_the_panes_for_any_count() {
    for n in 1..=9 {
        for preset in [Preset::Columns, Preset::Grid, Preset::MainStack] {
            let mut m = tab_of(n);
            let mut before = m.tab(1).unwrap().root.panes();
            let fx = m.apply(Intent::Arrange { tab: 1, preset, main: None }).unwrap();
            assert_eq!(fx, vec![], "n={n} {preset:?}");
            assert_eq!(m.validate(), Ok(()), "n={n} {preset:?}");
            let mut after = m.tab(1).unwrap().root.panes();
            before.sort();
            after.sort();
            assert_eq!(after, before, "n={n} {preset:?}");
        }
    }
}

#[test]
fn presets_have_the_planned_shapes() {
    use crate::Dir;
    let shape = |n, preset, main| {
        let mut m = tab_of(n);
        m.apply(Intent::Arrange { tab: 1, preset, main }).unwrap();
        m.tab(1).unwrap().root.clone()
    };
    let row_len = |node: &Node| match node {
        Node::Split { dir: Dir::Row, children, .. } => children.len(),
        Node::Pane { .. } => 1,
        n => panic!("not a row: {n:?}"),
    };
    // Columns: one row of n, in reading order.
    let Node::Split { dir: Dir::Row, children, .. } = shape(4, Preset::Columns, None) else { panic!() };
    assert_eq!(children.len(), 4);
    // Grid of 7: three columns, rows of 3, 3 and 1, the last one widened.
    let Node::Split { dir: Dir::Column, children, .. } = shape(7, Preset::Grid, None) else { panic!() };
    assert_eq!(children.iter().map(|c| row_len(&c.node)).collect::<Vec<_>>(), vec![3, 3, 1]);
    // Grid of 2 is a row; of 1, the pane.
    assert_eq!(row_len(&shape(2, Preset::Grid, None)), 2);
    assert_eq!(shape(1, Preset::Grid, None), Node::pane(1));
    // MainStack of 2: columns, main first.
    assert_eq!(shape(2, Preset::MainStack, Some(2)).panes(), vec![2, 1]);
    assert_eq!(shape(1, Preset::MainStack, Some(1)), Node::pane(1));
}

#[test]
fn main_stack_makes_main_the_largest() {
    for n in 2..=9 {
        let mut m = tab_of(n);
        let main = n / 2 + 1;
        m.apply(Intent::Arrange { tab: 1, preset: Preset::MainStack, main: Some(main) }).unwrap();
        let l = m.layout(1).unwrap();
        let area = |p| l.panes.iter().find(|(q, _)| *q == p).map(|(_, r)| r.cols as u32 * r.rows as u32).unwrap();
        for p in m.tab(1).unwrap().root.panes().into_iter().filter(|p| *p != main) {
            assert!(area(main) > area(p) || n == 2 && area(main) >= area(p), "n={n}: %{main} vs %{p}");
        }
    }
}

#[test]
fn arrange_is_refused_when_it_wont_fit_or_main_is_elsewhere() {
    let mut m = tab_of(6);
    // Six stacked need 11 rows: a 40x8 tab fits the grid but not the stack.
    m.apply(Intent::Arrange { tab: 1, preset: Preset::Columns, main: None }).unwrap();
    m.view(1, 1, 40, 8, None, true).unwrap();
    let before = m.tab(1).unwrap().root.clone();
    assert!(m.apply(Intent::Arrange { tab: 1, preset: Preset::MainStack, main: Some(1) }).is_err());
    assert_eq!(m.tab(1).unwrap().root, before);
    m.apply(Intent::Arrange { tab: 1, preset: Preset::Grid, main: None }).unwrap();
    m.apply(Intent::NewTab { session: 1, from_pane: None, cwd: None }).unwrap();
    assert!(m.apply(Intent::Arrange { tab: 1, preset: Preset::MainStack, main: Some(7) }).is_err());
    assert!(m.apply(Intent::Arrange { tab: 9, preset: Preset::Grid, main: None }).is_err());
}

#[test]
fn focus_then_unfocus_gives_back_the_layout() {
    let mut m = tab_of(4);
    let before = m.layout(1).unwrap();
    m.apply(Intent::Focus { tab: 1, pane: Some(3) }).unwrap();
    let t = m.tab(1).unwrap();
    let focused = m.layout(1).unwrap();
    assert_eq!(focused.panes, vec![(3, crate::Rect { x: 0, y: 0, cols: t.cols, rows: t.rows })]);
    // Only the focused pane is sized; the others keep theirs.
    assert_eq!(m.pane_rects().keys().copied().collect::<Vec<_>>(), vec![3]);
    m.apply(Intent::Focus { tab: 1, pane: None }).unwrap();
    assert_eq!(m.layout(1).unwrap(), before);
    // A pane from elsewhere can't be focused.
    assert!(m.apply(Intent::Focus { tab: 1, pane: Some(42) }).is_err());
    // Focus is saved with the layout; an older layout loads without it.
    m.apply(Intent::Focus { tab: 1, pane: Some(2) }).unwrap();
    let back: Mux = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.tab(1).unwrap().focus, Some(2));
    let mut old = serde_json::to_value(&m).unwrap();
    old["tabs"]["1"].as_object_mut().unwrap().remove("focus");
    assert_eq!(serde_json::from_value::<Mux>(old).unwrap().tab(1).unwrap().focus, None);
}

#[test]
fn layout_changes_clear_focus() {
    let mut m = tab_of(3);
    m.apply(Intent::Focus { tab: 1, pane: Some(2) }).unwrap();
    m.apply(Intent::Split { pane: 1, edge: Edge::Right, local: false, cwd: None }).unwrap();
    assert_eq!(m.tab(1).unwrap().focus, None, "split");
    m.apply(Intent::Focus { tab: 1, pane: Some(2) }).unwrap();
    m.apply(Intent::Arrange { tab: 1, preset: Preset::Grid, main: None }).unwrap();
    assert_eq!(m.tab(1).unwrap().focus, None, "arrange");
    m.apply(Intent::Focus { tab: 1, pane: Some(2) }).unwrap();
    m.apply(Intent::ClosePane { pane: 2 }).unwrap();
    assert_eq!(m.tab(1).unwrap().focus, None, "closed");
    // Closing another pane keeps it.
    m.apply(Intent::Focus { tab: 1, pane: Some(3) }).unwrap();
    m.apply(Intent::ClosePane { pane: 1 }).unwrap();
    assert_eq!(m.tab(1).unwrap().focus, Some(3));
    // A swap across tabs takes the focused pane away.
    m.apply(Intent::NewTab { session: 1, from_pane: None, cwd: None }).unwrap();
    m.apply(Intent::MovePane { pane: 3, target: 5, edge: Edge::Center }).unwrap();
    assert_eq!(m.validate(), Ok(()));
    assert_eq!(m.tab(1).unwrap().focus, None, "swapped away");
}
