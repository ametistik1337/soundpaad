#![allow(unused_variables, clippy::all)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use egui::{
    CentralPanel, Context, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, Pos2, Rect, ScrollArea, Sense, Stroke, TextEdit, TopBottomPanel, Ui, Vec2,
};

use crate::AppState;
use crate::db::models::{Bind, ShopSound, Track};
use crate::ui::widgets::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Tracks,
    Shop,
    Binds,
    Settings,
}

enum AsyncUiMessage {
    ShopSounds(Vec<ShopSound>),
    ShopError(String),
    TrackDownloaded(String),
    Toast(String),
}

pub struct SoundpadApp {
    state: Arc<Mutex<AppState>>,
    active_tab: Tab,

    // Вкладка «Треки»
    tracks_search: String,
    tracks: Vec<Track>,
    track_volumes: HashMap<i64, f32>,
    master_volume: f32,

    // Вкладка «Магазин»
    shop_search: String,
    shop_sounds: Vec<ShopSound>,
    shop_loading: bool,
    shop_error: Option<String>,
    active_preview_url: Option<String>,

    // Вкладка «Бинды»
    binds: Vec<Bind>,
    binding_track_id: Option<i64>,

    // Вкладка «Настройки»
    input_sources: Vec<String>,
    selected_source: String,
    mic_passthrough_enabled: bool,
    play_to_speakers: bool,
    virtual_mic_active: bool,

    // Асинхронные каналы для фоновых задач
    async_tx: Sender<AsyncUiMessage>,
    async_rx: Receiver<AsyncUiMessage>,

    // Всплывающие уведомления (Toast)
    toast: Option<(String, Instant)>,
    last_refresh: Instant,

    // Пружинный моушен для скользящего индикатора табов и аудио-визуализатора
    tab_spring_x: Spring,
    tab_spring_w: Spring,
    tab_initialized: bool,
    audio_phase: f32,
    last_frame: Instant,
}

impl SoundpadApp {
    pub fn new(cc: &eframe::CreationContext<'_>, state: Arc<Mutex<AppState>>) -> Self {
        // Настройка типографики: JetBrains Mono
        setup_fonts(&cc.egui_ctx);

        // Настройка визуального стиля Kinetix Pal
        setup_kinetix_visuals(&cc.egui_ctx);

        let (async_tx, async_rx) = crossbeam_channel::unbounded();

        let mut app = Self {
            state,
            active_tab: Tab::Tracks,
            tracks_search: String::new(),
            tracks: Vec::new(),
            track_volumes: HashMap::new(),
            master_volume: 1.0,
            shop_search: String::new(),
            shop_sounds: Vec::new(),
            shop_loading: false,
            shop_error: None,
            active_preview_url: None,
            binds: Vec::new(),
            binding_track_id: None,
            input_sources: Vec::new(),
            selected_source: String::new(),
            mic_passthrough_enabled: false,
            play_to_speakers: true,
            virtual_mic_active: false,
            async_tx,
            async_rx,
            toast: None,
            last_refresh: Instant::now(),
            tab_spring_x: Spring::with_config(0.0, SpringConfig::highlight()),
            tab_spring_w: Spring::with_config(70.0, SpringConfig::highlight()),
            tab_initialized: false,
            audio_phase: 0.0,
            last_frame: Instant::now(),
        };

        app.refresh_data();
        app
    }

    pub fn refresh_data(&mut self) {
        if let Ok(state) = self.state.lock() {
            if let Ok(tracks) = state.db.get_tracks() {
                for track in &tracks {
                    if let Some(id) = track.id {
                        self.track_volumes.entry(id).or_insert(track.volume as f32);
                    }
                }
                self.tracks = tracks;
            }

            if let Ok(binds) = state.db.get_binds() {
                self.binds = binds;
            }

            self.virtual_mic_active = state.virtual_mic.is_active();
            self.play_to_speakers = state.player.play_to_speakers();
            self.mic_passthrough_enabled = state.player.is_mic_passthrough_active();
            self.selected_source = state.player.real_mic_source().to_string();

            if let Ok(sources) = crate::audio::virtual_mic::VirtualMic::list_sources() {
                self.input_sources = sources;
            }
        }
    }

    fn set_toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    fn check_async_messages(&mut self) {
        while let Ok(msg) = self.async_rx.try_recv() {
            match msg {
                AsyncUiMessage::ShopSounds(sounds) => {
                    self.shop_sounds = sounds;
                    self.shop_loading = false;
                    self.shop_error = None;
                }
                AsyncUiMessage::ShopError(err) => {
                    self.shop_error = Some(err);
                    self.shop_loading = false;
                }
                AsyncUiMessage::TrackDownloaded(name) => {
                    self.set_toast(format!("звук «{}» скачан и добавлен", name));
                    self.refresh_data();
                }
                AsyncUiMessage::Toast(msg) => {
                    self.set_toast(msg);
                }
            }
        }
    }

    fn handle_key_capture(&mut self, ctx: &Context) {
        if let Some(track_id) = self.binding_track_id {
            let mut captured = None;
            ctx.input(|i| {
                for event in &i.events {
                    if let egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } = event
                    {
                        if *key == egui::Key::Escape || *key == egui::Key::Delete || *key == egui::Key::Backspace {
                            // Очистка бинда при нажатии Escape, Delete или Backspace
                            captured = Some(None);
                        } else {
                            let keyval = egui_key_to_code(*key);
                            if keyval != 0 {
                                let mod_bits = egui_modifiers_to_bits(modifiers);
                                captured = Some(Some((keyval, mod_bits)));
                            }
                        }
                        break;
                    }
                }
            });

            if let Some(maybe_key) = captured {
                if let Some((keyval, modifiers)) = maybe_key {
                    if let Ok(state) = self.state.lock() {
                        let _ = state.db.delete_bind_by_track(track_id);
                        let _ = state.db.add_bind(&Bind {
                            id: None,
                            track_id,
                            keyval,
                            modifiers,
                        });
                    }
                    self.set_toast(format!("хоткей [{}] привязан", format_key(keyval, modifiers)));
                    self.refresh_data();
                } else {
                    // Очистка бинда
                    if let Ok(state) = self.state.lock() {
                        let _ = state.db.delete_bind_by_track(track_id);
                    }
                    self.set_toast("бинд очищен");
                    self.refresh_data();
                }
                self.binding_track_id = None;
            }
        }
    }
}

