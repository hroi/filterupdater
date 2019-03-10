// https://www.radb.net/support/tutorials/query-options-flags.html
use bufstream::BufStream;
use std::net::ToSocketAddrs;

use super::*;

#[derive(Debug, PartialEq)]
pub enum Reply {
    A(String),
    C,
    D,
}

pub struct RadbClient {
    stream: BufStream<TcpStream>,
    init_done: bool,
}

impl RadbClient {
    const CLIENT: &'static str = env!("CARGO_PKG_NAME");
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    pub fn open<S: ToSocketAddrs>(target: S) -> io::Result<Self> {
        let mut err: io::Error = io::Error::new(io::ErrorKind::Other, "unreachable");
        for sock_addr in target.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(30)) {
                Ok(conn) => {
                    let mut client = RadbClient {
                        stream: BufStream::new(conn),
                        init_done: false,
                    };
                    writeln!(client.stream, "!!\n!n{}-{}", Self::CLIENT, Self::VERSION)?;
                    return Ok(client);
                }
                Err(e) => err = e,
            }
        }
        Err(err)
    }

    fn read_reply(&mut self) -> io::Result<Reply> {
        let mut buf = Vec::<u8>::new();
        loop {
            self.stream.read_until(b'\n', &mut buf)?;
            let ret = match buf.get(0) {
                Some(b'A') => {
                    let decimal_length = std::str::from_utf8(&buf[1..buf.len() - 1])
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;;
                    let alen: usize = decimal_length
                        .parse()
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    buf.resize(alen, 0);
                    self.stream.read_exact(&mut buf)?;
                    let content = String::from_utf8(buf.clone())
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    Ok(Reply::A(content))
                }
                Some(b'C') => Ok(Reply::C),
                Some(b'D') => Ok(Reply::D),
                Some(code) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown code {}", code),
                )),
                None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "empty reply")),
            };
            if self.init_done {
                return ret;
            } else if let Ok(Reply::C) = ret {
                buf.clear();
                self.init_done = true;
            } else {
                return Err(io::Error::new(io::ErrorKind::Other, "protocol error"));
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
        input[2..]
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, input))
    }
}

pub fn parse_prefix(input: &str) -> io::Result<Prefix> {
    let mut elems = input.split('/');
    if let (Some(ip), Some(masklen), None) = (elems.next(), elems.next(), elems.next()) {
        if let (Ok(ip), Ok(masklen)) = (ip.parse(), masklen.parse()) {
            return Ok((ip, masklen));
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, input))
}
