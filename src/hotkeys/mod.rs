use anyhow::Result;
use crossbeam_channel::Sender;
use evdev::KeyCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyEvent {
    pub key_code: u16,
    pub key_name: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

pub struct HotkeyListener {
    running: Arc<AtomicBool>,
    _handles: Vec<JoinHandle<()>>,
}

impl HotkeyListener {
    /// Запускает фоновые потоки чтения клавиатурных событий через evdev
    pub fn start(sender: Sender<HotkeyEvent>) -> Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let mut handles = Vec::new();

        for (path, mut device) in evdev::enumerate() {
            // Проверяем, является ли устройство клавиатурой
            let is_keyboard = device.supported_keys().map_or(false, |keys| {
                keys.contains(KeyCode::KEY_ENTER) && keys.contains(KeyCode::KEY_SPACE)
            });

            if !is_keyboard {
                continue;
            }

            let running_clone = Arc::clone(&running);
            let sender_clone = sender.clone();
            let dev_name = device.name().unwrap_or("Unknown").to_string();

            let handle = thread::Builder::new()
                .name(format!("hotkey-listener-{:?}", path))
                .spawn(move || {
                    let mut ctrl = false;
                    let mut shift = false;
                    let mut alt = false;
                    let mut meta = false;

                    while running_clone.load(Ordering::Relaxed) {
                        match device.fetch_events() {
                            Ok(events) => {
                                for ev in events {
                                    if ev.event_type() == evdev::EventType::KEY {
                                        let code = ev.code();
                                        let key_code = KeyCode::new(code);
                                        let val = ev.value(); // 0 = release, 1 = press, 2 = repeat

                                        let is_pressed = val != 0;
                                        match key_code {
                                            KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => {
                                                ctrl = is_pressed;
                                            }
                                            KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => {
                                                shift = is_pressed;
                                            }
                                            KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => {
                                                alt = is_pressed;
                                            }
                                            KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => {
                                                meta = is_pressed;
                                            }
                                            _ => {}
                                        }

                                        // Отправляем событие при нажатии клавиши
                                        if val == 1 {
                                            let key_str = format!("{:?}", key_code);
                                            let key_name = key_str
                                                .strip_prefix("KEY_")
                                                .unwrap_or(&key_str)
                                                .to_string();

                                            let event = HotkeyEvent {
                                                key_code: code,
                                                key_name,
                                                ctrl,
                                                shift,
                                                alt,
                                                meta,
                                            };

                                            if sender_clone.send(event).is_err() {
                                                // Канал закрыт
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(_) => {
                                // Ошибка чтения (например, устройство отключено)
                                break;
                            }
                        }
                    }
                })?;

            eprintln!("HotkeyListener: запущен слушатель для {} ({:?})", dev_name, path);
            handles.push(handle);
        }

        Ok(Self {
            running,
            _handles: handles,
        })
    }

    /// Останавливает слушатель хоткеев
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for HotkeyListener {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_event() {
        let ev = HotkeyEvent {
            key_code: 30,
            key_name: "A".to_string(),
            ctrl: true,
            shift: false,
            alt: true,
            meta: false,
        };
        assert_eq!(ev.key_code, 30);
        assert_eq!(ev.key_name, "A");
        assert!(ev.ctrl);
        assert!(!ev.shift);
        assert!(ev.alt);
        assert!(!ev.meta);
    }
}
