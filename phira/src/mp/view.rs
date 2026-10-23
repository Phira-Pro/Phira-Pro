//! Multiplayer presentation only. Session and room rules live in their own modules.
// Layout reference: ppy/osu 920019169c2cb7ea3c6956499e529450064619ea,
// MultiplayerMatchSubScreen / ParticipantPanel / MultiplayerMatchFooter.
use super::{protocol::Phase, room::Snapshot};
use crate::{
    get_data,
    icons::Icons,
    page::{local_illustration, Illustration},
};
use macroquad::prelude::*;
use prpr::{
    core::BOLD_FONT,
    ext::{semi_white, RectExt, ScaleType},
    ui::{clip_rounded_rect, DRectButton, Scroll, Ui},
};
use std::sync::Arc;

const PINK: u32 = 0xff78b7;
const GREEN: u32 = 0x80dcc6;
const MUTED: u32 = 0xb6acbd;

#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub frame: Rect,
    header: Rect,
    server: Rect,
    body: Rect,
    footer: Rect,
    members: Rect,
    chart: Rect,
    chat: Rect,
}
impl Layout {
    pub fn new(top: f32) -> Self {
        let frame = Rect::new(-0.94, -top + 0.028, 1.88, top * 2. - 0.056);
        let header = Rect::new(frame.x, frame.y, frame.w, 0.10);
        let server = Rect::new(frame.x, header.bottom() + 0.016, frame.w, 0.07);
        let footer = Rect::new(frame.x, frame.bottom() - 0.085, frame.w, 0.085);
        let body = Rect::new(frame.x, server.bottom() + 0.016, frame.w, footer.y - server.bottom() - 0.032);
        let members = Rect::new(body.x, body.y, 0.45, body.h);
        let chart = Rect::new(members.right() + 0.018, body.y, 0.75, body.h);
        let chat = Rect::new(chart.right() + 0.018, body.y, body.right() - chart.right() - 0.018, body.h);
        Self {
            frame,
            header,
            server,
            body,
            footer,
            members,
            chart,
            chat,
        }
    }
}

