use std::collections::HashMap;
use std::io::prelude::*;
use std::io::{self, Error, ErrorKind::*};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::{AppResult, Prefix};
use bufstream::BufStream;

// Docs:
// https://www.radb.net/support/tutorials/query-options-flags.html
// ftp://ftp.grnet.gr/pub/net/irrd/irrd-user.pdf - Appendix B

pub struct RadbClient {
    stream: BufStream<TcpStream>,
    buf: Vec<u8>,
}

const TIMEOUT: Duration = Duration::from_secs(30);

impl RadbClient {
    const CLIENT: &'static str = env!("CARGO_PKG_NAME");
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const GIT_HASH: Option<&'static str> = option_env!("GIT_HASH");

    pub fn open<S: ToSocketAddrs>(target: S, sources: &str) -> AppResult<Self> {
        let mut err: io::Error = Error::new(Other, "no address for host");
        for sock_addr in target.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sock_addr, TIMEOUT) {
                Ok(conn) => {
                    conn.set_read_timeout(Some(TIMEOUT))?;
                    conn.set_write_timeout(Some(TIMEOUT))?;
                    let mut client = RadbClient {
                        stream: BufStream::new(conn),
                        buf: Vec::with_capacity(4096),
                    };
                    if let Some(hash) = Self::GIT_HASH {
                        writeln!(
                            client.stream,
                            "!!\n!n{}-{}-{}",
                            Self::CLIENT,
                            Self::VERSION,
                            &hash[..8]
                        )
                    } else {
                        writeln!(client.stream, "!!\n!n{}-{}", Self::CLIENT, Self::VERSION,)
                    }?;
                    // radb,afrinic,ripe,ripe-nonauth,bell,apnic,nttcom,altdb,panix,risq,
                    // nestegg,level3,reach,aoltw,openface,arin,easynet,jpirr,host,rgnet,
                    // rogers,bboi,tc,canarie
                    writeln!(client.stream, "!s{}", sources)?;
                    return Ok(client);
                }
                Err(e) => err = e,
            }
        }
        Err(err.into())
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.stream.get_ref().peer_addr()
    }

    fn read_reply(&mut self) -> AppResult<Option<String>> {
        let mut reply: Option<String> = None;
        loop {
            self.buf.clear();
            let len = self.stream.read_until(b'\n', &mut self.buf)? - 1;
            match char::from(self.buf[0]) {
                'A' => {
                    let len_bytes = &self.buf[1..len];
                    let content_len: usize = std::str::from_utf8(len_bytes)
                        .map_err(|e| Error::new(InvalidData, e))
                        .and_then(|s| s.parse().map_err(|e| Error::new(InvalidData, e)))?;
                    self.buf.resize(content_len, 0);
                    self.stream.read_exact(&mut self.buf)?;
                    let content = String::from_utf8(self.buf.clone())
                        .map_err(|e| Error::new(InvalidData, e))?;
                    reply = Some(content);
                }
                'C' => {
                    if reply.is_some() {
                        return Ok(reply);
                    }
                }
                'D' => {
                    return Ok(None);
                }
                'F' => {
                    return Err(
                        Error::new(Other, String::from_utf8_lossy(&self.buf[1..len])).into(),
                    );
                }
                code => Err(Error::new(InvalidData, format!("unknown code {:?}", code)))?,
            };
        }
    }

    pub fn resolve_as_sets<'a, I: Iterator<Item = &'a &'a str> + Clone>(
        &mut self,
        sets: I,
    ) -> AppResult<HashMap<&'a str, Vec<u32>>> {
        let mut ret: HashMap<&str, Vec<u32>> = HashMap::new();
        for set in sets.clone() {
            writeln!(self.stream, "!i{},1", set)?;
        }
        self.stream.flush()?;
        for set in sets.clone() {
            let autnums = ret.entry(set).or_insert_with(|| vec![]);
            if let Some(reply) = self.read_reply()? {
                for autnum in reply.split_whitespace().map(|s| parse_autnum(s)) {
                    let autnum = autnum?;
                    match autnum {
                        // invalid as'es
                        0 | 23_456 | 64_496...65_535 | 4_200_000_000...4_294_967_294 => continue,
                        valid => autnums.push(valid),
                    }
                }
            }
        }
        Ok(ret)
    }

    pub fn resolve_autnums<'a, I: Iterator<Item = &'a u32> + Clone>(
        &mut self,
        autnums: I,
    ) -> AppResult<HashMap<u32, Vec<Prefix>>> {
        for autnum in autnums.clone() {
            writeln!(self.stream, "!gas{}", autnum)?;
            writeln!(self.stream, "!6as{}", autnum)?;
        }
        let mut ret = HashMap::new();

        self.stream.flush()?;

        for autnum in autnums.clone() {
            let prefixlist = ret.entry(*autnum).or_insert_with(|| vec![]);
            for family in &[4, 6] {
                if let Some(reply) = self.read_reply()? {
                    for elem in reply.split_whitespace() {
                        let prefix = parse_prefix(elem)?;
                        if family == &4 {
                            assert!(prefix.0.is_ipv4());
                        } else {
                            assert!(prefix.0.is_ipv6());
                        }
                        prefixlist.push(prefix);
                    }
                }
            }
        }

        Ok(ret)
    }
}

impl Drop for RadbClient {
    fn drop(&mut self) {
        self.stream.write_all(b"!q\n").ok();
    }
}

pub fn parse_autnum(input: &str) -> io::Result<u32> {
    if input.starts_with("AS") {
        input[2..].parse().map_err(|e| Error::new(InvalidData, e))
    } else {
        Err(Error::new(InvalidData, input))
    }
}

pub fn parse_prefix(input: &str) -> io::Result<Prefix> {
    let mut elems = input.split('/');
    if let (Some(ip), Some(masklen), None) = (elems.next(), elems.next(), elems.next()) {
        if let (Ok(ip), Ok(masklen)) = (ip.parse(), masklen.parse()) {
            return Ok((ip, masklen));
        }
    }
    Err(Error::new(InvalidData, input))
}
