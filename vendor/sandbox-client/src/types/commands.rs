use super::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CommandResult {
    Completed(CommandResponse),
    Started(CommandStartedResponse),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandStatusQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail_bytes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_offset: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandStartedResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub success: bool,
    pub process_id: i64,
    pub pid: i64,
    pub command: String,
    pub cwd: Option<String>,
    pub started_at: String,
    pub log_path: Option<String>,
    pub err_log_path: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for CommandStartedResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandStartedResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("success", &self.success)
            .field("process_id", &self.process_id)
            .field("pid", &self.pid)
            .field("command_len", &self.command.len())
            .field("cwd_len", &self.cwd.as_ref().map(String::len))
            .field("started_at_len", &self.started_at.len())
            .field("log_path_len", &self.log_path.as_ref().map(String::len))
            .field(
                "err_log_path_len",
                &self.err_log_path.as_ref().map(String::len),
            )
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandStatusResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub success: bool,
    pub process_id: i64,
    pub pid: Option<i64>,
    pub status: String,
    pub known: Option<bool>,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: Option<bool>,
    pub stderr_truncated: Option<bool>,
    pub log_path: Option<String>,
    pub err_log_path: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for CommandStatusResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandStatusResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("success", &self.success)
            .field("process_id", &self.process_id)
            .field("pid", &self.pid)
            .field("status", &self.status)
            .field("known", &self.known)
            .field("running", &self.running)
            .field("exit_code", &self.exit_code)
            .field("signal_len", &self.signal.as_ref().map(String::len))
            .field("command_len", &self.command.as_ref().map(String::len))
            .field("cwd_len", &self.cwd.as_ref().map(String::len))
            .field("started_at_len", &self.started_at.as_ref().map(String::len))
            .field(
                "finished_at_len",
                &self.finished_at.as_ref().map(String::len),
            )
            .field("stdout_len", &self.stdout.len())
            .field("stderr_len", &self.stderr.len())
            .field("stdout_truncated", &self.stdout_truncated)
            .field("stderr_truncated", &self.stderr_truncated)
            .field("log_path_len", &self.log_path.as_ref().map(String::len))
            .field(
                "err_log_path_len",
                &self.err_log_path.as_ref().map(String::len),
            )
            .field("extra_count", &self.extra.len())
            .finish()
    }
}
