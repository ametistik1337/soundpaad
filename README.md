# SoundPad

Минималистичный саундпад на чистом Rust для Linux с поддержкой виртуального микрофона, глобальными хоткеями и интеграцией с MyInstants.

![Rust](https://img.shields.io/badge/Rust-2024-F59E0B?logo=rust)
![UI](https://img.shields.io/badge/UI-egui_0.31-blue)
![Design](https://img.shields.io/badge/Design-Kinetix_UI-amber)
![License](https://img.shields.io/badge/License-MIT-green)

---

## Особенности

- **Чистый Rust без GTK и GStreamer**: легкий, быстрый, нативный GUI на `eframe`/`egui` 0.31 и воспроизведение на `rodio`/`cpal`.
- **Дизайн Kinetix UI**: терминальный минимализм, палитра Pal (`#0A0B0E`, `#13151A`, `#191C24`, `#23262E`), защищенный янтарный акцент `#F59E0B`, субпиксельные линии 1px со сдвигом 0.5px.
- **Тактильный отклик (Emil Kowalski UX)**: упругие ODE-пружины, сжатие `:active` до `scale(0.97)`, время переходов < 200 мс, отсутствие визуального мусора.
- **24-Сегментный регулятор громкости**: аппаратный ритм Kinetix DS, непрерывный рендеринг и коммит значения на pointer release.
- **Виртуальный микрофон PipeWire / PulseAudio**: создание `SoundPad_Sink` и `SoundPad_Mic` через `pactl`. Автоматическая очистка модулей при выходе.
- **Сквозной микрофон (Mic Passthrough)**: наложение вашего реального голоса на звуки саундпада с ультра-низкой задержкой (PipeWire loopback).
- **Глобальные хоткеи (evdev)**: перехват клавиш на уровне ядра Linux (`/dev/input/event*`), работает глобально в любых играх (Dota 2, CS2) и полноэкранных окнах под Wayland и X11.
- **Магазин MyInstants**: мгновенный поиск и предпрослушивание топовых звуков прямо в приложении с сохранением в локальный саундборд в один клик.

---

## Архитектура

- **Язык**: Rust (Edition 2024)
- **GUI**: `eframe` / `egui` (0.31) + OpenGL / Wayland / X11
- **Аудио**: `rodio` (0.23) + `cpal` (0.18, PulseAudio / PipeWire backend)
- **БД**: `rusqlite` (bundled SQLite 3)
- **Хоткеи**: `evdev` (0.13)
- **Сеть & Парсинг**: `reqwest` + `scraper`

---

## Сборка и установка

### Зависимости

Для работы требуются только базовые аудио-утилиты PipeWire/PulseAudio:
```bash
sudo pacman -S pipewire-pulse libpulse openssl
```

### Сборка из исходников

```bash
git clone https://github.com/ametistik1337/soundpaad.git
cd soundpaad
cargo build --release
sudo install -Dm755 target/release/soundpaad /usr/bin/soundpaad
sudo install -Dm644 com.soundpaad.app.desktop /usr/share/applications/com.soundpaad.app.desktop
sudo install -Dm644 com.soundpaad.app.svg /usr/share/icons/hicolor/scalable/apps/com.soundpaad.app.svg
```

---

## Использование

1. Запустите `soundpaad`.
2. В разделе **Настройки** нажмите кнопку `создать` для инициализации виртуального микрофона `SoundPad_Mic`.
3. При необходимости включите `сквозной микрофон` и выберите входное устройство вашего микрофона.
4. В Discord, OBS, TeamSpeak или игре выберите **SoundPad_Mic** в качестве устройства ввода.
5. Во вкладке **Магазин** найдите нужные звуки и нажмите `скачать` — они сразу появятся на вашем саундборде.
6. Назначайте бинды во вкладке **Бинды** или прямо на карточках треков.

---

## Лицензия

MIT License