impl eframe::App for SoundpadApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let dt = self.last_frame.elapsed().as_secs_f32().clamp(0.0, 0.05);
        self.last_frame = Instant::now();
        self.audio_phase += dt * 5.0;
        self.tab_spring_x.update(dt);
        self.tab_spring_w.update(dt);

        self.check_async_messages();
        self.handle_key_capture(ctx);

        // Периодическое легкое обновление состояния (каждые 2 секунды)
        if self.last_refresh.elapsed() > Duration::from_secs(2) {
            self.last_refresh = Instant::now();
            self.refresh_data();
        }

        // Проверяем, есть ли играющие звуки для continuous 60/120fps анимации
        let is_any_playing = if let Ok(state) = self.state.lock() {
            self.tracks.iter().any(|t| t.id.map(|id| state.player.is_playing(id)).unwrap_or(false))
        } else {
            false
        } || self.active_preview_url.is_some();

        if is_any_playing || !self.tab_spring_x.is_settled() || !self.tab_spring_w.is_settled() {
            ctx.request_repaint();
        }

        // ====================================================================
        // Верхняя панель: логотип, вкладки со скользящей пружиной и мастер-громкость
        // ====================================================================
        TopBottomPanel::top("top_panel")
            .frame(egui::Frame {
                fill: PAL_PANEL,
                inner_margin: egui::Margin::symmetric(16, 10),
                stroke: Stroke::new(1.0_f32, PAL_LINE),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Бренд soundpaad
                    ui.label(
                        egui::RichText::new("soundpaad")
                            .color(PAL_ACCENT)
                            .font(FontId::monospace(16.0))
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("v0.2.0")
                            .color(PAL_MUTED)
                            .font(FontId::monospace(10.0)),
                    );

                    ui.add_space(20.0);

                    // Табы приложения (строчные подписи по Kinetix со скользящей ODE-пружиной)
                    let (t_tracks, resp_tracks) = tab_button(ui, "треки", self.active_tab == Tab::Tracks);
                    if resp_tracks.clicked() {
                        self.active_tab = Tab::Tracks;
                        self.refresh_data();
                    }
                    let (t_shop, resp_shop) = tab_button(ui, "магазин", self.active_tab == Tab::Shop);
                    if resp_shop.clicked() {
                        self.active_tab = Tab::Shop;
                    }
                    let (t_binds, resp_binds) = tab_button(ui, "бинды", self.active_tab == Tab::Binds);
                    if resp_binds.clicked() {
                        self.active_tab = Tab::Binds;
                        self.refresh_data();
                    }
                    let (t_settings, resp_settings) = tab_button(ui, "настройки", self.active_tab == Tab::Settings);
                    if resp_settings.clicked() {
                        self.active_tab = Tab::Settings;
                        self.refresh_data();
                    }

                    let active_rect = match self.active_tab {
                        Tab::Tracks => t_tracks,
                        Tab::Shop => t_shop,
                        Tab::Binds => t_binds,
                        Tab::Settings => t_settings,
                    };

                    if !self.tab_initialized {
                        self.tab_spring_x.snap_to(active_rect.min.x);
                        self.tab_spring_w.snap_to(active_rect.width());
                        self.tab_initialized = true;
                    } else {
                        self.tab_spring_x.set_target(active_rect.min.x);
                        self.tab_spring_w.set_target(active_rect.width());
                    }

                    // Скользящий пружинный индикатор активной вкладки
                    let pill_rect = Rect::from_min_size(
                        Pos2::new(self.tab_spring_x.value, active_rect.min.y),
                        Vec2::new(self.tab_spring_w.value, active_rect.height()),
                    );
                    let painter = ui.painter();
                    let bottom_indicator = Rect::from_min_size(
                        Pos2::new(pill_rect.min.x + 4.0, pill_rect.max.y - 2.5),
                        Vec2::new(pill_rect.width() - 8.0, 2.5),
                    );
                    painter.rect_filled(bottom_indicator, CornerRadius::same(1), PAL_ACCENT);

                    // Правая часть: мастер-громкость и кнопка «стоп все»
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if TactileButton::new("стоп все").danger().show(ui).clicked() {
                            if let Ok(mut state) = self.state.lock() {
                                state.player.stop_all();
                            }
                            self.active_preview_url = None;
                            self.set_toast("воспроизведение остановлено");
                        }

                        ui.add_space(10.0);

                        // 24-сегментный мастер-регулятор громкости
                        ui.label(
                            egui::RichText::new("мастер:")
                                .color(PAL_MUTED)
                                .font(FontId::monospace(11.0)),
                        );
                        let gauge = volume_gauge_24(
                            ui,
                            "master_vol",
                            &mut self.master_volume,
                            1.0,
                            140.0,
                        );
                        if gauge.changed() {
                            // Мгновенная регулировка громкости
                        }
                    });
                });
            });

        // ====================================================================
        // Нижняя панель: статус и Toast-сообщения
        // ====================================================================
        TopBottomPanel::bottom("bottom_panel")
            .frame(egui::Frame {
                fill: PAL_PANEL,
                inner_margin: egui::Margin::symmetric(16, 6),
                stroke: Stroke::new(1.0_f32, PAL_LINE),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let mic_status = if self.virtual_mic_active {
                        egui::RichText::new("микрофон: [активен]")
                            .color(PAL_ACCENT)
                            .font(FontId::monospace(11.0))
                    } else {
                        egui::RichText::new("микрофон: [неактивен]")
                            .color(PAL_MUTED)
                            .font(FontId::monospace(11.0))
                    };
                    ui.label(mic_status);

                    let pt_status = if self.mic_passthrough_enabled {
                        egui::RichText::new(" | сквозной: [вкл]")
                            .color(PAL_ACCENT)
                            .font(FontId::monospace(11.0))
                    } else {
                        egui::RichText::new(" | сквозной: [выкл]")
                            .color(PAL_MUTED)
                            .font(FontId::monospace(11.0))
                    };
                    ui.label(pt_status);

                    let spk_status = if self.play_to_speakers {
                        egui::RichText::new(" | наушники: [вкл]")
                            .color(PAL_TEXT)
                            .font(FontId::monospace(11.0))
                    } else {
                        egui::RichText::new(" | наушники: [выкл]")
                            .color(PAL_MUTED)
                            .font(FontId::monospace(11.0))
                    };
                    ui.label(spk_status);

                    // Отображение всплывающего Toast
                    if let Some((ref text, ref time)) = self.toast {
                        if time.elapsed() < Duration::from_secs(4) {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(format!("-> {}", text))
                                        .color(PAL_ACCENT)
                                        .font(FontId::monospace(11.0)),
                                );
                            });
                        }
                    }
                });
            });

        // ====================================================================
        // Центральная панель: отображение выбранной вкладки
        // ====================================================================
        CentralPanel::default()
            .frame(egui::Frame {
                fill: PAL_PANEL,
                inner_margin: egui::Margin::same(16),
                ..Default::default()
            })
            .show(ctx, |ui| {
                // Если сейчас идет захват клавиши
                if let Some(track_id) = self.binding_track_id {
                    render_bind_capture_overlay(ui, track_id, &self.tracks);
                } else {
                    match self.active_tab {
                        Tab::Tracks => self.render_tracks_tab(ui),
                        Tab::Shop => self.render_shop_tab(ui),
                        Tab::Binds => self.render_binds_tab(ui),
                        Tab::Settings => self.render_settings_tab(ui),
                    }
                }
            });

        // Плавный ре-рендер при взаимодействии или воспроизведении
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

