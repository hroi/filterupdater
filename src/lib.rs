#![forbid(unsafe_code)]
pub mod aggregate;
pub mod filterclass;
pub mod format;
pub mod radb;

#[cfg(feature = "hashbrown")]
pub(crate) use hashbrown::{HashMap, HashSet};
#[cfg(not(feature = "hashbrown"))]
pub(crate) use std::collections::{HashMap, HashSet};

pub type Map<K, V> = HashMap<K, V>;
pub type Set<K> = HashSet<K>;

pub type Prefix = (std::net::IpAddr, u8);
pub type AppResult<T> = Result<T, Box<std::error::Error>>;
