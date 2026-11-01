mod audio;
mod db;
mod hotkeys;
mod shop;
mod ui;

use std::sync::{Arc, Mutex};
use audio::player::AudioPlayer;
use audio::virtual_mic::VirtualMic;
use db::repository::Database;
use hotkeys::{HotkeyEvent, HotkeyListener};
use ui::app::SoundpadApp;

pub struct AppState {
    pub db: Database,
    pub player: AudioPlayer,
    pub virtual_mic: VirtualMic,
}

fn main() -> eframe::Result {
    // Убиваем старые зависшие процессы soundpaad
    kill_old_processes();

    // 1. Инициализация БД
    let db = Database::open_default().expect("Не удалось открыть базу данных soundpaad.db");

    // 2. Инициализация и проверка виртуального микрофона
    let mut virtual_mic = VirtualMic::new().expect("Не удалось определить аудио сервер (PipeWire/PulseAudio)");
    if let Err(e) = virtual_mic.create() {
        eprintln!("Предупреждение при создании виртуального микрофона: {}", e);
    }
    let sink_name = virtual_mic.sink_name().to_string();

    // 3. Инициализация чистого Rust аудио-плеера (rodio + cpal)
    let player = AudioPlayer::new(&sink_name).expect("Не удалось инициализировать AudioPlayer");

    let state = Arc::new(Mutex::new(AppState {
        db,
        player,
        virtual_mic,
    }));

    // 4. Запуск фонового слушателя глобальных хоткеев через evdev
    let (hotkey_tx, hotkey_rx) = crossbeam_channel::unbounded::<HotkeyEvent>();
    let _hotkey_listener = match HotkeyListener::start(hotkey_tx) {
        Ok(listener) => Some(listener),
        Err(err) => {
            eprintln!("Не удалось запустить слушатель хоткеев evdev: {}", err);
            None
        }
    };

    let hotkey_state = Arc::clone(&state);
    std::thread::spawn(move || {
        while let Ok(event) = hotkey_rx.recv() {
            let mut modifiers = 0u32;
            if event.ctrl {
                modifiers |= 1;
            }
            if event.shift {
                modifiers |= 2;
            }
            if event.alt {
                modifiers |= 4;
            }
            if event.meta {
                modifiers |= 8;
            }

            let keyval = event.key_code as u32;

            if let Ok(mut s) = hotkey_state.lock() {
                if let Ok(Some(track)) = s.db.find_track_by_bind(keyval, modifiers) {
                    if let Some(id) = track.id {
                        let vol = track.volume as f32;
                        let _ = s.player.play(id, &track.file_path, vol);
                    }
                }
            }
        }
    });

    // 5. Запуск eframe GUI с палитрой Kinetix Pal
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([880.0, 620.0])
            .with_min_inner_size([680.0, 480.0])
            .with_title("soundpaad")
            .with_app_id("com.soundpaad.app"),
        ..Default::default()
    };

    let app_state = Arc::clone(&state);
    eframe::run_native(
        "soundpaad",
        native_options,
        Box::new(move |cc| Ok(Box::new(SoundpadApp::new(cc, app_state)))),
    )
}

fn kill_old_processes() {
    let current = std::process::id();
    if let Ok(output) = std::process::Command::new("pgrep")
        .args(["-x", "soundpaad"])
        .output()
    {
        let pids = String::from_utf8_lossy(&output.stdout);
        for line in pids.lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                if pid != current {
                    eprintln!("Убиваем старый процесс soundpaad: {}", pid);
                    let _ = std::process::Command::new("kill")
                        .args(["-9", &pid.to_string()])
                        .output();
                }
            }
        }
    }
}