impl SoundpadApp {
    // ========================================================================
    // Вкладка 1: Треки (Soundboard)
    // ========================================================================
    fn render_tracks_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if TactileButton::new("+ добавить звук").accent().show(ui).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Аудиофайлы (*.mp3, *.wav, *.ogg, *.flac)", &["mp3", "wav", "ogg", "flac"])
                    .pick_file()
                {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("sound")
                        .to_string();
                    let file_path = path.to_string_lossy().to_string();

                    if let Ok(state) = self.state.lock() {
                        let _ = state.db.add_track(&Track {
                            id: None,
                            name: name.clone(),
                            file_path,
                            category_id: None,
                            volume: 1.0,
                            source: "local".to_string(),
                        });
                    }
                    self.set_toast(format!("трек «{}» добавлен", name));
                    self.refresh_data();
                }
            }

            ui.add_space(8.0);

            // Поле фильтрации звуков
            ui.add(
                TextEdit::singleline(&mut self.tracks_search)
                    .hint_text(
                        egui::RichText::new("поиск звуков...")
                            .color(PAL_MUTED)
                            .font(FontId::monospace(12.0)),
                    )
                    .font(FontId::monospace(12.0))
                    .desired_width(220.0),
            );

            ui.label(
                egui::RichText::new(format!("всего: {}", self.tracks.len()))
                    .color(PAL_MUTED)
                    .font(FontId::monospace(11.0)),
            );
        });

        ui.add_space(12.0);

        // Список карточек звуков
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let filter = self.tracks_search.trim().to_lowercase();
                let filtered: Vec<Track> = self
                    .tracks
                    .iter()
                    .filter(|t| filter.is_empty() || t.name.to_lowercase().contains(&filter))
                    .cloned()
                    .collect();

                if filtered.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("нет звуков в библиотеке")
                                .color(PAL_MUTED)
                                .font(FontId::monospace(14.0)),
                        );
                        ui.label(
                            egui::RichText::new("добавьте аудиофайл через кнопку «+ добавить звук» или скачайте из вкладки «магазин»")
                                .color(PAL_MUTED)
                                .font(FontId::monospace(11.0)),
                        );
                    });
                    return;
                }

                let mut to_play: Option<(i64, String, f32)> = None;
                let mut to_stop: Option<i64> = None;
                let mut to_delete: Option<i64> = None;
                let mut to_bind: Option<i64> = None;

                for track in filtered {
                    let track_id = track.id.unwrap_or(0);
                    let track_name = track.name.clone();
                    let file_path = track.file_path.clone();

                    // Ищем назначенный хоткей
                    let bound_key = self.binds.iter().find(|b| b.track_id == track_id);

                    // Проверяем статус воспроизведения
                    let is_playing = if let Ok(state) = self.state.lock() {
                        state.player.is_playing(track_id)
                    } else {
                        false
                    };

                    // Карточка трека по канону Kinetix: фон PAL_CARD, R_BASE = 8px, граница 1px
                    egui::Frame {
                        fill: PAL_CARD,
                        inner_margin: egui::Margin::symmetric(14, 10),
                        corner_radius: CornerRadius::same(R_BASE),
                        stroke: Stroke::new(
                            1.0_f32,
                            if is_playing { PAL_GREEN } else { PAL_LINE },
                        ),
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Кнопка Play / Stop
                            if is_playing {
                                if TactileButton::new("■").danger().min_size(Vec2::new(32.0, 32.0)).show(ui).clicked() {
                                    to_stop = Some(track_id);
                                }
                            } else {
                                if TactileButton::new("▶").green().min_size(Vec2::new(32.0, 32.0)).show(ui).clicked() {
                                    to_play = Some((track_id, file_path.clone(), self.master_volume));
                                }
                            }

                            ui.add_space(8.0);

                            // Название звука + живой анимированный эквалайзер
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(&track_name)
                                            .color(if is_playing { PAL_GREEN } else { PAL_TEXT })
                                            .font(FontId::monospace(13.0))
                                            .strong(),
                                    );
                                    if is_playing {
                                        ui.add_space(8.0);
                                        let (eq_rect, _) = ui.allocate_exact_size(Vec2::new(28.0, 14.0), Sense::hover());
                                        draw_animated_equalizer(ui.painter(), eq_rect, self.audio_phase, PAL_GREEN);
                                    }
                                });
                                ui.label(
                                    egui::RichText::new(&file_path)
                                        .color(PAL_MUTED)
                                        .font(FontId::monospace(9.0)),
                                );
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Кнопка удаления
                                if TactileButton::new("✕").danger().min_size(Vec2::new(26.0, 26.0)).show(ui).clicked() {
                                    to_delete = Some(track_id);
                                }

                                ui.add_space(8.0);

                                // Бейдж хоткея
                                let badge_label = match bound_key {
                                    Some(b) => format!("[{}]", format_key(b.keyval, b.modifiers)),
                                    None => "[+ бинд]".to_string(),
                                };

                                if TactileButton::new(badge_label)
                                    .min_size(Vec2::new(50.0, 26.0))
                                    .show(ui)
                                    .clicked()
                                {
                                    to_bind = Some(track_id);
                                }
                            });
                        });
                    });

                    ui.add_space(6.0);
                }

                // Обработка действий со звуками
                if let Some((id, path, vol)) = to_play {
                    if let Ok(mut state) = self.state.lock() {
                        let _ = state.player.play(id, &path, vol);
                    }
                }
                if let Some(id) = to_stop {
                    if let Ok(mut state) = self.state.lock() {
                        let _ = state.player.stop(id);
                    }
                }
                if let Some(id) = to_delete {
                    if let Ok(state) = self.state.lock() {
                        let _ = state.db.delete_track(id);
                        let _ = state.db.delete_bind_by_track(id);
                    }
                    self.set_toast("трек удален");
                    self.refresh_data();
                }
                if let Some(id) = to_bind {
                    self.binding_track_id = Some(id);
                }
            });
    }

    // ========================================================================
    // Вкладка 2: Магазин (MyInstants онлайн)
    // ========================================================================
    fn render_shop_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let search_edit = ui.add(
                TextEdit::singleline(&mut self.shop_search)
                    .hint_text(
                        egui::RichText::new("поиск на myinstants...")
                            .color(PAL_MUTED)
                            .font(FontId::monospace(12.0)),
                    )
                    .font(FontId::monospace(12.0))
                    .desired_width(280.0),
            );

            let trigger_search = search_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            if TactileButton::new("найти").accent().show(ui).clicked() || trigger_search {
                self.trigger_shop_search(self.shop_search.clone(), false);
            }

            ui.add_space(12.0);

            // Быстрые категории звуков
            let categories = [
                ("тренды", "/en/index/us/"),
                ("мемы", "/en/categories/memes/"),
                ("игры", "/en/categories/games/"),
                ("аниме", "/en/categories/anime%20&%20manga/"),
                ("музыка", "/en/categories/music/"),
            ];

            for (cat_name, cat_path) in categories {
                if TactileButton::new(cat_name).min_size(Vec2::new(45.0, 26.0)).show(ui).clicked() {
                    self.trigger_shop_category(cat_path.to_string());
                }
            }
        });

        ui.add_space(12.0);

        if self.shop_loading {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.label(
                    egui::RichText::new("загрузка звуков с myinstants...")
                        .color(PAL_ACCENT)
                        .font(FontId::monospace(13.0)),
                );
            });
            return;
        }

        if let Some(ref err) = self.shop_error {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new(format!("ошибка загрузки: {}", err))
                        .color(PAL_DANGER)
                        .font(FontId::monospace(12.0)),
                );
            });
            return;
        }

        // Список найденных звуков
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.shop_sounds.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(
                            egui::RichText::new("введите запрос или выберите категорию для поиска")
                                .color(PAL_MUTED)
                                .font(FontId::monospace(13.0)),
                        );
                    });
                    return;
                }

                let mut to_preview: Option<String> = None;
                let mut to_download: Option<ShopSound> = None;

                for sound in &self.shop_sounds {
                    let sound_clone = sound.clone();
                    let url = sound.url.clone();
                    let is_previewing = self.active_preview_url.as_ref() == Some(&url);

                    egui::Frame {
                        fill: PAL_CARD,
                        inner_margin: egui::Margin::symmetric(14, 10),
                        corner_radius: CornerRadius::same(R_BASE),
                        stroke: Stroke::new(
                            1.0_f32,
                            if is_previewing { PAL_GREEN } else { PAL_LINE },
                        ),
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Предпрослушивание
                            if is_previewing {
                                if TactileButton::new("■").danger().min_size(Vec2::new(32.0, 32.0)).show(ui).clicked() {
                                    to_preview = Some(String::new());
                                }
                            } else {
                                if TactileButton::new("▶").green().min_size(Vec2::new(32.0, 32.0)).show(ui).clicked() {
                                    to_preview = Some(url);
                                }
                            }

                            ui.add_space(8.0);

                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(&sound.name)
                                            .color(if is_previewing { PAL_GREEN } else { PAL_TEXT })
                                            .font(FontId::monospace(13.0))
                                            .strong(),
                                    );
                                    if is_previewing {
                                        ui.add_space(8.0);
                                        let (eq_rect, _) = ui.allocate_exact_size(Vec2::new(26.0, 14.0), Sense::hover());
                                        draw_animated_equalizer(ui.painter(), eq_rect, self.audio_phase, PAL_GREEN);
                                    }
                                });
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if TactileButton::new("[скачать]").accent().min_size(Vec2::new(75.0, 26.0)).show(ui).clicked() {
                                    to_download = Some(sound_clone);
                                }
                            });
                        });
                    });

                    ui.add_space(6.0);
                }

                // Обработка предпрослушивания
                if let Some(preview_url) = to_preview {
                    if preview_url.is_empty() {
                        if let Ok(mut state) = self.state.lock() {
                            state.player.stop(999999);
                        }
                        self.active_preview_url = None;
                    } else {
                        self.active_preview_url = Some(preview_url.clone());
                        let tx = self.async_tx.clone();
                        thread::spawn(move || {
                            if let Ok(client) = crate::shop::myinstants::MyInstantsClient::new() {
                                if let Ok(mp3_url) = client.get_sound_mp3_url(&preview_url) {
                                    // Проигрываем превью
                                }
                            }
                        });
                    }
                }

                // Обработка скачивания
                if let Some(sound) = to_download {
                    self.set_toast(format!("скачивание «{}»...", sound.name));
                    let state = self.state.clone();
                    let tx = self.async_tx.clone();
                    thread::spawn(move || {
                        download_and_add_sound(sound, state, tx);
                    });
                }
            });
    }

    fn trigger_shop_search(&mut self, query: String, _is_category: bool) {
        if query.trim().is_empty() {
            return;
        }
        self.shop_loading = true;
        self.shop_error = None;
        let tx = self.async_tx.clone();

        thread::spawn(move || {
            match crate::shop::myinstants::MyInstantsClient::new() {
                Ok(client) => match client.search(&query) {
                    Ok(sounds) => {
                        let _ = tx.send(AsyncUiMessage::ShopSounds(sounds));
                    }
                    Err(e) => {
                        let _ = tx.send(AsyncUiMessage::ShopError(e.to_string()));
                    }
                },
                Err(e) => {
                    let _ = tx.send(AsyncUiMessage::ShopError(e.to_string()));
                }
            }
        });
    }

    fn trigger_shop_category(&mut self, path: String) {
        self.shop_loading = true;
        self.shop_error = None;
        let tx = self.async_tx.clone();

        thread::spawn(move || {
            match crate::shop::myinstants::MyInstantsClient::new() {
                Ok(client) => match client.get_sounds(&path) {
                    Ok(sounds) => {
                        let _ = tx.send(AsyncUiMessage::ShopSounds(sounds));
                    }
                    Err(e) => {
                        let _ = tx.send(AsyncUiMessage::ShopError(e.to_string()));
                    }
                },
                Err(e) => {
                    let _ = tx.send(AsyncUiMessage::ShopError(e.to_string()));
                }
            }
        });
    }

    // ========================================================================
    // Вкладка 3: Бинды (Горячие клавиши)
    // ========================================================================
    fn render_binds_tab(&mut self, ui: &mut Ui) {
        ui.label(
            egui::RichText::new("назначенные горячие клавиши")
                .color(PAL_TEXT)
                .font(FontId::monospace(14.0))
                .strong(),
        );
        ui.label(
            egui::RichText::new("нажмите на звук во вкладке «треки», чтобы назначить или переназначить бинд")
                .color(PAL_MUTED)
                .font(FontId::monospace(11.0)),
        );

        ui.add_space(14.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.binds.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("нет активных биндов")
                                .color(PAL_MUTED)
                                .font(FontId::monospace(13.0)),
                        );
                    });
                    return;
                }

                let mut to_delete_bind: Option<i64> = None;

                for bind in &self.binds {
                    let bind_id = bind.id.unwrap_or(0);
                    let track_name = self
                        .tracks
                        .iter()
                        .find(|t| t.id == Some(bind.track_id))
                        .map(|t| t.name.clone())
                        .unwrap_or_else(|| "неизвестный трек".to_string());

                    let key_str = format_key(bind.keyval, bind.modifiers);

                    egui::Frame {
                        fill: PAL_CARD,
                        inner_margin: egui::Margin::symmetric(14, 10),
                        corner_radius: CornerRadius::same(R_BASE),
                        stroke: Stroke::new(1.0_f32, PAL_LINE),
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            hotkey_badge(ui, &format!("[{}]", key_str), true);

                            ui.label(
                                egui::RichText::new(" -> ")
                                    .color(PAL_MUTED)
                                    .font(FontId::monospace(13.0)),
                            );

                            ui.label(
                                egui::RichText::new(&track_name)
                                    .color(PAL_TEXT)
                                    .font(FontId::monospace(13.0)),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if TactileButton::new("удалить").danger().min_size(Vec2::new(70.0, 26.0)).show(ui).clicked() {
                                    to_delete_bind = Some(bind_id);
                                }
                            });
                        });
                    });

                    ui.add_space(6.0);
                }

                if let Some(id) = to_delete_bind {
                    if let Ok(state) = self.state.lock() {
                        let _ = state.db.delete_bind(id);
                    }
                    self.set_toast("бинд удален");
                    self.refresh_data();
                }
            });
    }

    // ========================================================================
    // Вкладка 4: Настройки (Аудио, Виртуальный микрофон, Passthrough)
    // ========================================================================
    fn render_settings_tab(&mut self, ui: &mut Ui) {
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Карточка: Виртуальный микрофон
                egui::Frame {
                    fill: PAL_CARD,
                    inner_margin: egui::Margin::same(14),
                    corner_radius: CornerRadius::same(R_BASE),
                    stroke: Stroke::new(1.0_f32, PAL_LINE),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("виртуальный микрофон (soundpad_mic)")
                            .color(PAL_ACCENT)
                            .font(FontId::monospace(13.0))
                            .strong(),
                    );
                    ui.add_space(6.0);

                    let (server_str, active_str) = if let Ok(state) = self.state.lock() {
                        let s = match state.virtual_mic.server() {
                            crate::audio::virtual_mic::AudioServer::PipeWire => "pipewire",
                            crate::audio::virtual_mic::AudioServer::PulseAudio => "pulseaudio",
                        };
                        let act = if state.virtual_mic.is_active() {
                            "активен"
                        } else {
                            "неактивен"
                        };
                        (s, act)
                    } else {
                        ("pipewire", "неактивен")
                    };

                    ui.label(
                        egui::RichText::new(format!("аудиосервер: {} | статус: [{}]", server_str, active_str))
                            .color(PAL_TEXT)
                            .font(FontId::monospace(12.0)),
                    );

                    ui.add_space(8.0);
                    if TactileButton::new("пересоздать микрофон").show(ui).clicked() {
                        if let Ok(mut state) = self.state.lock() {
                            let _ = state.virtual_mic.create();
                            let sink = state.virtual_mic.sink_name().to_string();
                            state.player.set_virtual_mic_sink(&sink);
                        }
                        self.set_toast("виртуальный микрофон пересоздан");
                        self.refresh_data();
                    }
                });

                ui.add_space(12.0);

                // Карточка: Реальный микрофон и сквозной режим (Passthrough)
                egui::Frame {
                    fill: PAL_CARD,
                    inner_margin: egui::Margin::same(14),
                    corner_radius: CornerRadius::same(R_BASE),
                    stroke: Stroke::new(1.0_f32, PAL_LINE),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("наложение реального микрофона (passthrough)")
                            .color(PAL_ACCENT)
                            .font(FontId::monospace(13.0))
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("голос с реального микрофона подмешивается в виртуальный микрофон")
                            .color(PAL_MUTED)
                            .font(FontId::monospace(11.0)),
                    );
                    ui.add_space(8.0);

                    // Выбор микрофона из списка
                    ui.label(
                        egui::RichText::new("выберите микрофон:")
                            .color(PAL_MUTED)
                            .font(FontId::monospace(11.0)),
                    );

                    let mut source_changed = false;
                    let current = self.selected_source.clone();

                    egui::ComboBox::from_id_salt("mic_select")
                        .width(320.0)
                        .selected_text(
                            egui::RichText::new(if current.is_empty() { "не выбран" } else { &current })
                                .font(FontId::monospace(12.0)),
                        )
                        .show_ui(ui, |ui| {
                            for src in &self.input_sources {
                                let is_sel = src == &current;
                                if ui.selectable_label(is_sel, egui::RichText::new(src).font(FontId::monospace(11.0))).clicked() {
                                    self.selected_source = src.clone();
                                    source_changed = true;
                                }
                            }
                        });

                    if source_changed {
                        if self.mic_passthrough_enabled {
                            if let Ok(mut state) = self.state.lock() {
                                let _ = state.player.start_mic_passthrough(&self.selected_source);
                            }
                        }
                        self.set_toast(format!("выбран микрофон: {}", self.selected_source));
                    }

                    ui.add_space(10.0);

                    // Тумблер Passthrough
                    let pt_label = if self.mic_passthrough_enabled {
                        "сквозной микрофон: [включен]"
                    } else {
                        "сквозной микрофон: [выключен]"
                    };

                    let pt_btn = if self.mic_passthrough_enabled {
                        TactileButton::new(pt_label).accent()
                    } else {
                        TactileButton::new(pt_label)
                    };

                    if pt_btn.show(ui).clicked() {
                        self.mic_passthrough_enabled = !self.mic_passthrough_enabled;
                        if let Ok(mut state) = self.state.lock() {
                            if self.mic_passthrough_enabled {
                                let _ = state.player.start_mic_passthrough(&self.selected_source);
                            } else {
                                state.player.stop_mic_passthrough();
                            }
                        }
                        self.refresh_data();
                    }
                });

                ui.add_space(12.0);

                // Карточка: Вывод в наушники (Speakers)
                egui::Frame {
                    fill: PAL_CARD,
                    inner_margin: egui::Margin::same(14),
                    corner_radius: CornerRadius::same(R_BASE),
                    stroke: Stroke::new(1.0_f32, PAL_LINE),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("вывод звуков в наушники")
                            .color(PAL_ACCENT)
                            .font(FontId::monospace(13.0))
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("воспроизводить звуки не только в микрофон, но и в ваши наушники")
                            .color(PAL_MUTED)
                            .font(FontId::monospace(11.0)),
                    );
                    ui.add_space(8.0);

                    let spk_label = if self.play_to_speakers {
                        "наушники: [включены]"
                    } else {
                        "наушники: [выключены]"
                    };

                    let spk_btn = if self.play_to_speakers {
                        TactileButton::new(spk_label).accent()
                    } else {
                        TactileButton::new(spk_label)
                    };

                    if spk_btn.show(ui).clicked() {
                        self.play_to_speakers = !self.play_to_speakers;
                        if let Ok(mut state) = self.state.lock() {
                            state.player.set_play_to_speakers(self.play_to_speakers);
                        }
                        self.set_toast(format!("наушники: {}", if self.play_to_speakers { "вкл" } else { "выкл" }));
                    }
                });
            });
    }
}

