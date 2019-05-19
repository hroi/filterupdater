#![forbid(unsafe_code)]
use std::convert::TryFrom;
use std::env;
use std::fs;
use std::fs::{rename, File};
use std::io::prelude::*;
use std::path::Path;
use std::process;

use fup::aggregate::{aggregate, AggPrefix};
use fup::filterclass::{InvalidQuery, Query};
use fup::format::{CiscoPrefixList, CiscoPrefixSet};
use fup::{radb, AppResult, Map, Prefix, Set};
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
    // radb,afrinic,ripe,ripe-nonauth,bell,apnic,nttcom,altdb,panix,risq,
    // nestegg,level3,reach,aoltw,openface,arin,easynet,jpirr,host,rgnet,
    // rogers,bboi,tc,canarie
    sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RouterConfig {
    hostname: String,
    style: String,
    filters: Vec<String>,
}

fn main() -> AppResult<()> {
    let mut args = env::args();
    let progname = args.next().unwrap();
    let config_file_name = if let Some(arg) = args.next() {
        arg
    } else {
        eprintln!(
            "Usage: {} <config.toml>",
            Path::new(&progname).file_name().unwrap().to_string_lossy()
        );
        process::exit(1);
    };
    let mut config_file = File::open(config_file_name)?;
    let mut file_contents = String::new();
    config_file.read_to_string(&mut file_contents)?;
    let root_config: RootConfig = toml::from_str(&file_contents)?;
    fs::create_dir_all(&root_config.global.outputdir)?;

    let filters: Set<&str> = root_config
        .routers
        .iter()
        .flat_map(|router| router.filters.iter())
        .map(String::as_str)
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
        &root_config.global.server,
        &root_config.global.sources.join(","),
    )?;
    eprintln!("Connected to {}.", client.peer_addr()?);

    let as_set_members = client.resolve_as_sets(&q_as_sets)?;

    q_autnums.extend(as_set_members.values().flat_map(|s| s));

    let rt_set_prefixes = client.resolve_rt_sets(&q_rt_sets)?;

    let asprefixes = client.resolve_autnums(&q_autnums)?;
    let elapsed = time::SteadyTime::now() - start_time;
    eprintln!(
        "{} objects downloaded in {:.2} s.",
        q_as_sets.len() + q_rt_sets.len() + q_autnums.len(),
        f64::from(elapsed.num_milliseconds() as u32) / 1000.0
    );

    let mut prefix_set_configs: Map<&str, String> = Default::default();
    let mut prefix_list_configs: Map<&str, String> = Default::default();

    for r in root_config.routers.iter() {
        let iter = r.filters.iter().map(String::as_str);
        let target = match r.style.as_str() {
            "prefix-set" => &mut prefix_set_configs,
            "prefix-list" => &mut prefix_list_configs,
            style => Err(format!("Unknow output style {}", style))?,
        };
        iter.for_each(|f| {
            target.entry(f).or_default();
        });
    }

    let generated_at = time::now_utc();
    let generated_at = generated_at.rfc3339();

    filters.iter().for_each(|filter_name| {
        let mut prefix_set: Set<Prefix> = Default::default();

        match Query::try_from(*filter_name).unwrap() {
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
        } else {
            let mut prefix_list: Vec<&Prefix> = prefix_set.iter().collect();

            let mut entry_list: Vec<AggPrefix> = if root_config.global.aggregate {
                prefix_list.sort_unstable();
                aggregate(&prefix_list[..])
            } else {
                prefix_list
                    .iter()
                    .map(|p| AggPrefix::from_prefix(p))
                    .collect()
            };
            entry_list.sort_unstable();
            let comment: String = format!("Generated at {}", generated_at);

            prefix_set_configs.entry(filter_name).and_modify(|s| {
                *s = CiscoPrefixSet(filter_name, &comment, &entry_list[..]).to_string()
            });
            prefix_list_configs.entry(filter_name).and_modify(|s| {
                *s = CiscoPrefixList(filter_name, &comment, &entry_list[..]).to_string()
            });
        }
    });

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
