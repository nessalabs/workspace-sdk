use super::{
    dispatcher::handle,
    filesystem::open_request,
    protocol::{SupervisorContext, SupervisorRequest, SupervisorResponse},
};
use crate::{Error, Result};
use std::{
    env::{
        self,
        consts::{ARCH, OS},
    },
    io::{self, Read},
    path::Path,
};

/// Run one stdin or private-file protocol request in the single-threaded executable.
/// Private request files are opened without following symlinks and unlinked before parsing.
///
/// # Safety
/// Call only as the entry point of a dedicated process before creating any
/// threads. Launch requests fork and detach that process; this is not an SDK
/// API for dispatching requests inside an application or an async runtime.
pub unsafe fn run_cli() -> i32 {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args == ["--version"] {
        println!("workspace-supervisor 1 {} {}", OS, ARCH);
        return 0;
    }
    let input: Result<(Box<dyn Read>, Option<SupervisorContext>)> = if args.is_empty() {
        Ok((Box::new(io::stdin()), None))
    } else if args.len() == 2 && args[0] == "--request-file" {
        (move || {
            let (file, context) = open_request(Path::new(&args[1]))?;
            Ok((Box::new(file) as Box<dyn Read>, Some(context)))
        })()
    } else {
        Err(Error::InvalidInput(
            "expected stdin request, --request-file PATH, or --version".into(),
        ))
    };
    let result = input.and_then(|(input, context)| {
        let mut bytes = Vec::new();
        input.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(Error::InvalidInput(
                "supervisor request exceeds 32 MiB".into(),
            ));
        }
        let request: SupervisorRequest = serde_json::from_slice(&bytes)
            .map_err(|_| Error::InvalidInput("invalid supervisor request schema".into()))?;
        if context.is_some_and(|context| context != request.context) {
            return Err(Error::Conflict(
                "request context does not match its private file owner".into(),
            ));
        }
        handle(request)
    });
    match serde_json::to_string(&SupervisorResponse::from_result(result)) {
        Ok(response) => {
            println!("{response}");
            0
        }
        Err(_) => 1,
    }
}
