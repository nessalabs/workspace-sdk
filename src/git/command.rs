use crate::CommandSpec;

/// Preserve target Git configuration while excluding inherited repository and
/// inline-config redirects. Mandatory Git write locks remain enabled.
pub(super) fn git_command(program: &str) -> CommandSpec {
    let mut command = CommandSpec::new(program);
    command.env_remove = [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_REPLACE_REF_BASE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_SHALLOW_FILE",
        "GIT_GRAFT_FILE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_PREFIX",
        "GIT_SUPER_PREFIX",
        "GIT_INTERNAL_SUPER_PREFIX",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    for key in [
        "GIT_CONFIG_COUNT",
        "GIT_TERMINAL_PROMPT",
        "GIT_OPTIONAL_LOCKS",
    ] {
        command.env.insert(key.into(), "0".into());
    }
    command
}
