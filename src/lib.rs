pub mod aggregate;
pub mod radb;
pub mod format;

#[cfg(feature = "hashbrown")]
pub(crate) use hashbrown::{HashMap, HashSet};
#[cfg(not(feature = "hashbrown"))]
pub(crate) use std::collections::{HashMap, HashSet};

pub type Map<K,V> = HashMap<K,V>;
pub type Set<K> = HashSet<K>;

pub type Prefix = (std::net::IpAddr, u8);
pub type AppResult<T> = Result<T, Box<std::error::Error>>;
