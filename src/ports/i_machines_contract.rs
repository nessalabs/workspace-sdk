//! The one contract every [`IMachines`] adapter passes, run by each cloud's
//! tests against its own fake.
use super::IMachines;
use crate::domain::machines::{
    Disposition, Lifetime, MachineError, MachineRequest, Made, Proven, StopReason,
};

/// What a cloud's test hands the contract.
pub(crate) struct Case {
    /// A request the cloud's fake makes an admitted machine from.
    pub request: MachineRequest,
    /// A key the fake expects.
    pub key: &'static str,
    /// A lifetime the cloud cannot enforce.
    pub unenforced: Lifetime,
    /// A well-formed proof of something other than what the fake runs.
    pub other: Proven,
}

/// Run the contract. The fake starts with no machine.
pub(crate) async fn contract(cloud: &dyn IMachines, case: Case) -> Made {
    assert!(cloud.backstop().enforces(case.request.lifetime()));
    assert!(!cloud.backstop().enforces(case.unenforced));
    let unenforced = MachineRequest::new(
        case.request.template(),
        case.unenforced,
        case.request.variables().clone(),
    )
    .unwrap();
    // Refused before anything is asked.
    assert_eq!(
        cloud.prepare(&unenforced).await,
        Err(MachineError::InvalidInput)
    );
    assert_eq!(
        cloud.create(&unenforced, case.key).await,
        Err(MachineError::InvalidInput)
    );
    assert_eq!(
        cloud.create(&case.request, "").await,
        Err(MachineError::InvalidInput)
    );

    cloud.prepare(&case.request).await.unwrap();
    let made = cloud.create(&case.request, case.key).await.unwrap();
    assert_eq!(made.refusal(), None);
    assert_eq!(made.disposition(), Disposition::Remove);
    // Asked again under the key, the same machine.
    let again = cloud.create(&case.request, case.key).await.unwrap();
    assert_eq!(again.machine(), made.machine());
    assert_eq!(again.proven(), made.proven());

    // A proof survives being saved and loaded again.
    let saved = Proven::new(
        made.proven().identity().to_owned(),
        made.proven().digest().map(str::to_owned),
    )
    .unwrap();
    cloud
        .ready(&case.request, made.machine(), &saved)
        .await
        .unwrap();
    // Held to exactly what was proven.
    assert!(matches!(
        cloud
            .ready(&case.request, made.machine(), &case.other)
            .await,
        Err(MachineError::AdmissionRejected(_) | MachineError::InvalidInput)
    ));
    cloud
        .stop(made.machine(), StopReason::Closed)
        .await
        .unwrap();

    // One that may be in use elsewhere is never removed.
    let retained = Made::restore(made.machine().to_owned(), saved, Disposition::Retain);
    assert_eq!(
        cloud.remove(&retained).await,
        Err(MachineError::InvalidInput)
    );
    made
}