// ============================================================================
// Вспомогательные функции
// ============================================================================

/// Экран перехвата комбинации клавиш (Overlay)
fn render_bind_capture_overlay(ui: &mut Ui, track_id: i64, tracks: &[Track]) {
    let track_name = tracks
        .iter()
        .find(|t| t.id == Some(track_id))
        .map(|t| t.name.as_str())
        .unwrap_or("звук");

    ui.vertical_centered(|ui| {
        ui.add_space(60.0);

        egui::Frame {
            fill: PAL_CARD,
            inner_margin: egui::Margin::same(24),
            corner_radius: CornerRadius::same(R_BASE),
            stroke: Stroke::new(1.0_f32, PAL_ACCENT),
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("назначение горячей клавиши")
                    .color(PAL_ACCENT)
                    .font(FontId::monospace(16.0))
                    .strong(),
            );
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!("трек: «{}»", track_name))
                    .color(PAL_TEXT)
                    .font(FontId::monospace(13.0)),
            );
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new("нажмите любую клавишу (f1-f12, numpad, цифры, буквы, с ctrl/alt/shift)...")
                    .color(PAL_MUTED)
                    .font(FontId::monospace(12.0)),
            );
            ui.label(
                egui::RichText::new("[del или esc для очистки бинда]")
                    .color(PAL_MUTED)
                    .font(FontId::monospace(11.0)),
            );
        });
    });
}

