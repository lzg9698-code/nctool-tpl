//! One wire fixture is consumed by Rust, TypeScript and the Python SDK tests.
use nctool_cli::workbench::Run;
use nctool_plugin_sdk::*;

#[test]
fn shared_wire_fixture_roundtrips_and_preserves_precision() {
    let f = parse_json(include_bytes!("../../tests/fixtures/contracts/v2.json")).unwrap();
    assert_eq!(f["protocol_version"], PROTOCOL_VERSION);
    macro_rules! roundtrip {
        ($key:literal, $ty:ty) => {
            let decoded: $ty = serde_json::from_value(f[$key].clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), f[$key]);
        };
    }
    roundtrip!("action", ActionDescriptor);
    // Descriptor defaults are normalized by the SDK, not copied from a fixture.
    let descriptor: PluginDescriptor = serde_json::from_value(f["plugin"].clone()).unwrap();
    assert_eq!(descriptor.protocol_version, PROTOCOL_VERSION);
    roundtrip!("result", ActionResult);
    roundtrip!("error", PluginError);
    roundtrip!("field_error", PluginError);
    roundtrip!("run", Run);
    assert_eq!(
        f["asset"]["asset"]["defaults"]["n"].as_u64(),
        Some(u64::MAX)
    );
    assert_eq!(
        f["asset"]["asset"]["custom"][1]["n"].as_i64(),
        Some(i64::MIN)
    );
    assert_eq!(
        f["result"]["data"]["n"].as_u64(),
        Some(9_007_199_254_740_993)
    );
}
