//! 右下角通知牌堆:默认只完整显示最新一张,更早的向下错开露边;
//! 悬停平铺展开(EXPAND_MS 缓动),超出安全高度时滚轮回看更早条目。
//! 计时策略在 mod.rs(堆顶消化+悬停暂停),这里只管几何与交互。
//! 展开布局:视口右下角锚定,内容自上而下 = 新到旧;滚动 0 = 最新
//! 在视口顶(与"默认只展示最新"心智一致),滚轮向下翻看更早。

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, CornerRadius, FontId, Rect, Sense, Stroke};

use super::{Toast, ToastAction};
use crate::ui::{icons, theme};

/// 卡片固定宽高:单行内容,牌堆几何需预知尺寸
const CARD_W: f32 = 320.0;
const CARD_H: f32 = 46.0;
/// 收拢态相邻卡片错开的露边高度
const EDGE: f32 = 6.0;
/// 展开态卡片间距(不含卡高)
const GAP: f32 = 8.0;
/// 展开动画时长
const EXPAND_MS: f32 = 150.0;
/// 右下角锚点内缩
const ANCHOR_MARGIN: f32 = 16.0;
/// 展开视口顶部安全边距(避让标题栏)
const SAFE_TOP: f32 = 64.0;

/// 牌堆交互状态(App 持有,跨帧保留)
pub struct StackState {
    /// 展开进度 0..1(1 = 完全平铺)
    pub(super) expand: f32,
    /// 展开态滚动偏移(0 = 最新在视口顶)
    pub(super) scroll_off: f32,
    /// 指针是否悬停在判定区(本帧值由 draw 更新,mod.rs 下一帧用于暂停计时)
    pub(super) hover: bool,
    last_tick: Instant,
}

impl Default for StackState {
    fn default() -> Self {
        Self {
            expand: 0.0,
            scroll_off: 0.0,
            hover: false,
            last_tick: Instant::now(),
        }
    }
}

impl StackState {
    /// 帧间隔(物理时间,驱动 TTL 消化与展开动画)
    pub(super) fn tick(&mut self) -> Duration {
        let dt = self.last_tick.elapsed();
        self.last_tick = Instant::now();
        dt
    }

    /// 牌堆清空时复位视图状态
    pub(super) fn reset(&mut self) {
        self.expand = 0.0;
        self.scroll_off = 0.0;
        self.hover = false;
    }
}

/// smoothstep 缓动:动画两端速度为零,收拢/展开不生硬
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// 本帧被点击的卡片(seq 供 mod.rs 定位消化)
pub(super) struct CardClick {
    pub(super) seq: u64,
    pub(super) action: ToastAction,
}

pub(super) fn draw(
    ctx: &egui::Context,
    p: &theme::Palette,
    items: &[&Toast],
    st: &mut StackState,
    dt: Duration,
) -> Option<CardClick> {
    let n = items.len();
    let step_e = CARD_H + GAP;
    let content_h = CARD_H + (n as f32 - 1.0) * step_e;
    let collapsed_h = CARD_H + (n as f32 - 1.0) * EDGE;
    let mut top_card: Option<Rect> = None;
    let mut click: Option<CardClick> = None;
    egui::Area::new(egui::Id::unique("toast-stack"))
        .order(egui::Order::Foreground)
        .anchor(
            Align2::RIGHT_BOTTOM,
            egui::vec2(-ANCHOR_MARGIN, -ANCHOR_MARGIN),
        )
        .show(ctx, |ui| {
            // 零尺寸探针定位锚点(Area 的 RIGHT_BOTTOM 锚把探针右下角
            // 钉在窗口右下内缩处);交互区改经 ui.interact 注册,
            // 不参与 Area 尺寸测量
            let (probe, _) = ui.allocate_exact_size(egui::Vec2::ZERO, Sense::hover());
            // 几何全部以锚点为基准(局部坐标系)
            let right = probe.right();
            let bottom = probe.bottom();
            // 视口顶部安全边(避让标题栏);高取展开内容高与可用高的较小值
            let avail_h = (bottom - SAFE_TOP + ANCHOR_MARGIN).max(CARD_H);
            let view_h = content_h.min(avail_h);
            let overflow = (content_h - view_h).max(0.0);
            let view = Rect::from_min_max(
                egui::pos2(right - CARD_W, bottom - view_h),
                egui::pos2(right, bottom),
            );

            // 判定/交互区跟随上一帧动画态的实际包围盒:收拢时只有
            // 收拢高,指针碰到牌堆上方空白(展开后才占的区域)不触发
            // 展开、不挡周围页面操作;展开后跟随展开高度,指针划过
            // 卡片间隙不收拢(用推进前的 expand 避免判定与动画循环依赖)
            let eased_before = ease(st.expand);
            let judge_h = collapsed_h + (view_h - collapsed_h) * eased_before;
            let judge = Rect::from_min_max(
                egui::pos2(right - CARD_W, bottom - judge_h),
                egui::pos2(right, bottom),
            );
            // 注册交互区:hit-test 层级遮挡让地图等底层画布的 hovered
            // 失效,堆上滚轮/拖拽不穿透
            ui.interact(judge, egui::Id::unique("toast-stack-hit"), Sense::hover());

            st.hover = ui
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|pos| judge.contains(pos));
            // 离开牌堆即复位滚动:下次展开从最新一张开始
            if !st.hover {
                st.scroll_off = 0.0;
            }
            // 滚轮向下 = 翻看更早条目(smooth_scroll_delta 已含惯性与触控板)
            if st.hover && overflow > 0.0 {
                let dy = ui.input(|i| i.smooth_scroll_delta.y);
                st.scroll_off = (st.scroll_off + dy).clamp(0.0, overflow);
            }
            // 展开进度定时推进
            let dir = if st.hover { 1.0 } else { -1.0 };
            st.expand = (st.expand + dir * dt.as_secs_f32() * 1000.0 / EXPAND_MS).clamp(0.0, 1.0);
            let eased = ease(st.expand);

            let painter = ui.painter().with_clip_rect(view);
            // 从最老到最新绘制:上层新卡覆盖下层旧卡,自然形成露边堆叠;
            // interact 同序注册,同层后注册者赢 hit-test,与视觉层叠一致
            for (j, t) in items.iter().enumerate() {
                let top_c = bottom - CARD_H - j as f32 * EDGE;
                let top_e = view.top() - st.scroll_off + (n as f32 - 1.0 - j as f32) * step_e;
                let top = top_c + (top_e - top_c) * eased;
                let card = Rect::from_min_size(
                    egui::pos2(right - CARD_W, top),
                    egui::vec2(CARD_W, CARD_H),
                );
                // 可点卡注册点击区并给悬停反馈(描边提亮 + 手型光标)
                let mut hovered = false;
                if let Some(action) = t.action {
                    let resp = ui.interact(
                        card,
                        egui::Id::unique(("toast-card", t.seq)),
                        egui::Sense::click(),
                    );
                    hovered = resp.hovered();
                    if hovered {
                        ui.ctx()
                            .output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        click = Some(CardClick { seq: t.seq, action });
                    }
                }
                draw_card(&painter, p, t, card, hovered);
                top_card = Some(card);
            }
            // 收拢态堆数角标:最新卡右上角 +N(N-1 = 被压住的条数)
            if let Some(card) = top_card
                && eased < 0.5
                && n > 1
            {
                draw_count(&painter, p, n - 1, card);
            }
        });

    // 倒计时消化需要周期帧;悬停(平滑滚动)与展开动画需要连续帧
    let animating = st.hover || (st.expand > 0.0 && st.expand < 1.0);
    ctx.request_repaint_after(Duration::from_millis(if animating { 16 } else { 250 }));
    click
}

