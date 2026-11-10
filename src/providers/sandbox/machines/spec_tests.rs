use super::*;

fn variables(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).into(), (*value).into()))
        .collect()
}

#[test]
fn a_spec_boat_would_refuse_after_making_a_vm_is_refused_first() {
    let long = "x".repeat(129);
    let many: BTreeMap<String, String> = (0..=MAX_VARIABLES)
        .map(|i| (format!("V{i}"), String::new()))
        .collect();
    let big = "v".repeat(MAX_VARIABLE_VALUE + 1);
    for (image, environment, machine_type, lifetime, vars) in [
        ("", "agents", "small", None, BTreeMap::new()),
        ("a/b", "agents", "small", None, BTreeMap::new()),
        ("bsh_x", "", "small", None, BTreeMap::new()),
        ("bsh_x", "a b", "small", None, BTreeMap::new()),
        ("bsh_x", long.as_str(), "small", None, BTreeMap::new()),
        ("bsh_x", "agents", "small?x", None, BTreeMap::new()),
        ("bsh_x", "agents", "small", Some(0), BTreeMap::new()),
        ("bsh_x", "agents", "small", None, many),
        ("bsh_x", "agents", "small", None, variables(&[("1A", "v")])),
        ("bsh_x", "agents", "small", None, variables(&[("A-B", "v")])),
        ("bsh_x", "agents", "small", None, variables(&[("", "v")])),
        (
            "bsh_x",
            "agents",
            "small",
            None,
            variables(&[("A", "a\0b")]),
        ),
        ("bsh_x", "agents", "small", None, variables(&[("A", &big)])),
    ] {
        assert_eq!(
            MachineSpec::new(image, environment, machine_type, lifetime, vars),
            Err(MachineFailure::InvalidInput),
            "{image} {environment} {machine_type} {lifetime:?}"
        );
    }
    let spec = MachineSpec::new(
        "bsh_x",
        "agents.v1",
        "small",
        Some(1),
        variables(&[("_A1", &"v".repeat(MAX_VARIABLE_VALUE))]),
    )
    .unwrap();
    assert_eq!(spec.lifetime(), Some(1));
}

#[test]
fn a_spec_shows_variable_names_and_never_their_values() {
    let spec = MachineSpec::new(
        "bsh_x",
        "agents",
        "small",
        None,
        variables(&[("TOKEN", "SENTINEL_SECRET")]),
    )
    .unwrap();
    let text = format!("{spec:?}");
    assert!(text.contains("TOKEN"), "{text}");
    assert!(!text.contains("SENTINEL_SECRET"), "{text}");
}

#[test]
fn a_budget_is_neither_empty_nor_unbounded() {
    assert_eq!(
        Budget::new(Duration::ZERO),
        Err(MachineFailure::InvalidInput)
    );
    assert_eq!(
        Budget::new(MAX_BUDGET + Duration::from_nanos(1)),
        Err(MachineFailure::InvalidInput)
    );
    assert_eq!(Budget::new(MAX_BUDGET).unwrap().duration(), MAX_BUDGET);
    assert_eq!(Budget::default().duration(), Duration::from_secs(120));
}
