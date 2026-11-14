//! Against Agent37 itself: makes, connects to, stops and deletes one
//! instance in the workspace whose key is in `AGENT37_API_KEY`. It costs a
//! few minutes of the smallest instance, so it runs only when asked:
//! `cargo test --features agent37 agent37::live -- --ignored --nocapture`.
use super::{
    api::Api, provider::Locator, target::Agent37Target, Agent37Config, Agent37Identity,
    Agent37Machines, ApiKey, MachineSpec,
};
use crate::ports::{IConnection, ProcessEnd, SocketForward};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TEMPLATE: &str = "agent37-base@2026.10.24a";

#[tokio::test]
#[ignore = "makes a billed Agent37 instance; needs AGENT37_API_KEY"]
async fn an_instance_is_made_connected_forwarded_stopped_and_deleted() {
    let key = ApiKey::new(std::env::var("AGENT37_API_KEY").expect("AGENT37_API_KEY")).unwrap();
    let config = Agent37Config::new(key);
    let machines = Agent37Machines::new(config.clone())
        .unwrap()
        .with_budget(Duration::from_secs(240));
    let identity = Agent37Identity::generate("workspace-sdk-live")
        .await
        .unwrap();
    machines
        .register(&identity, "workspace-sdk-live")
        .await
        .unwrap();
    let spec = MachineSpec::new(TEMPLATE, Duration::from_secs(300), BTreeMap::new()).unwrap();
    machines.prepare(&spec).await.unwrap();
    let probe_key = format!("wsdk-live-{}", uuid::Uuid::new_v4().simple());
    let began = std::time::Instant::now();
    let created = machines.create(&spec, &probe_key).await.unwrap();
    eprintln!(
        "created {} in {:?}: {:?}",
        created.instance(),
        began.elapsed(),
        created.admission()
    );
    let id = created.instance().to_owned();
    let outcome = async {
        let again = machines.create(&spec, &probe_key).await.unwrap();
        assert_eq!(again.instance(), id);
        assert!(!again.made());
        machines.ready(&id, created.admission()).await.unwrap();
        let target = Agent37Target {
            api: Api::new(config.clone()).unwrap(),
            identity: Arc::new(identity.clone()),
            user: "node",
            locator: Locator {
                version: 1,
                instance: id.clone(),
                root: "/home/node/work".into(),
                control: "/home/node/.workspace-sdk/live".into(),
                supervisor: "/bin/false".into(),
            },
            private_link: super::idle_slot::IdleSlot::new(super::target::PRIVATE_IDLE),
        };
        // Key changes reach a running instance within about two minutes.
        let local_dir = tempfile::tempdir().unwrap();
        let local = local_dir.path().join("relay.sock");
        let listener = tokio::net::UnixListener::bind(&local).unwrap();
        let mut connected = None;
        for attempt in 0..30 {
            let began = std::time::Instant::now();
            match target.connect(Some(SocketForward::new(&local).unwrap())).await {
                Ok(connection) => {
                    eprintln!("connected on attempt {attempt} in {:?}", began.elapsed());
                    connected = Some(connection);
                    break;
                }
                Err(error) => eprintln!("attempt {attempt}: {error}"),
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        let connection = connected.expect("never connected");
        let began = std::time::Instant::now();
        let ran = connection
            .run("id -un; uname -sm; echo $HOME", None, Duration::from_secs(30))
            .await
            .unwrap();
        eprintln!("run in {:?}: {:?} {:?}", began.elapsed(), ran.output, ran.stderr);
        assert_eq!(ran.end, ProcessEnd::Exited(0));
        let remote = connection.forwarded_socket().unwrap().to_owned();
        eprintln!("forwarded at {remote}");
        let echo = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut line = [0u8; 5];
            stream.read_exact(&mut line).await.unwrap();
            stream.write_all(&line).await.unwrap();
        });
        let through = connection
            .run(
                &format!("printf hello | (command -v nc >/dev/null && nc -U {remote} -q 1 || python3 -c \"import socket,sys;s=socket.socket(socket.AF_UNIX);s.connect('{remote}');s.sendall(b'hello');print(s.recv(5).decode())\")"),
                None,
                Duration::from_secs(30),
            )
            .await
            .unwrap();
        eprintln!("through the forward: {:?} {:?}", through.output, through.stderr);
        let _ = tokio::time::timeout(Duration::from_secs(5), echo).await;
        let mut files = connection.clone().files().await.unwrap();
        files.to_machine.shutdown().await.ok();
        drop(files);
        drop(connection);
        machines.stop(&id).await.unwrap();
        machines.ready(&id, created.admission()).await.unwrap();
        eprintln!("stopped and woken again");
    }
    .await;
    machines.delete(&id).await.unwrap();
    eprintln!("deleted {id}; total {:?}", began.elapsed());
    outcome
}
