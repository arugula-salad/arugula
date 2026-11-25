//! The checks of the whole daemon (#232): what needs every feature crate in
//! it, such as the tool list an agent sees, which is the base's table with
//! the crates' tools put in. The checks are the base's (`whole` modules,
//! cargo feature `whole-tests`); they run here, after [`register`].

use arugulad_base::whole;
use arugulad_mcp::tools::whole as tools;

use super::register;

#[test]
fn every_type_the_daemon_makes_has_a_kind() {
    register();
    whole::every_type_the_daemon_makes_has_a_kind();
}

#[test]
fn the_tool_list_and_each_schema_are_unchanged() {
    register();
    tools::the_tool_list_and_each_schema_are_unchanged();
}

#[test]
fn every_description_and_the_instructions_fit_the_cut() {
    register();
    tools::every_description_and_the_instructions_fit_the_cut();
}

#[test]
fn annotations_are_honest() {
    register();
    tools::annotations_are_honest();
}

#[test]
fn unlisted_tools_are_not_listed_without_labs_but_are_there() {
    register();
    tools::unlisted_tools_are_not_listed_without_labs_but_are_there();
}

#[test]
fn permissions_snippet_is_every_tool() {
    register();
    tools::permissions_snippet_is_every_tool();
}

#[test]
fn the_card_lists_what_this_machine_offers() {
    register();
    tools::the_card_lists_what_this_machine_offers();
}

#[test]
fn doc_is_listed_with_no_flag() {
    register();
    tools::doc_is_listed_with_no_flag();
}

#[test]
fn an_output_schema_is_an_object_with_a_summary() {
    register();
    tools::an_output_schema_is_an_object_with_a_summary();
}

#[test]
fn every_tool_and_kind_is_placed() {
    register();
    tools::every_tool_and_kind_is_placed();
}

#[test]
fn a_guests_agent_is_refused_every_owners_tool() {
    register();
    tools::a_guests_agent_is_refused_every_owners_tool();
}

#[test]
fn old_argument_and_kind_names_are_read_as_the_new() {
    register();
    tools::old_argument_and_kind_names_are_read_as_the_new();
}

/// The chief of staff on a machine (Arugula #1337), rendered as its first
/// prompt, names only tools an agent block has, and the ones it works with:
/// `ask_approval` for a start or a close since Arugula #1417.
#[test]
fn the_chief_of_staff_on_a_machine_names_an_agent_blocks_tools() {
    register();
    let text = arugulad_control_tools::chief_of_staff_on_a_machine(Some("brief me"));
    assert!(text.ends_with("\n\nbrief me"), "{text}");
    let named = tools::a_recipe_names_only_a_blocks_tools(&text);
    let works = [
        "watch",
        "post_thread",
        "read_thread",
        "forge_read",
        "issue_write",
        "board_read",
        "board_write",
        "link",
        "doc",
        "ask_approval",
    ];
    for tool in works {
        assert!(named.iter().any(|n| n == tool), "it names {tool}: {named:?}");
    }
}

/// A board's lead (Arugula #1440), rendered as its first prompt, names only
/// tools and kinds an agent block has, and the ones it runs a board with:
/// it starts visible issue agents with `start_agent`, never sub-agents.
#[test]
fn the_board_lead_names_an_agent_blocks_tools() {
    register();
    let text = arugulad_control_tools::board_lead_on_a_machine(Some("Your board is brd_1."));
    assert!(text.ends_with("\n\nYour board is brd_1."), "{text}");
    let named = tools::a_recipe_names_only_a_blocks_tools(&text);
    let works = [
        "board_read",
        "board_write",
        "start_agent",
        "post_thread",
        "read_thread",
        "forge_read",
        "wait",
        "close",
        "read_output",
        "doc",
        "list",
        "run",
    ];
    for tool in works {
        assert!(named.iter().any(|n| n == tool), "it names {tool}: {named:?}");
    }
}

/// What an agent started on an issue is told first (Arugula #1440): it
/// leads the issue's thread, whose posts are its person's or its lead's
/// word. It names only tools and kinds an agent block has.
#[test]
fn the_issue_agent_names_an_agent_blocks_tools() {
    register();
    let text = arugulad_control_tools::issue_agent_on_a_machine(Some("Your issue is #7.\n\nFix it"));
    assert!(text.ends_with("Your issue is #7.\n\nFix it"), "{text}");
    assert!(text.contains("You lead that issue's thread"), "{text}");
    let named = tools::a_recipe_names_only_a_blocks_tools(&text);
    for tool in ["forge_read", "post_thread", "board_write"] {
        assert!(named.iter().any(|n| n == tool), "it names {tool}: {named:?}");
    }
}

/// What a start puts first (Arugula #1440): an agent on an issue is told
/// its issue and board, then the issue agent's instructions; a board's
/// lead, its board, then the lead's. The ladder's starts and `start_agent`
/// use these.
#[test]
fn a_start_puts_its_rungs_recipe_first() {
    use arugulad_base::mcp::registry;
    register();
    let p = registry::issue_agent_prompt("#7", Some("brd_1"), " Fix #7 ");
    assert!(p.starts_with("You are the agent on one issue"), "{p}");
    assert!(p.ends_with("\n\nYour issue is #7, on the board brd_1.\n\nFix #7"), "{p}");
    let p = registry::issue_agent_prompt("#7", None, "Fix #7");
    assert!(p.ends_with("\n\nYour issue is #7.\n\nFix #7"), "{p}");
    let p = registry::board_lead_prompt(Some("brd_1"), "Lead it").unwrap();
    assert!(p.starts_with("You are the lead of one board"), "{p}");
    assert!(p.ends_with("\n\nYour board is brd_1.\n\nLead it"), "{p}");
    assert!(registry::own_recipe("board-lead") && registry::own_recipe(" issue-agent"));
    assert!(!registry::own_recipe("arugula-hand"));
}