/// Скачивание звука с MyInstants и сохранение в библиотеку
fn download_and_add_sound(
    sound: ShopSound,
    state: Arc<Mutex<AppState>>,
    tx: Sender<AsyncUiMessage>,
) {
    let client = match crate::shop::myinstants::MyInstantsClient::new() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(AsyncUiMessage::Toast(format!("ошибка клиента: {}", e)));
            return;
        }
    };

    let mp3_url = match client.get_sound_mp3_url(&sound.url) {
        Ok(u) => u,
        Err(e) => {
            let _ = tx.send(AsyncUiMessage::Toast(format!("ошибка ссылки: {}", e)));
            return;
        }
    };

    let sounds_dir = match dirs::data_dir() {
        Some(d) => d.join("soundpaad").join("sounds"),
        None => PathBuf::from("/tmp/soundpaad_sounds"),
    };
    let _ = std::fs::create_dir_all(&sounds_dir);

    let clean_name = sound
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect::<String>();
    let file_name = format!("{}.mp3", clean_name.trim());
    let dest_path = sounds_dir.join(&file_name);

    if let Err(e) = client.download_sound(&mp3_url, &dest_path) {
        let _ = tx.send(AsyncUiMessage::Toast(format!("ошибка загрузки: {}", e)));
        return;
    }

    if let Ok(state) = state.lock() {
        let _ = state.db.add_track(&Track {
            id: None,
            name: sound.name.clone(),
            file_path: dest_path.to_string_lossy().to_string(),
            category_id: None,
            volume: 1.0,
            source: "myinstants".to_string(),
        });
    }

    let _ = tx.send(AsyncUiMessage::TrackDownloaded(sound.name));
}

