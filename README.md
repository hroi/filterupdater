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
nocbox$ fup ./examples/telianet.toml
fup version 0.7.1 (2bd7b0c70f5ce3f3ccbefa6d20922ab6ec504790)
Connected to 198.108.0.18:43.
71271 objects downloaded in 10.09 s.
Aggregated 1693814 prefixes into 355263 entries.
Wrote ./output/xr-router.txt
```
