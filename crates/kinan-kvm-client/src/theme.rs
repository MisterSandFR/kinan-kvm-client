//! Lightweight theming: tuned `Visuals` for dark and light modes.
use egui::epaint::Shadow;
use egui::{Color32, Context, Rounding, Stroke, Visuals};

use crate::config::Theme;

pub fn apply(ctx: &Context, theme: Theme) {
    let mut visuals = match theme {
        Theme::Light => Visuals::light(),
        Theme::Dark => Visuals::dark(),
        Theme::System => {
            if ctx.style().visuals.dark_mode {
                Visuals::dark()
            } else {
                Visuals::light()
            }
        }
    };
    style_visuals(&mut visuals, matches!(theme, Theme::Dark));
    ctx.set_visuals(visuals);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(14.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.visuals.widgets.active.rounding = Rounding::same(6.0);
    style.visuals.widgets.hovered.rounding = Rounding::same(6.0);
    style.visuals.widgets.inactive.rounding = Rounding::same(6.0);
    style.visuals.widgets.noninteractive.rounding = Rounding::same(6.0);
    style.visuals.widgets.open.rounding = Rounding::same(6.0);
    style.visuals.window_rounding = Rounding::same(10.0);
    style.visuals.panel_fill = style.visuals.window_fill;
    ctx.set_style(style);
}

fn style_visuals(v: &mut Visuals, dark: bool) {
    if dark {
        v.window_fill = Color32::from_rgb(20, 24, 31);
        v.panel_fill = Color32::from_rgb(16, 19, 25);
        v.extreme_bg_color = Color32::from_rgb(10, 12, 16);
        v.widgets.noninteractive.bg_fill = Color32::from_rgb(28, 32, 40);
        v.widgets.inactive.bg_fill = Color32::from_rgb(38, 44, 55);
        v.widgets.hovered.bg_fill = Color32::from_rgb(50, 60, 76);
        v.widgets.active.bg_fill = Color32::from_rgb(56, 70, 92);
        v.selection.bg_fill = Color32::from_rgb(70, 110, 180);
        v.hyperlink_color = Color32::from_rgb(120, 170, 240);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(210, 215, 225));
    } else {
        v.window_fill = Color32::from_rgb(246, 247, 250);
        v.panel_fill = Color32::from_rgb(241, 243, 247);
        v.extreme_bg_color = Color32::WHITE;
        v.widgets.noninteractive.bg_fill = Color32::from_rgb(230, 234, 240);
        v.widgets.inactive.bg_fill = Color32::from_rgb(220, 226, 234);
        v.widgets.hovered.bg_fill = Color32::from_rgb(200, 212, 230);
        v.widgets.active.bg_fill = Color32::from_rgb(160, 185, 220);
        v.selection.bg_fill = Color32::from_rgb(70, 130, 220);
        v.hyperlink_color = Color32::from_rgb(40, 80, 170);
    }
    v.popup_shadow = Shadow {
        offset: egui::vec2(0.0, 2.0),
        blur: 12.0,
        spread: 0.0,
        color: Color32::from_black_alpha(80),
    };
    v.window_shadow = v.popup_shadow;
}
