//! HTTP request/response/error contracts produced by the real SDK 1.9.7.
mod common;
use boat_client::*;
use common::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn decode<T: DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
fn encode<T: Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap()
}
async fn invoke(api: &BoatApi, operation: &str, p: &Value) -> Result<Value> {
    let scoped = if operation.to_ascii_lowercase().contains("environment") {
        api.clone().with_org(p["org"].as_str().map(str::to_owned))
    } else {
        api.clone()
    };
    let api = &scoped;
    let s = |key: &str| p[key].as_str().unwrap();
    Ok(match operation {
        "addEnvironmentRepo" => encode(
            api.add_environment_repo(
                s("environmentId"),
                decode(p["addEnvironmentRepoRequest"].clone()),
            )
            .await?,
        ),
        "apiKeyUsage" => encode(api.api_key_usage(s("apiKeyId")).await?),
        "apiKeys" => encode(api.api_keys().await?),
        "artifact" => encode(api.artifact(s("sandboxId"), s("path")).await?),
        "sandboxes" => encode(api.sandboxes(Some(&decode(p.clone()))).await?),
        "command" => encode(
            api.command_raw(s("sandboxId"), decode(p["commandRequest"].clone()))
                .await?,
        ),
        "commandStatus" => encode(
            api.command_status(
                s("sandboxId"),
                &p["processId"].to_string(),
                Some(&decode(p.clone())),
            )
            .await?,
        ),
        "create" => encode(
            api.create(
                p.get("createSandboxRequest").cloned().map(decode),
                &CreateOptions {
                    org: p["org"].as_str().map(str::to_owned),
                    x_boat_org: p["xBoatOrg"].as_str().map(str::to_owned),
                    idempotency_key: p["idempotencyKey"].as_str().map(str::to_owned),
                },
            )
            .await?,
        ),
        "createEnvironment" => encode(
            api.create_environment(decode(p["createSandboxEnvironmentRequest"].clone()))
                .await?,
        ),
        "createWebhook" => encode(
            api.create_webhook(decode(p["webhookCreateRequest"].clone()))
                .await?,
        ),
        "deleteSandbox" => encode(api.delete_sandbox(s("sandboxId")).await?),
        "deleteEnvironment" => encode(api.delete_environment(s("environmentId")).await?),
        "deleteEnvironmentRepo" => encode(
            api.delete_environment_repo(s("environmentId"), s("repositoryId"))
                .await?,
        ),
        "deleteEnvironmentSecretFile" => encode(
            api.delete_environment_secret_file(s("environmentId"), s("path"))
                .await?,
        ),
        "deleteEnvironmentVar" => encode(
            api.delete_environment_var(s("environmentId"), s("key"))
                .await?,
        ),
        "deleteNamedSnapshot" => encode(api.delete_named_snapshot(s("name")).await?),
        "deleteSnapshot" => encode(api.delete_snapshot(s("snapshotId")).await?),
        "deleteWebhook" => encode(api.delete_webhook(s("webhookId")).await?),
        "desktop" => encode(
            api.desktop(
                s("sandboxId"),
                Some(&decode(p.clone())),
                p.get("desktopRequest").cloned().map(decode),
            )
            .await?,
        ),
        "environments" => encode(api.environments().await?),
        "events" => encode(api.events(s("sandboxId"), Some(&decode(p.clone()))).await?),
        "fork" => encode(
            api.fork(
                s("sandboxId"),
                p.get("forkRequest").cloned().map(decode),
                p["idempotencyKey"].as_str(),
            )
            .await?,
        ),
        "get" => encode(api.get(s("sandboxId")).await?),
        "getDataRetention" => encode(api.get_data_retention().await?),
        "getDeletionOperation" => encode(api.get_deletion_operation(s("operationId")).await?),
        "getLatestSandboxSnapshot" => encode(api.latest_sandbox_snapshot(s("sandboxId")).await?),
        "getNamedSnapshot" => encode(api.get_named_snapshot(s("name")).await?),
        "getSnapshotDownload" => encode(api.snapshot_download(s("snapshotId")).await?),
        "getSnapshotFile" => encode(
            api.snapshot_file(s("snapshotId"), p["path"].as_str())
                .await?,
        ),
        "getSnapshotTree" => encode(api.snapshot_tree(s("snapshotId")).await?),
        "getWebhook" => encode(api.get_webhook(s("webhookId")).await?),
        "hostPort" => encode(
            api.host_port(s("sandboxId"), decode(p["hostPortRequest"].clone()))
                .await?,
        ),
        "interrupt" => encode(
            api.interrupt_with_query(s("sandboxId"), Some(&decode(p.clone())))
                .await?,
        ),
        "limits" => encode(
            api.limits(&LimitsOptions {
                org: p["org"].as_str().map(str::to_owned),
                team_id: p["teamId"].as_str().map(str::to_owned),
                x_boat_org: p["xBoatOrg"].as_str().map(str::to_owned),
            })
            .await?,
        ),
        "listSandboxSnapshots" => encode(
            api.list_sandbox_snapshots(s("sandboxId"), Some(&decode(p.clone())))
                .await?,
        ),
        "listNamedSnapshots" => encode(api.list_named_snapshots().await?),
        "listSnapshots" => encode(api.list_snapshots(Some(&decode(p.clone()))).await?),
        "listWebhooks" => encode(api.list_webhooks().await?),
        "me" => encode(api.me().await?),
        "prompt" => encode(
            api.prompt(s("sandboxId"), decode(p["promptRequest"].clone()))
                .await?,
        ),
        "promptRunStatus" => encode(api.prompt_run_status(s("sandboxId"), s("promptId")).await?),
        "readFile" => encode(
            api.read_file(s("sandboxId"), s("path"), p["encoding"].as_str())
                .await?,
        ),
        "repos" => encode(api.repos(Some(&decode(p.clone()))).await?),
        "resume" => encode(
            api.resume(s("sandboxId"), p.get("resumeRequest").cloned().map(decode))
                .await?,
        ),
        "rotateWebhookSigningSecret" => {
            encode(api.rotate_webhook_signing_secret(s("webhookId")).await?)
        }
        "saveNamedSnapshot" => encode(
            api.save_named_snapshot(decode(p["namedSnapshotSaveRequest"].clone()))
                .await?,
        ),
        "secrets" => encode(api.secrets().await?),
        "selectRepo" => encode(
            api.select_repo(decode(p["repoSelectionRequest"].clone()))
                .await?,
        ),
        "setEnvironmentSecretFile" => encode(
            api.set_environment_secret_file(
                s("environmentId"),
                decode(p["setEnvironmentSecretFileRequest"].clone()),
            )
            .await?,
        ),
        "setEnvironmentVar" => encode(
            api.set_environment_var(
                s("environmentId"),
                s("key"),
                decode(p["setEnvironmentVarRequest"].clone()),
            )
            .await?,
        ),
        "sshKey" => encode(
            api.ssh_key(s("sandboxId"), p["sshKeyRequest"]["key"].as_str().unwrap())
                .await?,
        ),
        "stop" => encode(
            api.stop(s("sandboxId"), p.get("stopRequest").cloned().map(decode))
                .await?,
        ),
        "update" => encode(
            api.update(s("sandboxId"), decode(p["updateSandboxRequest"].clone()))
                .await?,
        ),
        "updateDataRetention" => encode(
            api.update_data_retention(decode(p["dataRetentionUpdateRequest"].clone()))
                .await?,
        ),
        "updateEnvironment" => encode(
            api.update_environment(
                s("environmentId"),
                decode(p["updateSandboxEnvironmentRequest"].clone()),
            )
            .await?,
        ),
        "updateSecrets" => encode(
            api.update_secrets(decode(p["secretsUpdateRequest"].clone()))
                .await?,
        ),
        "updateWebhook" => encode(
            api.update_webhook(s("webhookId"), decode(p["webhookUpdateRequest"].clone()))
                .await?,
        ),
        "upgradeEnvironment" => encode(
            api.upgrade_environment(
                s("environmentId"),
                p.get("upgradeSandboxEnvironmentRequest")
                    .cloned()
                    .map(decode),
            )
            .await?,
        ),
        "writeFile" => encode(
            api.write_file(s("sandboxId"), decode(p["fileWriteRequest"].clone()))
                .await?,
        ),
        _ => panic!("missing Rust adapter for {operation}"),
    })
}

