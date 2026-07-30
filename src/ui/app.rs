use adw::prelude::*;
use adw::{ApplicationWindow, ViewStack, ViewSwitcher};
use gtk4::Box as GtkBox;
use gtk4::{HeaderBar, Orientation};
use gtk4::glib::translate::IntoGlib;
use std::cell::RefCell;
use std::rc::Rc;

use crate::AppState;

use super::binds::BindsPage;
use super::settings::SettingsPage;
use super::shop::ShopPage;
use super::soundpad::SoundpadPage;

pub struct AppUI {
    _window: ApplicationWindow,
    _soundpad: Rc<SoundpadPage>,
    _shop_page: ShopPage,
    _settings_page: Rc<SettingsPage>,
    _binds_page: Rc<BindsPage>,
}

pub fn keycode_to_english_keyval(keycode: u32) -> u32 {
    match keycode {
        9 => 65307, // Escape
        10 => 49,   // 1
        11 => 50,   // 2
        12 => 51,   // 3
        13 => 52,   // 4
        14 => 53,   // 5
        15 => 54,   // 6
        16 => 55,   // 7
        17 => 56,   // 8
        18 => 57,   // 9
        19 => 48,   // 0
        20 => 45,   // -
        21 => 61,   // =
        22 => 65288, // BackSpace
        23 => 65289, // Tab
        24 => 113,  // q
        25 => 119,  // w
        26 => 101,  // e
        27 => 114,  // r
        28 => 116,  // t
        29 => 121,  // y
        30 => 117,  // u
        31 => 105,  // i
        32 => 111,  // o
        33 => 112,  // p
        34 => 91,   // [
        35 => 93,   // ]
        36 => 65293, // Return
        38 => 97,   // a
        39 => 115,  // s
        40 => 100,  // d
        41 => 102,  // f
        42 => 103,  // g
        43 => 104,  // h
        44 => 106,  // j
        45 => 107,  // k
        46 => 108,  // l
        47 => 59,   // ;
        48 => 39,   // '
        49 => 96,   // `
        51 => 92,   // \
        52 => 122,  // z
        53 => 120,  // x
        54 => 99,   // c
        55 => 118,  // v
        56 => 98,   // b
        57 => 110,  // n
        58 => 109,  // m
        59 => 44,   // ,
        60 => 46,   // .
        61 => 47,   // /
        65 => 32,   // Space
        67 => 65470, // F1
        68 => 65471, // F2
        69 => 65472, // F3
        70 => 65473, // F4
        71 => 65474, // F5
        72 => 65475, // F6
        73 => 65476, // F7
        74 => 65477, // F8
        75 => 65478, // F9
        76 => 65479, // F10
        95 => 65480, // F11
        96 => 65481, // F12
        111 => 65362, // Up
        113 => 65361, // Left
        114 => 65363, // Right
        116 => 65364, // Down
        _ => 0,
    }
}

/// Конвертирует Linux evdev keycode в GTK keyval (независимо от раскладки)
fn evdev_keycode_to_keyval(code: u16) -> u32 {
    // evdev keycode = X11 keycode - 8
    keycode_to_english_keyval(code as u32 + 8)
}

/// Находит все устройства ввода, которые являются клавиатурами
fn find_keyboard_devices() -> Vec<evdev::Device> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/dev/input") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(dev) = evdev::Device::open(&path) {
                // Проверяем, что устройство поддерживает события клавиш EV_KEY
                // и имеет типичные клавиши клавиатуры (Return, Space, etc.)
                let types = dev.supported_events();
                if types.contains(evdev::EventType::KEY) {
                    let keys = match dev.supported_keys() {
                        Some(k) => k,
                        None => continue,
                    };
                    // Проверяем наличие Return (28) как признак клавиатуры
                    if keys.contains(evdev::KeyCode::KEY_ENTER) {
                        result.push(dev);
                    }
                }
            }
        }
    }
    result
}

struct GlobalKeyMsg {
    keyval: u32,
    modifiers: u32,
}

impl AppUI {
    pub fn build(app: &adw::Application, state: Rc<RefCell<AppState>>) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("SoundPad")
            .default_width(650)
            .default_height(520)
            .build();

