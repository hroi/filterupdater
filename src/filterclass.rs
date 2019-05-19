use std::convert::TryFrom;
use std::error;
use std::fmt;

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Query<'a> {
    AsSet(&'a str),
    RouteSet(&'a str),
    AutNum(u32),
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidQuery(String);

impl fmt::Display for InvalidQuery {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid query: {:?}", self.0)
    }
}

impl error::Error for InvalidQuery {}

impl<'a> TryFrom<&'a str> for Query<'a> {
    type Error = InvalidQuery;

    fn try_from(input: &'a str) -> Result<Query<'a>, InvalidQuery> {
        if input.contains(':') {
            // From RFC 2622:
            //   Set names can also be hierarchical.  A hierarchical set name is a
            //   sequence of set names and AS numbers separated by colons ":".  At
            //   least one component of such a name must be an actual set name (i.e.
            //   start with one of the prefixes above).  All the set name components
            //   of an hierarchical name has to be of the same type.  For example, the
            //   following names are valid: AS1:AS-CUSTOMERS, AS1:RS-EXPORT:AS2, RS-
            //   EXCEPTIONS:RS-BOGUS.
            let elems = input.split(':');
            for elem in elems {
                match parse_name_component(elem) {
                    Ok(Query::AutNum(_)) | Err(_) => continue,
                    Ok(Query::AsSet(_)) => return Ok(Query::AsSet(input)),
                    Ok(Query::RouteSet(_)) => return Ok(Query::RouteSet(input)),
                }
            }
            Err(InvalidQuery(input.to_string()))
        } else {
            parse_name_component(input)
        }
    }
}

fn parse_name_component(input: &str) -> Result<Query, InvalidQuery> {
    match input.get(0..3) {
        Some(name) if name.eq_ignore_ascii_case("as-") => Ok(Query::AsSet(input)),
        Some(name) if name.eq_ignore_ascii_case("rs-") => Ok(Query::RouteSet(input)),
        Some(name) if name[..2].eq_ignore_ascii_case("as") => input[2..]
            .parse::<u32>()
            .map(Query::AutNum)
            .map_err(|_| InvalidQuery(input.to_string())),
        _ => Err(InvalidQuery(input.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    mod realworld {
        use super::*;
        use filterclass::*;
        use std::fs::File;
        use std::io::prelude::*;
        use std::io::BufReader;

        #[test]
        #[ignore]
        fn parse_route_set_names() {
            let mut num_parsed = 0;
            File::open("ripe.db.route-set")
                .map(BufReader::new)
                .unwrap()
                .lines()
                .filter_map(Result::ok)
                .filter(|l| l.starts_with("route-set:"))
                .for_each(|line| {
                    let name = line.split_whitespace().nth(1).unwrap();
                    match Query::try_from(name).unwrap() {
                        Query::RouteSet(_) => num_parsed += 1,
                        _ => panic!(name.to_owned()),
                    }
                });
            eprintln!("All {} route-set names parsed correctly", num_parsed);
        }

        #[test]
        #[ignore]
        fn parse_as_set_names() {
            let mut num_parsed = 0;
            File::open("ripe.db.as-set")
                .map(BufReader::new)
                .unwrap()
                .lines()
                .filter_map(Result::ok)
                .filter(|l| l.starts_with("as-set:"))
                .for_each(|line| {
                    let name = line.split_whitespace().nth(1).unwrap();
                    match Query::try_from(name).unwrap() {
                        Query::AsSet(_) => num_parsed += 1,
                        _ => panic!(name.to_owned()),
                    }
                });
            eprintln!("All {} as-set names parsed correctly", num_parsed);
        }
    }
}