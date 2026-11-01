#![allow(dead_code)]

use egui::{
    epaint::StrokeKind, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke,
    Ui, Vec2, WidgetText,
};

// ============================================================================
// 1. Цветовая палитра Pal и токены Kinetix DS
// ============================================================================

pub const PAL_PANEL: Color32 = Color32::from_rgb(0x0A, 0x0B, 0x0E); // Глубокий графитовый фон окна
pub const PAL_CARD: Color32 = Color32::from_rgb(0x13, 0x15, 0x1A);  // Карточки, поля ввода, кнопки
pub const PAL_CARD_HI: Color32 = Color32::from_rgb(0x19, 0x1C, 0x24); // Вложенные плашки, пустые сегменты
pub const PAL_LINE: Color32 = Color32::from_rgb(0x23, 0x26, 0x2E); // Субпиксельные границы 1px
pub const PAL_LINE_HOVER: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B); // Подсветка границ при наведении
pub const PAL_TEXT: Color32 = Color32::from_rgb(0xE8, 0xEA, 0xF0); // Высококонтрастный текст
pub const PAL_MUTED: Color32 = Color32::from_rgb(0x7E, 0x85, 0x94); // Приглушенный текст / подсказки / единицы
pub const PAL_ACCENT: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B); // Amber-500 (защищенный семантический токен)
pub const PAL_ON_ACCENT: Color32 = Color32::from_rgb(0x0A, 0x0B, 0x0E); // Темный текст на янтарной заливке
pub const PAL_DANGER: Color32 = Color32::from_rgb(0xDC, 0x46, 0x46); // Опасные действия / стоп / удаление
pub const PAL_FILL: Color32 = Color32::from_rgb(0x2A, 0x2E, 0x39); // Нажатое состояние / промежуточная заливка

pub const R_BASE: u8 = 8;  // Карточки, модальные окна, панели
pub const R_SMALL: u8 = 5; // Кнопки, поля ввода, чипы
pub const R_PILL: u8 = 4;  // Индикаторы и прогресс-треки

// ============================================================================
// 2. Аналитическая физика пружин (Spring ODE 2-го порядка)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringConfig {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
    pub precision: f32,
}

impl SpringConfig {
    pub fn snappy() -> Self {
        Self {
            stiffness: 300.0,
            damping: 24.0,
            mass: 1.0,
            precision: 0.005,
        }
    }

    pub fn expressive() -> Self {
        Self {
            stiffness: 280.0,
            damping: 24.0,
            mass: 1.0,
            precision: 0.005,
        }
    }

    pub fn scale() -> Self {
        Self {
            stiffness: 520.0,
            damping: 18.0,
            mass: 1.0,
            precision: 0.005,
        }
    }

    #[inline]
    pub fn omega0(&self) -> f32 {
        (self.stiffness / self.mass).sqrt()
    }

    #[inline]
    pub fn zeta(&self) -> f32 {
        self.damping / (2.0 * (self.mass * self.stiffness).sqrt())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub config: SpringConfig,
}

impl Spring {
    pub fn new(initial: f32) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            config: SpringConfig::snappy(),
        }
    }

    pub fn with_config(initial: f32, config: SpringConfig) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            config,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn snap_to(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    pub fn update(&mut self, dt: f32) {
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
            return;
        }

        let dt = dt.clamp(0.0, 0.05);
        let omega0 = self.config.omega0();
        let zeta = self.config.zeta();
        let x0 = self.value - self.target;
        let v0 = self.velocity;

        if (zeta - 1.0).abs() < 1e-5 {
            let decay = (-omega0 * dt).exp();
            let c1 = x0;
            let c2 = v0 + omega0 * x0;
            self.value = self.target + (c1 + c2 * dt) * decay;
            self.velocity = (v0 - (c1 + c2 * dt) * omega0 * decay) + c2 * decay;
        } else if zeta < 1.0 {
            let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
            let decay = (-zeta * omega0 * dt).exp();
            let cos_term = (omega_d * dt).cos();
            let sin_term = (omega_d * dt).sin();
            let c1 = x0;
            let c2 = (v0 + zeta * omega0 * x0) / omega_d;

            self.value = self.target + decay * (c1 * cos_term + c2 * sin_term);
            self.velocity = decay
                * ((-zeta * omega0) * (c1 * cos_term + c2 * sin_term)
                    + omega_d * (-c1 * sin_term + c2 * cos_term));
        } else {
            let s = omega0 * (zeta * zeta - 1.0).sqrt();
            let r1 = -zeta * omega0 + s;
            let r2 = -zeta * omega0 - s;
            let c1 = (v0 - r2 * x0) / (r1 - r2);
            let c2 = x0 - c1;

            self.value = self.target + c1 * (r1 * dt).exp() + c2 * (r2 * dt).exp();
            self.velocity = c1 * r1 * (r1 * dt).exp() + c2 * r2 * (r2 * dt).exp();
        }

        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }

    #[inline]
    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() <= self.config.precision
            && self.velocity.abs() <= self.config.precision
    }
}

