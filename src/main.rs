use io::{Error, ErrorKind::*};
use std::collections::{HashMap, HashSet};
use std::io;
use std::io::prelude::*;
use std::net::IpAddr;
use std::net::TcpStream;
use std::str::FromStr;
use std::time::Duration;

mod aggregate;
use aggregate::*;

pub mod radb;
use radb::*;

const WHOIS_HOST: &str = "whois.radb.net:43";

pub type Prefix = (IpAddr, u8);

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
            Err(Error::new(InvalidInput, s))
        }
    }
}

fn main() -> io::Result<()> {
    let mut q_sets: HashSet<String> = Default::default();
    let mut q_autnums: HashSet<u32> = Default::default();

    let mut do_agg = false;

    let mut queries: HashSet<Query> = Default::default();
    let mut args = std::env::args();
    let progname = args.next().unwrap();
    for arg in args {
        if arg == "-a" {
            do_agg = true;
            continue;
        }
        let query: Query = arg.parse()?;
        queries.insert(query);
    }

    if queries.is_empty() {
        eprintln!("Usage: {} [-a] expr ...", progname);
        std::process::exit(1);
    }

    for q in queries.iter() {
        match q {
            Query::Autnum(autnum) => q_autnums.insert(*autnum),
            Query::AsSet(set) => q_sets.insert(set.clone()),
        };
    }

    let mut client = RadbClient::open(WHOIS_HOST)?;

    let as_sets: HashMap<&str, Vec<u32>> = client.resolve_as_sets(q_sets.iter())?;
    for (_as_set, autnums) in as_sets.iter() {
        q_autnums.extend(autnums.iter());
    }
    let autnums: HashMap<u32, Vec<Prefix>> = client.resolve_autnums(q_autnums.iter())?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for q in queries.iter() {
        let mut prefixes = HashSet::<Prefix>::new();
        match q {
            Query::AsSet(setname) => {
                writeln!(out, "no prefix-set {}", setname)?;
                write!(out, "prefix-set {}", setname)?;
                for autnum in &as_sets[setname.as_str()] {
                    prefixes.extend(autnums[&autnum].iter());
                }
            }
            Query::Autnum(autnum) => {
                writeln!(out, "no prefix-set AS{}", autnum)?;
                write!(out, "prefix-set AS{}", autnum)?;
                prefixes.extend(autnums[&autnum].iter());
            }
        };
        let mut prefixes: Vec<&Prefix> = prefixes.iter().collect();
        prefixes.sort();
        let mut first = true;
        if do_agg {
            let mut aggregated = aggregate(&prefixes[..]);
            aggregated.sort();
            for entry in aggregated {
                if first {
                    write!(out, "\n {}", entry)?;
                    first = false;
                } else {
                    write!(out, ",\n {}", entry)?;
                }
            }
        } else {
            for (ip, masklen) in prefixes.iter() {
                if first {
                    write!(out, "\n {}/{}", ip, masklen)?;
                    first = false;
                } else {
                    write!(out, ",\n {}/{}", ip, masklen)?;
                }
            }
        }

        writeln!(out, "\nend-set")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
