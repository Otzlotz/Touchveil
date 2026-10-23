//! Shared types of touchveil-daemon and touchveil-overlay.
//!
//! - [`edge`]: edge or corner the overlay is pulled open from, and the rotation.
//! - [`protocol`]: line-based protocol over the Unix socket.
//! - [`paths`]: shared paths and the instance lock.
//!
//! All coordinates are normalized (0..1) and already rotated to the screen orientation
//! (see [`rotate`]); (0, 0) is top left.

pub mod edge;
pub mod paths;
pub mod protocol;

pub use edge::{check_rotation, rotate, Edge, Side};
pub use paths::{config_candidates, default_socket, find_config, lock_instance};
pub use protocol::{ToDaemon, ToOverlay};
