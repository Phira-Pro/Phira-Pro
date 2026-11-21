mod dialog;
pub(super) use dialog::ImportDialog;

use anyhow::{anyhow, ensure, Context, Result};
use prpr::{
    core::BpmList,
    info::{ChartFormat, ChartInfo},
    judge::X_DIFF_MAX,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, Cursor, Read, Seek, Write},
    path::Path,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MAX_LANES: usize = 32;
const MAX_CHART_SIZE: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_CHART_SIZE: u64 = 64 * 1024 * 1024;
const MAX_ASSET_SIZE: u64 = 256 * 1024 * 1024;
const AUDIO_EXTS: &[&str] = &["ogg", "mp3", "wav", "flac", "aac"];
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif"];

pub(super) struct LanePackage {
    archive: ZipArchive<BufReader<File>>,
    difficulties: Vec<Difficulty>,
}

struct Difficulty {
    info: ChartInfo,
    lanes: usize,
    notes: Vec<LaneNote>,
    audio: String,
    cover: Option<String>,
}

struct LaneNote {
    lane: usize,
    start: f64,
    end: Option<f64>,
}

pub(super) struct Layout {
    positions: Vec<f32>,
    line_y: f32,
    speed: f32,
}

impl Layout {
    fn evenly_spaced(lanes: usize, spacing: f32) -> Vec<f32> {
        (0..lanes).map(|i| (i as f32 - (lanes - 1) as f32 / 2.) * spacing).collect()
    }

    fn validate_positions(positions: &[f32], lanes: usize) -> bool {
        positions.len() == lanes && positions.iter().all(|x| (-675. ..=675.).contains(x)) && positions.windows(2).all(|pair| pair[0] < pair[1])
    }
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn archive_key(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    let parts: Vec<_> = name.split('/').filter(|&part| part != ".").collect();
    if parts.iter().any(|part| part.is_empty() || *part == ".." || part.contains(':')) {
        return None;
    }
    Some(parts.join("/").to_lowercase())
}

fn find_asset(names: &HashMap<String, String>, chart: &str, name: &str, extensions: &[&str]) -> Option<String> {
    let key = archive_key(name.trim())?;
    if !extensions.contains(&extension(&key).as_str()) {
        return None;
    }
    let chart = archive_key(chart)?;
    chart
        .rsplit_once('/')
        .and_then(|(dir, _)| names.get(&format!("{dir}/{key}")))
        .or_else(|| names.get(&key))
        .cloned()
}

pub(super) fn has_lane_entries<R: Read + Seek>(archive: &ZipArchive<R>) -> bool {
    archive.file_names().any(|name| matches!(extension(name).as_str(), "osu" | "mc"))
}

impl LanePackage {
    pub(super) fn read(mut archive: ZipArchive<BufReader<File>>) -> Result<Self> {
        let names = archive
            .file_names()
            .filter_map(|name| archive_key(name).map(|key| (key, name.to_owned())))
            .collect::<HashMap<_, _>>();
        let mut chart_names = names
            .values()
            .filter_map(|name| match extension(name).as_str() {
                "osu" => Some((name, parse_osu as fn(&str) -> Result<Option<Difficulty>>)),
                "mc" => Some((name, parse_malody as fn(&str) -> Result<Option<Difficulty>>)),
                _ => None,
            })
            .collect::<Vec<_>>();
        chart_names.sort_unstable_by_key(|&(name, _)| name);
        let mut difficulties = Vec::new();
        let mut first_error = None;
        let mut total_size = 0;
        for (name, parse) in chart_names {
            let parsed = (|| {
                let entry = archive.by_name(name)?;
                ensure!(entry.size() <= MAX_CHART_SIZE, "Chart data exceeds 16 MiB");
                let mut source = String::new();
                entry.take(MAX_CHART_SIZE + 1).read_to_string(&mut source)?;
                ensure!(source.len() as u64 <= MAX_CHART_SIZE, "Chart data exceeds 16 MiB");
                total_size += source.len() as u64;
                ensure!(total_size <= MAX_TOTAL_CHART_SIZE, "Archive chart data exceeds 64 MiB");
                let Some(mut chart) = parse(source.trim_start_matches('\u{feff}'))? else {
                    return Ok(None);
                };
                chart.audio = find_asset(&names, name, &chart.audio, AUDIO_EXTS).context("Missing or unsupported chart audio")?;
                chart.cover = chart.cover.and_then(|cover| find_asset(&names, name, &cover, IMAGE_EXTS));
                Ok::<_, anyhow::Error>(Some(chart))
            })();
            match parsed {
                Ok(Some(chart)) => difficulties.push(chart),
                Ok(None) => {}
                Err(err) => {
                    first_error.get_or_insert(err);
                }
            }
            if total_size > MAX_TOTAL_CHART_SIZE {
                break;
            }
        }
        if difficulties.is_empty() {
            return Err(first_error.unwrap_or_else(|| anyhow!("No osu!mania or Malody Key difficulty found")));
        }
        difficulties.sort_by(|a, b| (a.lanes, &a.info.level).cmp(&(b.lanes, &b.info.level)));
        Ok(Self { archive, difficulties })
    }

