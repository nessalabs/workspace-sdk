//! What a Sandbox workspace's reference saves: where the workspace is and,
//! for a Boat machine, what it was made from and admitted under, so that
//! every wake checks it again.
//!
//! The reference refuses fields it does not know, so an SDK that cannot
//! check a machine refuses to reconnect to one rather than waking it
//! unchecked.
use super::machines::{EnvironmentIdentity, MachineSpec};
use crate::providers::managed::{validate_remote_path, Locator};
use crate::{domain::*, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    root: String,
    control: String,
    ownership: Ownership,
    target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    machine: Option<SavedMachine>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedMachine {
    image: String,
    environment: String,
    machine_type: String,
    lifetime: Option<u32>,
    environment_id: String,
    version_id: String,
    version: u32,
}

/// A Boat machine as a workspace keeps it: the spec it was made from,
/// without its variables, which only creation uses, and the environment it
/// was admitted under.
#[derive(Clone, Debug)]
pub(super) struct Machine {
    pub(super) spec: MachineSpec,
    pub(super) admitted: EnvironmentIdentity,
}
impl Machine {
    /// What the SDK saves in the workspace's record.
    pub(super) fn admission(&self) -> Admission {
        Admission {
            environment_id: self.admitted.id().into(),
            version_id: self.admitted.version_id().into(),
            version: self.admitted.version().to_string(),
            name: self.admitted.name().into(),
        }
    }
    fn save(&self) -> SavedMachine {
        SavedMachine {
            image: self.spec.image().into(),
            environment: self.spec.environment().into(),
            machine_type: self.spec.machine_type().into(),
            lifetime: self.spec.lifetime(),
            environment_id: self.admitted.id().into(),
            version_id: self.admitted.version_id().into(),
            version: self.admitted.version(),
        }
    }
    fn load(saved: SavedMachine) -> Result<Self> {
        let invalid = |_| Error::InvalidInput("the saved Boat machine is malformed".into());
        let admitted = EnvironmentIdentity::new(
            saved.environment_id,
            saved.environment.clone(),
            saved.version_id,
            saved.version,
        )
        .map_err(invalid)?;
        let spec = MachineSpec::new(
            saved.image,
            saved.environment,
            saved.machine_type,
            saved.lifetime,
            BTreeMap::new(),
        )
        .map_err(invalid)?;
        Ok(Self { spec, admitted })
    }
}

/// The reference's locator for `locator`, and its machine if it is one.
pub(super) fn encode(locator: &Locator, machine: Option<&Machine>) -> Result<WorkspaceLocator> {
    WorkspaceLocator::from_locator(&Saved {
        root: locator.root.clone(),
        control: locator.control.clone(),
        ownership: locator.ownership.clone(),
        target: locator.target.clone(),
        machine: machine.map(Machine::save),
    })
}

/// Where `reference` is, and its machine if it is one.
///
/// # Errors
/// [`Error::InvalidInput`] for a reference of another version or provider,
/// or with a path or machine that is malformed.
pub(super) fn decode(
    reference: &WorkspaceRef,
    provider: &ProviderId,
) -> Result<(Locator, Option<Machine>)> {
    if reference.version != 1 || &reference.provider != provider {
        return Err(Error::InvalidInput(
            "workspace reference version or provider does not match".into(),
        ));
    }
    let saved: Saved = reference.locator.decode()?;
    validate_remote_path(&saved.root)?;
    validate_remote_path(&saved.control)?;
    let machine = saved.machine.map(Machine::load).transpose()?;
    Ok((
        Locator {
            root: saved.root,
            control: saved.control,
            ownership: saved.ownership,
            target: saved.target,
        },
        machine,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reference(locator: serde_json::Value) -> WorkspaceRef {
        WorkspaceRef {
            version: 1,
            id: WorkspaceId::new("fixture").unwrap(),
            provider: ProviderId::new("boat").unwrap(),
            locator: WorkspaceLocator::from_locator(&locator).unwrap(),
        }
    }
    fn plain() -> serde_json::Value {
        json!({"root":"/w/fixture/project","control":"/w/fixture/control",
               "ownership":"sdk_owned","target":"bx_23456789"})
    }

    #[test]
    fn a_plain_reference_reads_as_before_and_a_machine_round_trips() {
        let provider = ProviderId::new("boat").unwrap();
        let (locator, machine) = decode(&reference(plain()), &provider).unwrap();
        assert!(machine.is_none());
        assert_eq!(
            serde_json::to_value(
                encode(&locator, None)
                    .unwrap()
                    .decode::<serde_json::Value>()
                    .unwrap()
            )
            .unwrap(),
            plain()
        );
        let mut saved = plain();
        saved["machine"] = json!({"image":"bsh_fixture","environment":"agents",
            "machine_type":"small","lifetime":3600,
            "environment_id":"11111111-1111-4111-8111-111111111111",
            "version_id":"22222222-2222-4222-8222-222222222222","version":2});
        let (locator, machine) = decode(&reference(saved.clone()), &provider).unwrap();
        let machine = machine.unwrap();
        assert_eq!(machine.spec.lifetime(), Some(3600));
        assert_eq!(machine.admission().version, "2");
        let again: serde_json::Value = encode(&locator, Some(&machine)).unwrap().decode().unwrap();
        assert_eq!(again, saved);
    }

    #[test]
    fn an_unknown_or_malformed_machine_is_refused() {
        let provider = ProviderId::new("boat").unwrap();
        let mut unknown = plain();
        unknown["other"] = json!(1);
        assert!(decode(&reference(unknown), &provider).is_err());
        let mut malformed = plain();
        malformed["machine"] = json!({"image":"../x","environment":"agents",
            "machine_type":"small","lifetime":null,
            "environment_id":"11111111-1111-4111-8111-111111111111",
            "version_id":"22222222-2222-4222-8222-222222222222","version":2});
        assert!(matches!(
            decode(&reference(malformed), &provider),
            Err(Error::InvalidInput(_))
        ));
    }
}
