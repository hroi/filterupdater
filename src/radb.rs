// https://www.radb.net/support/tutorials/query-options-flags.html
use std::fmt::Write as _;

use super::*;

#[derive(Debug, PartialEq)]
pub enum Reply {
    A(String),
    C,
    D,
}

pub fn read_reply<R: BufRead>(input: &mut R) -> Result<Reply, io::Error> {
    let mut buf = Vec::<u8>::new();
    input.read_until(b'\n', &mut buf)?;
    match buf.get(0) {
        Some(b'A') => {
            let decimal_length = std::str::from_utf8(&buf[1..buf.len() - 1])
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;;
            let alen: usize = decimal_length
                .parse()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            buf.resize(alen, 0);
            input.read_exact(&mut buf)?;
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

pub fn resolve_as_sets(
    conn: &mut TcpStream,
    sets: &HashSet<String>,
) -> io::Result<HashMap<String, Vec<u32>>> {
    //let mut query = format!("!i{},1\n", q);
    let mut ret: HashMap<String, Vec<u32>> = HashMap::new();
    let mut query = String::new();
    for set in sets.iter() {
        writeln!(&mut query, "!i{},1", set).unwrap();
    }
    conn.write_all(query.as_bytes())?;

    let mut bufreader = io::BufReader::new(conn);
    for set in sets.iter() {
        let autnums = ret.entry(set.to_string()).or_insert_with(|| vec![]);
        while let Reply::A(reply) = read_reply(&mut bufreader)? {
            for autnum in reply.split_whitespace().map(|s| parse_autnum(s)) {
                let autnum = autnum?;
                autnums.push(autnum);
            }
        }
    }
    Ok(ret)
}

pub fn resolve_autnums(
    conn: &mut TcpStream,
    autnums: &HashSet<u32>,
) -> io::Result<HashMap<u32, Vec<Prefix>>> {
    let mut query = String::new();
    for autnum in autnums.iter() {
        writeln!(&mut query, "!gas{}", autnum).unwrap();
        writeln!(&mut query, "!6as{}", autnum).unwrap();
    }
    conn.write_all(query.as_bytes())?;
    let mut bufreader = io::BufReader::new(conn);
    let mut ret = HashMap::new();

    for autnum in autnums.iter() {
        let prefixlist = ret.entry(*autnum).or_insert_with(|| vec![]);
        for family in &[4, 6] {
            while let Reply::A(reply) = read_reply(&mut bufreader)? {
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
