use chaffnet_eval::evaluator::evaluate_path;
use std::path::Path;

#[test]
fn baseline_fixture_meets_regression_floor() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/regression.jsonl");
    let report = evaluate_path(&fixture, 0.5).unwrap();
    assert_eq!(report.source_records, 8);
    let spam = report.spam.unwrap();
    assert_eq!(spam.labeled, 8);
    assert_eq!(spam.positive, 4);
    assert!(spam.selected.precision.unwrap() >= 0.75);
    assert!(spam.selected.recall.unwrap() >= 0.50);
    assert!(spam.brier.is_finite());
    assert!(report.slop.is_none());
}
