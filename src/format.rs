use std::fmt::{Display, Formatter, Result};

use crate::aggregate::AggPrefix;

pub struct CiscoPrefixList<'a>(pub &'a str, pub &'a str, pub &'a [AggPrefix]);
pub struct CiscoPrefixSet<'a>(pub &'a str, pub &'a str, pub &'a [AggPrefix]);
pub struct CiscoEntryFmt<'a>(&'a AggPrefix);

impl<'a> Display for CiscoEntryFmt<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result {
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

impl<'a> Display for CiscoPrefixList<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let (name, comment, list) = (self.0, self.1, self.2);
        writeln!(
            f,
            "no ip prefix-list {name}\n\
             ip prefix-list {name} description {comment}\n\
             no ipv6 prefix-list {name}\n\
             ipv6 prefix-list {name} description {comment}",
            name = name,
            comment = comment,
        )?;
        for prefix in list.iter() {
            assert!(prefix.valid);
            let family = if prefix.prefix.is_ipv4() {
                "ip"
            } else {
                "ipv6"
            };
            let prefix = CiscoEntryFmt(prefix);
            writeln!(f, "{} prefix-list {} permit {}", family, name, prefix)?;
        }
        Ok(())
    }
}

impl<'a> Display for CiscoPrefixSet<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let (name, comment, list) = (self.0, self.1, self.2);
        writeln!(f, "no prefix-set {}", name)?;
        writeln!(f, "prefix-set {}\n # {}", name, comment)?;
        let mut first = true;
        for prefix in list.iter().map(CiscoEntryFmt) {
            if first {
                write!(f, " {}", prefix)?;
                first = false;
            } else {
                write!(f, ",\n {}", prefix)?;
            }
        }
        writeln!(f, "\nend-set")
    }
}
