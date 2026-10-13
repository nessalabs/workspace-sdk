mod locator;
mod managed_backend;
mod protocol;
mod target;
pub(in crate::providers) use locator::{decode, Locator};
pub(in crate::providers) use managed_backend::ManagedBackend;
pub(in crate::providers) use protocol::{
    quote, require_no_resources, response, validate_remote_path, MAX_CHUNK,
};
pub(in crate::providers) use target::ITarget;
