use super::*;
use crate::ports::i_machines_contract::{contract, Case};
use crate::providers::agent37::machines::tests::{Server, DIGEST, KEY, TEMPLATE};
use std::{collections::BTreeMap, time::Duration};

fn request(lifetime: Lifetime) -> MachineRequest {
    MachineRequest::new(
        TEMPLATE,
        lifetime,
        BTreeMap::from([("NESSA_THREAD_ID".into(), "0001".into())]),
    )
    .unwrap()
}

#[tokio::test]
async fn agent37_passes_the_machines_contract() {
    let server = Server::start().await;
    let cloud = Agent37Cloud::new(server.machines.clone());
    let made = contract(
        &cloud,
        Case {
            request: request(Lifetime::Idle(Duration::from_secs(900))),
            key: KEY,
            unenforced: Lifetime::Hard(Duration::from_secs(3600)),
            other: Proven::new(format!("agent37:{TEMPLATE}#3"), Some("sha256:other".into()))
                .unwrap(),
        },
    )
    .await;
    assert_eq!(made.proven().identity(), format!("agent37:{TEMPLATE}#3"));
    assert_eq!(made.proven().digest(), Some(DIGEST));
    // Agent37 confirms a deletion.
    cloud.remove(&made).await.unwrap();
    assert!(server.fake.lock().unwrap().instances.is_empty());
}

#[test]
fn an_unlimited_lifetime_sleeps_after_the_longest_idle_and_a_hard_one_is_refused() {
    let machines = Agent37Machines::new(crate::providers::agent37::Agent37Config::new(
        crate::providers::agent37::ApiKey::new("sk_live_fixture").unwrap(),
    ))
    .unwrap();
    let cloud = Agent37Cloud::new(machines);
    assert_eq!(
        cloud.spec(&request(Lifetime::Unlimited)).unwrap().idle,
        *IDLE.end()
    );
    assert_eq!(
        cloud
            .spec(&request(Lifetime::Idle(Duration::from_secs(600))))
            .unwrap()
            .idle,
        Duration::from_secs(600)
    );
    assert_eq!(
        cloud.spec(&request(Lifetime::Hard(Duration::from_secs(600)))),
        Err(MachineError::InvalidInput)
    );
    assert_eq!(
        cloud.spec(&request(Lifetime::Idle(Duration::from_secs(60)))),
        Err(MachineError::InvalidInput)
    );
}

#[test]
fn an_agent37_proof_round_trips_and_nothing_else_is_agent37s() {
    let proof = proof("nessa-runtime@3", Some(3), Some("sha256:a".into())).unwrap();
    let admitted = admission(&proof).unwrap();
    assert_eq!(
        (admitted.template(), admitted.revision(), admitted.digest()),
        ("nessa-runtime@3", Some(3), "sha256:a")
    );
    let unrevised = admission(&proof_of("agent37:base@latest", Some("d"))).unwrap();
    assert_eq!(unrevised.revision(), None);
    for foreign in [
        proof_of("boat:a:b:1:agents", None),
        // Without a digest it was never admitted.
        proof_of("agent37:nessa-runtime@3#3", None),
        proof_of("agent37:nessa-runtime@3#three", Some("d")),
    ] {
        assert_eq!(
            admission(&foreign).map(drop),
            Err(MachineError::InvalidInput),
            "{foreign:?}"
        );
    }
}

fn proof_of(identity: &str, digest: Option<&str>) -> Proven {
    Proven::new(identity.into(), digest.map(Into::into)).unwrap()
}
