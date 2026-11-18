use super::*;
use crate::ports::i_machines_contract::{contract, Case};
use crate::providers::sandbox::machines::account::tests::{server, KEY};
use std::{collections::BTreeMap, time::Duration};

fn request(lifetime: Lifetime) -> MachineRequest {
    MachineRequest::new(
        "bsh_fixture",
        lifetime,
        BTreeMap::from([("WORKSPACE".into(), "fixture-workspace".into())]),
    )
    .unwrap()
}

#[tokio::test]
async fn boat_passes_the_machines_contract() {
    let server = server().await;
    server.state.lock().unwrap().answers_stop = true;
    let cloud = BoatCloud::new(server.machines.clone(), "agents", "small").unwrap();
    let other = Proven::new(
        "boat:11111111-1111-4111-8111-111111111111:33333333-3333-4333-8333-333333333333:3:agents"
            .into(),
        None,
    )
    .unwrap();
    let made = contract(
        &cloud,
        Case {
            request: request(Lifetime::Hard(Duration::from_secs(3600))),
            key: KEY,
            unenforced: Lifetime::Idle(Duration::from_secs(600)),
            other,
        },
    )
    .await;
    assert_eq!(made.machine(), "bx_23456789");
    // Boat accepts a removal but never confirms it.
    assert_eq!(
        cloud.remove(&made).await,
        Err(MachineError::Provider),
        "the fixture refuses deletions"
    );
    assert_eq!(server.state.lock().unwrap().deleted, 1);
}

#[test]
fn a_boat_proof_names_its_environment_exactly_and_nothing_else_is_boats() {
    let expected = EnvironmentIdentity::new(
        "11111111-1111-4111-8111-111111111111".into(),
        "agents".into(),
        "22222222-2222-4222-8222-222222222222".into(),
        2,
    )
    .unwrap();
    let proof = proven(&expected).unwrap();
    assert_eq!(identity(&proof).unwrap(), expected);
    for foreign in [
        Proven::new("agent37:t@1#1".into(), Some("sha256:a".into())).unwrap(),
        Proven::new(format!("{}:extra", proof.identity()), None).unwrap(),
        Proven::new(proof.identity().into(), Some("sha256:a".into())).unwrap(),
        Proven::new("boat:not-a-uuid:x:1:agents".into(), None).unwrap(),
    ] {
        assert_eq!(
            identity(&foreign),
            Err(MachineError::InvalidInput),
            "{foreign:?}"
        );
    }
}

#[test]
fn a_lifetime_is_carried_in_whole_seconds_rounded_up() {
    let cloud = futures_free_cloud();
    let spec = |lifetime| cloud.spec(&request(lifetime));
    assert_eq!(spec(Lifetime::Unlimited).unwrap().lifetime(), None);
    assert_eq!(
        spec(Lifetime::Hard(Duration::from_millis(1500)))
            .unwrap()
            .lifetime(),
        Some(2)
    );
    assert_eq!(
        spec(Lifetime::Hard(Duration::from_secs(u64::from(u32::MAX) + 1))),
        Err(MachineError::InvalidInput)
    );
    assert_eq!(
        spec(Lifetime::Idle(Duration::from_secs(600))),
        Err(MachineError::InvalidInput)
    );
}

fn futures_free_cloud() -> BoatCloud {
    let config = sandbox_client::ClientConfig::new("fixture-token").unwrap();
    let machines = BoatMachines::for_account(
        sandbox_client::Api::new(config).unwrap(),
        std::sync::Arc::new(NoEvents),
    )
    .unwrap();
    BoatCloud::new(machines, "agents", "small").unwrap()
}
struct NoEvents;
impl super::super::IMachineEvents for NoEvents {
    fn record(&self, _: super::super::MachineEvent) {}
}

#[test]
fn a_cloud_is_refused_an_environment_or_size_boat_could_not_take() {
    let cloud = futures_free_cloud();
    assert!(BoatCloud::new(cloud.machines.clone(), "two words", "small").is_err());
    assert!(BoatCloud::new(cloud.machines, "agents", "").is_err());
}
