use super::aggregate::Entry;
use std::fmt;

pub struct CiscoPrefixList<'a>(pub &'a str, pub &'a str, pub &'a [Entry]);
pub struct CiscoPrefixSet<'a>(pub &'a str, pub &'a str, pub &'a [Entry]);

impl<'a> fmt::Display for CiscoPrefixList<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (name, comment, list) = (self.0, self.1, self.2);
        writeln!(f, "no ip prefix-list {}", name)?;
        writeln!(f, "no ipv6 prefix-list {}", name)?;
        writeln!(f, "ip prefix-list {} description {}", name, comment)?;
        writeln!(f, "ipv6 prefix-list {} description {}", name, comment)?;
        for prefix in list.iter() {
            if prefix.prefix.is_ipv4() {
                writeln!(f, "ip prefix-list {} permit {}", name, prefix.fmt_cisco())?;
            } else {
                writeln!(f, "ipv6 prefix-list {} permit {}", name, prefix.fmt_cisco())?;
            }
        }
        Ok(())
    }
}

impl<'a> fmt::Display for CiscoPrefixSet<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (name, comment, list) = (self.0, self.1, self.2);
        writeln!(f, "no prefix-set {}", name)?;
        writeln!(f, "prefix-set {}\n # {}", name, comment)?;
        let mut first = true;
        for prefix in list.iter().map(|p| p.fmt_cisco()) {
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