/// 单张卡片:圆角背景 + 边框 + 语义图标 + 单行文本(超宽截断);
/// hovered = 可点卡悬停时描边提亮 accent
fn draw_card(painter: &egui::Painter, p: &theme::Palette, t: &Toast, rect: Rect, hovered: bool) {
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_MD), p.bg_elevated);
    let stroke_color = if hovered { p.accent } else { p.stroke };
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_MD),
        Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );
    let (glyph, accent) = match t.kind {
        super::ToastKind::Info => (icons::ETHERNET, p.accent),
        super::ToastKind::Warn => (icons::EXCLAMATION_CIRCLE, p.status_warn),
        super::ToastKind::Success => (icons::CHECK_CIRCLE, p.status_ok),
    };
    let cy = rect.center().y;
    let icon_x = rect.left() + theme::sp::MD;
    painter.text(
        egui::pos2(icon_x, cy),
        Align2::LEFT_CENTER,
        glyph,
        FontId::proportional(theme::font::H3),
        accent,
    );
    let text_x = icon_x + theme::font::H3 + theme::sp::SM;
    let avail = (rect.right() - theme::sp::MD - text_x).max(0.0);
    let text = truncate(painter, &t.text, theme::font::BODY, avail);
    painter.text(
        egui::pos2(text_x, cy),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(theme::font::BODY),
        p.text,
    );
}

/// 堆数角标:胶囊底 + 内嵌 +N 计数(纯数字,与语言无关)
fn draw_count(painter: &egui::Painter, p: &theme::Palette, count: usize, card: Rect) {
    let label = format!("+{count}");
    let font = FontId::proportional(theme::font::MICRO);
    let text_w = painter
        .layout_no_wrap(label.clone(), font.clone(), egui::Color32::WHITE)
        .rect
        .width();
    let h = theme::font::MICRO + 6.0;
    let w = text_w + 10.0;
    let r = Rect::from_min_size(
        egui::pos2(card.right() - w - 5.0, card.top() - h * 0.5),
        egui::vec2(w, h),
    );
    painter.rect_filled(r, CornerRadius::same(theme::RADIUS_PILL), p.accent);
    painter.text(r.center(), Align2::CENTER_CENTER, label, font, p.on_accent);
}

/// 按像素尾部省略(与 map::card::fit_text 同型,此处固定宽度场景的局部实现)
fn truncate(painter: &egui::Painter, text: &str, size: f32, max_w: f32) -> String {
    let font = FontId::proportional(size);
    let width = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
            .rect
            .width()
    };
    if width(text) <= max_w {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while chars.len() > 1 {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if width(&candidate) <= max_w {
            return candidate;
        }
    }
    "…".to_owned()
}
