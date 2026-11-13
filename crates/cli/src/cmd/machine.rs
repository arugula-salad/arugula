//! `arugula machine rename`: a machine's name on Arugula control (#500).
//! The name is for people and for `@machine` mentions; the machine is the
//! same one. Through control, as this CLI's account (`arugula login`).

use anyhow::bail;
use serde_json::json;

use crate::control::Enrolled;
use crate::util::print_json;

#[derive(clap::Subcommand)]
pub enum MachineCmd {
    /// Give a machine a new name on Arugula cloud.
    ///
    /// The name is what the machines list, the switcher, chat and `@machine`
    /// mentions call it: 1 to 40 characters, and not another of your
    /// machines' (or your team's). The old name keeps working in mentions.
    /// You rename your own machines, and a team's owners rename the team's.
    /// Without `--machine` it renames the one called this computer's
    /// hostname, the name a machine has until it is renamed.
    Rename {
        /// The new name.
        name: String,
        /// The machine: its name or id, as `arugula hosts` lists it.
        #[arg(long)]
        machine: Option<String>,
    },
}

pub fn run(cmd: MachineCmd, json_out: bool) -> anyhow::Result<i32> {
    let Some(mut e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula cloud: `arugula login` first");
    };
    match cmd {
        MachineCmd::Rename { name, machine } => {
            let listed = e.directory()?;
            let machines: Vec<(&str, &str)> = listed.iter().map(|m| (m.id.as_str(), m.name.as_str())).collect();
            let id = machine_id(&machines, machine.as_deref(), crate::util::hostname().as_deref())?;
            let done = e.api("POST", &format!("/api/daemons/{id}/name"), Some(&json!({ "name": name })))?;
            if json_out {
                print_json(&done);
            } else {
                println!("{} is now {}.", done["was"].as_str().unwrap_or(&id), done["name"].as_str().unwrap_or(&name));
                println!("Mention it as @{}", handle(done["name"].as_str().unwrap_or(&name)));
            }
        }
    }
    Ok(0)
}

fn handle(name: &str) -> String {
    arugula_control_wire::machine::handle(name)
}

/// The machine to rename: the one named on the command line (by name, any
/// case, or id), else the one called as this computer is (its hostname).
/// A hostname with a domain also finds the machine called by its first label.
fn machine_id(machines: &[(&str, &str)], want: Option<&str>, local: Option<&str>) -> anyhow::Result<String> {
    let wanted_here = want.is_none();
    let Some(want) = want.or(local) else {
        bail!("which machine? `--machine NAME` (or id)");
    };
    let hits: Vec<&str> = machines
        .iter()
        .filter(|(id, name)| *id == want || name.eq_ignore_ascii_case(want))
        .map(|(id, _)| *id)
        .collect();
    let hits = if hits.is_empty() && wanted_here {
        let short = want.split('.').next().unwrap_or(want);
        machines.iter().filter(|(_, name)| name.eq_ignore_ascii_case(short)).map(|(id, _)| *id).collect()
    } else {
        hits
    };
    match hits.as_slice() {
        [id] => Ok((*id).to_owned()),
        [] => {
            let names: Vec<&str> = machines.iter().map(|(_, n)| *n).collect();
            bail!("no machine of yours is called {want}: {} (pass `--machine`)", names.join(", "))
        }
        _ => bail!("more than one machine is called {want}: name one by its id ({})", hits.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_is_named_by_its_name_or_id_once() {
        let ms = [("m1", "Mini"), ("m2", "pad"), ("m3", "pad")];
        assert_eq!(machine_id(&ms, Some("mini"), None).unwrap(), "m1");
        assert_eq!(machine_id(&ms, Some("m2"), None).unwrap(), "m2");
        assert_eq!(machine_id(&ms, None, Some("Mini")).unwrap(), "m1", "the local daemon's name finds it");
        assert!(machine_id(&ms, Some("pad"), None).unwrap_err().to_string().contains("more than one"));
        assert!(machine_id(&ms, Some("nope"), None).unwrap_err().to_string().contains("Mini, pad, pad"));
        assert!(machine_id(&ms, None, None).is_err());
    }
}