    pub(super) fn convert(mut self, index: usize, layout: Layout) -> Result<File> {
        let chart = &self.difficulties[index];
        let mut info = chart.info.clone();
        info.music = format!("audio.{}", extension(&chart.audio));
        if let Some(cover) = &chart.cover {
            info.illustration = format!("cover.{}", extension(cover));
        }
        let mut output = ZipWriter::new(tempfile::tempfile()?);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        output.start_file("info.yml", options)?;
        serde_yaml::to_writer(&mut output, &info)?;
        output.start_file(&info.chart, options)?;
        serde_json::to_writer(&mut output, &rpe_chart(&info, &chart.notes, &layout))?;
        copy_asset(&mut self.archive, &mut output, &chart.audio, &info.music)?;
        if let Some(cover) = &chart.cover {
            copy_asset(&mut self.archive, &mut output, cover, &info.illustration)?;
        } else {
            output.start_file(&info.illustration, options)?;
            let mut png = Cursor::new(Vec::new());
            image::RgbImage::from_pixel(16, 9, image::Rgb([25, 30, 39])).write_to(&mut png, image::ImageFormat::Png)?;
            output.write_all(png.get_ref())?;
        }
        let mut output = output.finish()?;
        output.rewind()?;
        Ok(output)
    }
}

fn copy_asset(archive: &mut ZipArchive<BufReader<File>>, output: &mut ZipWriter<File>, source: &str, target: &str) -> Result<()> {
    let entry = archive.by_name(source)?;
    ensure!(entry.size() <= MAX_ASSET_SIZE, "Chart asset exceeds 256 MiB");
    output.start_file(target, SimpleFileOptions::default())?;
    let size = io::copy(&mut entry.take(MAX_ASSET_SIZE + 1), output)?;
    ensure!(size <= MAX_ASSET_SIZE, "Chart asset exceeds 256 MiB");
    Ok(())
}

fn chart_info(name: &str, artist: &str, charter: &str, level: &str, preview_ms: f64) -> ChartInfo {
    let mut info = ChartInfo::default();
    for (dst, src) in [
        (&mut info.name, name),
        (&mut info.composer, artist),
        (&mut info.charter, charter),
        (&mut info.level, level),
    ] {
        if !src.trim().is_empty() {
            *dst = src.trim().to_owned();
        }
    }
    info.format = Some(ChartFormat::Rpe);
    info.preview_start = (preview_ms.max(0.) / 1000.) as f32;
    info.use_rpe_170_speed = Some(false);
    info.use_rpen = Some(false);
    info
}

fn parse_osu(source: &str) -> Result<Option<Difficulty>> {
    let mut section = "";
    let mut properties = HashMap::new();
    let mut objects = Vec::new();
    let mut cover = None;
    for line in source.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with("//")) {
        if let Some(name) = line.strip_prefix('[').and_then(|it| it.strip_suffix(']')) {
            section = name;
            continue;
        }
        match section {
            "HitObjects" => objects.push(line),
            "Events" => {
                if let Some(rest) = line.strip_prefix("0,0,").or_else(|| line.strip_prefix("Background,0,")) {
                    let name = if let Some(rest) = rest.trim_start().strip_prefix('"') {
                        rest.split('"').next()
                    } else {
                        rest.split(',').next()
                    };
                    cover = cover.or_else(|| name.map(str::to_owned));
                }
            }
            _ => {
                if let Some((key, value)) = line.split_once(':') {
                    properties.insert((section, key.trim()), value.trim());
                }
            }
        }
    }
    let get = |key, name| properties.get(&(key, name)).copied().unwrap_or_default();
    if get("General", "Mode") != "3" {
        return Ok(None);
    }
    let lanes = get("Difficulty", "CircleSize").parse::<f64>().context("Invalid osu!mania key count")?;
    ensure!(lanes.fract() == 0. && (1. ..=MAX_LANES as f64).contains(&lanes), "Invalid osu!mania key count");
    let lanes = lanes as usize;
    let mut notes = Vec::new();
    for line in objects {
        let fields: Vec<_> = line.split(',').collect();
        ensure!(fields.len() >= 5, "Invalid osu!mania hit object");
        let kind: u8 = fields[3].trim().parse().context("Invalid osu!mania hit object type")?;
        if kind & (128 | 1) == 0 {
            continue;
        }
        let x: i64 = fields[0].trim().parse().context("Invalid osu!mania note column")?;
        let start = fields[2].trim().parse::<f64>().context("Invalid osu!mania note time")? / 1000.;
        let end = if kind & 128 != 0 {
            let end = fields
                .get(5)
                .and_then(|tail| tail.split(':').next())
                .context("Missing osu!mania hold end time")?;
            Some(end.trim().parse::<f64>().context("Invalid osu!mania hold end time")? / 1000.)
        } else {
            None
        };
        notes.push(LaneNote {
            lane: (x.clamp(0, 511) as usize) * lanes / 512,
            start,
            end,
        });
    }
    validate_notes(&mut notes, lanes)?;
    let localized = |unicode, fallback| {
        let value = get("Metadata", unicode);
        if value.is_empty() {
            get("Metadata", fallback)
        } else {
            value
        }
    };
    let info = chart_info(
        localized("TitleUnicode", "Title"),
        localized("ArtistUnicode", "Artist"),
        get("Metadata", "Creator"),
        get("Metadata", "Version"),
        get("General", "PreviewTime").parse().unwrap_or_default(),
    );
    Ok(Some(Difficulty {
        info,
        lanes,
        notes,
        audio: get("General", "AudioFilename").trim_matches('"').to_owned(),
        cover,
    }))
}

