![CI status](https://github.com/hroi/filterupdater/workflows/CI/badge.svg)
# Routing filter updater

Simple `bgpq3` alternative in Rust. Generates prefix-lists/sets for Cisco routers using IRR data.

# Features
* Fast (pipelined communication)
* Configuration file (TOML)
* Multiple file output
* Improved prefix aggregation/compression

## Example configuration

```toml
[global]
server = "whois.radb.net:43"
sources = ["RADB", "RIPE", "APNIC"]
aggregate  = true  # default = true
timestamps = true  # default = false
outputdir = "./output"

[[routers]]
hostname = "xr-router"
style = "prefix-set"
filters = [
  "AS-RIPENCC",
  "AS3333",
]

[[routers]]
hostname = "ios-router"
style = "prefix-list"
filters = [
  "AS-RIPENCC",
  "AS3333",
]
```

## Example usage
```
nocbox$ fup ./examples/config.toml
fup version 0.7.1 (f2b36bcf5a37eca33106d10821ca7f248b1e6519)
Connected to 198.108.0.18:43.
4 objects downloaded in 0.51 s.
Aggregated 109 prefixes into 41 entries.
Wrote ./output/xr-router.txt
Wrote ./output/ios-router.txt
```
