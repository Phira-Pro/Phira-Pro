use macroquad::prelude::*;
use prpr::info::ChartInfo;

pub(super) async fn local<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(value) = prpr::ext::poll_future(future.as_mut()) {
            return value;
        }
        next_frame().await;
    }
}
pub(super) fn wave(seconds: u32) -> Vec<u8> {
    let n = 8000 * seconds * 2;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + n).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(8000u32.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(n.to_le_bytes());
    b.resize(b.len() + n as usize, 0);
    b
}
pub(super) fn chart_fixture(root: &std::path::Path, name: &str) -> ChartInfo {
    let directory = root.join("data/charts").join(name);
    std::fs::create_dir_all(&directory).unwrap();
    let info = ChartInfo {
        name: "Replay transport QA".into(),
        level: "IN 15".into(),
        music: "song.wav".into(),
        ..Default::default()
    };
    std::fs::write(directory.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
    std::fs::write(directory.join("song.wav"), wave(12)).unwrap();
    Image::gen_image_color(32, 32, Color::new(0.1, 0.2, 0.3, 1.)).export_png(directory.join("background.png").to_str().unwrap());
    let note = |kind: u8, time: i32, hold: i32| serde_json::json!({"type":kind,"time":time,"positionX":0,"holdTime":hold,"speed":1,"floorPosition":time as f32/64.});
    let event = |v: f32| serde_json::json!([{"startTime":0,"endTime":1000000,"start":v,"end":v,"start2":0.5,"end2":0.5}]);
    let chart = serde_json::json!({"formatVersion":3,"offset":0,"numOfNotes":4,"judgeLineList":[{"bpm":120,"notesAbove":[note(1,64,0),note(3,128,192),note(1,384,0),note(1,512,0)],"notesBelow":[],"speedEvents":[{"startTime":0,"endTime":1000000,"value":1}],"judgeLineMoveEvents":event(0.5),"judgeLineRotateEvents":event(0.),"judgeLineDisappearEvents":event(1.)}]});
    std::fs::write(directory.join("chart.json"), serde_json::to_vec(&chart).unwrap()).unwrap();
    info
}
