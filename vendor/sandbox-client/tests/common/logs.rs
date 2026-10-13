use std::sync::{Arc, Mutex};
#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Logs {
    pub fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync {
        let logs = self.clone();
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap())
            .without_time()
            .with_ansi(false)
            .with_writer(move || logs.clone())
            .finish()
    }
    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

/// Run logging assertions in an isolated application process. This avoids global
/// tracing callsite/subscriber caches being affected by parallel unrelated tests,
/// and exercises actual RUST_LOG configuration without mutating a shared env.
pub fn isolated(test_name: &str, filter: &str) -> bool {
    let case = format!("{test_name}:{filter}");
    if let Ok(active) = std::env::var("BOX_CLIENT_LOG_TEST_CASE") {
        return active != case;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env("BOX_CLIENT_LOG_TEST_CASE", case)
        .env("RUST_LOG", filter)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}
