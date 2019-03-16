#[macro_use]
extern crate serde_derive;
extern crate time;
extern crate toml;

use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt::Write as WriteFmt;
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
    sources: Vec<String>,
    aggregate: bool,
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
        process::exit(-1);
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

    let start_time = time::now();
    let mut client = RadbClient::open(
        root_config.global.server,
        &root_config.global.sources.join(","),
    )?;
    eprintln!("Connected to {}.", client.peer_addr()?);
    let nums = client.resolve_as_sets(q_sets.iter())?;

    q_nums.extend(nums.values().flat_map(|s| s));

    let asprefixes = client.resolve_autnums(q_nums.iter())?;
    let end_time = time::now();
    let elapsed = end_time - start_time;
    eprintln!(
        "{} objects downloaded in {:.3} s.",
        q_sets.len() + q_nums.len(),
        f64::from(elapsed.num_milliseconds() as u32) / 1000.0
    );

    let generated_at = end_time.rfc3339();
    let mut prefix_set_configs: HashMap<&str, String> = Default::default();
    let mut prefix_list_configs: HashMap<&str, String> = Default::default();
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
        prefix_list.sort();

        if root_config.global.aggregate {
            let mut prefix_list = aggregate::aggregate(&prefix_list[..]);
            prefix_list.sort();

            if root_config.routers.iter().any(|r| r.style == "prefix-set") {
                let mut prefix_set_config = String::new();
                writeln!(
                    &mut prefix_set_config,
                    "no prefix-set {}\nprefix-set {}\n # Generated at {}",
                    object_name, object_name, &generated_at
                )?;
                let mut first = true;
                for prefix in prefix_list.iter() {
                    if first {
                        write!(&mut prefix_set_config, " {}", prefix)?;
                        first = false;
                    } else {
                        write!(&mut prefix_set_config, ",\n {}", prefix)?;
                    }
                }
                writeln!(&mut prefix_set_config, "\nend-set")?;
                prefix_set_configs.insert(object_name, prefix_set_config);
            }

            if root_config.routers.iter().any(|r| r.style == "prefix-list") {
                let mut prefix_list_config = String::new();
                writeln!(&mut prefix_list_config, "no ip prefix-list {}", object_name)?;
                writeln!(
                    &mut prefix_list_config,
                    "ip prefix-list {} description Generated at {}",
                    object_name, &generated_at
                )?;
                writeln!(
                    &mut prefix_list_config,
                    "no ipv6 prefix-list {}",
                    object_name
                )?;
                writeln!(
                    &mut prefix_list_config,
                    "ipv6 prefix-list {} description Generated at {}",
                    object_name, &generated_at
                )?;
                for prefix in prefix_list.iter() {
                    if prefix.prefix.is_ipv4() {
                        writeln!(
                            &mut prefix_list_config,
                            "ip prefix-list {} permit {}",
                            object_name, prefix
                        )?;
                    } else {
                        writeln!(
                            &mut prefix_list_config,
                            "ipv6 prefix-list {} permit {}",
                            object_name, prefix
                        )?;
                    }
                }
                prefix_list_configs.insert(object_name, prefix_list_config);
            }
        } else {
            if root_config.routers.iter().any(|r| r.style == "prefix-set") {
                let mut prefix_set_config = String::new();
                writeln!(
                    &mut prefix_set_config,
                    "no prefix-set {}\nprefix-set {}",
                    object_name, object_name
                )?;
                let mut first = true;
                for prefix in prefix_list.iter() {
                    if first {
                        write!(&mut prefix_set_config, " {}/{}", prefix.0, prefix.1)?;
                        first = false;
                    } else {
                        write!(&mut prefix_set_config, ",\n {}/{}", prefix.0, prefix.1)?;
                    }
                }
                writeln!(&mut prefix_set_config, "\nend-set")?;
                prefix_set_configs.insert(object_name, prefix_set_config);
            }
            if root_config.routers.iter().any(|r| r.style == "prefix-list") {
                let mut prefix_list_config = String::new();
                writeln!(&mut prefix_list_config, "no ip prefix-list {}", object_name)?;
                writeln!(
                    &mut prefix_list_config,
                    "no ipv6 prefix-list {}",
                    object_name
                )?;
                for prefix in prefix_list.iter() {
                    if prefix.0.is_ipv4() {
                        writeln!(
                            &mut prefix_list_config,
                            "ip prefix-list {} permit {}/{}",
                            object_name, prefix.0, prefix.1
                        )?;
                    } else {
                        writeln!(
                            &mut prefix_list_config,
                            "ipv6 prefix-list {} permit {}/{}",
                            object_name, prefix.0, prefix.1
                        )?;
                    }
                }
                prefix_list_configs.insert(object_name, prefix_list_config);
            }
        }
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
            unknown => Err(format!("unknown style: {}", unknown))?,
        }
        rename(&temp_name, &outputfile_name)?;
        eprintln!("Wrote {}", outputfile_name);
    }

    Ok(())
}
