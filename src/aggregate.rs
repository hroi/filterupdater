use std::error::Error;
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone)]
pub struct Entry {
    prefix: IpAddr,
    mask: u8,
    min: u8,
    max: u8,
    valid: bool,
}

impl Entry {
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

use super::*;

impl Entry {
    fn from_prefix((ip, masklen): &Prefix) -> Self {
        Entry {
            prefix: *ip,
            mask: *masklen,
            min: *masklen,
            max: *masklen,
            valid: true,
        }
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.valid {
            write!(f, "{}/{}", self.prefix, self.mask)?;
            if self.mask != self.min {
                write!(f, " ge {}", self.min)?;
            }
            if self.mask != self.max {
                write!(f, " le {}", self.max)?;
            }
            Ok(())
        } else {
            write!(f, "INVALID")
        }
    }
}

impl FromStr for Entry {
    type Err = Box<Error>;

    fn from_str(s: &str) -> Result<Entry, Self::Err> {
        let mut elems = s.split('/');
        if let (Some(ip), Some(mask), None) = (elems.next(), elems.next(), elems.next()) {
            if let (Ok(prefix), Ok(mask)) = (ip.parse(), mask.parse()) {
                return Ok(Entry {
                    prefix,
                    mask,
                    min: mask,
                    max: mask,
                    valid: true,
                });
            }
        }
        Err("invalid prefix".into())
    }
}

use std::cmp::{max, min};

fn touching(this: &Entry, that: &Entry) -> bool {
    let wildcard_bits = 32 - u32::from(this.mask);
    match (this.prefix, that.prefix) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let ua = u32::from(a);
            let ub = u32::from(b);
            let next_prefix = ua + (1 << wildcard_bits);
            ub <= next_prefix
        }

        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let ua = u128::from(a);
            let ub = u128::from(b);
            let next_prefix = ua + (1 << wildcard_bits);
            ub <= next_prefix
        }
        _ => false,
    }
}

fn level_up(this: &mut Vec<Entry>, next: &mut Vec<Entry>) {
    let mut did_change = true;
    while did_change {
        did_change = false;
        this.sort();
        let mut this = &mut this[..];
        while this.len() >= 2 {
            let (a, rest) = this.split_first_mut().unwrap();
            this = rest;
            if !a.valid {
                continue;
            }
            //dbg!(this.len());
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
                if (a.prefix, a.mask, a.min + 1) == (b.prefix, b.mask, b.min)
                    // || (a.prefix, a.mask, a.min - 1) == (b.prefix, b.mask, b.min)
                    // || (a.prefix, a.mask, a.max + 1) == (b.prefix, b.mask, b.min)
                    // || (a.prefix, a.mask, a.max - 1) == (b.prefix, b.mask, b.max)
                {
                    a.min = min(a.min, b.min);
                    a.max = max(a.max, b.max);
                    b.valid = false;
                    did_change = true;
                    continue;
                }
                if !touching(a, b) {
                    // dbg!(("not touching", a, b));
                    break;
                }
            }
        }
    }
}

pub fn aggregate(prefixes: &[&Prefix]) -> Vec<Entry> {
    let prefixes: Vec<_> = prefixes.iter().map(|p| Entry::from_prefix(p)).collect();
    let mut levels = Vec::<Vec<Entry>>::new();
    levels.resize_with(129, Default::default);
    for prefix in prefixes.iter() {
        levels[prefix.mask as usize].push(prefix.clone());
    }
    for cur in (1..=128).rev() {
        use std::mem;
        let mut this = mem::replace(&mut levels[cur], vec![]);
        let mut next = mem::replace(&mut levels[cur - 1], vec![]);

        level_up(&mut this, &mut next);
        mem::replace(&mut levels[cur], this);
        mem::replace(&mut levels[cur - 1], next);
    }
    let mut filter = Vec::new();
    for (_level, entries) in levels.iter().enumerate().rev() {
        if !entries.is_empty() {
            for entry in entries.iter().filter(|e| e.valid) {
                filter.push(entry.clone());
            }
        }
    }
    filter
}