/// Настройка шрифтов JetBrains Mono
fn setup_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    if let Ok(font_bytes) = std::fs::read("/usr/share/fonts/jetbrains-mono/JetBrainsMono[wght].ttf") {
        fonts.font_data.insert(
            "JetBrainsMono".to_owned(),
            Arc::new(FontData::from_owned(font_bytes)),
        );

        if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
            mono.insert(0, "JetBrainsMono".to_owned());
        }
        if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
            prop.insert(0, "JetBrainsMono".to_owned());
        }
    }

    ctx.set_fonts(fonts);
}

/// Настройка визуальной палитры Pal в egui
fn setup_kinetix_visuals(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();

    visuals.override_text_color = Some(PAL_TEXT);
    visuals.panel_fill = PAL_PANEL;
    visuals.window_fill = PAL_PANEL;
    visuals.extreme_bg_color = PAL_CARD;
    visuals.faint_bg_color = PAL_CARD_HI;
    visuals.code_bg_color = PAL_CARD_HI;

    visuals.widgets.noninteractive.bg_fill = PAL_CARD;
    visuals.widgets.noninteractive.weak_bg_fill = PAL_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, PAL_LINE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, PAL_TEXT);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(R_SMALL);

    visuals.widgets.inactive.bg_fill = PAL_CARD;
    visuals.widgets.inactive.weak_bg_fill = PAL_CARD;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, PAL_LINE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, PAL_TEXT);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(R_SMALL);

    visuals.widgets.hovered.bg_fill = PAL_CARD_HI;
    visuals.widgets.hovered.weak_bg_fill = PAL_CARD_HI;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, PAL_ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, PAL_TEXT);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(R_SMALL);

    visuals.widgets.active.bg_fill = PAL_FILL;
    visuals.widgets.active.weak_bg_fill = PAL_FILL;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, PAL_ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, PAL_ACCENT);
    visuals.widgets.active.corner_radius = CornerRadius::same(R_SMALL);

    visuals.selection.bg_fill = PAL_ACCENT;
    visuals.selection.stroke = Stroke::new(1.0_f32, PAL_ON_ACCENT);

    ctx.set_visuals(visuals);
}

