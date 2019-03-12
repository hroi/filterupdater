// https://www.radb.net/support/tutorials/query-options-flags.html
use super::*;

use bufstream::BufStream;
use std::net::ToSocketAddrs;

#[derive(Debug, PartialEq)]
// See ftp://ftp.grnet.gr/pub/net/irrd/irrd-user.pdf - Appendix B
pub enum Reply {
    // Successful query data
    A(String),
    // Key not found
    None,
}

pub struct RadbClient {
    stream: BufStream<TcpStream>,
    buf: Vec<u8>,
}

impl RadbClient {
    const CLIENT: &'static str = env!("CARGO_PKG_NAME");
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    pub fn open<S: ToSocketAddrs>(target: S) -> AppResult<Self> {
        let mut err: io::Error = Error::new(Other, "unreachable");
        for sock_addr in target.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(30)) {
                Ok(conn) => {
                    let mut client = RadbClient {
                        stream: BufStream::new(conn),
                        buf: Vec::with_capacity(4096),
                    };
                    writeln!(client.stream, "!!\n!n{}-{}", Self::CLIENT, Self::VERSION)?;
                    // radb,afrinic,ripe,ripe-nonauth,bell,apnic,nttcom,altdb,panix,risq,
                    // nestegg,level3,reach,aoltw,openface,arin,easynet,jpirr,host,rgnet,
                    // rogers,bboi,tc,canarie
                    writeln!(client.stream, "!sripe,apnic,arin")?;
                    return Ok(client);
                }
                Err(e) => err = e,
            }
        }
        Err(err.into())
    }

    fn read_reply(&mut self) -> AppResult<Reply> {
        let mut reply: Option<String> = None;
        loop {
            self.buf.clear();
            let len = self.stream.read_until(b'\n', &mut self.buf)? - 1;
            match self.buf.get(0) {
                Some(b'A') => {
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
                Some(b'C') => {
                    if let Some(reply) = reply {
                        return Ok(Reply::A(reply));
                    }
                }
                Some(b'D') => {
                    return Ok(Reply::None);
                }
                Some(b'F') => {
                    return Err(
                        Error::new(Other, String::from_utf8_lossy(&self.buf[1..len])).into(),
                    );
                }
                Some(code) => Err(Error::new(
                    InvalidData,
                    format!("unknown code {:?}", char::from(*code)),
                ))?,
                None => Err(Error::new(UnexpectedEof, "empty reply"))?,
            };
        }
    }

    pub fn resolve_as_sets<'a, I: Iterator<Item = &'a String> + Clone>(
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
            if let Reply::A(reply) = self.read_reply()? {
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
                if let Reply::A(reply) = self.read_reply()? {
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
