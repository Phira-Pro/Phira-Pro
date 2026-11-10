use super::*;
use crate::scene::game::lifecycle_tests::interrupted_audio;
use std::sync::atomic::Ordering;

impl EndingScene {
    pub(in crate::scene) fn check_interrupted_resume(&mut self, tm: &mut TimeManager) {
        let (mut audio, starts) = interrupted_audio();
        let clip = AudioClip::from_raw(vec![sasa::Frame(0., 0.); 8000], 8000);
        let mut bgm = audio.create_music(clip, MusicParams::default()).unwrap();
        for _ in 0..16 {
            bgm.pause().unwrap();
        }
        self.bgm = bgm;
        self.audio = audio;
        self.resume(tm).unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 2, "foreground must recover audio before sending play");
        self.next = 1;
        self.tr_start = f32::NAN;
        assert!(matches!(self.next_scene(tm), NextScene::Pop));
    }
}