        let content = GtkBox::new(Orientation::Vertical, 0);

        let header = HeaderBar::new();
        content.append(&header);

        let view_stack = ViewStack::new();

        let binds_page = BindsPage::new(state.clone());
        let soundpad_page = SoundpadPage::new(state.clone(), binds_page.clone());
        let shop_page = ShopPage::new(state.clone(), soundpad_page.clone());
        let settings_page = SettingsPage::new(state.clone());

        // Настраиваем страницы в ViewStack
        let p_soundpad = view_stack.add_titled(&soundpad_page.widget(), Some("soundpad"), "Саундпад");
        p_soundpad.set_icon_name(Some("audio-volume-high-symbolic"));

        let p_shop = view_stack.add_titled(&shop_page.widget(), Some("shop"), "Магазин");
        p_shop.set_icon_name(Some("folder-download-symbolic"));

        let p_binds = view_stack.add_titled(&binds_page.widget(), Some("binds"), "Бинды");
        p_binds.set_icon_name(Some("keyboard-symbolic"));

        let p_settings = view_stack.add_titled(&settings_page.widget(), Some("settings"), "Настройки");
        p_settings.set_icon_name(Some("emblem-system-symbolic"));

        // Создаем ViewSwitcher и встраиваем его в HeaderBar для экономии места
        let view_switcher = ViewSwitcher::new();
        view_switcher.set_stack(Some(&view_stack));
        header.set_title_widget(Some(&view_switcher));

        content.append(&view_stack);
        window.set_content(Some(&content));