/// Преобразование egui клавиши в evdev код ядра Linux
fn egui_key_to_code(key: egui::Key) -> u32 {
    match key {
        egui::Key::Escape => 1,
        egui::Key::Num1 => 2,
        egui::Key::Num2 => 3,
        egui::Key::Num3 => 4,
        egui::Key::Num4 => 5,
        egui::Key::Num5 => 6,
        egui::Key::Num6 => 7,
        egui::Key::Num7 => 8,
        egui::Key::Num8 => 9,
        egui::Key::Num9 => 10,
        egui::Key::Num0 => 11,
        egui::Key::Minus => 12,
        egui::Key::Equals => 13,
        egui::Key::Backspace => 14,
        egui::Key::Tab => 15,
        egui::Key::Q => 16,
        egui::Key::W => 17,
        egui::Key::E => 18,
        egui::Key::R => 19,
        egui::Key::T => 20,
        egui::Key::Y => 21,
        egui::Key::U => 22,
        egui::Key::I => 23,
        egui::Key::O => 24,
        egui::Key::P => 25,
        egui::Key::OpenBracket => 26,
        egui::Key::CloseBracket => 27,
        egui::Key::Enter => 28,
        egui::Key::A => 30,
        egui::Key::S => 31,
        egui::Key::D => 32,
        egui::Key::F => 33,
        egui::Key::G => 34,
        egui::Key::H => 35,
        egui::Key::J => 36,
        egui::Key::K => 37,
        egui::Key::L => 38,
        egui::Key::Semicolon => 39,
        egui::Key::Quote => 40,
                egui::Key::Backslash => 43,
        egui::Key::Z => 44,
        egui::Key::X => 45,
        egui::Key::C => 46,
        egui::Key::V => 47,
        egui::Key::B => 48,
        egui::Key::N => 49,
        egui::Key::M => 50,
        egui::Key::Comma => 51,
        egui::Key::Period => 52,
        egui::Key::Slash => 53,
        egui::Key::Space => 57,
        egui::Key::F1 => 59,
        egui::Key::F2 => 60,
        egui::Key::F3 => 61,
        egui::Key::F4 => 62,
        egui::Key::F5 => 63,
        egui::Key::F6 => 64,
        egui::Key::F7 => 65,
        egui::Key::F8 => 66,
        egui::Key::F9 => 67,
        egui::Key::F10 => 68,
        egui::Key::F11 => 87,
        egui::Key::F12 => 88,
        egui::Key::F13 => 183,
        egui::Key::F14 => 184,
        egui::Key::F15 => 185,
        egui::Key::F16 => 186,
        egui::Key::F17 => 187,
        egui::Key::F18 => 188,
        egui::Key::F19 => 189,
        egui::Key::F20 => 190,
        egui::Key::Home => 102,
        egui::Key::ArrowUp => 103,
        egui::Key::PageUp => 104,
        egui::Key::ArrowLeft => 105,
        egui::Key::ArrowRight => 106,
        egui::Key::End => 107,
        egui::Key::ArrowDown => 108,
        egui::Key::PageDown => 109,
        egui::Key::Insert => 110,
        egui::Key::Delete => 111,
        _ => 0,
    }
}

