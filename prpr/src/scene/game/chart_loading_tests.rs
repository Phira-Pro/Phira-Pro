use super::*;
use serde_json::json;

#[test]
fn format_detection_accepts_editor_whitespace_and_bom() {
    let info = ChartInfo::default();
    for prefix in ["", "\r\n  ", "\u{feff}", "\u{feff}\n\t"] {
        for (source, expected) in [
            (r#"{"META":{},"BPMList":[],"judgeLineList":[]}"#, ChartFormat::Rpe),
            (r#"{"BPMList":[],"judgeLineList":[]}"#, ChartFormat::Rpe),
            (r#"{"formatVersion":3,"judgeLineList":[]}"#, ChartFormat::Pgr),
        ] {
            assert_eq!(GameScene::infer_chart_format(&info, format!("{prefix}{source}").as_bytes()), expected);
        }
    }
    assert_eq!(GameScene::infer_chart_format(&info, b"0\n bp 0 120"), ChartFormat::Pec);
    assert_eq!(GameScene::infer_chart_format(&info, &[0xff, 0, 0x80]), ChartFormat::Pbc);
    let explicit = ChartInfo { format: Some(ChartFormat::Pec), ..info };
    assert_eq!(GameScene::infer_chart_format(&explicit, b"{}"), ChartFormat::Pec);
}

async fn finish<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(value) = crate::ext::poll_future(future.as_mut()) { return value; }
        next_frame().await;
    }
}

#[test]
#[ignore = "requires desktop OpenGL; run separately with --ignored --test-threads=1"]
fn editor_sidecars_load_in_production_with_chart_tempo_and_embedded_effects() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    macroquad::Window::from_config(
        Conf { window_title: "Chart loading regression".into(), headless: true, ..Default::default() },
        async {
            let directory = tempfile::tempdir().unwrap();
            let mut raw = json!({
                "META":{"RPEVersion":240,"offset":0},
                "BPMList":[{"bpm":120,"startTime":[0,0,1]},{"bpm":60,"startTime":[4,0,1]}],
                "judgeLineList":[]
            });
            let mut fs = crate::fs::ExternalFileSystem(std::sync::Arc::new(crate::dir::Dir::new(directory.path()).unwrap()));
            let info = ChartInfo { chart:"notes.json".into(), ..Default::default() };
            for sidecar in [
                "{}", "{\n\"effects\": []\n}", "\u{feff}{\"videos\":[]}",
                r#"{"effects":[{"start":[4,0,1],"end":[8,0,1],"shader":"vignette"}]}"#,
                r#"{"bpm":90,"effects":[{"start":[4,0,1],"end":[8,0,1],"shader":"vignette","global":true}]}"#,
            ] {
                std::fs::write(directory.path().join("notes.json"), format!("\u{feff}\n{}", raw)).unwrap();
                std::fs::write(directory.path().join("extra.json"), sidecar).unwrap();
                let (chart, bytes, format) = finish(GameScene::load_chart(&mut fs, &info)).await.unwrap();
                assert_eq!(format, ChartFormat::Rpe);
                assert!(bytes.starts_with(b"\xef\xbb\xbf"));
                assert_eq!(chart.bpm_list.borrow().elements(), &[(0.,0.,120.),(4.,2.,60.)]);
                assert_eq!(chart.extra.effects.len() + chart.extra.global_effects.len(), usize::from(sidecar.contains("shader")));
            }
            // Embedded RPEN shaders survive assembly with sidecar effects.
            raw["effects"] = json!([{"name":"vignette","startTime":0,"endTime":8}]);
            std::fs::write(directory.path().join("notes.json"), raw.to_string()).unwrap();
            std::fs::write(directory.path().join("extra.json"), r#"{"effects":[{"start":[0,0,1],"end":[4,0,1],"shader":"vignette"}]}"#).unwrap();
            let (chart, _, _) = finish(GameScene::load_chart(&mut fs, &info)).await.unwrap();
            assert_eq!(chart.extra.effects.len() + chart.extra.global_effects.len(), 2);
            // Analysis remains independent of visual resources and bad sidecars.
            std::fs::write(directory.path().join("extra.json"), "{").unwrap();
            assert!(finish(GameScene::load_chart(&mut fs, &info)).await.is_err());
            assert!(finish(GameScene::load_chart_for_analysis(&mut fs, &info)).await.is_ok());
        },
    );
}
