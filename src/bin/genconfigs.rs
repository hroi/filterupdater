#[macro_use]
extern crate serde_derive;
extern crate toml;

use std::collections::{HashMap, HashSet};
use std::fmt::Write as WriteFmt;
use std::fs::File;
use std::io::prelude::*;

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

use filterupdater::*;
use radb::*;

// #[derive(PartialEq, Eq, Hash)]
// enum Query {
//     Autnum(u32),
//     AsSet(String),
// }
// impl FromStr for Query {
//     type Err = io::Error;
//     fn from_str(s: &str) -> io::Result<Query> {
//         if s.starts_with("AS") {
//             if let Ok(autnum) = parse_autnum(s) {
//                 Ok(Query::Autnum(autnum))
//             } else {
//                 Ok(Query::AsSet(s.to_string()))
//             }
//         } else {
//             Err(Error::new(InvalidInput, s))
//         }
//     }
// }

fn main() -> AppResult<()> {
    let mut config_file = File::open("config.toml")?;
    let mut file_contents = String::new();
    config_file.read_to_string(&mut file_contents)?;
    let root_config: RootConfig = toml::from_str(&file_contents)?;
    //println!("{:#?}", root_config);
    std::fs::create_dir_all(&root_config.global.outputdir)?;

    let objects: HashSet<&str> = root_config
        .routers
        .iter()
        .flat_map(|r| r.filters.iter())
        .map(|s| s.as_str())
        .collect();
    // println!("resolving: {:?}", objects);

    //let mut q_sets: HashSet<String> = Default::default();
    //let mut q_autnums: HashSet<u32> = Default::default();

    let mut q_sets: HashSet<&str> = Default::default();
    let mut q_nums: HashSet<u32> = Default::default();
    for o in objects.iter() {
        if let Ok(num) = parse_autnum(o) {
            q_nums.insert(num);
        } else {
            q_sets.insert(o);
        }
    }
    // println!("q_sets: {:#?}", q_sets);
    // println!("q_nums: {:#?}", q_nums);

    let mut client = RadbClient::open(root_config.global.server)?;
    let nums = client.resolve_as_sets(q_sets.iter())?;

    // println!("resolve_as_sets: {:?}", nums);
    q_nums.extend(nums.values().flat_map(|s| s));

    // println!("full nums: {:#?}", q_nums);

    let asprefixes = client.resolve_autnums(q_nums.iter())?;
    // println!("as prefixes: {:#?}", asprefixes);

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
        let mut prefix_list: Vec<&Prefix> = prefix_set.iter().collect();
        prefix_list.sort();

        if root_config.global.aggregate {
            let prefix_list = aggregate::aggregate(&prefix_list[..]);
            if root_config.routers.iter().any(|r| r.style == "prefix-set") {
                let mut prefix_set_config = String::new();
                writeln!(
                    &mut prefix_set_config,
                    "no prefix-set {}\nprefix_set {}",
                    object_name, object_name
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
                    "no ipv6 prefix-list {}",
                    object_name
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
                    "no prefix-set {}\nprefix_set {}",
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
        let mut outputfile = File::create(&outputfile_name)?;
        eprintln!("Writing {}", outputfile_name);
        for object_name in router_config.filters.iter() {
            let config = match router_config.style.as_str() {
                "prefix-set" => prefix_set_configs
                    .get::<str>(object_name.as_str())
                    .expect("object name not found"),
                "prefix-list" => prefix_list_configs
                    .get::<str>(object_name.as_str())
                    .expect("object name not found"),
                unknown => Err(format!("unknown style: {}", unknown))?,
            };
            outputfile.write_all(config.as_bytes())?;
        }
    }

    Ok(())
}
