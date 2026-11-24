//! The country of an address, from the DB-IP IP to Country Lite table (CC BY 4.0, "IP Geolocation by
//! DB-IP", https://db-ip.com). The table is a CSV of `start,end,CC` lines (IPv4 and IPv6 ranges), read
//! by the monthly timer (`server/deploy/geo-update.sh`), never committed. The address is looked up in
//! memory and dropped: it is never stored or logged. No table, or an address outside it, gives "unknown".
//!
//! Why CSV and not MMDB: the CSV needs no binary parser and no new dependency, and the tests can write a
//! small table as text. The lookup is a binary search on sorted ranges, a few MB in memory.

use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

/// The country of an address the table does not place (or no table at all).
pub const UNKNOWN: &str = "unknown";

#[derive(Debug, Default, PartialEq)]
pub struct Geo {
    v4: Vec<(u32, u32, [u8; 2])>,
    v6: Vec<(u128, u128, [u8; 2])>,
}

impl Geo {
    /// Parses the table from a reader. Lines that do not parse are skipped; a table with no usable line is
    /// an error (a truncated or HTML download must not read as "every address is unknown").
    pub fn parse_reader<R: BufRead>(mut reader: R) -> Result<Geo, String> {
        let mut g = Geo::default();
        let mut buf = Vec::new();
        loop {
            buf.clear();
            let n = reader.read_until(b'\n', &mut buf).map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                break;
            }
            let line = String::from_utf8_lossy(&buf);
            g.add_line(&line);
        }
        if g.v4.is_empty() && g.v6.is_empty() {
            return Err("no country ranges".into());
        }
        g.v4.sort_unstable();
        g.v6.sort_unstable();
        Ok(g)
    }

    /// Parses the table from text (the tests and the nginx export use it).
    pub fn parse(text: &str) -> Result<Geo, String> {
        Geo::parse_reader(text.as_bytes())
    }

    fn add_line(&mut self, line: &str) {
        let mut it = line.trim().split(',');
        let (Some(a), Some(b), Some(cc)) = (it.next(), it.next(), it.next()) else {
            return;
        };
        let cc = cc.trim().trim_matches('"').as_bytes();
        if cc.len() != 2 || !cc.iter().all(u8::is_ascii_uppercase) {
            return;
        }
        let code = [cc[0], cc[1]];
        let a = a.trim().trim_matches('"');
        let b = b.trim().trim_matches('"');
        if let (Ok(s), Ok(e)) = (a.parse::<Ipv4Addr>(), b.parse::<Ipv4Addr>()) {
            if s <= e {
                self.v4.push((u32::from(s), u32::from(e), code));
            }
        } else if let (Ok(s), Ok(e)) = (a.parse::<Ipv6Addr>(), b.parse::<Ipv6Addr>()) {
            if s <= e {
                self.v6.push((u128::from(s), u128::from(e), code));
            }
        }
    }

    /// Loads the table from a file. A missing or bad file logs one line (the path, never an address) and
    /// gives `None`: every ping then stores "unknown", and nothing fails.
    pub fn load(path: &Path) -> Option<Geo> {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => {
                tracing::warn!("geo: no country table at {}; countries stay unknown", path.display());
                return None;
            }
        };
        match Geo::parse_reader(BufReader::new(file)) {
            Ok(g) => {
                tracing::info!("geo: {} country ranges loaded", g.len());
                Some(g)
            }
            Err(e) => {
                tracing::warn!("geo: {}: {e}; countries stay unknown", path.display());
                None
            }
        }
    }

    /// The number of ranges (both families).
    pub fn len(&self) -> usize {
        self.v4.len() + self.v6.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The country code of an address, or "unknown". An IPv4-mapped IPv6 address is looked up as IPv4.
    pub fn country(&self, ip: IpAddr) -> String {
        let code = match ip {
            IpAddr::V4(v4) => self.find_v4(u32::from(v4)),
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => self.find_v4(u32::from(v4)),
                None => self.find_v6(u128::from(v6)),
            },
        };
        match code {
            Some(c) => std::str::from_utf8(&c).unwrap_or(UNKNOWN).to_string(),
            None => UNKNOWN.to_string(),
        }
    }

    fn find_v4(&self, x: u32) -> Option<[u8; 2]> {
        let i = self.v4.partition_point(|r| r.0 <= x);
        let r = self.v4.get(i.checked_sub(1)?)?;
        (x <= r.1).then_some(r.2)
    }

    fn find_v6(&self, x: u128) -> Option<[u8; 2]> {
        let i = self.v6.partition_point(|r| r.0 <= x);
        let r = self.v6.get(i.checked_sub(1)?)?;
        (x <= r.1).then_some(r.2)
    }

    /// The IPv4 ranges as nginx `geo` lines (`ranges` mode): `a.b.c.d-e.f.g.h CC;`, ascending. nginx's
    /// `ranges` form is documented for IPv4 only, so the IPv6 rows are left out: a `/hub/` download from
    /// an IPv6 address is logged as "ZZ" (no country). The owner's server reads these lines through
    /// `razdor-server geo-nginx`.
    pub fn nginx_ranges(&self) -> String {
        let mut out = String::new();
        for (s, e, code) in &self.v4 {
            let cc = std::str::from_utf8(code).unwrap_or("ZZ");
            out.push_str(&format!("{}-{} {};\n", Ipv4Addr::from(*s), Ipv4Addr::from(*e), cc));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "\
\"1.0.0.0\",\"1.0.0.255\",\"AU\"
1.0.1.0,1.0.3.255,CN
not a line
2001:db8::,2001:db8:ffff:ffff:ffff:ffff:ffff:ffff,JP
9.9.9.9,9.9.9.1,US
5.5.5.5,5.5.5.5,xx
";

    fn table() -> Geo {
        Geo::parse(TABLE).expect("the fixture parses")
    }

    #[test]
    fn looks_up_v4_edges_and_gaps() {
        let g = table();
        let ip = |s: &str| s.parse::<IpAddr>().expect("an address");
        assert_eq!(g.country(ip("1.0.0.0")), "AU");
        assert_eq!(g.country(ip("1.0.0.255")), "AU");
        assert_eq!(g.country(ip("1.0.1.0")), "CN");
        assert_eq!(g.country(ip("1.0.3.255")), "CN");
        assert_eq!(g.country(ip("1.0.4.0")), UNKNOWN, "a gap between ranges");
        assert_eq!(g.country(ip("0.0.0.1")), UNKNOWN, "below every range");
        assert_eq!(g.country(ip("200.0.0.1")), UNKNOWN, "above every range");
        assert_eq!(g.country(ip("192.168.1.1")), UNKNOWN, "a private address is in no table");
    }

    #[test]
    fn skips_bad_lines_and_inverted_ranges() {
        let g = table();
        // "5.5.5.5 ... xx" has a lower-case code and is skipped; "not a line" has no commas.
        assert_eq!(g.country("5.5.5.5".parse().expect("an address")), UNKNOWN);
        assert_eq!(g.country("9.9.9.5".parse().expect("an address")), UNKNOWN, "an inverted range is skipped");
        assert_eq!(g.len(), 3, "AU, CN and JP only");
    }

    #[test]
    fn looks_up_v6_and_mapped_v4() {
        let g = table();
        let ip = |s: &str| s.parse::<IpAddr>().expect("an address");
        assert_eq!(g.country(ip("2001:db8::1")), "JP");
        assert_eq!(g.country(ip("2001:db9::1")), UNKNOWN);
        assert_eq!(g.country(ip("::ffff:1.0.0.7")), "AU", "an IPv4-mapped address is looked up as IPv4");
    }

    #[test]
    fn a_table_with_no_ranges_is_refused() {
        assert!(Geo::parse("<html>error</html>\n").is_err());
        assert!(Geo::parse("").is_err());
    }

    #[test]
    fn a_missing_file_is_unknown_not_an_error() {
        let path = std::env::temp_dir().join("razdor-geo-test-missing-file.csv");
        let _ = std::fs::remove_file(&path);
        assert!(Geo::load(&path).is_none());
    }

    #[test]
    fn nginx_export_is_ascending_ipv4_ranges() {
        let text = table().nginx_ranges();
        assert_eq!(text, "1.0.0.0-1.0.0.255 AU;\n1.0.1.0-1.0.3.255 CN;\n");
    }

    #[test]
    fn a_table_round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("razdor-geo-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp folder");
        let path = dir.join("table.csv");
        std::fs::write(&path, TABLE).expect("writes the fixture");
        let loaded = Geo::load(&path).expect("loads");
        assert_eq!(loaded, table());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
