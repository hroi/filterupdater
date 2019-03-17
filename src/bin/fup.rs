#[macro_use]
extern crate serde_derive;
extern crate time;
extern crate toml;

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{rename, File};
use std::io::prelude::*;
use std::process;

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

use fup::*;
use radb::*;

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

    let objects: HashSet<&str> = root_config
        .routers
        .iter()
        .flat_map(|r| r.filters.iter())
        .map(|s| s.as_str())
        .collect();

    let mut q_sets: HashSet<&str> = Default::default();
    let mut q_nums: HashSet<u32> = Default::default();
    for o in objects.iter() {
        if let Ok(num) = parse_autnum(o) {
            q_nums.insert(num);
        } else {
            q_sets.insert(o);
        }
    }

    let start_time = time::SteadyTime::now();
    let mut client = RadbClient::open(
        root_config.global.server,
        &root_config.global.sources.join(","),
    )?;
    eprintln!("Connected to {}.", client.peer_addr()?);
    let nums = client.resolve_as_sets(q_sets.iter())?;

    q_nums.extend(nums.values().flat_map(|s| s));

    let asprefixes = client.resolve_autnums(q_nums.iter())?;
    let elapsed = time::SteadyTime::now() - start_time;
    eprintln!(
        "{} objects downloaded in {:.3} s.",
        q_sets.len() + q_nums.len(),
        f64::from(elapsed.num_milliseconds() as u32) / 1000.0
    );

    let generated_at = time::now_utc();
    let generated_at = generated_at.rfc3339();
    let mut prefix_set_configs: HashMap<&str, String> = Default::default();
    let mut prefix_list_configs: HashMap<&str, String> = Default::default();

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

    for object_name in objects.iter() {
        let mut prefix_set: HashSet<Prefix> = Default::default();

        if let Ok(num) = parse_autnum(object_name) {
            prefix_set.extend(asprefixes[&num].iter());
        } else {
            prefix_set.extend(
                nums[object_name]
                    .iter()
                    .flat_map(|num| asprefixes[num].iter()),
            );
        }

        if prefix_set.is_empty() {
            eprintln!("Warning: {} is empty, skipping", object_name);
            continue;
        }

        let mut prefix_list: Vec<&Prefix> = prefix_set.iter().collect();

        let mut entry_list: Vec<aggregate::Entry> = if root_config.global.aggregate {
            prefix_list.sort();
            aggregate::aggregate(&prefix_list[..])
        } else {
            prefix_list
                .iter()
                .map(|p| aggregate::Entry::from_prefix(p))
                .collect()
        };
        entry_list.sort();
        let comment: String = format!("Generated at {}", generated_at);

        prefix_set_configs.entry(object_name).and_modify(|s| {
            *s = format::CiscoPrefixSet(object_name, &comment, &entry_list[..]).to_string()
        });
        prefix_list_configs.entry(object_name).and_modify(|s| {
            *s = format::CiscoPrefixList(object_name, &comment, &entry_list[..]).to_string()
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
