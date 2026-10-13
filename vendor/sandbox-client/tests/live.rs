//! Live API smoke — gated behind `#[ignore]` + `BOAT_API_KEY`.
//!
//! ```bash
//! BOAT_API_KEY=… cargo test --test live -- --ignored --nocapture
//! BOAT_API_KEY=… SANDBOX_ID=bx_… cargo test --test live -- --ignored
//! ```

use boat_client::{exec_command, BoatApi, BoatClientConfig};

fn client() -> BoatApi {
    let key = std::env::var("BOAT_API_KEY").expect("BOAT_API_KEY required for live tests");
    BoatApi::new(BoatClientConfig::new(key).expect("config")).expect("client")
}

#[tokio::test]
#[ignore = "requires BOAT_API_KEY; live network"]
async fn live_me_and_boxes() {
    let api = client();
    let me = api.me().await.expect("me");
    assert!(me.ok);
    let list = api.sandboxes(None).await.expect("sandboxes");
    assert!(list.ok);
    eprintln!(
        "user={:?} sandboxes={}",
        me.user.email,
        list.sandboxes.len()
    );
}

#[tokio::test]
#[ignore = "requires BOAT_API_KEY and SANDBOX_ID; live network"]
async fn live_get_and_exec_existing_box() {
    let api = client();
    let sandbox_id = std::env::var("SANDBOX_ID").expect("SANDBOX_ID required");
    let info = api.get(&sandbox_id).await.expect("get");
    eprintln!(
        "box {} state={}",
        info.sandbox.id,
        info.sandbox.state.as_str()
    );
    assert!(
        info.sandbox.state.is_operable(),
        "box must be ready/idle/running (got {})",
        info.sandbox.state.as_str()
    );
    let out = exec_command(
        &api,
        &sandbox_id,
        "echo parity-ok && uname -s",
        None,
        Some(30),
    )
    .await
    .expect("exec");
    assert!(out.stdout.contains("parity-ok"), "stdout={}", out.stdout);
    assert!(!out.timed_out);
}

/// Read-only coverage for the response models and routes corrected in the review.
#[tokio::test]
#[ignore = "requires BOAT_API_KEY; live network, read-only"]
async fn live_runtime_read_contracts() {
    let api = client();
    let environments = api.environments().await.expect("environments");
    // Do not print environment contents, snapshot paths, or signed URLs.
    eprintln!("environment count={}", environments.environments.len());
    let snapshots = api
        .list_snapshots(Some(&boat_client::SnapshotsQuery {
            limit: Some(1),
            sort: Some("desc".into()),
            ..Default::default()
        }))
        .await
        .expect("snapshots");
    if let Some(snapshot) = snapshots.snapshots.first() {
        let tree = api
            .snapshot_tree(&snapshot.id)
            .await
            .expect("snapshot tree");
        let download = api
            .snapshot_download(&snapshot.id)
            .await
            .expect("snapshot download");
        assert_eq!(tree.snapshot_id, snapshot.id);
        assert_eq!(download.snapshot_id, snapshot.id);
        eprintln!(
            "snapshot entries={} chunks={}",
            tree.entries.len(),
            download.chunks.len()
        );
    }
}

