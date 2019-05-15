use std::convert::TryFrom;
use std::env;
use std::error;
use std::fmt;
use std::fs::{rename, File};
use std::io::prelude::*;
use std::process;

use fup::*;
use serde_derive::Deserialize;
use time;
use toml;

#[derive(Debug, Deserialize)]
struct RootConfig {
    global: GlobalConfig,
    routers: Vec<RouterConfig>,
}

#[derive(Debug, Deserialize)]
struct GlobalConfig {
    server: String,
    outputdir: String,
    aggregate: bool,
    sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RouterConfig {
    hostname: String,
    style: String,
    filters: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum Query<'a> {
    AsSet(&'a str),
    RouteSet(&'a str),
    AutNum(u32),
}

#[derive(Debug, PartialEq, Eq)]
struct InvalidQuery(String);

impl fmt::Display for InvalidQuery {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid query: {:?}", self.0)
    }
}

impl error::Error for InvalidQuery {}

impl<'a> TryFrom<&'a str> for Query<'a> {
    type Error = InvalidQuery;

    fn try_from(input: &'a str) -> Result<Query<'a>, InvalidQuery> {
        if input.contains(':') {
            // From RFC 2622:
            //   Set names can also be hierarchical.  A hierarchical set name is a
            //   sequence of set names and AS numbers separated by colons ":".  At
            //   least one component of such a name must be an actual set name (i.e.
            //   start with one of the prefixes above).  All the set name components
            //   of an hierarchical name has to be of the same type.  For example, the
            //   following names are valid: AS1:AS-CUSTOMERS, AS1:RS-EXPORT:AS2, RS-
            //   EXCEPTIONS:RS-BOGUS.
            let elems = input.split(':');
            for elem in elems {
                match parse_nonhier_name(elem) {
                    Ok(Query::AutNum(_)) | Err(_) => continue,
                    Ok(Query::AsSet(_)) => return Ok(Query::AsSet(input)),
                    Ok(Query::RouteSet(_)) => return Ok(Query::RouteSet(input)),
                }
            }
            return Err(InvalidQuery(input.to_string()))
        } else {
            parse_nonhier_name(input)
        }
    }
}

fn parse_nonhier_name<'a>(input: &'a str) -> Result<Query<'a>, InvalidQuery> {
    match input.get(0..3) {
        Some(name) if name.eq_ignore_ascii_case("as-") => Ok(Query::AsSet(input)),
        Some(name) if name.eq_ignore_ascii_case("rs-") => Ok(Query::RouteSet(input)),
        Some(name) if name[..2].eq_ignore_ascii_case("as") => {
            input[2..].parse::<u32>().map(Query::AutNum).map_err(|_| InvalidQuery(input.to_string()))
        },
        _ => Err(InvalidQuery(input.to_string()))
    }
}

