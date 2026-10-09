//! Isolated review evidence for HarryMoss #2640 with zanetworker #4288.
//! Tests the unchanged #4288 downgrade using the serialized #2640 trace shape.
use openshell_ocsf::{ActionId, Endpoint, NetworkActivityBuilder};
use openshell_ocsf::format::downgrade::{downgrade_event, DowngradeOutcome};
use openshell_ocsf::validation::{load_class_schema_for_version, validate_required_fields};
use serde_json::{json, Value};

const TRACE: &str = "0af7651916cd43dd8448eb211c80319c";

fn correlated() -> Value {
    let mut value = NetworkActivityBuilder::new(openshell_ocsf::ctx::ctx())
        .action(ActionId::Denied)
        .src_endpoint_addr("192.0.2.5".parse().unwrap(), 51234)
        .dst_endpoint(Endpoint::from_ip("192.0.2.1".parse().unwrap(), 443))
        .unmapped("trace", json!({"uid": "source-owned-unmapped-trace"}))
        .build().to_json().unwrap();
    value["trace"] = json!({"uid": TRACE});
    value["metadata"]["profiles"].as_array_mut().unwrap().push(json!("trace"));
    value
}

#[test]
fn successful_targets_remove_only_schema_trace_and_preserve_unmapped_correlation() {
    for target in ["1.1.0", "1.3.0"] {
        let mut value = correlated();
        assert_eq!(downgrade_event(&mut value, target), DowngradeOutcome::Downgraded);
        assert!(value.get("trace").is_none());
        assert!(!value["metadata"]["profiles"].as_array().unwrap().contains(&json!("trace")));
        assert_eq!(value["metadata"]["version"], target);
        assert_eq!(value["unmapped"]["downgraded_attributes"]["trace"]["uid"], TRACE);
        assert_eq!(value["unmapped"]["trace"]["uid"], "source-owned-unmapped-trace");
        let schema = load_class_schema_for_version(target, "network_activity");
        validate_required_fields(&value, &schema);
        println!("{target}: {value}");
    }
}

#[test]
fn unsatisfied_targets_keep_native_trace_and_profile_atomically() {
    for (target, missing) in [("1.1.0", "src_endpoint"), ("1.3.0", "dst_endpoint")] {
        let mut value = correlated();
        value.as_object_mut().unwrap().remove(missing);
        let original = value.clone();
        let result = downgrade_event(&mut value, target);
        assert!(matches!(&result, DowngradeOutcome::KeptNative { reason } if reason.contains(missing)));
        assert_eq!(value, original);
        assert_eq!(value["metadata"]["version"], "1.8.0");
        println!("{target}: {result:?}");
    }
}

#[test]
fn native_target_does_not_strip_trace_or_existing_unmapped_data() {
    let mut value = correlated();
    let original = value.clone();
    assert_eq!(downgrade_event(&mut value, "1.8.0"), DowngradeOutcome::NotNeeded);
    assert_eq!(value, original);
}
