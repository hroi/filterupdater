use std::io::prelude::*;
use std::io::{self, Error, ErrorKind::*};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::{AppResult, Map, Prefix, Set};
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
                    if let Some(hash) = crate::GIT_HASH {
                        writeln!(
                            client.stream,
                            "!!\n!n{}-{}-{}",
                            crate::CLIENT,
                            crate::VERSION,
                            &hash[..8]
                        )
                    } else {
                        writeln!(client.stream, "!!\n!n{}-{}", crate::CLIENT, crate::VERSION,)
                    }?;
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

    pub fn resolve_as_sets<'a>(
        &mut self,
        sets: &Set<&'a str>,
    ) -> AppResult<Map<&'a str, Vec<u32>>> {
        let iter = sets.iter();
        let mut ret: Map<&str, Vec<u32>> = Map::new();
        for set in iter.clone() {
            writeln!(self.stream, "!i{},1", set)?;
        }
        self.stream.flush()?;
        for set in iter.clone() {
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

    pub fn resolve_rt_sets<'a>(
        &mut self,
        sets: &'a Set<&str>,
    ) -> AppResult<Map<&'a str, Vec<Prefix>>> {
        let iter = sets.iter();
        let mut ret: Map<&str, Vec<Prefix>> = Map::new();
        for set in iter.clone() {
            writeln!(self.stream, "!i{},1", set)?;
        }
        self.stream.flush()?;
        for set in iter.clone() {
            let prefixlist = ret.entry(*set).or_insert_with(|| vec![]);
            if let Some(reply) = self.read_reply()? {
                for elem in reply.split_whitespace() {
                    let prefix = parse_prefix(elem)?;
                    prefixlist.push(prefix);
                }
            }
        }
        Ok(ret)
    }

    pub fn resolve_autnums(&mut self, autnums: &Set<u32>) -> AppResult<Map<u32, Vec<Prefix>>> {
        let iter = autnums.iter();
        for autnum in iter.clone() {
            writeln!(self.stream, "!gas{}", autnum)?;
            writeln!(self.stream, "!6as{}", autnum)?;
        }
        let mut ret = Map::new();

        self.stream.flush()?;

        for autnum in iter.clone() {
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