        // Автоматически создаем виртуальный микрофон при запуске
        {
            let mut s = state.borrow_mut();
            if let Err(e) = s.virtual_mic.create() {
                eprintln!("Ошибка автоматического создания виртуального микрофона: {}", e);
            } else {
                let sink = s.virtual_mic.sink_name().to_string();
                s.player.set_virtual_mic_sink(&sink);
                
                // Запускаем passthrough с задержкой 500мс, чтобы PulseAudio/PipeWire успели инициализировать устройства
                let state_clone = state.clone();
                let settings_clone = settings_page.clone();
                gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    {
                        let mut s = state_clone.borrow_mut();
                        if let Ok(sources) = crate::audio::virtual_mic::VirtualMic::list_sources() {
                            if let Some(first_src) = sources.first() {
                                s.player.set_real_mic_source(first_src);
                            }
                        }
                    }
                    settings_clone.refresh();
                });
            }
        }

        // Обработчик локальных горячих клавиш для биндов (когда окно в фокусе)
        let state_key = state.clone();
        let controller = gtk4::EventControllerKey::new();
        controller.connect_key_pressed(move |_controller, keyval, keycode, modifier| {
            let english_kv = keycode_to_english_keyval(keycode);
            let final_kv = if english_kv != 0 { english_kv } else { keyval.into_glib() as u32 };
            let mod_bits = modifier.bits() as u32;
            
            let s = state_key.borrow();
            if let Ok(Some(bind)) = s.db.get_bind_by_key(final_kv, mod_bits) {
                if let Ok(tracks) = s.db.get_tracks() {
                    if let Some(track) = tracks.iter().find(|t| t.id == Some(bind.track_id)) {
                        let track_id = track.id.unwrap_or(0);
                        let track_path = track.file_path.clone();
                        let track_volume = track.volume;
                        drop(s);
                        let mut s = state_key.borrow_mut();
                        if let Err(e) = s.player.play(track_id, &track_path, track_volume) {
                            eprintln!("Ошибка воспроизведения бинда: {}", e);
                        }
                        return gtk4::glib::Propagation::Stop;
                    }
                }
            }
            gtk4::glib::Propagation::Proceed
        });
        window.add_controller(controller);

        // Настройка глобального слушателя клавиатуры (когда окно НЕ в фокусе)
        let (global_tx, mut global_rx) = tokio::sync::mpsc::unbounded_channel::<GlobalKeyMsg>();
        let state_global = state.clone();
        let window_weak = window.downgrade();
        
        gtk4::glib::MainContext::default().spawn_local(async move {
            while let Some(msg) = global_rx.recv().await {
                // Если наше окно активно, локальный EventControllerKey уже обработает нажатие.
                // Игнорируем глобальное событие во избежание двойного проигрывания.
                if let Some(w) = window_weak.upgrade() {
                    if w.is_active() {
                        continue;
                    }
                }
                
                let s = state_global.borrow();
                if let Ok(Some(bind)) = s.db.get_bind_by_key(msg.keyval, msg.modifiers) {
                    if let Ok(tracks) = s.db.get_tracks() {
                        if let Some(track) = tracks.iter().find(|t| t.id == Some(bind.track_id)) {
                            let track_id = track.id.unwrap_or(0);
                            let track_path = track.file_path.clone();
                            let track_volume = track.volume;
                            drop(s);
                            let mut s = state_global.borrow_mut();
                            let _ = s.player.play(track_id, &track_path, track_volume);
                        }
                    }
                }
            }
        });

        std::thread::spawn(move || {
            let keyboards = find_keyboard_devices();
            if keyboards.is_empty() {
                eprintln!("Global hotkeys: клавиатурные устройства не найдены в /dev/input");
                return;
            }
            eprintln!("Global hotkeys: найдено {} клавиатурных устройств", keyboards.len());

            // Для каждой клавиатуры запускаем отдельный поток
            let mut handles = Vec::new();
            for mut kbd in keyboards {
                let tx = global_tx.clone();
                let handle = std::thread::spawn(move || {
                    let mut ctrl = false;
                    let mut shift = false;
                    let mut alt = false;
                    let mut meta = false;

                    loop {
                        let events = match kbd.fetch_events() {
                            Ok(e) => e,
                            Err(e) => {
                                eprintln!("Global hotkeys: ошибка чтения устройства: {}", e);
                                break;
                            }
                        };
                        for ev in events {
                            if ev.event_type() != evdev::EventType::KEY { continue; }
                            let code = ev.code();
                            let value = ev.value(); // 1 = press, 0 = release, 2 = repeat

                            // Отслеживаем модификаторы по evdev-кодам
                            match code {
                                29 | 97  => ctrl  = value != 0, // Ctrl L/R
                                42 | 54  => shift = value != 0, // Shift L/R
                                56 | 100 => alt   = value != 0, // Alt L/R
                                125| 126 => meta  = value != 0, // Meta L/R
                                _ => {}
                            }

                            if value == 1 { // только нажатие (не удержание и не отпускание)
                                let kv = evdev_keycode_to_keyval(code);
                                if kv != 0 {
                                    let mut mod_bits = 0u32;
                                    if ctrl  { mod_bits |= gtk4::gdk::ModifierType::CONTROL_MASK.bits() as u32; }
                                    if shift { mod_bits |= gtk4::gdk::ModifierType::SHIFT_MASK.bits() as u32; }
                                    if alt   { mod_bits |= gtk4::gdk::ModifierType::ALT_MASK.bits() as u32; }
                                    if meta  { mod_bits |= gtk4::gdk::ModifierType::SUPER_MASK.bits() as u32; }
                                    let _ = tx.send(GlobalKeyMsg { keyval: kv, modifiers: mod_bits });
                                }
                            }
                        }
                    }
                });
                handles.push(handle);
            }
            for h in handles { let _ = h.join(); }
        });

        // Обработчик авто-удаления виртуального микрофона при закрытии приложения
        let state_close = state.clone();
        let app_close = app.clone();
        window.connect_close_request(move |_window| {
            let mut s = state_close.borrow_mut();
            s.player.stop_mic_passthrough();
            if let Err(e) = s.virtual_mic.destroy() {
                eprintln!("Ошибка при автоматическом удалении виртуального микрофона: {}", e);
            }
            app_close.quit();
            gtk4::glib::Propagation::Proceed
        });

        window.present();

        Self {
            _window: window,
            _soundpad: soundpad_page,
            _shop_page: shop_page,
            _settings_page: settings_page,
            _binds_page: binds_page,
        }
    }
}
