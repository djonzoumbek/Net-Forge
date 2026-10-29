//! Tauri commands: thin layer that validates input and delegates to the domain
//! modules. Each sub-module groups the commands of one domain.

pub mod known_hosts;
pub mod profiles;
pub mod sessions;
