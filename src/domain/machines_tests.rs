use super::*;

fn variables(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn a_request_is_checked_when_made() {
    let ok = MachineRequest::new(
        "nessa-runtime@3",
        Lifetime::Unlimited,
        variables(&[("A_1", "x")]),
    );
    assert!(ok.is_ok());
    for bad in [
        MachineRequest::new("", Lifetime::Unlimited, BTreeMap::new()),
        MachineRequest::new("has space", Lifetime::Unlimited, BTreeMap::new()),
        MachineRequest::new("x".repeat(129), Lifetime::Unlimited, BTreeMap::new()),
        MachineRequest::new("t", Lifetime::Hard(Duration::ZERO), BTreeMap::new()),
        MachineRequest::new("t", Lifetime::Unlimited, variables(&[("1A", "x")])),
        MachineRequest::new("t", Lifetime::Unlimited, variables(&[("A-B", "x")])),
        MachineRequest::new("t", Lifetime::Unlimited, variables(&[("A", "x\0")])),
        MachineRequest::new(
            "t",
            Lifetime::Unlimited,
            variables(&[("A", &"x".repeat(MAX_MACHINE_VARIABLE + 1))]),
        ),
    ] {
        assert_eq!(bad, Err(MachineError::InvalidInput));
    }
    let many = (0..=MAX_MACHINE_VARIABLES)
        .map(|i| (format!("V{i}"), String::new()))
        .collect();
    assert!(MachineRequest::new("t", Lifetime::Unlimited, many).is_err());
}

#[test]
fn a_requests_debug_never_shows_variable_values() {
    let request =
        MachineRequest::new("t", Lifetime::Unlimited, variables(&[("TOKEN", "secret")])).unwrap();
    let shown = format!("{request:?}");
    assert!(shown.contains("TOKEN"));
    assert!(!shown.contains("secret"));
}

#[test]
fn each_backstop_enforces_only_its_own_lifetimes() {
    let idle = Backstop::IdleSleep {
        shortest: Duration::from_secs(300),
        longest: Duration::from_secs(3600),
    };
    let minutes = |m: u64| Duration::from_secs(m * 60);
    assert!(idle.enforces(Lifetime::Unlimited));
    assert!(idle.enforces(Lifetime::Idle(minutes(5))));
    assert!(idle.enforces(Lifetime::Idle(minutes(60))));
    assert!(!idle.enforces(Lifetime::Idle(minutes(4))));
    assert!(!idle.enforces(Lifetime::Idle(minutes(61))));
    // An idle sleep is never a hard cap: open connections keep it awake.
    assert!(!idle.enforces(Lifetime::Hard(minutes(10))));
    assert!(Backstop::HardLifetime.enforces(Lifetime::Hard(minutes(10))));
    assert!(Backstop::HardLifetime.enforces(Lifetime::Unlimited));
    assert!(!Backstop::HardLifetime.enforces(Lifetime::Idle(minutes(10))));
}

#[test]
fn a_proof_is_plain_and_bounded() {
    assert!(Proven::new("agent37:t@3".into(), Some("sha256:ab".into())).is_ok());
    assert!(Proven::new("boat:a:b".into(), None).is_ok());
    for (identity, digest) in [
        ("", None),
        ("has space", None),
        ("ok", Some(String::new())),
        ("ok", Some("a\nb".into())),
    ] {
        assert_eq!(
            Proven::new(identity.into(), digest),
            Err(MachineError::InvalidInput)
        );
    }
    assert!(Proven::new("x".repeat(MAX_PROVEN + 1), None).is_err());
}
