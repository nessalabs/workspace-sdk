mod locator;
mod managed_backend;
mod protocol;
mod target;
#[cfg(any(feature = "space", feature = "agent37"))]
pub(in crate::providers) use locator::decode_reference;
#[cfg(any(feature = "space", feature = "agent37"))]
pub(in crate::providers) use locator::{control_beside, require_same};
pub(in crate::providers) use locator::{decode, Locator};
pub(in crate::providers) use managed_backend::ManagedBackend;
pub(in crate::providers) use protocol::{
    quote, require_no_create_options, require_no_resources, response, validate_remote_path,
    MAX_CHUNK,
};
pub(in crate::providers) use target::ITarget;