async fn check(operation: &str) {
    let case = sdk_case(operation);
    for variant in ["full", "minimal", "minimal_request", "error", "transient"] {
        if variant == "transient" && case["request"]["method"] == "GET" {
            continue;
        }
        let failure = matches!(variant, "error" | "transient");
        let status = if variant == "transient" { 503 } else { 400 };
        let response_body = if variant == "minimal" {
            &case["minimalResponse"]
        } else {
            &case["response"]
        };
        let server = MockServer::start().await;
        let response = if failure {
            ResponseTemplate::new(status).set_body_json(json!({"ok":false,"error":{"code":"fixture_error","message":"private-message","details":{"secret":"private-detail"}},"requestId":"req_fixture"}))
        } else if case["responseModel"] == "Blob" {
            ResponseTemplate::new(200).set_body_bytes(vec![0, 255, 128, 10])
        } else {
            ResponseTemplate::new(200).set_body_json(response_body)
        };
        Mock::given(|_: &Request| true)
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let parameters = if variant == "minimal_request" {
            &case["minimalParameters"]
        } else {
            &case["parameters"]
        };
        let result = invoke(&client(&server), operation, parameters).await;
        if failure {
            let error = result.expect_err(operation);
            assert_eq!(error.status(), Some(status), "{operation}");
            assert_eq!(error.api_message(), Some("private-message"), "{operation}");
            assert_eq!(
                error.api_details(),
                Some(&json!({"secret":"private-detail"})),
                "{operation}"
            );
            assert!(!format!("{error} {error:?}").contains("private-"));
            assert_eq!(error.is_retryable(), variant == "transient");
        } else {
            let actual = result.unwrap_or_else(|error| panic!("{operation}: {error:?}"));
            if case["responseModel"] == "Blob" {
                assert_eq!(actual, json!([0, 255, 128, 10]), "{operation}");
            } else {
                assert_eq!(
                    without_nulls(actual),
                    without_nulls(response_body.clone()),
                    "{operation} response"
                );
            }
        }
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "{operation}");
        let request = &requests[0];
        let expected = if variant == "minimal_request" {
            &case["minimalRequest"]
        } else {
            &case["request"]
        };
        assert_eq!(
            request.method.as_str(),
            expected["method"].as_str().unwrap(),
            "{operation}"
        );
        assert_eq!(
            request.url.path(),
            expected["path"].as_str().unwrap(),
            "{operation}"
        );
        let actual_query: std::collections::BTreeMap<String, String> = request
            .url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let expected_query: std::collections::BTreeMap<String, String> = expected["query"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string()),
                )
            })
            .collect();
        assert_eq!(actual_query, expected_query, "{operation} query");
        for (key, value) in expected["headers"].as_object().unwrap() {
            let values: Vec<_> = request
                .headers
                .get_all(key)
                .iter()
                .map(|v| v.to_str().unwrap())
                .collect();
            assert_eq!(
                values,
                vec![value.as_str().unwrap()],
                "{operation} header {key}"
            );
        }
        assert_eq!(request.headers["authorization"], "Bearer fixture-only");
        if !expected["headers"]
            .as_object()
            .unwrap()
            .contains_key("X-Boat-Org")
        {
            if expected["path"]
                .as_str()
                .unwrap()
                .starts_with("/environments")
            {
                assert!(!request.headers.contains_key("x-boat-org"));
            } else {
                assert_eq!(request.headers["x-boat-org"], "configured-org");
            }
        }
        if let Some(body) = expected.get("body") {
            assert_eq!(
                request.body_json::<Value>().unwrap(),
                *body,
                "{operation} body"
            );
        } else {
            assert!(request.body.is_empty(), "{operation} unexpected body");
        }
    }
}
// Compare canonical wire fields across the Rust and TypeScript models.
// Rust Option emits null; JSON has one numeric type while serde distinguishes integers/floats.
fn without_nulls(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, without_nulls(v)))
                .collect(),
        ),
        Value::Array(array) => Value::Array(array.into_iter().map(without_nulls).collect()),
        Value::Number(n) => json!(n.as_f64().unwrap()),
        other => other,
    }
}
#[tokio::test]
async fn add_environment_repo() {
    check("addEnvironmentRepo").await;
}
#[tokio::test]
async fn api_key_usage() {
    check("apiKeyUsage").await;
}
#[tokio::test]
async fn api_keys() {
    check("apiKeys").await;
}
#[tokio::test]
async fn artifact() {
    check("artifact").await;
}
#[tokio::test]
async fn sandboxes() {
    check("sandboxes").await;
}
#[tokio::test]
async fn command() {
    check("command").await;
}
#[tokio::test]
async fn command_status() {
    check("commandStatus").await;
}
#[tokio::test]
async fn create() {
    check("create").await;
}
#[tokio::test]
async fn create_environment() {
    check("createEnvironment").await;
}
#[tokio::test]
async fn create_webhook() {
    check("createWebhook").await;
}
#[tokio::test]
async fn delete_sandbox() {
    check("deleteSandbox").await;
}
#[tokio::test]
async fn delete_environment() {
    check("deleteEnvironment").await;
}
#[tokio::test]
async fn delete_environment_repo() {
    check("deleteEnvironmentRepo").await;
}
#[tokio::test]
async fn delete_environment_secret_file() {
    check("deleteEnvironmentSecretFile").await;
}
#[tokio::test]
async fn delete_environment_var() {
    check("deleteEnvironmentVar").await;
}
#[tokio::test]
async fn delete_named_snapshot() {
    check("deleteNamedSnapshot").await;
}
#[tokio::test]
async fn delete_snapshot() {
    check("deleteSnapshot").await;
}
#[tokio::test]
async fn delete_webhook() {
    check("deleteWebhook").await;
}
#[tokio::test]
async fn desktop() {
    check("desktop").await;
}
#[tokio::test]
async fn environments() {
    check("environments").await;
}
#[tokio::test]
async fn events() {
    check("events").await;
}
#[tokio::test]
async fn fork() {
    check("fork").await;
}
#[tokio::test]
async fn get() {
    check("get").await;
}
#[tokio::test]
async fn get_data_retention() {
    check("getDataRetention").await;
}
#[tokio::test]
async fn get_deletion_operation() {
    check("getDeletionOperation").await;
}
#[tokio::test]
async fn get_latest_sandbox_snapshot() {
    check("getLatestSandboxSnapshot").await;
}
#[tokio::test]
async fn get_named_snapshot() {
    check("getNamedSnapshot").await;
}
#[tokio::test]
async fn get_snapshot_download() {
    check("getSnapshotDownload").await;
}
#[tokio::test]
async fn get_snapshot_file() {
    check("getSnapshotFile").await;
}
#[tokio::test]
async fn get_snapshot_tree() {
    check("getSnapshotTree").await;
}
#[tokio::test]
async fn get_webhook() {
    check("getWebhook").await;
}
#[tokio::test]
async fn host_port() {
    check("hostPort").await;
}
#[tokio::test]
async fn interrupt() {
    check("interrupt").await;
}
#[tokio::test]
async fn limits() {
    check("limits").await;
}
#[tokio::test]
async fn list_sandbox_snapshots() {
    check("listSandboxSnapshots").await;
}
#[tokio::test]
async fn list_named_snapshots() {
    check("listNamedSnapshots").await;
}
#[tokio::test]
async fn list_snapshots() {
    check("listSnapshots").await;
}
#[tokio::test]
async fn list_webhooks() {
    check("listWebhooks").await;
}
#[tokio::test]
async fn me() {
    check("me").await;
}
#[tokio::test]
async fn prompt() {
    check("prompt").await;
}
#[tokio::test]
async fn prompt_run_status() {
    check("promptRunStatus").await;
}
#[tokio::test]
async fn read_file() {
    check("readFile").await;
}
#[tokio::test]
async fn repos() {
    check("repos").await;
}
#[tokio::test]
async fn resume() {
    check("resume").await;
}
#[tokio::test]
async fn rotate_webhook_signing_secret() {
    check("rotateWebhookSigningSecret").await;
}
#[tokio::test]
async fn save_named_snapshot() {
    check("saveNamedSnapshot").await;
}
#[tokio::test]
async fn secrets() {
    check("secrets").await;
}
#[tokio::test]
async fn select_repo() {
    check("selectRepo").await;
}
#[tokio::test]
async fn set_environment_secret_file() {
    check("setEnvironmentSecretFile").await;
}
#[tokio::test]
async fn set_environment_var() {
    check("setEnvironmentVar").await;
}
#[tokio::test]
async fn ssh_key() {
    check("sshKey").await;
}
#[tokio::test]
async fn stop() {
    check("stop").await;
}
#[tokio::test]
async fn update() {
    check("update").await;
}
#[tokio::test]
async fn update_data_retention() {
    check("updateDataRetention").await;
}
#[tokio::test]
async fn update_environment() {
    check("updateEnvironment").await;
}
#[tokio::test]
async fn update_secrets() {
    check("updateSecrets").await;
}
#[tokio::test]
async fn update_webhook() {
    check("updateWebhook").await;
}
#[tokio::test]
async fn upgrade_environment() {
    check("upgradeEnvironment").await;
}
#[tokio::test]
async fn write_file() {
    check("writeFile").await;
}