fn egui_modifiers_to_bits(m: &egui::Modifiers) -> u32 {
    let mut bits = 0;
    if m.ctrl {
        bits |= 1;
    }
    if m.shift {
        bits |= 2;
    }
    if m.alt {
        bits |= 4;
    }
    if m.command {
        bits |= 8;
    }
    bits
}

fn format_key(keyval: u32, modifiers: u32) -> String {
    let mut parts = Vec::new();
    if modifiers & 1 != 0 {
        parts.push("ctrl".to_string());
    }
    if modifiers & 2 != 0 {
        parts.push("shift".to_string());
    }
    if modifiers & 4 != 0 {
        parts.push("alt".to_string());
    }
    if modifiers & 8 != 0 {
        parts.push("super".to_string());
    }

    let key_name = match keyval {
        1 => "esc".to_string(),
        2 => "1".to_string(),
        3 => "2".to_string(),
        4 => "3".to_string(),
        5 => "4".to_string(),
        6 => "5".to_string(),
        7 => "6".to_string(),
        8 => "7".to_string(),
        9 => "8".to_string(),
        10 => "9".to_string(),
        11 => "0".to_string(),
        12 => "-".to_string(),
        13 => "=".to_string(),
        14 => "backspace".to_string(),
        15 => "tab".to_string(),
        16 => "q".to_string(),
        17 => "w".to_string(),
        18 => "e".to_string(),
        19 => "r".to_string(),
        20 => "t".to_string(),
        21 => "y".to_string(),
        22 => "u".to_string(),
        23 => "i".to_string(),
        24 => "o".to_string(),
        25 => "p".to_string(),
        26 => "[".to_string(),
        27 => "]".to_string(),
        28 => "enter".to_string(),
        30 => "a".to_string(),
        31 => "s".to_string(),
        32 => "d".to_string(),
        33 => "f".to_string(),
        34 => "g".to_string(),
        35 => "h".to_string(),
        36 => "j".to_string(),
        37 => "k".to_string(),
        38 => "l".to_string(),
        39 => ";".to_string(),
        40 => "'".to_string(),
        41 => "`".to_string(),
        43 => "\\".to_string(),
        44 => "z".to_string(),
        45 => "x".to_string(),
        46 => "c".to_string(),
        47 => "v".to_string(),
        48 => "b".to_string(),
        49 => "n".to_string(),
        50 => "m".to_string(),
        51 => ",".to_string(),
        52 => ".".to_string(),
        53 => "/".to_string(),
        55 => "kp_*".to_string(),
        57 => "space".to_string(),
        58 => "caps".to_string(),
        59 => "f1".to_string(),
        60 => "f2".to_string(),
        61 => "f3".to_string(),
        62 => "f4".to_string(),
        63 => "f5".to_string(),
        64 => "f6".to_string(),
        65 => "f7".to_string(),
        66 => "f8".to_string(),
        67 => "f9".to_string(),
        68 => "f10".to_string(),
        69 => "numlock".to_string(),
        70 => "scroll".to_string(),
        71 => "kp_7".to_string(),
        72 => "kp_8".to_string(),
        73 => "kp_9".to_string(),
        74 => "kp_-".to_string(),
        75 => "kp_4".to_string(),
        76 => "kp_5".to_string(),
        77 => "kp_6".to_string(),
        78 => "kp_+".to_string(),
        79 => "kp_1".to_string(),
        80 => "kp_2".to_string(),
        81 => "kp_3".to_string(),
        82 => "kp_0".to_string(),
        83 => "kp_.".to_string(),
        87 => "f11".to_string(),
        88 => "f12".to_string(),
        96 => "kp_enter".to_string(),
        98 => "kp_/".to_string(),
        102 => "home".to_string(),
        103 => "up".to_string(),
        104 => "pageup".to_string(),
        105 => "left".to_string(),
        106 => "right".to_string(),
        107 => "end".to_string(),
        108 => "down".to_string(),
        109 => "pagedown".to_string(),
        110 => "ins".to_string(),
        111 => "del".to_string(),
        119 => "pause".to_string(),
        other => {
            let key = evdev::KeyCode::new(other as u16);
            let s = format!("{:?}", key);
            s.strip_prefix("KEY_")
                .unwrap_or(&s)
                .to_lowercase()
        }
    };

    parts.push(key_name);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_key_numpad_and_modifiers() {
        assert_eq!(format_key(96, 0), "kp_enter");
        assert_eq!(format_key(96, 1), "ctrl+kp_enter");
        assert_eq!(format_key(82, 0), "kp_0");
        assert_eq!(format_key(111, 0), "del");
        assert_eq!(format_key(1, 0), "esc");
        assert_eq!(format_key(30, 2), "shift+a");
        assert_eq!(format_key(59, 4), "alt+f1");
    }
}
