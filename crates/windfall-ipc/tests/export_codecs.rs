use windfall_ipc::{ExportFormat, ExportOptions};
#[test]
fn legacy_export_options_default_aac_bitrate_and_new_formats_roundtrip() {
    let mut json = serde_json::to_value(ExportOptions::default()).unwrap();
    json.as_object_mut().unwrap().remove("aacBitrate");
    let legacy: ExportOptions = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(legacy.aac_bitrate, None);
    for (format, name) in [(ExportFormat::Wv, "wv"), (ExportFormat::M4a, "m4a")] {
        let options = ExportOptions {
            format,
            aac_bitrate: Some(128),
            ..legacy.clone()
        };
        let value = serde_json::to_value(&options).unwrap();
        assert_eq!(value["format"], name);
        assert_eq!(value["aacBitrate"], 128);
        assert_eq!(
            serde_json::from_value::<ExportOptions>(value).unwrap(),
            options
        );
    }
}
