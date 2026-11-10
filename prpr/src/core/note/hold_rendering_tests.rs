use super::*;
use crate::{config::Config, core::NoteStyle, ext::SafeTexture, info::ChartInfo};

fn install_skin(style: &mut NoteStyle, width: u16, repeat: bool) {
    let height = width * 3;
    let cap = width / 2;
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&if y < cap {
                [255, 30, 30, 255]
            } else if y >= height - cap {
                [30, 255, 30, 255]
            } else if (x / 4 + y / 4) % 2 == 0 {
                [30, 80, 255, 255]
            } else {
                [240, 200, 40, 255]
            });
        }
    }
    style.hold = Texture2D::from_rgba8(width, height, &pixels).into();
    style.hold_atlas = (cap, cap);
    style.hold_body = if repeat {
        let start = (cap * width * 4) as usize;
        let end = pixels.len() - start;
        let body = Texture2D::from_rgba8(width, height - cap * 2, &pixels[start..end]);
        body.raw_miniquad_texture_handle()
            .set_wrap(unsafe { get_internal_gl() }.quad_context, miniquad::TextureWrap::Repeat);
        Some(body.into())
    } else {
        None
    };
}

async fn render(res: &mut Resource, note: &Note, partial_cover: bool) -> Vec<u8> {
    next_frame().await;
    set_camera(&res.camera);
    clear_background(BLACK);
    let mut controls = CtrlObject::default();
    let settings = ChartSettings {
        hold_partial_cover: partial_cover,
        ..Default::default()
    };
    let mut config = RenderConfig {
        settings: &settings,
        ctrl_obj: &mut controls,
        line_height: 0.,
        appear_before: f64::INFINITY,
        draw_below: false,
        incline_sin: 0.,
    };
    note.render(res, &mut config, &mut BpmList::new(vec![(0., 120.)]));
    res.note_buffer.borrow_mut().draw_all();
    unsafe { get_internal_gl() }.flush();
    get_screen_data().bytes
}

#[test]
#[ignore = "requires desktop OpenGL/audio; run separately with --ignored --test-threads=1"]
fn legacy_hold_pixels_match_original_for_resource_pack_layouts() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Hold resource pack compatibility regression".into(),
            headless: true,
            window_width: 640,
            window_height: 640,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            crate::core::init_assets();
            let directory = tempfile::tempdir().unwrap();
            std::fs::copy("assets/ending.ogg", directory.path().join("song.ogg")).unwrap();
            let fs = Box::new(crate::fs::ExternalFileSystem(std::sync::Arc::new(crate::dir::Dir::new(directory.path()).unwrap())));
            let info = ChartInfo {
                music: "song.ogg".into(),
                ..Default::default()
            };
            let mut config = Config::default();
            config.volume_music = 0.;
            config.volume_sfx = 0.;
            let texture: SafeTexture = Texture2D::from_rgba8(1, 1, &[0, 0, 0, 255]).into();
            let mut future = Box::pin(Resource::new(config, info, fs, None, texture.clone(), texture, true));
            let mut res = loop {
                if let Some(result) = crate::ext::poll_future(future.as_mut()) {
                    break result.unwrap();
                }
                next_frame().await;
            };
            res.aspect_ratio = 1.;
            res.camera.zoom = vec2(1., 1.);
            let mut note = Note {
                object: Object::default(),
                kind: NoteKind::Hold {
                    end_time: 4.,
                    end_height: 0.8,
                },
                hitsound: HitSound::Click,
                time: 2.,
                height: 0.1,
                speed: 1.,
                color: WHITE,
                fx_color: None,
                judge_area: 1.,
                hold_body_speed: None,
                above: true,
                multiple_hint: false,
                fake: false,
                judge: JudgeStatus::NotJudged,
            };
            let mut changed = 0;
            for repeat in [false, true] {
                res.res_pack.info.hold_repeat = repeat;
                install_skin(&mut res.res_pack.note_style, 16, repeat);
                install_skin(&mut res.res_pack.note_style_mh, 24, repeat);
                for compact in [false, true] {
                    res.res_pack.info.hold_compact = compact;
                    for keep_head in [false, true] {
                        res.res_pack.info.hold_keep_head = keep_head;
                        for multiple in [false, true] {
                            note.multiple_hint = multiple;
                            for above in [false, true] {
                                note.above = above;
                                res.model_stack =
                                    vec![Matrix::identity().append_nonuniform_scaling(&super::super::Vector::new(1., if above { 1. } else { -1. }))];
                                for partial in [false, true] {
                                    for now in [1., 3.] {
                                        res.time = now;
                                        note.hold_body_speed = None;
                                        res.config.legacy_hold_rendering = false;
                                        let original = render(&mut res, &note, partial).await;
                                        assert!(original.chunks_exact(4).any(|p| p[..3].iter().any(|&v| v > 100)));
                                        note.hold_body_speed = Some(0.15);
                                        res.config.legacy_hold_rendering = true;
                                        let legacy = render(&mut res, &note, partial).await;
                                        let different = original.iter().zip(&legacy).filter(|(a, b)| a != b).count();
                                        if different > 0 {
                                            std::fs::create_dir_all("target/hold-rendering-qa").unwrap();
                                            for (name, bytes) in [("original", original.clone()), ("legacy", legacy)] {
                                                Image {
                                                    width: 640,
                                                    height: 640,
                                                    bytes,
                                                }
                                                .export_png(&format!("target/hold-rendering-qa/{name}.png"));
                                            }
                                        }
                                        assert!(different == 0, "legacy skin pixels changed: {different}; repeat={repeat}, compact={compact}, keep_head={keep_head}, multiple={multiple}, above={above}, partial={partial}, now={now}");
                                        res.config.legacy_hold_rendering = false;
                                        if original != render(&mut res, &note, partial).await {
                                            changed += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            assert_eq!(changed, 128, "both Hold modes must be exercised in every skin/state combination");
        },
    );
}
