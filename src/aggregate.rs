use std::cmp::{max, min};
use std::error::Error;
use std::fmt;
use std::mem;
use std::net::IpAddr;
use std::str::FromStr;

use crate::Prefix;

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone)]
pub struct AggPrefix {
    pub prefix: IpAddr,
    pub mask: u8,
    pub min: u8,
    pub max: u8,
    valid: bool,
}

impl AggPrefix {
    fn can_level_up_with(&self, other: &Self) -> bool {
        let overlaps = match (self.prefix, other.prefix) {
            (IpAddr::V4(a), IpAddr::V4(b)) => {
                (u32::from(a) ^ u32::from(b)) == (1 << 31) >> (u32::from(self.mask) - 1)
            }
            (IpAddr::V6(a), IpAddr::V6(b)) => {
                (u128::from(a) ^ u128::from(b)) == (1 << 127) >> (u32::from(self.mask) - 1)
            }
            _ => false,
        };
        overlaps && (self.min, self.max) == (other.min, other.max)
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

    pub fn fmt_cisco(&self) -> FmtCiscoEntry {
        FmtCiscoEntry(self)
    }
}

pub struct FmtCiscoEntry<'a>(&'a AggPrefix);

impl<'a> fmt::Display for FmtCiscoEntry<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.0.valid {
            write!(f, "{}/{}", self.0.prefix, self.0.mask)?;
            if self.0.mask != self.0.min {
                write!(f, " ge {}", self.0.min)?;
            }
            if self.0.mask != self.0.max {
                write!(f, " le {}", self.0.max)?;
            }
            Ok(())
        } else {
            write!(f, "INVALID")
        }
    }
}

impl FromStr for AggPrefix {
    type Err = Box<Error>;

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

fn touching(this: &AggPrefix, that: &AggPrefix) -> bool {
    match (this.prefix, that.prefix) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let wildcard_bits = 32 - u32::from(this.mask);
            let ua = u32::from(a);
            let ub = u32::from(b);
            let next_prefix = ua + (1 << wildcard_bits);
            ub <= next_prefix
        }

        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let wildcard_bits = 128 - u32::from(this.mask);
            let ua = u128::from(a);
            let ub = u128::from(b);
            let next_prefix = ua + (1 << wildcard_bits);
            ub <= next_prefix
        }
        _ => false,
    }
}

fn level_up(this: &mut Vec<AggPrefix>, next: &mut Vec<AggPrefix>) {
    let mut did_change = true;
    while did_change {
        did_change = false;
        this.sort_unstable();
        let mut this = &mut this[..];
        while let Some((a, rest)) = this.split_first_mut() {
            this = rest;
            if !a.valid {
                continue;
            }
            for b in this.iter_mut().filter(|e| e.valid) {
                if a.can_level_up_with(b) {
                    let mut merged = a.clone();
                    merged.mask -= 1;
                    a.valid = false;
                    b.valid = false;
                    next.push(merged);
                    did_change = true;
                    continue;
                }
                if (a.prefix, a.mask, a.min + 1) == (b.prefix, b.mask, b.min) {
                    a.min = min(a.min, b.min);
                    a.max = max(a.max, b.max);
                    b.valid = false;
                    did_change = true;
                    continue;
                }
                if !touching(a, b) {
                    break;
                }
            }
        }
    }
}

pub fn aggregate(prefixes: &[&Prefix]) -> Vec<AggPrefix> {
    let prefixes: Vec<_> = prefixes.iter().map(|p| AggPrefix::from_prefix(p)).collect();
    let mut levels = Vec::<Vec<AggPrefix>>::new();
    levels.resize_with(129, Default::default);
    prefixes.iter().for_each(|prefix| {
        levels[prefix.mask as usize].push(prefix.clone());
    });
    (1..=128).rev().for_each(|cur| {
        let mut this = mem::replace(&mut levels[cur], vec![]);
        let mut next = mem::replace(&mut levels[cur - 1], vec![]);

        level_up(&mut this, &mut next);
        mem::replace(&mut levels[cur], this);
        mem::replace(&mut levels[cur - 1], next);
    });
    levels
        .into_iter()
        .flat_map(IntoIterator::into_iter)
        .filter(|entry| entry.valid)
        .collect()
}
