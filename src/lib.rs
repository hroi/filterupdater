#![forbid(unsafe_code)]
pub mod aggregate;
pub mod filterclass;
pub mod format;
pub mod irr;

pub(crate) use std::collections::{HashMap, HashSet};

pub type Map<K, V> = HashMap<K, V>;
pub type Set<K> = HashSet<K>;

pub type Prefix = (std::net::IpAddr, u8);
pub type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

pub const CLIENT: &str = env!("CARGO_PKG_NAME");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
