use super::{
    filesystem,
    protocol::{SupervisorFault, SupervisorOperation, SupervisorRequest, SupervisorValue},
    run_record, workload,
};
use crate::{
    checkpoint::infrastructure::{cleanup_transfer, export_remote_tree, import_remote_tree},
    runtime::infrastructure::{ensure_private_directory, inspect_resources, write_private_file},
    Error, Result,
};
use std::path::Path;

pub(super) fn handle(request: SupervisorRequest) -> Result<SupervisorValue> {
    if request.version != 1 {
        return Err(Error::Unsupported("supervisor protocol version".into()));
    }
    let context = &request.context;
    if let SupervisorOperation::Initialize { create_project } = request.operation {
        filesystem::initialize(context, create_project)?;
        return Ok(SupervisorValue::Empty);
    }
    if let Err(error) = filesystem::check(context) {
        return if matches!(
            &request.operation,
            SupervisorOperation::TerminalInput { .. }
        ) {
            Ok(SupervisorValue::InputRejected(SupervisorFault::from_error(
                error,
            )))
        } else {
            Err(error)
        };
    }
    let root = Path::new(&context.root);
    let control = Path::new(&context.control);
    match request.operation {
        SupervisorOperation::Initialize { .. } => unreachable!(),
        SupervisorOperation::Ready => Ok(SupervisorValue::Empty),
        SupervisorOperation::ReadFile {
            path,
            offset,
            limit,
        } => Ok(SupervisorValue::FileChunk(filesystem::read_file(
            root, &path, offset, limit,
        )?)),
        SupervisorOperation::WriteFile {
            path,
            offset,
            bytes,
            truncate,
            executable,
        } => {
            filesystem::write_file(root, &path, offset, &bytes, truncate, executable)?;
            Ok(SupervisorValue::Empty)
        }
        SupervisorOperation::Start {
            id,
            execution,
            output_limit_bytes,
        } => Ok(SupervisorValue::Run(workload::start(
            context,
            &id,
            &execution,
            output_limit_bytes,
        )?)),
        SupervisorOperation::StartTerminal {
            id,
            execution,
            output_limit_bytes,
            size,
        } => Ok(SupervisorValue::Run(workload::start_terminal(
            context,
            &id,
            &execution,
            output_limit_bytes,
            size,
        )?)),
        SupervisorOperation::TerminalInput { id, input } => {
            let path = match run_record::run_directory(context, &id, false) {
                Ok(path) => path,
                Err(error) => {
                    return Ok(SupervisorValue::InputRejected(SupervisorFault::from_error(
                        error,
                    )))
                }
            };
            super::terminal_control::request(
                &path,
                SupervisorOperation::TerminalInput { id, input },
            )
        }
        SupervisorOperation::TerminalResize { id, size } => {
            let path = run_record::run_directory(context, &id, false)?;
            super::terminal_control::request(
                &path,
                SupervisorOperation::TerminalResize { id, size },
            )
        }
        SupervisorOperation::Inspect { id } => Ok(SupervisorValue::Run(run_record::inspect(
            &run_record::run_directory(context, &id, false)?,
        )?)),
        SupervisorOperation::Cancel { id } => Ok(SupervisorValue::Run(workload::cancel(
            &run_record::run_directory(context, &id, false)?,
        )?)),
        SupervisorOperation::Output { id, cursor, limit } => {
            Ok(SupervisorValue::Output(run_record::output(
                &run_record::run_directory(context, &id, false)?,
                cursor,
                limit,
            )?))
        }
        SupervisorOperation::StopOwnedRuns => {
            workload::stop_owned(context, false)?;
            Ok(SupervisorValue::Empty)
        }
        SupervisorOperation::DestroyOwnedFiles => {
            workload::stop_owned(context, true)?;
            Ok(SupervisorValue::Empty)
        }
        SupervisorOperation::ExportTree { transfer } => Ok(SupervisorValue::TreeExport(
            export_remote_tree(root, &transfer)?,
        )),
        SupervisorOperation::ImportTree { transfer } => Ok(SupervisorValue::TreeImport(
            import_remote_tree(root, &transfer)?,
        )),
        SupervisorOperation::RemoveTransfer { transfer } => {
            cleanup_transfer(root, &transfer)?;
            Ok(SupervisorValue::Empty)
        }
        SupervisorOperation::EnsurePrivateDirectory { path } => {
            ensure_private_directory(control, &control.join(path.as_str()))?;
            Ok(SupervisorValue::Empty)
        }
        SupervisorOperation::WritePrivateFile { path, bytes } => {
            write_private_file(control, &control.join(path.as_str()), &bytes, 0o600)?;
            Ok(SupervisorValue::Empty)
        }
        SupervisorOperation::InspectResources => {
            Ok(SupervisorValue::Resources(inspect_resources(root)?))
        }
        SupervisorOperation::PrepareRequest { id } => {
            filesystem::prepare_request(context, id)?;
            Ok(SupervisorValue::Empty)
        }
        SupervisorOperation::RemoveRequest { id } => {
            filesystem::remove_request(context, id)?;
            Ok(SupervisorValue::Empty)
        }
    }
}
