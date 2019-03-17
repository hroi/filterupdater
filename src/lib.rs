pub mod aggregate;
pub mod radb;
pub mod format;

pub type Prefix = (std::net::IpAddr, u8);
pub type AppResult<T> = Result<T, Box<std::error::Error>>;
