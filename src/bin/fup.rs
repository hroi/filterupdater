#![forbid(unsafe_code)]

use std::convert::TryFrom;
use std::env;
use std::error;
use std::fs::{create_dir_all, rename, File};
use std::io::prelude::*;
use std::path::Path;
use std::process::exit;

use fup::aggregate::{aggregate, AggPrefix};
use fup::filterclass::FilterClass;
use fup::format::{CiscoPrefixList, CiscoPrefixSet};
use fup::radb::RadbClient;
use fup::{AppResult, Map, Prefix, Set};
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
    aggregate: Option<bool>,
    timestamps: Option<bool>,
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

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        exit(1);
    }
}

fn run() -> AppResult<()> {
    let mut args = env::args();
    let progname = args.next().unwrap();
    let config_file_name = if let Some(arg) = args.next() {
        arg
    } else {
        eprintln!(
            "Usage: {} <config.toml>",
            Path::new(&progname).file_name().unwrap().to_string_lossy()
        );
        exit(1);
    };
    let mut config_file = File::open(&config_file_name)
        .map_err(|e| format!("failed to open {}: {}", &config_file_name, e))?;
    let mut file_contents = String::new();
    config_file
        .read_to_string(&mut file_contents)
        .map_err(|e| format!("failed to read config: {}", e))?;
    let root_config: RootConfig =
        toml::from_str(&file_contents).map_err(|e| format!("failed to parse config: {}", e))?;
    create_dir_all(&root_config.global.outputdir).map_err(|e| {
        format!(
            "failed to create output dir {}: {}",
            &root_config.global.outputdir, e
        )
    })?;

    let filters: Set<&str> = root_config
        .routers
        .iter()
        .flat_map(|router| router.filters.iter())
        .map(String::as_str)
        .collect();

    let queries: Result<Set<FilterClass>, Box<dyn error::Error>> =
        filters.iter().map(|s| FilterClass::try_from(*s)).collect();

    let queries = queries.map_err(|e| format!("failed to parse filter name: {}", e))?;

    let mut as_set_queries: Set<&str> = Default::default();
    let mut route_set_queries: Set<&str> = Default::default();
    let mut autnum_queries: Set<u32> = Default::default();

    queries.into_iter().for_each(|q| {
        match q {
            FilterClass::AsSet(name) => as_set_queries.insert(name),
            FilterClass::RouteSet(name) => route_set_queries.insert(name),
            FilterClass::AutNum(num) => autnum_queries.insert(num),
        };
    });

    let start_time = time::SteadyTime::now();
    eprintln!(
        "{} version {} ({})",
        fup::CLIENT,
        fup::VERSION,
        fup::GIT_HASH.unwrap_or("unknown"),
    );
    let mut client = RadbClient::open(
        &root_config.global.server,
        &root_config.global.sources.join(","),
    )
    .map_err(|e| format!("failed to connect to {}: {}", &root_config.global.server, e))?;
    eprintln!("Connected to {}.", client.peer_addr()?);

    let route_set_prefixes = client
        .resolve_route_sets(&route_set_queries)
        .map_err(|e| format!("failed to resolve route-sets: {}", e))?;
    let as_set_members = client
        .resolve_as_sets(&as_set_queries)
        .map_err(|e| format!("failed to resolve as-sets: {}", e))?;
    autnum_queries.extend(as_set_members.values().flatten());
    let autnum_prefixes = client
        .resolve_autnums(&autnum_queries)
        .map_err(|e| format!("failed to resolve autnums: {}", e))?;

    let elapsed = time::SteadyTime::now() - start_time;
    eprintln!(
        "{} objects downloaded in {:.2} s.",
        as_set_queries.len() + route_set_queries.len() + autnum_queries.len(),
        elapsed.num_milliseconds() as f32 / 1000.0
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

    let mut agg_count = 0;
    let mut nonagg_count = 0;
    filters.iter().for_each(|filter_name| {
        let mut prefix_set: Set<Prefix> = Default::default();

        match FilterClass::try_from(*filter_name).expect("BUG: invalid filter") {
            FilterClass::AsSet(name) => {
                prefix_set.extend(
                    as_set_members[name]
                        .iter()
                        .flat_map(|num| autnum_prefixes[num].iter()),
                );
            }
            FilterClass::RouteSet(name) => {
                prefix_set.extend(route_set_prefixes[name].iter());
            }
            FilterClass::AutNum(num) => {
                prefix_set.extend(autnum_prefixes[&num].iter());
            }
        }

        if prefix_set.is_empty() {
            eprintln!("Warning: {} is empty, skipping", filter_name);
        } else {
            let mut prefix_list: Vec<&Prefix> = prefix_set.iter().collect();

            let mut entry_list: Vec<AggPrefix> = if root_config.global.aggregate.unwrap_or(true) {
                prefix_list.sort_unstable();
                let ret = aggregate(&prefix_list[..]);
                nonagg_count += prefix_list.len();
                agg_count += ret.len();
                ret
            } else {
                prefix_list
                    .iter()
                    .map(|p| AggPrefix::from_prefix(p))
                    .collect()
            };
            entry_list.sort_unstable();
            let comment: String = if root_config.global.timestamps.unwrap_or(false) {
                format!(
                    "Generated by {}-{} at {}",
                    fup::CLIENT,
                    fup::VERSION,
                    generated_at
                )
            } else {
                format!("Generated by {}-{}", fup::CLIENT, fup::VERSION)
            };

            prefix_set_configs.entry(filter_name).and_modify(|s| {
                *s = CiscoPrefixSet(filter_name, &comment, &entry_list[..]).to_string()
            });
            prefix_list_configs.entry(filter_name).and_modify(|s| {
                *s = CiscoPrefixList(filter_name, &comment, &entry_list[..]).to_string()
            });
        }
    });

    if root_config.global.aggregate.unwrap_or(true) {
        eprintln!(
            "Aggregated {} prefixes into {} entries.",
            nonagg_count, agg_count
        );
    }

    for router_config in root_config.routers.iter() {
        let output_filename = format!(
            "{}/{}.txt",
            root_config.global.outputdir, router_config.hostname
        );
        let temp_filename = format!("{}.tmp", &output_filename);
        let mut output_file = File::create(&temp_filename)
            .map_err(|e| format!("failed to create {}: {}", temp_filename, e))?;
        match router_config.style.as_str() {
            "prefix-set" => {
                for object_name in router_config.filters.iter() {
                    if let Some(config) = prefix_set_configs.get(object_name.as_str()) {
                        output_file
                            .write_all(config.as_bytes())
                            .map_err(|e| format!("failed to write to output file: {}", e))?;
                    }
                }
            }
            "prefix-list" => {
                for object_name in router_config.filters.iter() {
                    if let Some(config) = prefix_list_configs.get(object_name.as_str()) {
                        output_file
                            .write_all(config.as_bytes())
                            .map_err(|e| format!("failed to write to output file: {}", e))?;
                    }
                }
                writeln!(&mut output_file, "end")
                    .map_err(|e| format!("failed to write to output file: {}", e))?;
            }
            unknown => Err(format!("Unknown style: {}", unknown))?,
        }
        rename(&temp_filename, &output_filename)
            .map_err(|e| format!("rename {} to {}: {}", temp_filename, output_filename, e))?;
        eprintln!("Wrote {}", output_filename);
    }

    Ok(())
}
