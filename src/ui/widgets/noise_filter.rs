//! 噪音过滤下拉:历史页与连接页筛选栏共用(漏斗按钮 + 隐藏本地/
//! 局域网两项可勾选)。选中态以前置对钩标示,不用 selectable_label
//! 的选中底色——两项同选时底色相同,对钩才能直观区分。

use eframe::egui;
use egui::{Color32, RichText, containers::PopupCloseBehavior, containers::menu::MenuConfig};

use super::super::{TOOLBAR_ROW_H, icons, text_width, theme};
use crate::i18n::I18n;

/// 菜单行高(与工具栏行高一致,保证点击目标)
const ROW_H: f32 = TOOLBAR_ROW_H;
/// 菜单项水平内边距与对钩列宽(对钩列预留宽度,未勾选留透明占位)
const ITEM_PAD: f32 = theme::sp::SM;
const CHECK_COL_W: f32 = 18.0;

/// 漏斗按钮 + 两项可勾选;任一项开启时按钮 accent 高亮。勾选点击后
/// 菜单保持(CloseOnClickOutside),可连续勾两项,点外部/Esc 关闭。
/// 返回本帧是否有切换(调用方据此刷新查询并持久化 config)
pub fn noise_filter(
    ui: &mut egui::Ui,
    i18n: &I18n,
    hide_local: &mut bool,
    hide_lan: &mut bool,
) -> bool {
    let mut changed = false;
    // 按钮文案 = 漏斗 + 文本;右置实心三角与 ComboBox 默认箭头同源
    // (同尺寸/同形/同取色),不用字形 caret——字形宽扁,与协议/时间
    // 下拉的箭头明显不一致。按钮按 min_size 预留三角宽度,文字左对齐
    let label = format!("{}  {}", icons::FUNNEL, i18n.t("filter-noise"));
    let pad = ui.spacing().button_padding;
    let icon_w = ui.spacing().icon_width;
    let btn_w = text_width(ui, &label, theme::font::BODY)
        + ui.spacing().icon_spacing
        + icon_w
        + pad.x * 2.0;
    let mut button = egui::Button::new(RichText::new(label).size(theme::font::BODY))
        .min_size(egui::vec2(btn_w, ROW_H))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_SM));
    if *hide_local || *hide_lan {
        button = button.fill(theme::c().accent_soft);
    }
    // 外层布局强制 main_align=Min:left_to_right 的 main_align 默认
    // Center(egui 为"按钮内文字居中"而设),文字会在预留了三角宽度的
    // 内容区里居中,与右置三角重叠;左对齐后文字贴内容区左缘、三角贴
    // 右缘,与 ComboBox 的文本/箭头排布一致
    let (resp, popup) = ui
        .with_layout(
            egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min),
            |ui| {
                egui::containers::menu::MenuButton::from_button(button)
                    .config(
                        MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside),
                    )
                    .ui(ui, |ui| {
                        ui.with_layout(
                            egui::Layout::top_down(egui::Align::LEFT).with_cross_justify(true),
                            |ui| {
                                if check_item(ui, *hide_local, i18n.t("filter-hide-local")) {
                                    *hide_local = !*hide_local;
                                    changed = true;
                                }
                                ui.add_space(theme::sp::XS);
                                if check_item(ui, *hide_lan, i18n.t("filter-hide-lan")) {
                                    *hide_lan = !*hide_lan;
                                    changed = true;
                                }
                            },
                        );
                    })
            },
        )
        .inner;
    // 叠加绘制实心三角(复刻 ComboBox 默认箭头:内容区右置 icon_width
    // 方形内接 70%x45% 三角,取交互态 fg 色;菜单展开用 open 态)
    let visuals = if popup.is_some() {
        ui.visuals().widgets.open
    } else {
        *ui.style().interact(&resp)
    };
    let content = resp.rect.shrink2(egui::vec2(pad.x, 0.0));
    let icon_rect = egui::Align2::RIGHT_CENTER
        .align_size_within_rect(egui::vec2(icon_w, icon_w), content)
        .expand(visuals.expansion);
    let tri = egui::Rect::from_center_size(
        icon_rect.center(),
        egui::vec2(icon_rect.width() * 0.7, icon_rect.height() * 0.45),
    );
    ui.painter().add(egui::Shape::convex_polygon(
        vec![tri.left_top(), tri.right_top(), tri.center_bottom()],
        visuals.fg_stroke.color,
        egui::Stroke::NONE,
    ));
    changed
}

/// 可勾选菜单项:整行可点(悬停高亮),前置对钩标示选中(未勾选留
/// 透明占位保持文字对齐)。行宽按内容算(对钩列 + 文字 + 内边距),
/// 由父布局 cross_justify 拉齐——不能取 available_width,弹窗会继承
/// 工具栏行的剩余宽度而撑满半个窗口。返回是否被点击
fn check_item(ui: &mut egui::Ui, checked: bool, text: String) -> bool {
    let text_w = text_width(ui, &text, theme::font::BODY);
    let row_w = ITEM_PAD + CHECK_COL_W + text_w + ITEM_PAD;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(row_w, ROW_H), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::RADIUS_SM),
            theme::c().hover_bg,
        );
    }
    let painter = ui.painter();
    let center_y = rect.center().y;
    // 对钩:勾选取 accent,未勾选透明占位
    let check_color = if checked {
        theme::c().accent
    } else {
        Color32::TRANSPARENT
    };
    let galley = painter.layout_no_wrap(
        icons::CHECK_LG.to_owned(),
        egui::FontId::proportional(theme::font::SM),
        check_color,
    );
    painter.galley(
        egui::pos2(
            rect.left() + ITEM_PAD,
            center_y - galley.rect.height() * 0.5,
        ),
        galley,
        check_color,
    );
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(theme::font::BODY),
        theme::c().text,
    );
    painter.galley(
        egui::pos2(
            rect.left() + ITEM_PAD + CHECK_COL_W,
            center_y - galley.rect.height() * 0.5,
        ),
        galley,
        theme::c().text,
    );
    resp.clicked()
}
