//! Keyboard layout and binding editor.
use super::{Page, SharedState};
use crate::{get_data, get_data_mut, save_data};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{hotkeys::{Action, Bindings}, ui::{DRectButton, Scroll, Ui}};
use std::borrow::Cow;

fn layout() -> Vec<(KeyCode, Rect)> {
    use KeyCode::*;
    let rows = vec![
        vec![(GraveAccent,1.),(Key1,1.),(Key2,1.),(Key3,1.),(Key4,1.),(Key5,1.),(Key6,1.),(Key7,1.),(Key8,1.),(Key9,1.),(Key0,1.),(Minus,1.),(Equal,1.),(Backspace,2.)],
        vec![(Tab,1.5),(Q,1.),(W,1.),(E,1.),(R,1.),(T,1.),(Y,1.),(U,1.),(I,1.),(O,1.),(P,1.),(LeftBracket,1.),(RightBracket,1.),(Backslash,1.5)],
        vec![(CapsLock,1.75),(A,1.),(S,1.),(D,1.),(F,1.),(G,1.),(H,1.),(J,1.),(K,1.),(L,1.),(Semicolon,1.),(Apostrophe,1.),(Enter,2.25)],
        vec![(LeftShift,2.25),(Z,1.),(X,1.),(C,1.),(V,1.),(B,1.),(N,1.),(M,1.),(Comma,1.),(Period,1.),(Slash,1.),(RightShift,2.75)],
        vec![(LeftControl,1.25),(LeftSuper,1.25),(LeftAlt,1.25),(Space,6.25),(RightAlt,1.25),(RightSuper,1.25),(Menu,1.25),(RightControl,1.25)],
    ];
    let mut keys = vec![(Escape,Rect::new(0.,0.,1.,1.))];
    for (i,key) in [F1,F2,F3,F4,F5,F6,F7,F8,F9,F10,F11,F12].into_iter().enumerate() {
        keys.push((key,Rect::new(2. + i as f32 + (i/4) as f32*0.5,0.,1.,1.)));
    }
    for (row,items) in rows.into_iter().enumerate() {
        let mut x=0.;
        for (key,width) in items { keys.push((key,Rect::new(x,row as f32+1.25,width,1.))); x+=width; }
    }
    for (row,items) in [[PrintScreen,ScrollLock,Pause],[Insert,Home,PageUp],[Delete,End,PageDown]].into_iter().enumerate() {
        for (col,key) in items.into_iter().enumerate() { keys.push((key,Rect::new(15.5+col as f32,if row==0 {0.} else {row as f32+0.25},1.,1.))); }
    }
    keys.push((Up,Rect::new(16.5,4.25,1.,1.)));
    for (col,key) in [Left,Down,Right].into_iter().enumerate() { keys.push((key,Rect::new(15.5+col as f32,5.25,1.,1.))); }
    for (row,items) in [[NumLock,KpDivide,KpMultiply,KpSubtract],[Kp7,Kp8,Kp9,KpAdd],[Kp4,Kp5,Kp6,Unknown],[Kp1,Kp2,Kp3,KpEnter],[Kp0,Unknown,KpDecimal,Unknown]].into_iter().enumerate() {
        for (col,key) in items.into_iter().enumerate() {
            if key==Unknown { continue; }
            keys.push((key,Rect::new(19.+col as f32,row as f32+1.25,if key==Kp0 {2.} else {1.},if key==KpAdd || key==KpEnter {2.} else {1.})));
        }
    }
    keys
}