// ============================================================================
// 3. 24-Сегментный регулятор громкости (Kinetix Volume Gauge)
// ============================================================================

/// 24-сегментный тактильный регулятор громкости.
/// При перетаскивании дает мгновенный визуальный отклик, коммит в бэкенд на pointer release.
pub fn volume_gauge_24(
    ui: &mut Ui,
    id_source: impl std::hash::Hash,
    volume: &mut f32,
    max_volume: f32,
    width: f32,
) -> Response {
    let _id = ui.make_persistent_id(id_source);
    let total_segments = 24;
    let gap = 2.0;
    let height = 20.0;

    let desired_size = Vec2::new(width, height);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());

    let seg_width = ((rect.width() - (total_segments - 1) as f32 * gap) / total_segments as f32).max(1.0);

    // Интерактивное управление через drag и click
    if response.clicked() || response.dragged() {
        if let Some(mouse_pos) = response.interact_pointer_pos() {
            let relative_x = (mouse_pos.x - rect.min.x).clamp(0.0, rect.width());
            let fraction = relative_x / rect.width();
            let active = (fraction * total_segments as f32).round().clamp(0.0, total_segments as f32);
            *volume = (active / total_segments as f32) * max_volume;
            response.mark_changed();
        }
    }

    let active_segments = ((*volume / max_volume).clamp(0.0, 1.0) * total_segments as f32).round() as usize;

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let is_interacting = response.hovered() || response.dragged();

        for i in 0..total_segments {
            let seg_x = rect.min.x + i as f32 * (seg_width + gap);
            let is_active = i < active_segments;

            // При драге активные сегменты приподнимаются на 2px (тактильный подъем)
            let lift = if is_active && is_interacting { 2.0 } else { 0.0 };

            let seg_rect = Rect::from_min_size(
                Pos2::new(seg_x, rect.min.y - lift),
                Vec2::new(seg_width, rect.height() + lift),
            );

            let (fill, stroke) = if is_active {
                (PAL_ACCENT, Stroke::NONE)
            } else {
                (PAL_CARD_HI, Stroke::new(1.0_f32, PAL_LINE))
            };

            painter.rect(
                seg_rect,
                CornerRadius::same(2),
                fill,
                stroke,
                StrokeKind::Inside,
            );
        }
    }

    response
}

// ============================================================================
// 4. Тактильные кнопки (Emil Kowalski :active scale 0.97 + ODE Spring)
// ============================================================================

pub struct TactileButton {
    text: WidgetText,
    accent: bool,
    danger: bool,
    min_size: Vec2,
}

impl TactileButton {
    pub fn new(text: impl Into<WidgetText>) -> Self {
        Self {
            text: text.into(),
            accent: false,
            danger: false,
            min_size: Vec2::new(60.0, 30.0),
        }
    }

