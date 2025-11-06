// Expose modules for integration testing
pub mod db;
pub mod errors;
pub mod service;
pub mod types;
pub mod wg;

#[cfg(target_os = "linux")]
pub mod dnsfs;

pub mod api;
pub mod http;
pub mod logging;
#[cfg(target_os = "linux")]
pub mod netns;
pub mod run;
