use wifi_map::{
    heatmap::{self, Cancellation},
    model::*,
    persistence,
};
#[test]
fn android_and_windows_samples_round_trip_without_fabricated_quality() {
    let mut project: Project =
        serde_json::from_str(include_str!("../../shared/fixtures/mixed-platform.wifimap")).unwrap();
    persistence::validate(&project).unwrap();
    assert_eq!(project.schema_version, CURRENT_SCHEMA);
    let sample = &project.surveys[0].samples[2];
    assert_eq!(sample.observation.source, ObservationSource::AndroidScan);
    assert_eq!(sample.observation.ssid, vec![255, 128, 65]);
    assert!(sample.observation.link_quality_percent.is_none());
    assert_eq!(
        sample.observation.ssid_encoding,
        Some(SsidEncoding::RawBytes)
    );
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/interop");
    std::fs::create_dir_all(&output).unwrap();
    persistence::save(&output.join("windows-roundtrip.wifimap"), &project).unwrap();
    let map = heatmap::compute(
        &project,
        0,
        None,
        project.surveys[0].samples[0].observation.bssid,
        Metric::RssiDbm,
        3.0,
        &Cancellation::default(),
    )
    .unwrap();
    std::fs::write(
        output.join("windows-heat.json"),
        serde_json::to_vec(
            &map.cells
                .iter()
                .map(|c| c.map(|c| c.value))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    )
    .unwrap();
    project.surveys[0].samples[2]
        .observation
        .link_quality_percent = Some(80);
    assert!(persistence::validate(&project).is_err());
}
#[test]
fn legacy_windows_projects_upgrade_only_when_loaded() {
    let mut p = demo_project();
    p.schema_version = 1;
    let output =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/interop/legacy.wifimap");
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    persistence::save(&output, &p).unwrap();
    assert_eq!(
        persistence::load(&output).unwrap().schema_version,
        CURRENT_SCHEMA
    );
    assert_eq!(
        serde_json::from_slice::<Project>(&std::fs::read(&output).unwrap())
            .unwrap()
            .schema_version,
        1
    );
}
