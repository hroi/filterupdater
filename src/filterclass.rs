use std::{convert::TryFrom, error};

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum FilterClass<'a> {
    AsSet(&'a str),
    RouteSet(&'a str),
    AutNum(u32),
}

impl<'a> TryFrom<&'a str> for FilterClass<'a> {
    type Error = Box<dyn error::Error>;

    fn try_from(input: &'a str) -> Result<FilterClass<'a>, Self::Error> {
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
                    Ok(FilterClass::AutNum(_)) | Err(_) => continue,
                    Ok(FilterClass::AsSet(_)) => return Ok(FilterClass::AsSet(input)),
                    Ok(FilterClass::RouteSet(_)) => return Ok(FilterClass::RouteSet(input)),
                }
            }
            Err(input.into())
        } else {
            parse_name_component(input)
        }
    }
}

fn parse_name_component(input: &str) -> Result<FilterClass, Box<dyn error::Error>> {
    match input.get(0..3) {
        Some(name) if name.eq_ignore_ascii_case("as-") => Ok(FilterClass::AsSet(input)),
        Some(name) if name.eq_ignore_ascii_case("rs-") => Ok(FilterClass::RouteSet(input)),
        Some(name) if name[..2].eq_ignore_ascii_case("as") => input[2..]
            .parse::<u32>()
            .map(FilterClass::AutNum)
            .map_err(|_| input.into()),
        _ => Err(input.into()),
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    mod realworld {
        use super::*;
        use filterclass::*;
        use std::{
            fs::File,
            io::{prelude::*, BufReader},
        };

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
                    match FilterClass::try_from(name).unwrap() {
                        FilterClass::RouteSet(_) => num_parsed += 1,
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
                    match FilterClass::try_from(name).unwrap() {
                        FilterClass::AsSet(_) => num_parsed += 1,
                        _ => panic!(name.to_owned()),
                    }
                });
            eprintln!("All {} as-set names parsed correctly", num_parsed);
        }
    }
}
