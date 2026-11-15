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

    pub(in crate::scene) fn retry_after_offset(&mut self, tm: &mut TimeManager, offset: f32) -> Box<dyn std::any::Any> {
        self.applied_offset = Some(offset);
        let NextScene::PopWithResult(result) = self.next_scene(tm) else {
            panic!("retry must pass the applied offset back to gameplay");
        };
        result
    }
}
