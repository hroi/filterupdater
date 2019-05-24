use std::cmp::{max, min};
use std::error::Error;
use std::net::IpAddr;
use std::str::FromStr;

use crate::Prefix;

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone)]
pub struct AggPrefix {
    pub prefix: IpAddr,
    pub mask: u8,
    pub min: u8,
    pub max: u8,
    pub valid: bool,
}

impl AggPrefix {
    fn can_consolidate_with(&self, other: &Self) -> bool {
        let does_overlap = match (self.prefix, other.prefix) {
            (IpAddr::V4(a), IpAddr::V4(b)) => {
                (u32::from(a) ^ u32::from(b)) == (1 << 31) >> (u32::from(self.mask) - 1)
            }
            (IpAddr::V6(a), IpAddr::V6(b)) => {
                (u128::from(a) ^ u128::from(b)) == (1 << 127) >> (u32::from(self.mask) - 1)
            }
            _ => false,
        };
        does_overlap && (self.min, self.max) == (other.min, other.max)
    }

    fn touches(&self, other: &Self) -> bool {
        match (self.prefix, other.prefix) {
            (IpAddr::V4(a), IpAddr::V4(b)) => {
                let wildcard_bits = 32 - u32::from(self.mask);
                let ua = u32::from(a);
                let ub = u32::from(b);
                let next_prefix = ua + (1 << wildcard_bits);
                ub <= next_prefix
            }

            (IpAddr::V6(a), IpAddr::V6(b)) => {
                let wildcard_bits = 128 - u32::from(self.mask);
                let ua = u128::from(a);
                let ub = u128::from(b);
                let next_prefix = ua + (1 << wildcard_bits);
                ub <= next_prefix
            }
            _ => false,
        }
    }
}

impl AggPrefix {
    pub fn from_prefix((ip, masklen): &Prefix) -> Self {
        AggPrefix {
            prefix: *ip,
            mask: *masklen,
            min: *masklen,
            max: *masklen,
            valid: true,
        }
    }
}

impl FromStr for AggPrefix {
    type Err = Box<dyn Error>;

    fn from_str(s: &str) -> Result<AggPrefix, Self::Err> {
        let mut elems = s.split('/');
        if let (Some(ip), Some(mask), None) = (elems.next(), elems.next(), elems.next()) {
            let prefix = ip.parse()?;
            let mask = mask.parse()?;
            Ok(AggPrefix {
                prefix,
                mask,
                min: mask,
                max: mask,
                valid: true,
            })
        } else {
            Err("invalid prefix".into())
        }
    }
}

fn consolidate(level: &mut Vec<AggPrefix>, level_below: &mut Vec<AggPrefix>) {
    let mut did_change = true;
    while did_change {
        did_change = false;
        level.sort_unstable();
        let mut slice = level.as_mut_slice();
        while let Some((first, rest)) = slice.split_first_mut() {
            slice = rest;
            if first.valid {
                for prefix in slice.iter_mut().filter(|p| p.valid) {
                    if first.can_consolidate_with(prefix) {
                        // {192.0.2.0/24 , 192.0.3.0/24} -> {192.0.2.0/23 le 24}
                        let mut merged = first.clone();
                        merged.mask -= 1;
                        first.valid = false;
                        prefix.valid = false;
                        level_below.push(merged);
                        did_change = true;
                    } else if (first.prefix, first.mask, first.min + 1)
                        == (prefix.prefix, prefix.mask, prefix.min)
                    {
                        // {192.0.2.0/23 ge 24 le 24, 192.0.2.0/23 ge 25 le 25} -> {192.0.2.0/23 ge 24 le 25}
                        first.min = min(first.min, prefix.min);
                        first.max = max(first.max, prefix.max);
                        prefix.valid = false;
                        did_change = true;
                    } else if !first.touches(prefix) {
                        // {192.0.2.0/23 , 198.51.100.0/24}
                        break;
                    }
                }
            }
        }
    }
}

pub fn aggregate(prefixes: &[&Prefix]) -> Vec<AggPrefix> {
    let prefixes: Vec<AggPrefix> = prefixes.iter().map(|p| AggPrefix::from_prefix(p)).collect();
    let mut levels = Vec::<Vec<AggPrefix>>::new();
    levels.resize_with(129, Vec::new);
    prefixes
        .into_iter()
        .for_each(|p| levels[p.mask as usize].push(p));
    let mut view = levels.as_mut_slice();
    while let Some((cur, rest)) = view.split_last_mut() {
        if let Some(next) = rest.last_mut() {
            consolidate(cur, next);
        }
        view = rest;
    }
    levels
        .into_iter()
        .flat_map(IntoIterator::into_iter)
        .filter(|entry| entry.valid)
        .collect()
}