fn label(key: KeyCode) -> String {
    use KeyCode::*;
    match key {
        KpDivide => "÷".into(), KpMultiply => "×".into(), KpSubtract => "−".into(), KpAdd => "+".into(), KpDecimal => ".".into(), KpEnter => "Enter".into(), NumLock => "Num".into(),
        Escape => "Esc".into(), Backspace => "Back".into(), CapsLock => "Caps".into(),
        LeftShift|RightShift => "Shift".into(), LeftControl|RightControl => "Ctrl".into(),
        LeftAlt|RightAlt => "Alt".into(), LeftSuper|RightSuper => "Super".into(),
        Left => "←".into(), Right => "→".into(), Up => "↑".into(), Down => "↓".into(),
        PageUp => "PgUp".into(), PageDown => "PgDn".into(), PrintScreen => "PrtSc".into(), ScrollLock => "ScrLk".into(),
        GraveAccent => "`".into(), Minus => "-".into(), Equal => "=".into(), LeftBracket => "[".into(), RightBracket => "]".into(), Backslash => "\\".into(),
        Semicolon => ";".into(), Apostrophe => "'".into(), Comma => ",".into(), Period => ".".into(), Slash => "/".into(),
        _ => format!("{key:?}").trim_start_matches("Key").trim_start_matches("Kp").to_string(),
    }
}
pub struct HotkeyPage {
    scroll: Scroll,
    keys: Vec<(KeyCode, Rect, DRectButton)>,
    selected: KeyCode,
    actions: [DRectButton; 6],
    restore: DRectButton,
    rebind: DRectButton,
    remove: DRectButton,
    rows: Vec<(KeyCode, DRectButton)>,
    capture: bool,
}
impl HotkeyPage {
    pub fn new() -> Self {
        Self { scroll: Scroll::new(), keys: layout().into_iter().map(|(k,r)| (k,r,DRectButton::new())).collect(), selected: KeyCode::Space,
            actions: std::array::from_fn(|_| DRectButton::new()), restore: DRectButton::new(), rebind: DRectButton::new(), remove: DRectButton::new(), rows: Vec::new(), capture: false }
    }
    fn rebind_to(&mut self, key: KeyCode) -> Result<()> {
        let action = get_data().config.hotkeys.action(self.selected);
        if key != self.selected {
            get_data_mut().config.hotkeys.set(self.selected, Action::Disabled);
            get_data_mut().config.hotkeys.set(key, action);
            self.selected = key;
            save_data()?;
        }
        self.capture = false;
        Ok(())
    }
    fn set(&self, action: Action) -> Result<()> {
        get_data_mut().config.hotkeys.set(self.selected, action);
        save_data()
    }
}
impl Page for HotkeyPage {
    fn label(&self) -> Cow<'static, str> { "热键管理".into() }
    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        if self.capture {
            if let Some(key) = prpr::hotkeys::pressed_keys().into_iter().next() {
                self.rebind_to(key)?;
            }
        }
        Ok(())
    }
    fn on_back_pressed(&mut self, _: &mut SharedState) -> bool {
        std::mem::take(&mut self.capture)
    }
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.capture {
            let key = self.keys.iter_mut().find_map(|(key,_,button)| button.touch(touch,s.t).then_some(*key));
            if let Some(key) = key { self.rebind_to(key)?; }
            return Ok(true);
        }
        if self.scroll.touch(touch, s.t) { return Ok(true); }
        for (key,_,button) in &mut self.keys {
            if button.touch(touch,s.t) { self.selected = *key; return Ok(true); }
        }
        for (i,button) in self.actions.iter_mut().enumerate() {
            if button.touch(touch,s.t) { self.set(Action::ALL[i])?; return Ok(true); }
        }
        if self.remove.touch(touch,s.t) { self.set(Action::Disabled)?; return Ok(true); }
        if self.rebind.touch(touch,s.t) { self.capture = true; return Ok(true); }
        if self.restore.touch(touch,s.t) {
            get_data_mut().config.hotkeys = Bindings::default(); save_data()?; return Ok(true);
        }
        for (key, button) in &mut self.rows {
            if button.touch(touch,s.t) { self.selected = *key; return Ok(true); }
        }
        Ok(false)
    }
    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let r = ui.content_rect();
        let t = s.t;
        let bindings = get_data().config.hotkeys.clone();
        let mut list = vec![KeyCode::Space, KeyCode::Left, KeyCode::Right, KeyCode::Q];
        for key in prpr::hotkeys::KEYS {
            if bindings.overrides.contains_key(&(*key as u16)) && !list.contains(key) { list.push(*key); }
        }
        if self.rows.iter().map(|r| r.0).collect::<Vec<_>>() != list {
            self.rows = list.into_iter().map(|k| (k,DRectButton::new())).collect();
        }
        s.render_fader(ui, |ui| {
            self.scroll.size((r.w,r.h));
            ui.dx(r.x); ui.dy(r.y);
            self.scroll.render(ui, |ui| {
                let w = r.w - 0.03;
                ui.text("点击键盘选择按键，再选择下方操作。设置会自动保存。").size(0.34).pos(0.01,0.01).draw();
                let unit = w / 23.;
                let row_h = 0.085;
                for (key,grid,button) in &mut self.keys {
                    let rect = Rect::new(grid.x*unit,0.065+grid.y*row_h,grid.w*unit-0.004,grid.h*row_h-0.008);
                    button.render_text(ui,rect,t,&label(*key),0.23,*key==self.selected);
                    if bindings.action(*key) != Action::Hit && bindings.action(*key) != Action::Disabled {
                        ui.fill_rect(Rect::new(rect.x+0.008,rect.bottom()-0.007,rect.w-0.016,0.003),SKYBLUE);
                    }
                }
                let mut y = 0.065+6.25*row_h;
                ui.text(format!("{}：{}", label(self.selected),bindings.action(self.selected).label())).size(0.38).pos(0.,y).draw();
                y += 0.06;
                for (i,button) in self.actions.iter_mut().enumerate() {
                    button.render_text(ui,Rect::new(i as f32*w/6.,y,w/6.-0.008,0.07),t,Action::ALL[i].label(),0.28,bindings.action(self.selected)==Action::ALL[i]);
                }
                y += 0.085;
                self.rebind.render_text(ui,Rect::new(0.,y,w*0.32,0.07),t,if self.capture { "按键或点击键盘" } else { "修改为其他按键" },0.3,false);
                self.remove.render_text(ui,Rect::new(w*0.34,y,w*0.30,0.07),t,"删除此热键",0.3,false);
                self.restore.render_text(ui,Rect::new(w*0.66,y,w*0.33,0.07),t,"恢复全部默认",0.3,false);
                y += 0.09;
                ui.text("音符判定需要开启键盘模式。跳转操作仅在允许交互时生效。").size(0.3).pos(0.,y).draw();
                y += 0.055;
                ui.text("默认热键和自定义热键（点击编辑）").size(0.36).pos(0.,y).draw();
                y += 0.06;
                for (key,button) in &mut self.rows {
                    button.render_text(ui,Rect::new(0.,y,w,0.065),t,&format!("{}    {}",label(*key),bindings.action(*key).label()),0.32,*key==self.selected);
                    y += 0.075;
                }
                (r.w,y)
            });
        });
        Ok(())
    }
}