/// Mutations are limited to a newly created fixture environment, which is cleaned
/// up even when assertions fail. This never updates an existing environment.
#[tokio::test]
#[ignore = "requires BOAT_API_KEY and BOAT_TEST_MUTATIONS=1; creates a temporary environment"]
async fn live_environment_round_trip() {
    assert_eq!(std::env::var("BOAT_TEST_MUTATIONS").as_deref(), Ok("1"));
    let api = client();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("rust-contract-{unique}");
    let created = api
        .create_environment(boat_client::CreateSandboxEnvironmentRequest { name: name.clone() })
        .await
        .expect("create environment");
    let env = created
        .environments
        .iter()
        .find(|env| env.name == name)
        .expect("created fixture missing");
    let result = async {
        let updated = api
            .update_environment(
                &env.id,
                boat_client::UpdateSandboxEnvironmentRequest {
                    env_contents: Some("CONTRACT_MARKER=fixture-value".into()),
                    ..Default::default()
                },
            )
            .await?;
        if !updated.success {
            return Err(boat_client::Error::Unexpected("update unsuccessful".into()));
        }
        let listed = api.environments().await?;
        let fixture = listed
            .environments
            .iter()
            .find(|value| value.id == env.id)
            .ok_or_else(|| boat_client::Error::Unexpected("fixture missing after update".into()))?;
        if !fixture
            .env_contents
            .contains("CONTRACT_MARKER=fixture-value")
        {
            return Err(boat_client::Error::Unexpected(
                "environment marker mismatch".into(),
            ));
        }
        if format!("{fixture:?}").contains("fixture-value")
            || format!("{listed:?}").contains("fixture-value")
        {
            return Err(boat_client::Error::Unexpected(
                "Debug exposed environment marker".into(),
            ));
        }
        Ok::<_, boat_client::Error>(())
    }
    .await;
    let cleanup = api.delete_environment(&env.id).await;
    cleanup.expect("delete fixture environment");
    assert!(!api
        .environments()
        .await
        .expect("verify cleanup")
        .environments
        .iter()
        .any(|value| value.id == env.id));
    result.expect("environment round trip");
}

/// Creates one disposable sandbox; Rust and the actual SDK read each other's writes.
/// The sandbox is permanently deleted and deletion completion is observed on failure too.
#[tokio::test]
#[ignore = "requires BOAT_API_KEY, BOAT_TEST_MUTATIONS=1 and BOAT_SDK_PATH; creates and deletes a disposable box"]
async fn live_sdk_file_round_trip() {
    use boat_client::{
        read_text, wait_for_deletion, wait_until_ready, write_text, CreateSandboxRequest, Error,
    };
    assert_eq!(std::env::var("BOAT_TEST_MUTATIONS").as_deref(), Ok("1"));
    let sdk_path = std::env::var("BOAT_SDK_PATH").expect("BOAT_SDK_PATH required");
    let api = client();
    assert!(
        api.limits(&Default::default())
            .await
            .expect("limits")
            .can_start,
        "account cannot start a box"
    );
    let created = api
        .create(
            Some(CreateSandboxRequest {
                no_env: Some(true),
                ..CreateSandboxRequest::ttl(300)
            }),
            &Default::default(),
        )
        .await
        .expect("create fixture box");
    let sandbox_id = &created.sandbox.id;
    let result = async {
        wait_until_ready(&api, sandbox_id).await?;
        let path = "/tmp/rust-sdk-file-contract.txt";
        write_text(&api, sandbox_id, path, "written-by-rust").await?;
        let output = std::process::Command::new("node")
            .arg("tests/live_sdk_files.cjs")
            .env("BOAT_SDK_PATH", sdk_path)
            .env("SANDBOX_ID", sandbox_id)
            .env("BOAT_TEST_PATH", path)
            .output()
            .map_err(|_| Error::Config("could not start Node SDK probe".into()))?;
        if !output.status.success() {
            return Err(Error::Unexpected("SDK file round trip failed".into()));
        }
        let actual = read_text(&api, sandbox_id, path).await?;
        if actual != "written-by-typescript" {
            return Err(Error::Unexpected("cross-SDK file contents differ".into()));
        }
        Ok::<_, Error>(())
    }
    .await;
    eprintln!("file parity result={result:?}; fixture box={sandbox_id}");
    let cleanup = api
        .delete_sandbox(sandbox_id)
        .await
        .expect("delete disposable box");
    wait_for_deletion(&api, &cleanup.operation.id, None)
        .await
        .expect("verify disposable box deletion completed");
    result.expect("live Rust/TypeScript file parity");
    eprintln!(
        "PASS: Rust PUT → TypeScript read → TypeScript PUT → Rust read; box deletion completed"
    );
}
