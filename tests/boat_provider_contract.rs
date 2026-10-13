#![cfg(feature = "boat")]
//! Public Boat contracts: current provider/configuration names are canonical,
//! obsolete Box fields are rejected, and named registrations stay separate.
use serde_json::json;
use std::sync::Arc;
use workspace_sdk::{
    boat_client::{BoatApi, BoatClientConfig},
    infrastructure::MemoryStateRepository,
    providers::{BoatOpenOptions, BoatProvider, SandboxOpenOptions, SandboxProvider},
    Error, IProvider, ProviderId, ProviderOptions, WorkspaceRef, WorkspaceSdk,
};

fn provider() -> BoatProvider {
    let config = BoatClientConfig::new("fixture-only").unwrap();
    BoatProvider::new(BoatApi::new(config).unwrap())
}

#[test]
fn boat_and_neutral_public_names_share_the_client_and_boat_provider_id() {
    let config = BoatClientConfig::new("fixture-only").unwrap();
    assert_eq!(config.base_path, "https://boat.dev/api/v1");
    assert_eq!(
        BoatClientConfig::DEFAULT_BASE_PATH,
        "https://boat.dev/api/v1"
    );
    let client = BoatApi::new(config).unwrap();
    let branded: BoatProvider = BoatProvider::new(client);
    let neutral: SandboxProvider = branded.clone();
    for registered in [&branded, &neutral] {
        assert_eq!(registered.id().as_str(), "boat");
        assert_eq!(
            serde_json::to_value(registered.id()).unwrap(),
            json!("boat")
        );
    }
    let neutral_options: SandboxOpenOptions = BoatOpenOptions::new("/work/task");
    assert_eq!(neutral_options.root, "/work/task");
    assert_eq!(neutral_options.ttl_seconds, Some(3600));
}

#[test]
fn current_option_names_round_trip_to_the_canonical_schema() {
    let canonical = json!({
        "root": "/work/task",
        "sandbox_id": "bx_23456789",
        "sandbox_type": "large",
        "ttl_seconds": null,
        "environment": "customer-fixture"
    });
    let options: BoatOpenOptions = serde_json::from_value(canonical.clone()).unwrap();
    assert_eq!(options.sandbox_id.as_deref(), Some("bx_23456789"));
    assert_eq!(options.sandbox_type.as_deref(), Some("large"));
    assert_eq!(
        options.ttl_seconds, None,
        "explicit no-auto-stop must survive"
    );
    assert_eq!(serde_json::to_value(&options).unwrap(), canonical);
    let opaque = ProviderOptions::from_options(&options).unwrap();
    assert_eq!(serde_json::to_value(&opaque).unwrap(), canonical);
    let restored: BoatOpenOptions = opaque.decode().unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), canonical);

    let defaults: BoatOpenOptions = serde_json::from_value(json!({"root": "/work/task"})).unwrap();
    assert_eq!(defaults.ttl_seconds, Some(3600));
    assert_eq!(defaults.sandbox_id, None);
    assert_eq!(defaults.sandbox_type, None);
    assert_eq!(defaults.environment, None);
    assert!(
        serde_json::from_value::<BoatOpenOptions>(json!({"sandbox_id": "bx_23456789"})).is_err()
    );
    assert!(serde_json::from_value::<BoatOpenOptions>(
        json!({"root": "/work/task", "sandbox_typo": "large"})
    )
    .is_err());
}

#[test]
fn obsolete_box_option_fields_are_rejected_alone_and_with_current_fields() {
    for (obsolete, current, value) in [
        ("box_id", "sandbox_id", "bx_23456789"),
        ("box_type", "sandbox_type", "large"),
    ] {
        let old_only = format!(r#"{{"root":"/work/task","{obsolete}":"{value}"}}"#);
        assert!(serde_json::from_str::<BoatOpenOptions>(&old_only).is_err());
        for (first, second) in [(obsolete, current), (current, obsolete)] {
            let combined =
                format!(r#"{{"root":"/work/task","{first}":"{value}","{second}":"{value}"}}"#);
            assert!(
                serde_json::from_str::<BoatOpenOptions>(&combined).is_err(),
                "obsolete fields must not be accepted alongside current options: {combined}"
            );
        }
    }
}

#[test]
fn duplicate_current_option_fields_are_rejected_in_both_value_orders() {
    for (field, value) in [("sandbox_id", "bx_23456789"), ("sandbox_type", "large")] {
        for alternate in [json!(value), json!("different"), serde_json::Value::Null] {
            for (first, second) in [(json!(value), alternate.clone()), (alternate, json!(value))] {
                let input =
                    format!(r#"{{"root":"/work/task","{field}":{first},"{field}":{second}}}"#);
                assert!(
                    serde_json::from_str::<BoatOpenOptions>(&input).is_err(),
                    "duplicate fields must not pick a target or machine type by field order: {input}"
                );
            }
        }
    }
}

fn saved_reference(provider_id: &str, workspace_id: &str) -> WorkspaceRef {
    serde_json::from_value(json!({
        "version": 1,
        "id": workspace_id,
        "provider": provider_id,
        "locator": {
            "root": format!("/work/{workspace_id}/project"),
            "control": format!("/work/{workspace_id}/control"),
            "ownership": "attached",
            "target": "bx_23456789"
        }
    }))
    .unwrap()
}

#[tokio::test]
async fn explicit_provider_ids_keep_registration_and_reconnection_isolated() {
    let original = provider();
    let scoped = original
        .clone()
        .with_id(ProviderId::new("boat-team").unwrap());
    assert_eq!(original.id().as_str(), "boat");
    assert_eq!(scoped.id().as_str(), "boat-team");
    let default_reference = saved_reference("boat", "saved-default");
    let scoped_reference = saved_reference("boat-team", "saved-scoped");
    assert!(matches!(
        scoped.connect(&default_reference).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        original.connect(&scoped_reference).await,
        Err(Error::InvalidInput(_))
    ));

    let repository = Arc::new(MemoryStateRepository::default());
    let sdk = WorkspaceSdk::builder(repository)
        .provider(Arc::new(original.clone()))
        .unwrap()
        .provider(Arc::new(scoped))
        .unwrap()
        .build()
        .unwrap();
    let default = sdk.connect(&default_reference).await.unwrap();
    let scoped = sdk.connect(&scoped_reference).await.unwrap();
    assert_eq!(default.reference(), default_reference);
    assert_eq!(scoped.reference(), scoped_reference);
    assert!(!default.descriptor().capabilities.destroy);
    assert!(!scoped.descriptor().capabilities.destroy);
    assert_eq!(sdk.list().unwrap().len(), 2);

    let duplicate = WorkspaceSdk::builder(Arc::new(MemoryStateRepository::default()))
        .provider(Arc::new(original.clone()))
        .unwrap()
        .provider(Arc::new(original));
    assert!(matches!(duplicate, Err(Error::Conflict(_))));
}
