use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::io;
use std::io::prelude::*;
use std::net::IpAddr;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::str::FromStr;
use std::time::Duration;

mod aggregate;
use aggregate::*;

const WHOIS_HOST: &'static str = "whois.radb.net:43";

#[derive(Debug, PartialEq)]
pub enum Reply {
    A(String),
    C,
    D,
}

pub type Prefix = (IpAddr, u8);

fn read_reply<R: BufRead>(input: &mut R) -> Result<Reply, io::Error> {
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

fn parse_autnum(input: &str) -> io::Result<u32> {
    if input.starts_with("AS") {
        input[2..]
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, input))
    }
}

fn parse_prefix(input: &str) -> io::Result<Prefix> {
    let mut elems = input.split('/');
    if let (Some(ip), Some(masklen), None) = (elems.next(), elems.next(), elems.next()) {
        if let (Ok(ip), Ok(masklen)) = (ip.parse(), masklen.parse()) {
            return Ok((ip, masklen));
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, input))
}

fn resolve_as_sets(
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

fn resolve_autnums(
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

#[derive(PartialEq, Eq, Hash)]
enum Query {
    Autnum(u32),
    AsSet(String),
}

impl FromStr for Query {
    type Err = io::Error;
    fn from_str(s: &str) -> io::Result<Query> {
        if s.starts_with("AS") {
            if let Ok(autnum) = parse_autnum(s) {
                Ok(Query::Autnum(autnum))
            } else {
                Ok(Query::AsSet(s.to_string()))
            }
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidInput, s))
        }
    }
}

fn main() -> io::Result<()> {
    let mut q_sets: HashSet<String> = Default::default();
    let mut q_autnums: HashSet<u32> = Default::default();

    let mut do_agg = false;

    let mut queries: HashSet<Query> = Default::default();
    for arg in std::env::args()
        .skip(1)
    {
        if arg == "-a" {
            do_agg = true;
            continue;
        }
        let query: Query = arg.parse()?;
        queries.insert(query);
    }
        // .map(|arg| arg.parse::<Query>())
        // .collect();
    //let queries = queries?;

    for q in queries.iter() {
        match q {
            Query::Autnum(autnum) => q_autnums.insert(*autnum),
            Query::AsSet(set) => q_sets.insert(set.clone()),
        };
    }

    let sock_addr = WHOIS_HOST.to_socket_addrs()?.next().unwrap();
    let mut conn = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(30))?;
    conn.write_all(b"!!\n")?;

    let as_sets: HashMap<String, Vec<u32>> = resolve_as_sets(&mut conn, &q_sets)?;
    for (_as_set, autnums) in as_sets.iter() {
        q_autnums.extend(autnums.iter());
    }
    let autnums: HashMap<u32, Vec<Prefix>> = resolve_autnums(&mut conn, &q_autnums)?;

    conn.write_all(b"!q\n")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for q in queries.iter() {
        let mut prefixes = HashSet::<Prefix>::new();
        match q {
            Query::AsSet(setname) => {
                writeln!(out, "{}", setname)?;
                for autnum in &as_sets[setname] {
                    prefixes.extend(autnums[autnum].iter());
                }
            }
            Query::Autnum(autnum) => {
                writeln!(out, "AS{}", autnum)?;
                prefixes.extend(autnums[&autnum].iter());
            }
        };
        let mut prefixes: Vec<&Prefix> = prefixes.iter().collect();
        prefixes.sort();
        if do_agg {
            let mut aggregated = aggregate(&prefixes[..]);
            aggregated.sort();
            for entry in aggregated {
                writeln!(out, "\t{}", entry)?;
            }
        } else {
            for (ip, masklen) in prefixes.iter() {
                writeln!(out, "\t{}/{}", ip, masklen)?;
            }
        }
    }
    Ok(())
}
