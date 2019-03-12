// https://www.radb.net/support/tutorials/query-options-flags.html
use super::*;

use bufstream::BufStream;
use std::net::ToSocketAddrs;

#[derive(Debug, PartialEq)]
// See ftp://ftp.grnet.gr/pub/net/irrd/irrd-user.pdf - Appendix B
pub enum Reply {
    // Successful query data
    A(String),
    // Successful query, no data
    C,
    // Key not found
    D,
    // Error
    F(String),
}

pub struct RadbClient {
    stream: BufStream<TcpStream>,
    buf: Vec<u8>,
    acks_outstanding: usize,
}

impl RadbClient {
    const CLIENT: &'static str = env!("CARGO_PKG_NAME");
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    pub fn open<S: ToSocketAddrs>(target: S) -> io::Result<Self> {
        let mut err: io::Error = Error::new(Other, "unreachable");
        for sock_addr in target.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(30)) {
                Ok(conn) => {
                    let mut client = RadbClient {
                        stream: BufStream::new(conn),
                        buf: Vec::with_capacity(4096),
                        acks_outstanding: 0,
                    };
                    writeln!(client.stream, "!!\n!n{}-{}", Self::CLIENT, Self::VERSION)?;
                    client.acks_outstanding += 1;
                    // radb,afrinic,ripe,ripe-nonauth,bell,apnic,nttcom,altdb,panix,risq,
                    // nestegg,level3,reach,aoltw,openface,arin,easynet,jpirr,host,rgnet,
                    // rogers,bboi,tc,canarie
                    writeln!(client.stream, "!sripe,apnic,arin")?;
                    client.acks_outstanding += 1;
                    return Ok(client);
                }
                Err(e) => err = e,
            }
        }
        Err(err)
    }

    fn read_reply(&mut self) -> io::Result<Reply> {
        self.buf.clear();
        loop {
            self.stream.read_until(b'\n', &mut self.buf)?;
            let ret = match self.buf.get(0) {
                Some(b'A') => {
                    let len_bytes = &self.buf[1..self.buf.len() - 1];
                    let alen: usize = std::str::from_utf8(len_bytes)
                        .map_err(|e| Error::new(InvalidData, e))
                        .and_then(|s| s.parse().map_err(|e| Error::new(InvalidData, e)))?;
                    self.buf.resize(alen, 0);
                    self.stream.read_exact(&mut self.buf)?;
                    let content = String::from_utf8(self.buf.clone())
                        .map_err(|e| Error::new(InvalidData, e))?;
                    Ok(Reply::A(content))
                }
                Some(b'C') => Ok(Reply::C),
                Some(b'D') => Ok(Reply::D),
                Some(b'F') => Err(Error::new(
                    InvalidData,
                    String::from_utf8_lossy(&self.buf[1..self.buf.len() - 1]),
                )),
                Some(code) => Err(Error::new(InvalidData, format!("unknown code {}", code))),
                None => Err(Error::new(UnexpectedEof, "empty reply")),
            };
            if self.acks_outstanding > 0 {
                if let Ok(Reply::C) = ret {
                    self.acks_outstanding -= 1;
                    self.buf.clear();
                } else {
                    return Err(Error::new(Other, "protocol error"));
                }
            } else {
                return ret;
            }
        }
    }

    pub fn resolve_as_sets<'a, I: Iterator<Item = &'a String> + Clone>(
        &mut self,
        sets: I,
    ) -> io::Result<HashMap<&'a str, Vec<u32>>> {
        let mut ret: HashMap<&str, Vec<u32>> = HashMap::new();
        for set in sets.clone() {
            writeln!(self.stream, "!i{},1", set)?;
        }

        self.stream.flush()?;
        for set in sets.clone() {
            let autnums = ret.entry(set).or_insert_with(|| vec![]);
            while let Reply::A(reply) = self.read_reply()? {
                for autnum in reply.split_whitespace().map(|s| parse_autnum(s)) {
                    let autnum = autnum?;
                    if autnum == 23456 {
                        // 4-byte asn placeholder - skip!
                        continue;
                    }
                    autnums.push(autnum);
                }
            }
        }
        Ok(ret)
    }

    pub fn resolve_autnums<'a, I: Iterator<Item = &'a u32> + Clone>(
        &mut self,
        autnums: I,
    ) -> io::Result<HashMap<u32, Vec<Prefix>>> {
        for autnum in autnums.clone() {
            writeln!(self.stream, "!gas{}", autnum)?;
            writeln!(self.stream, "!6as{}", autnum)?;
        }
        let mut ret = HashMap::new();

        self.stream.flush()?;

        for autnum in autnums.clone() {
            let prefixlist = ret.entry(*autnum).or_insert_with(|| vec![]);
            for family in &[4, 6] {
                while let Reply::A(reply) = self.read_reply()? {
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
