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
