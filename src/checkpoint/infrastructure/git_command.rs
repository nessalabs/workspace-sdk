use super::file_tree::*;
use super::git_checkpoint_engine::GitCheckpointEngine;
use crate::{Error, Result};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

impl GitCheckpointEngine {
    pub(super) fn command(&self, repo: Option<&Path>) -> Command {
        let mut command = Command::new(&self.git_executable);
        // A controller launched by Git may inherit alternate object/index paths
        // or inline config. None may redirect the private snapshot repository.
        for (key, _) in env::vars_os() {
            if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
                command.env_remove(key);
            }
        }
        // Ignore global configuration, source filters, hooks and signing. Raw
        // hash-object/update-index preserve bytes irrespective of .gitattributes.
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Workspace SDK")
            .env("GIT_AUTHOR_EMAIL", "checkpoint@workspace.invalid")
            .env("GIT_COMMITTER_NAME", "Workspace SDK")
            .env("GIT_COMMITTER_EMAIL", "checkpoint@workspace.invalid")
            .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.autocrlf=false",
                "-c",
                "core.symlinks=true",
                "-c",
                "protocol.file.allow=always",
            ]);
        if let Some(repo) = repo {
            command.arg("-C").arg(repo);
        }
        command
    }

    pub(super) fn git_raw<I, S>(
        &self,
        repo: Option<&Path>,
        arguments: I,
        input: Option<&[u8]>,
    ) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command(repo);
        command
            .args(arguments)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        if let Some(bytes) = input {
            child.stdin.take().unwrap().write_all(bytes)?;
        }
        Ok(child.wait_with_output()?)
    }

    pub(super) fn git<I, S>(
        &self,
        repo: Option<&Path>,
        arguments: I,
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let result = self.git_raw(repo, arguments, input)?;
        if !result.status.success() {
            return Err(Error::CommandFailed {
                message: format!(
                    "checkpoint Git operation failed: {}",
                    String::from_utf8_lossy(&result.stderr)
                        .chars()
                        .take(2000)
                        .collect::<String>()
                ),
            });
        }
        Ok(result.stdout)
    }

    pub(super) fn git_text<I, S>(
        &self,
        repo: &Path,
        arguments: I,
        input: Option<&[u8]>,
    ) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        String::from_utf8(self.git(Some(repo), arguments, input)?)
            .map(|s| s.trim().to_owned())
            .map_err(|_| Error::State("Git returned non-UTF-8 metadata".into()))
    }

    pub(super) fn init_repo(&self, repo: &Path) -> Result<()> {
        fs::create_dir_all(repo)?;
        self.git(
            None,
            [
                OsStr::new("init"),
                OsStr::new("--quiet"),
                OsStr::new("--template="),
                repo.as_os_str(),
            ],
            None,
        )?;
        // Raw snapshots must not be transformed by source-controlled attributes.
        // Leave built-in merge selection intact (text versus binary).
        fs::create_dir_all(repo.join(".git/info"))?;
        fs::write(
            repo.join(".git/info/attributes"),
            b"* -text -filter -ident -working-tree-encoding\n",
        )?;
        Ok(())
    }

    pub(super) fn commit_tree(
        &self,
        repo: &Path,
        tree: &Tree,
        parent: Option<&str>,
    ) -> Result<String> {
        self.git(Some(repo), ["read-tree", "--empty"], None)?;
        let mut index = vec![];
        for file in tree.files.values() {
            let oid = self.git_text(repo, ["hash-object", "-w", "--stdin"], Some(&file.bytes))?;
            index.extend_from_slice(
                format!("{} {}\t{}\0", file.entry.mode, oid, file.entry.path).as_bytes(),
            );
        }
        self.git(
            Some(repo),
            ["update-index", "-z", "--index-info"],
            Some(&index),
        )?;
        let tree_oid = self.git_text(repo, ["write-tree"], None)?;
        let mut arguments = vec!["commit-tree", &tree_oid];
        if let Some(parent) = parent {
            arguments.extend(["-p", parent]);
        }
        self.git_text(repo, arguments, Some(b"Workspace file checkpoint\n"))
    }
}
