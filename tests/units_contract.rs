//! Contract: the units JSON that `razdor::dt::units_json` writes is read by the OpenMW converter's
//! input types (`razdor_openmw::units`, the `--units` file). Synthetic units only; no game data.

use razdor::dt::data::parse_units;
use razdor::dt::ini::Ini;
use razdor::dt::units_json::{export_to_string, FORMAT, VERSION};
use razdor_openmw::units::Units;

const UNITS_INI: &str = "[1 Unit]\r\nGlobalIndex=1\r\nName=One\r\nStartExpirience=1\r\nCost=10\r\nHits=10\r\nAttackBlow=5\r\nBonus=Flying\r\nEvasion=15\r\nManaDrain=2\r\n[2 Unit]\r\nGlobalIndex=2\r\nName=Two\r\nStartExpirience=1\r\nCost=10\r\nHits=10\r\nAttackBlow=5\r\nBonus=Splash\r\n";

#[test]
fn the_units_json_parses_in_the_converters_input_types() {
    let units = parse_units(&Ini::parse(UNITS_INI)).value;
    assert_eq!(units.len(), 2);
    for pretty in [false, true] {
        let text = export_to_string(&units, &[], "ru", pretty).expect("the export writes");
        let read = Units::from_json(&text).unwrap_or_else(|e| panic!("the converter refuses the export: {e}"));
        assert_eq!(read.len(), 2);
        let one = read.get(1).expect("unit 1");
        assert_eq!((one.name.as_str(), one.hit_points, one.attack.blow), ("One", 10, 5));
        assert_eq!(read.get(2).expect("unit 2").name, "Two");
    }
    assert_eq!((FORMAT, VERSION), ("razdor-units-json", 2), "the converter reads this format and version");
}