fn main() -> AppResult<()> {
    let mut args = env::args();
    let progname = args.next().unwrap();
    let config_file_name = if let Some(arg) = args.next() {
        arg
    } else {
        eprintln!(
            "Usage: {} <config.toml>",
            std::path::Path::new(&progname)
                .file_name()
                .unwrap()
                .to_string_lossy()
        );
        process::exit(1);
    };
    let mut config_file = File::open(config_file_name)?;
    let mut file_contents = String::new();
    config_file.read_to_string(&mut file_contents)?;
    let root_config: RootConfig = toml::from_str(&file_contents)?;
    std::fs::create_dir_all(&root_config.global.outputdir)?;

    let filters: Set<&str> = root_config
        .routers
        .iter()
        .flat_map(|router| router.filters.iter())
        .map(|s| s.as_str())
        .collect();

    let queries: Result<Set<Query>, InvalidQuery> =
        filters.iter().map(|s| Query::try_from(*s)).collect();

    let queries = queries?;

    let mut q_as_sets: Set<&str> = Default::default();
    let mut q_rt_sets: Set<&str> = Default::default();
    let mut q_autnums: Set<u32> = Default::default();

    queries.iter().for_each(|q| {
        match *q {
            Query::AsSet(name) => q_as_sets.insert(name),
            Query::RouteSet(name) => q_rt_sets.insert(name),
            Query::AutNum(num) => q_autnums.insert(num),
        };
    });

    let start_time = time::SteadyTime::now();
    let mut client = radb::RadbClient::open(
        root_config.global.server,
        &root_config.global.sources.join(","),
    )?;
    eprintln!("Connected to {}.", client.peer_addr()?);

    let as_set_members = client.resolve_as_sets(q_as_sets.iter())?;

    q_autnums.extend(as_set_members.values().flat_map(|s| s));

    let rt_set_prefixes = client.resolve_rt_sets(q_rt_sets.iter())?;

    let asprefixes = client.resolve_autnums(q_autnums.iter())?;
    let elapsed = time::SteadyTime::now() - start_time;
    eprintln!(
        "{} objects downloaded in {:.3} s.",
        q_as_sets.len() + q_autnums.len(),
        f64::from(elapsed.num_milliseconds() as u32) / 1000.0
    );

    let generated_at = time::now_utc();
    let generated_at = generated_at.rfc3339();
    let mut prefix_set_configs: Map<&str, String> = Default::default();
    let mut prefix_list_configs: Map<&str, String> = Default::default();

    for r in root_config.routers.iter() {
        let iter = r.filters.iter().map(|name| name.as_str());
        let target = match r.style.as_str() {
            "prefix-set" => &mut prefix_set_configs,
            "prefix-list" => &mut prefix_list_configs,
            style => Err(format!("Unknow output style {}", style))?,
        };
        iter.for_each(|f| {
            target.entry(f).or_default();
        });
    }

    for filter_name in filters.iter() {
        let mut prefix_set: Set<Prefix> = Default::default();

        match Query::try_from(*filter_name)? {
            Query::AsSet(name) => {
                prefix_set.extend(
                    as_set_members[name]
                        .iter()
                        .flat_map(|num| asprefixes[num].iter()),
                );
            }
            Query::RouteSet(name) => {
                prefix_set.extend(rt_set_prefixes[name].iter());
            }
            Query::AutNum(num) => {
                prefix_set.extend(asprefixes[&num].iter());
            }
        }

        if prefix_set.is_empty() {
            eprintln!("Warning: {} is empty, skipping", filter_name);
            continue;
        }

        let mut prefix_list: Vec<&Prefix> = prefix_set.iter().collect();

        let mut entry_list: Vec<aggregate::Entry> = if root_config.global.aggregate {
            prefix_list.sort_unstable();
            aggregate::aggregate(&prefix_list[..])
        } else {
            prefix_list
                .iter()
                .map(|p| aggregate::Entry::from_prefix(p))
                .collect()
        };
        entry_list.sort_unstable();
        let comment: String = format!("Generated at {}", generated_at);

        prefix_set_configs.entry(filter_name).and_modify(|s| {
            *s = format::CiscoPrefixSet(filter_name, &comment, &entry_list[..]).to_string()
        });
        prefix_list_configs.entry(filter_name).and_modify(|s| {
            *s = format::CiscoPrefixList(filter_name, &comment, &entry_list[..]).to_string()
        });
    }

    for router_config in root_config.routers.iter() {
        let outputfile_name = format!(
            "{}/{}.txt",
            root_config.global.outputdir, router_config.hostname
        );
        let temp_name = format!("{}.tmp", &outputfile_name);
        let mut outputfile = File::create(&temp_name)?;
        match router_config.style.as_str() {
            "prefix-set" => {
                for object_name in router_config.filters.iter() {
                    if let Some(config) = prefix_set_configs.get::<str>(object_name.as_str()) {
                        outputfile.write_all(config.as_bytes())?;
                    }
                }
            }
            "prefix-list" => {
                for object_name in router_config.filters.iter() {
                    if let Some(config) = prefix_list_configs.get::<str>(object_name.as_str()) {
                        outputfile.write_all(config.as_bytes())?;
                    }
                }
                writeln!(&mut outputfile, "end")?;
            }
            unknown => Err(format!("Unknown style: {}", unknown))?,
        }
        rename(&temp_name, &outputfile_name)?;
        eprintln!("Wrote {}", outputfile_name);
    }

    Ok(())
}