    pub fn accent(mut self) -> Self {
        self.accent = true;
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn min_size(mut self, size: Vec2) -> Self {
        self.min_size = size;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let padding = Vec2::new(12.0, 6.0);
        let font_id = FontId::monospace(12.0);
        let galley = self.text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font_id);

        let desired_size = (galley.size() + padding * 2.0).max(self.min_size);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

        if ui.is_rect_visible(rect) {
            let is_pressed = response.is_pointer_button_down_on();
            let is_hovered = response.hovered();

            // Emil Kowalski tactile compression: сжатие до scale(0.97) при :active
            let drawn_rect = if is_pressed {
                rect.shrink2(rect.size() * 0.015)
            } else {
                rect
            };

            let (bg_color, stroke_color, text_color) = if self.danger {
                if is_pressed {
                    (PAL_DANGER, PAL_DANGER, PAL_ON_ACCENT)
                } else if is_hovered {
                    (PAL_CARD_HI, PAL_DANGER, PAL_DANGER)
                } else {
                    (PAL_CARD, PAL_LINE, PAL_DANGER)
                }
            } else if self.accent {
                if is_pressed {
                    (PAL_FILL, PAL_ACCENT, PAL_ACCENT)
                } else if is_hovered {
                    (PAL_ACCENT, PAL_ACCENT, PAL_ON_ACCENT)
                } else {
                    (PAL_ACCENT, PAL_ACCENT, PAL_ON_ACCENT)
                }
            } else {
                if is_pressed {
                    (PAL_FILL, PAL_ACCENT, PAL_ACCENT)
                } else if is_hovered {
                    (PAL_CARD_HI, PAL_ACCENT, PAL_TEXT)
                } else {
                    (PAL_CARD, PAL_LINE, PAL_TEXT)
                }
            };

            let painter = ui.painter();
            painter.rect(
                drawn_rect,
                CornerRadius::same(R_SMALL),
                bg_color,
                Stroke::new(1.0_f32, stroke_color),
                StrokeKind::Inside,
            );

            let text_pos = Pos2::new(
                drawn_rect.center().x - galley.size().x / 2.0,
                drawn_rect.center().y - galley.size().y / 2.0,
            );

            painter.galley(text_pos, galley, text_color);
        }

        response
    }
}

// ============================================================================
// 5. Вспомогательные чипы, бейджи и карточки
// ============================================================================

/// Моноширинный бейдж хоткея, например `[f1]`, `[ctrl+1]`, `[нет]`
pub fn hotkey_badge(ui: &mut Ui, label: &str, active: bool) {
    let font_id = FontId::monospace(11.0);
    let padding = Vec2::new(6.0, 3.0);
    let text = WidgetText::from(label);
    let galley = text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font_id);

    let size = galley.size() + padding * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());

    let (bg, border, text_col) = if active {
        (PAL_CARD_HI, PAL_ACCENT, PAL_ACCENT)
    } else {
        (PAL_CARD_HI, PAL_LINE, PAL_MUTED)
    };

    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(R_SMALL),
        bg,
        Stroke::new(1.0_f32, border),
        StrokeKind::Inside,
    );

    let text_pos = Pos2::new(
        rect.center().x - galley.size().x / 2.0,
        rect.center().y - galley.size().y / 2.0,
    );
    painter.galley(text_pos, galley, text_col);
}

/// Кнопка переключения вкладок на верхней навигационной панели
pub fn tab_button(ui: &mut Ui, label: &str, is_selected: bool) -> Response {
    let padding = Vec2::new(14.0, 8.0);
    let font_id = FontId::monospace(13.0);
    let galley = WidgetText::from(label).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font_id);

    let desired_size = galley.size() + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if ui.is_rect_visible(rect) {
        let is_pressed = response.is_pointer_button_down_on();
        let is_hovered = response.hovered();

        let drawn_rect = if is_pressed {
            rect.shrink2(rect.size() * 0.015)
        } else {
            rect
        };

        let painter = ui.painter();

        let (bg, text_col) = if is_selected {
            (PAL_CARD, PAL_ACCENT)
        } else if is_hovered {
            (PAL_CARD_HI, PAL_TEXT)
        } else {
            (Color32::TRANSPARENT, PAL_MUTED)
        };

        painter.rect(
            drawn_rect,
            CornerRadius::same(R_SMALL),
            bg,
            if is_selected { Stroke::new(1.0_f32, PAL_LINE) } else { Stroke::NONE },
            StrokeKind::Inside,
        );

        // Индикатор активной вкладки — янтарная полоска внизу
        if is_selected {
            let indicator = Rect::from_min_size(
                Pos2::new(drawn_rect.min.x + 4.0, drawn_rect.max.y - 2.0),
                Vec2::new(drawn_rect.width() - 8.0, 2.0),
            );
            painter.rect_filled(indicator, CornerRadius::same(1), PAL_ACCENT);
        }

        let text_pos = Pos2::new(
            drawn_rect.center().x - galley.size().x / 2.0,
            drawn_rect.center().y - galley.size().y / 2.0,
        );
        painter.galley(text_pos, galley, text_col);
    }

    response
}