pub(super) struct RoomView {
    buttons: Vec<DRectButton>,
    messages: Scroll,
    members: Scroll,
    icons: Arc<Icons>,
    artwork: Option<(i32, Illustration)>,
    message_count: usize,
    follow_messages: bool,
}
impl RoomView {
    pub fn new(icons: Arc<Icons>) -> Self {
        let buttons = (0..12)
            .map(|_| {
                let mut b = DRectButton::new();
                b.config.radius = 0.012;
                b
            })
            .collect();
        Self {
            buttons,
            messages: Scroll::new(),
            members: Scroll::new(),
            icons,
            artwork: None,
            message_count: 0,
            follow_messages: false,
        }
    }
    pub fn update(&mut self, s: &Snapshot, t: f32) {
        self.messages.update(t);
        self.members.update(t);
        let chart = s.room.as_ref().and_then(|r| r.chart);
        if self.artwork.as_ref().map(|(id, _)| *id) != chart {
            self.artwork = chart.and_then(|id| {
                let local = get_data().charts.iter().find(|c| c.info.id == Some(id))?;
                let illu = local_illustration(local.local_path.clone(), self.icons.r#abstract.clone(), true);
                illu.notify();
                Some((id, illu))
            });
        }
        if let Some((_, illu)) = &mut self.artwork {
            illu.settle(t);
        }
        if self.message_count != s.messages.len() {
            self.message_count = s.messages.len();
            self.follow_messages = true;
        }
    }
    pub fn invalidate(&mut self) {
        for b in &mut self.buttons {
            b.invalidate();
        }
    }
    pub fn suspend(&mut self, t: f32) {
        for b in &mut self.buttons {
            b.cancel(t);
        }
        self.invalidate();
        self.messages.y_scroller.suspend(t);
        self.members.y_scroller.suspend(t);
    }
    pub fn touch(&mut self, touch: &Touch, t: f32) -> (bool, Option<usize>) {
        if self.messages.touch(touch, t) || self.members.touch(touch, t) {
            return (true, None);
        }
        for (i, b) in self.buttons.iter_mut().enumerate() {
            if b.touch(touch, t) {
                return (true, Some(i));
            }
        }
        (false, None)
    }
    fn button(&mut self, ui: &mut Ui, id: usize, r: Rect, t: f32, label: &str, primary: bool, enabled: bool) {
        let color = Color::from_hex_rgb(if primary { PINK } else { 0x514656 });
        self.buttons[id].render_shadow(ui, r, t, |ui, path| {
            ui.fill_path(
                &path,
                Color {
                    a: if enabled { 1. } else { 0.35 },
                    ..color
                },
            );
            ui.text(label)
                .pos(r.center().x, r.center().y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(if primary { 0.44 } else { 0.34 })
                .max_width(r.w - 0.025)
                .color(if enabled { WHITE } else { semi_white(0.35) })
                .draw_using(&BOLD_FONT);
        });
        if !enabled {
            self.buttons[id].invalidate();
        }
    }
    fn section(ui: &mut Ui, r: Rect, label: &str, detail: &str) {
        ui.fill_path(&r.rounded(0.016), Color::from_hex_rgb(0x302c36));
        ui.text(label)
            .pos(r.x + 0.02, r.y + 0.022)
            .size(0.35)
            .color(Color::from_hex_rgb(PINK))
            .draw_using(&BOLD_FONT);
        ui.text(detail)
            .pos(r.right() - 0.02, r.y + 0.024)
            .anchor(1., 0.)
            .size(0.29)
            .color(Color::from_hex_rgb(MUTED))
            .draw();
    }
    pub fn render(&mut self, ui: &mut Ui, s: &Snapshot, address: &str, connecting: bool, preparing: bool, busy: bool, t: f32) {
        self.invalidate();
        let l = Layout::new(ui.top);
        ui.text("MULTIPLAYER")
            .pos(l.header.x, l.header.y)
            .size(0.25)
            .color(Color::from_hex_rgb(PINK))
            .draw_using(&BOLD_FONT);
        ui.text(s.room.as_ref().map_or("多人联机", |r| r.id.as_str()))
            .pos(l.header.x, l.header.y + 0.034)
            .size(0.65)
            .max_width(1.22)
            .draw_using(&BOLD_FONT);
        let close = Rect::new(l.header.right() - 0.065, l.header.y + 0.008, 0.065, 0.065);
        let icon = self.icons.close.clone();
        self.buttons[11].build(ui, t, close, |ui, path| {
            ui.fill_path(&path, Color::from_hex_rgb(0x514656));
            let r = close.feather(-0.02);
            ui.fill_rect(r, (*icon, r, ScaleType::Fit));
        });
        let (status, color) = if connecting {
            ("正在连接", MUTED)
        } else if s.reconnecting {
            ("恢复连接中", PINK)
        } else if s.authenticated {
            ("已连接", GREEN)
        } else {
            ("未连接", MUTED)
        };
        let badge = Rect::new(close.x - 0.25, close.y + 0.01, 0.22, 0.042);
        ui.fill_path(&badge.rounded(0.021), Color::from_hex_rgb(0x3e3644));
        ui.fill_circle(badge.x + 0.021, badge.center().y, 0.005, Color::from_hex_rgb(color));
        ui.text(status)
            .pos(badge.x + 0.04, badge.center().y)
            .anchor(0., 0.5)
            .no_baseline()
            .size(0.28)
            .color(Color::from_hex_rgb(color))
            .draw();

        ui.fill_path(&l.server.rounded(0.012), Color::from_hex_rgb(0x25222c));
        let address_rect = Rect::new(l.server.x + 0.012, l.server.y + 0.009, l.server.w - 0.30, l.server.h - 0.018);
        self.buttons[0].build(ui, t, address_rect, |ui, _| {
            ui.text("服务器")
                .pos(address_rect.x + 0.01, address_rect.center().y)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.29)
                .color(Color::from_hex_rgb(MUTED))
                .draw();
            ui.text(if address.is_empty() { "点击设置服务器地址" } else { address })
                .pos(address_rect.x + 0.14, address_rect.center().y)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.33)
                .max_width(address_rect.w - 0.16)
                .draw();
        });
        if connecting || s.connected || s.reconnecting {
            self.buttons[0].invalidate();
        }
        self.button(
            ui,
            1,
            Rect::new(l.server.right() - 0.25, l.server.y + 0.009, 0.238, l.server.h - 0.018),
            t,
            if connecting {
                "连接中…"
            } else if s.reconnecting {
                "取消恢复"
            } else if s.connected {
                "断开连接"
            } else {
                "连接"
            },
            !s.connected,
            !connecting && !busy,
        );

        if s.room.is_none() {
            self.landing(ui, l, s, busy || connecting, t);
            return;
        }
        let room = s.room.as_ref().unwrap();
        let usable = s.authenticated && !s.reconnecting && !busy;
        self.participants(ui, l.members, s);
        self.chart(ui, l.chart, s, usable, t);
        self.chat(ui, l.chat, s, usable, t);

        self.button(ui, 7, Rect::new(l.footer.x, l.footer.y + 0.008, 0.23, 0.069), t, "离开房间", false, usable);
        let hint = if preparing {
            "正在检查谱面资源…"
        } else if busy {
            "正在等待服务器响应…"
        } else if s.reconnecting {
            "正在恢复连接，请稍候"
        } else {
            "成绩与回放保存在 Pro"
        };
        ui.text(hint)
            .pos(l.footer.x + 0.255, l.footer.center().y)
            .anchor(0., 0.5)
            .no_baseline()
            .size(0.28)
            .max_width(0.7)
            .color(Color::from_hex_rgb(MUTED))
            .draw();
        let ready = Rect::new(l.footer.right() - 0.53, l.footer.y, 0.53, l.footer.h);
        let (id, label) = match room.phase {
            Phase::Selecting if room.host => (5, "准备并开始"),
            Phase::Waiting if room.ready => (6, "取消准备"),
            Phase::Waiting => (5, "准备"),
            Phase::Playing => (5, "等待本局结束"),
            _ => (5, "等待房主开始"),
        };
        let enabled = usable && !preparing && room.phase != Phase::Playing && (room.host || room.phase == Phase::Waiting);
        self.button(ui, id, ready, t, label, true, enabled);
    }
    fn landing(&mut self, ui: &mut Ui, l: Layout, s: &Snapshot, busy: bool, t: f32) {
        let card = Rect::new(l.body.x, l.body.y, l.body.w, l.body.h);
        ui.fill_path(&card.rounded(0.018), Color::from_hex_rgb(0x302c36));
        let cy = card.center().y;
        ui.text(if s.authenticated {
            "选择你的房间"
        } else {
            "连接联机服务器"
        })
        .pos(card.x + 0.055, cy - 0.09)
        .size(0.69)
        .max_width(0.83)
        .draw_using(&BOLD_FONT);
        ui.text(if s.authenticated {
            "创建房间，或输入房间 ID 加入。"
        } else {
            "先设置服务器地址，再连接并创建或加入房间。"
        })
        .pos(card.x + 0.055, cy + 0.004)
        .size(0.33)
        .max_width(0.82)
        .multiline()
        .color(Color::from_hex_rgb(MUTED))
        .draw();
        let action_x = card.x + card.w * 0.54;
        self.button(ui, 2, Rect::new(action_x, cy - 0.092, card.w * 0.42, 0.08), t, "创建房间", true, s.authenticated && !busy);
        self.button(ui, 3, Rect::new(action_x, cy + 0.015, card.w * 0.42, 0.08), t, "加入房间", false, s.authenticated && !busy);
        ui.text("关闭面板后仍会保留连接")
            .pos(l.footer.x, l.footer.center().y)
            .anchor(0., 0.5)
            .no_baseline()
            .size(0.30)
            .color(Color::from_hex_rgb(MUTED))
            .draw();
    }
    fn participants(&mut self, ui: &mut Ui, r: Rect, s: &Snapshot) {
        let room = s.room.as_ref().unwrap();
        Self::section(ui, r, "房间成员", &room.users.len().to_string());
        let area = Rect::new(r.x + 0.012, r.y + 0.065, r.w - 0.024, r.h - 0.077);
        self.members.size((area.w, area.h));
        ui.scope(|ui| {
            ui.dx(area.x);
            ui.dy(area.y);
            self.members.render(ui, |ui| {
                for (i, user) in room.users.values().enumerate() {
                    let y = i as f32 * 0.087;
                    let row = Rect::new(0., y, area.w, 0.077);
                    let me = s.me.as_ref().is_some_and(|me| me.id == user.id);
                    ui.fill_path(&row.rounded(0.01), Color::from_hex_rgb(if me { 0x49384a } else { 0x3b3541 }));
                    ui.fill_circle(0.038, y + 0.038, 0.025, Color::from_hex_rgb(if me { PINK } else { 0x685d74 }));
                    let icon = Rect::new(0.023, y + 0.023, 0.03, 0.03);
                    ui.fill_rect(icon, (*self.icons.user, icon));
                    ui.text(&user.name)
                        .pos(0.077, y + 0.012)
                        .size(0.34)
                        .max_width(area.w - 0.09)
                        .draw_using(&BOLD_FONT);
                    ui.text(if user.monitor {
                        "观战"
                    } else if me && room.host {
                        "房主 · 你"
                    } else if me && room.ready {
                        "已准备 · 你"
                    } else if me {
                        "你"
                    } else {
                        "在房间"
                    })
                    .pos(0.077, y + 0.045)
                    .size(0.23)
                    .color(Color::from_hex_rgb(MUTED))
                    .draw();
                }
                (area.w, room.users.len() as f32 * 0.087)
            });
        });
    }
    fn chart(&mut self, ui: &mut Ui, r: Rect, s: &Snapshot, usable: bool, t: f32) {
        let room = s.room.as_ref().unwrap();
        Self::section(
            ui,
            r,
            "当前谱面",
            match room.phase {
                Phase::Selecting => "选曲",
                Phase::Waiting => "等待准备",
                Phase::Playing => "游玩中",
            },
        );
        let artwork = Rect::new(r.x + 0.018, r.y + 0.065, r.w - 0.036, (r.h - 0.25).clamp(0., (r.w - 0.036) * 9. / 16.));
        let tex = self.artwork.as_ref().map_or(&self.icons.r#abstract, |(_, illu)| &illu.texture.1);
        if artwork.h > 0.005 {
            clip_rounded_rect(ui, artwork, 0.01, |ui| ui.fill_rect(artwork, (**tex, artwork, ScaleType::CropCenter)));
        }
        let info = room.chart.and_then(|id| get_data().charts.iter().find(|c| c.info.id == Some(id)));
        let title = info
            .map(|c| c.info.name.clone())
            .unwrap_or_else(|| room.chart.map_or("尚未选择谱面".into(), |id| format!("谱面 #{id}")));
        ui.text(title)
            .pos(artwork.x, artwork.bottom() + 0.015)
            .size(0.45)
            .max_width(artwork.w)
            .draw_using(&BOLD_FONT);
        let subtitle = info.map_or_else(|| "选择已上架谱面开始联机".into(), |c| format!("{} · {}", c.info.level, c.info.composer));
        if r.h >= 0.3 {
            ui.text(subtitle)
                .pos(artwork.x, artwork.bottom() + 0.065)
                .size(0.28)
                .max_width(artwork.w)
                .color(Color::from_hex_rgb(MUTED))
                .draw();
        }
        let by = r.bottom() - 0.071;
        self.button(ui, 4, Rect::new(r.x + 0.018, by, 0.26, 0.05), t, "选择谱面 ID", false, usable && room.host && room.phase == Phase::Selecting);
        let w = (r.w - 0.31) / 2.;
        self.button(ui, 8, Rect::new(r.x + 0.29, by, w, 0.05), t, if room.locked { "已锁房" } else { "锁房" }, false, usable && room.host);
        self.button(ui, 9, Rect::new(r.x + 0.30 + w, by, w, 0.05), t, if room.cycle { "轮换开" } else { "轮换关" }, false, usable && room.host);
    }
    fn chat(&mut self, ui: &mut Ui, r: Rect, s: &Snapshot, usable: bool, t: f32) {
        Self::section(ui, r, "房间聊天", "");
        let area = Rect::new(r.x + 0.018, r.y + 0.065, r.w - 0.036, r.h - 0.15);
        self.messages.size((area.w, area.h));
        let mut content_h = 0.;
        ui.scope(|ui| {
            ui.dx(area.x);
            ui.dy(area.y);
            self.messages.render(ui, |ui| {
                let mut y = 0.;
                if s.messages.is_empty() {
                    ui.text("房间消息会显示在这里")
                        .pos(0., 0.01)
                        .size(0.28)
                        .max_width(area.w)
                        .color(Color::from_hex_rgb(MUTED))
                        .draw();
                }
                for text in &s.messages {
                    let bounds = ui
                        .text(text)
                        .pos(0., y)
                        .size(0.28)
                        .max_width(area.w)
                        .multiline()
                        .color(semi_white(0.84))
                        .draw();
                    y += bounds.h + 0.023;
                }
                content_h = y;
                (area.w, y)
            });
        });
        if self.follow_messages {
            self.messages.y_scroller.goto = Some((content_h - area.h).max(0.));
            self.follow_messages = false;
        }
        self.button(ui, 10, Rect::new(r.x + 0.018, r.bottom() - 0.067, r.w - 0.036, 0.05), t, "发送消息…", false, usable);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn room_regions_fit_tablet_phone_and_ultrawide() {
        for top in [0.75, 9. / 16., 9. / 20., 9. / 21., 9. / 32.] {
            let l = Layout::new(top);
            assert!(l.body.h > 0.15);
            for r in [l.header, l.server, l.body, l.footer, l.members, l.chart, l.chat] {
                assert!(r.x >= l.frame.x - 1e-5 && r.right() <= l.frame.right() + 1e-5);
                assert!(r.y >= l.frame.y - 1e-5 && r.bottom() <= l.frame.bottom() + 1e-5);
            }
            assert!(l.members.right() < l.chart.x && l.chart.right() < l.chat.x);
            assert!(l.body.bottom() < l.footer.y);
        }
    }
}