#[derive(Deserialize)]
struct MalodyChart {
    meta: Value,
    #[serde(default)]
    time: Vec<MalodyTiming>,
    #[serde(default)]
    note: Vec<MalodyNote>,
}

#[derive(Deserialize)]
struct MalodyTiming {
    beat: [i64; 3],
    bpm: f64,
}

#[derive(Deserialize)]
struct MalodyNote {
    beat: [i64; 3],
    endbeat: Option<[i64; 3]>,
    column: Option<usize>,
    #[serde(rename = "type", default)]
    kind: u8,
    sound: Option<String>,
    #[serde(default)]
    offset: f64,
}

fn malody_beat([whole, numerator, denominator]: [i64; 3]) -> Result<f64> {
    ensure!(denominator > 0, "Invalid Malody beat denominator");
    Ok(whole as f64 + numerator as f64 / denominator as f64)
}

fn parse_malody(source: &str) -> Result<Option<Difficulty>> {
    let chart: MalodyChart = serde_json::from_str(source).context("Invalid Malody chart")?;
    if chart.meta["mode"].as_i64() != Some(0) {
        return Ok(None);
    }
    let lanes = chart.meta["mode_ext"]["column"]
        .as_u64()
        .filter(|&n| (1..=MAX_LANES as u64).contains(&n))
        .context("Invalid Malody key count")? as usize;
    let mut tempos = chart
        .time
        .iter()
        .map(|point| {
            ensure!(point.bpm > 0. && point.bpm.is_finite(), "Invalid Malody BPM");
            Ok((malody_beat(point.beat)?, point.bpm))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(!tempos.is_empty(), "Missing Malody BPM");
    tempos.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut tempo = BpmList::new(tempos);
    let backing = chart
        .note
        .iter()
        .find(|note| note.kind == 1 && note.column.is_none() && note.sound.is_some())
        .context("Missing Malody backing track")?;
    let audio_time = tempo.time_beats(malody_beat(backing.beat)?) + backing.offset / 1000.;
    let mut notes = Vec::new();
    for note in &chart.note {
        if let Some(lane) = note.column {
            let start = tempo.time_beats(malody_beat(note.beat)?) - audio_time;
            let end = note
                .endbeat
                .map(|beat| Ok::<_, anyhow::Error>(tempo.time_beats(malody_beat(beat)?) - audio_time))
                .transpose()?;
            notes.push(LaneNote { lane, start, end });
        }
    }
    validate_notes(&mut notes, lanes)?;
    let song = &chart.meta["song"];
    let song_text = |preferred, fallback| {
        song[preferred]
            .as_str()
            .filter(|it| !it.is_empty())
            .or_else(|| song[fallback].as_str())
            .unwrap_or_default()
    };
    let info = chart_info(
        song_text("titleorg", "title"),
        song_text("artistorg", "artist"),
        chart.meta["creator"].as_str().unwrap_or_default(),
        chart.meta["version"].as_str().unwrap_or_default(),
        chart.meta["preview"].as_f64().unwrap_or_default(),
    );
    Ok(Some(Difficulty {
        info,
        lanes,
        notes,
        audio: backing.sound.clone().unwrap_or_default(),
        cover: chart.meta["background"].as_str().map(str::to_owned),
    }))
}

fn validate_notes(notes: &mut [LaneNote], lanes: usize) -> Result<()> {
    ensure!(!notes.is_empty(), "Chart has no lane notes");
    for note in notes.iter() {
        let valid_time = |time: f64| time.is_finite() && time.abs() < f64::from(i32::MAX) - 12.;
        ensure!(note.lane < lanes && valid_time(note.start), "Invalid lane note");
        if let Some(end) = note.end {
            ensure!(valid_time(end) && end > note.start, "Invalid lane hold end time");
        }
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(())
}

fn rpe_time(seconds: f64) -> [i32; 3] {
    // With 60 BPM one beat is one second. Euclidean division also encodes pre-roll notes.
    const RESOLUTION: i64 = 1_000_000;
    let time = (seconds * RESOLUTION as f64).round() as i64;
    [time.div_euclid(RESOLUTION) as i32, time.rem_euclid(RESOLUTION) as i32, RESOLUTION as i32]
}

fn rpe_chart(info: &ChartInfo, notes: &[LaneNote], layout: &Layout) -> Value {
    let start = notes.first().unwrap().start.min(0.).floor() - 10.;
    let end = notes.iter().map(|note| note.end.unwrap_or(note.start)).fold(0., f64::max).ceil() + 1.;
    let event = |value| {
        json!({
            "startTime": rpe_time(start), "endTime": rpe_time(end), "start": value, "end": value
        })
    };
    let notes: Vec<_> = notes
        .iter()
        .map(|note| {
            let lane = note.lane;
            let x = layout.positions[lane];
            let left = lane.checked_sub(1).map_or(f32::INFINITY, |i| x - layout.positions[i]);
            let right = layout.positions.get(lane + 1).map_or(f32::INFINITY, |next| next - x);
            let judge_area = (left.min(right) / (1350. * X_DIFF_MAX as f32)).clamp(0.01, 1.);
            json!({
                "type": if note.end.is_some() { 2 } else { 1 },
                "startTime": rpe_time(note.start), "endTime": rpe_time(note.end.unwrap_or(note.start)),
                "positionX": x, "above": 1, "yOffset": 0, "alpha": 255,
                "size": 1, "speed": 1, "isFake": 0, "visibleTime": 9999, "judgeArea": judge_area
            })
        })
        .collect();
    json!({
        "META": {
            "RPEVersion": 150, "offset": 0, "name": info.name, "composer": info.composer,
            "charter": info.charter, "level": info.level, "song": info.music, "background": info.illustration
        },
        "BPMList": [{"startTime": [0, 0, 1], "bpm": 60}],
        "judgeLineList": [{
            "Name": "Lane playfield", "Texture": "line.png", "isCover": 1,
            "eventLayers": [{
                "moveYEvents": [event(layout.line_y)], "alphaEvents": [event(255.)],
                "speedEvents": [event(layout.speed)]
            }],
            "notes": notes
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use prpr::{core::NoteKind, fs, scene::GameScene};

    const OSU: &str = "\
osu file format v14
[General]
AudioFilename: demo.ogg
Mode: 3
[Metadata]
Title: Example
Version: 4K
[Difficulty]
CircleSize: 4
[HitObjects]
511,192,-25,1,0
0,192,500,128,0,1250:0:0:0:0:
";

    const MALODY: &str = r#"{
        "meta": {"mode":0,"mode_ext":{"column":6},"song":{"title":"Example"},"version":"6K"},
        "time": [{"beat":[0,0,1],"bpm":120},{"beat":[4,0,1],"bpm":60}],
        "note": [
            {"beat":[1,0,1],"type":1,"sound":"demo.ogg","offset":31},
            {"beat":[2,1,2],"column":3,"endbeat":[5,0,1]},
            {"beat":[4,1,2],"column":0}
        ]
    }"#;

    fn read_test_archive() -> LanePackage {
        let mut zip = ZipWriter::new(tempfile::tempfile().unwrap());
        let options = SimpleFileOptions::default();
        for (name, data) in [("folder/demo.osu", OSU), ("folder/demo.mc", MALODY), ("folder/DEMO.ogg", "audio")] {
            zip.start_file(name, options).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        let file = zip.finish().unwrap();
        LanePackage::read(ZipArchive::new(BufReader::new(file)).unwrap()).unwrap()
    }

    async fn verify_difficulty(package: LanePackage, index: usize, layout: Layout, decode_media: bool) {
        let expected = package.difficulties[index]
            .notes
            .iter()
            .map(|n| (n.start, n.end, layout.positions[n.lane]))
            .collect::<Vec<_>>();
        let expected_y = layout.line_y / 450.;
        let output = package.convert(index, layout).unwrap();
        let stage = tempfile::tempdir().unwrap();
        crate::scene::import_chart_to(stage.path(), "custom/lane-test".into(), output)
            .await
            .unwrap();
        let mut fs = fs::fs_from_file(stage.path()).unwrap();
        let info = fs::load_info(fs.as_mut()).await.unwrap();
        let (chart, _, format) = GameScene::load_chart_for_analysis(fs.as_mut(), &info).await.unwrap();
        assert_eq!(format, ChartFormat::Rpe);
        let mut line = chart.lines.into_iter().next().unwrap();
        line.object.set_time(0.);
        assert!((line.object.translation.1.now() - expected_y).abs() < 1e-5);
        assert_eq!(line.notes.len(), expected.len());
        let mut actual = line
            .notes
            .iter()
            .map(|note| {
                let end = match note.kind {
                    NoteKind::Hold { end_time, .. } => Some(end_time),
                    NoteKind::Click => None,
                    _ => panic!("unexpected note type"),
                };
                (note.time, end, note.object.translation.0.now() * 675.)
            })
            .collect::<Vec<_>>();
        let order = |a: &(f64, Option<f64>, f32), b: &(f64, Option<f64>, f32)| rpe_time(a.0).cmp(&rpe_time(b.0)).then_with(|| a.2.total_cmp(&b.2));
        actual.sort_by(order);
        let mut expected = expected;
        expected.sort_by(order);
        for ((actual_start, actual_end, actual_x), (start, end, x)) in actual.into_iter().zip(expected) {
            assert!((actual_start - start).abs() < 1e-6);
            assert!((actual_x - x).abs() < 1e-4);
            assert_eq!(actual_end.is_some(), end.is_some());
            if let (Some(actual_end), Some(end)) = (actual_end, end) {
                assert!((actual_end - end).abs() < 1e-6);
            }
        }
        if decode_media {
            sasa::AudioClip::new(fs.load_file(&info.music).await.unwrap()).unwrap();
        }
        image::load_from_memory(&fs.load_file(&info.illustration).await.unwrap()).unwrap();
    }

    #[tokio::test]
    async fn lane_archives_round_trip_through_normal_chart_import() {
        let osu = read_test_archive();
        assert_eq!(osu.difficulties.len(), 2);
        assert_eq!(osu.difficulties[0].notes[0].lane, 3);
        assert_eq!(osu.difficulties[0].notes[0].start, -0.025);
        let malody = read_test_archive();
        let hold = &malody.difficulties[1].notes[0];
        assert!((hold.start - 0.719).abs() < 1e-9);
        assert!((hold.end.unwrap() - 2.469).abs() < 1e-9);
        verify_difficulty(
            osu,
            0,
            Layout {
                positions: vec![-500., -175., 90., 410.],
                line_y: -250.,
                speed: 8.,
            },
            false,
        )
        .await;
        verify_difficulty(
            malody,
            1,
            Layout {
                positions: Layout::evenly_spaced(6, 200.),
                line_y: -300.,
                speed: 12.,
            },
            false,
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires local sample files"]
    async fn local_lane_archive_samples() {
        for variable in ["PHIRA_TEST_OSZ", "PHIRA_TEST_MCZ"] {
            let path = std::env::var_os(variable).expect("missing sample path");
            let open = || LanePackage::read(ZipArchive::new(BufReader::new(File::open(&path).unwrap())).unwrap()).unwrap();
            for index in 0..open().difficulties.len() {
                let package = open();
                let chart = &package.difficulties[index];
                let lanes = chart.lanes;
                println!("{variable} difficulty {}: {} notes", index + 1, chart.notes.len());
                verify_difficulty(
                    package,
                    index,
                    Layout {
                        positions: Layout::evenly_spaced(lanes, 1200. / lanes as f32),
                        line_y: -300.,
                        speed: 12.,
                    },
                    true,
                )
                .await;
            }
        }
    }
}
