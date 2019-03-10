use crate::radb::RadbClient;
use std::collections::HashSet;

#[test]
fn client() {
    let mut client = RadbClient::open("whois.radb.net:43").unwrap();
    let sets: HashSet<String> = ["AS-NGDC", "AS-KRACON"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    dbg!(&sets);
    let as_sets = client.resolve_as_sets(sets.iter()).unwrap();
    dbg!(&as_sets);
    let mut autnums = HashSet::<u32>::new();
    for members in as_sets.values() {
        autnums.extend(members.iter());
    }
    let prefixes = client.resolve_autnums(autnums.iter()).unwrap();
    dbg!(&prefixes);
}
