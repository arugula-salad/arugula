//! Enrolling a hosted workspace's box into its project, as the platform
//! asks the hub for it ("Control enrolls hosted machines under a project
//! delegation", aru-095; M178, #1239).
//!
//! - As it makes a box, the platform asks the hub to record a one-time
//!   enrollment into the project for the workspace
//!   ([`HubPort::hosted_enrollment`], an [`EnrollAsk`]): by the box's
//!   sandbox id once it's made, or by a ticket the hub makes for the box's
//!   env before it's made ([`super::EnrollBy`]).
//! - The box enrolls with the hub itself (M180). The platform learns it did
//!   from [`HubPort::hosted_enrolled`], and tells the hub when a workspace's
//!   box is gone ([`HubPort::hosted_gone`]), so its machine leaves the
//!   project.
//!
//! The hub may also say an enrollment was **taken**: someone tried the box's
//! enrollment again, so the machine that used it was removed. The platform
//! deletes that box (fail closed).
//!
//! [`HubPort::hosted_enrollment`]: crate::HubPort::hosted_enrollment
//! [`HubPort::hosted_enrolled`]: crate::HubPort::hosted_enrolled
//! [`HubPort::hosted_gone`]: crate::HubPort::hosted_gone

pub use arugula_control_wire::hosted::{EnrollAsk, EnrollGiven, Enrolled, SANDBOX_MS, TICKET_MS};

/// What a person is told when the project has no delegation.
pub const NO_DELEGATION: &str = "This project has no delegation for hosted machines: an owner signs one from a device \
     (Hosted: the operator can read), then rebuild the workspace.";

/// What a person is told when the hub refused the box's enrollment as
/// taken: the box was deleted, as nothing else may enroll in its place.
pub const TAKEN: &str = "The box's enrollment was taken, so it was deleted. Rebuild the workspace to make another.";

/// What a person is told when the box didn't enroll in time.
pub const LATE: &str =
    "The box didn't enroll with control in time, so it was deleted. Rebuild the workspace to make another.";
