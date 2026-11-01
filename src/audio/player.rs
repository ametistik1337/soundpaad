use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use std::process::Command;

use super::virtual_mic::VIRTUAL_SINK_NAME;

pub trait IntoF32 {
    fn into_f32(self) -> f32;
}

impl IntoF32 for f32 {
    #[inline]
    fn into_f32(self) -> f32 {
        self
    }
}

impl IntoF32 for f64 {
    #[inline]
    fn into_f32(self) -> f32 {
        self as f32
    }
}

struct ActiveTrack {
    speaker_player: Option<Player>,
    mic_player: Option<Player>,
    volume: f32,
}

pub struct AudioPlayer {
    virtual_mic_sink: String,
    speakers_sink: Option<MixerDeviceSink>,
    mic_sink: Option<MixerDeviceSink>,
    active_tracks: HashMap<i64, ActiveTrack>,
    play_to_speakers: bool,
    loopback_id: Option<u32>,
    real_mic_source: Option<String>,
}

impl AudioPlayer {
    pub fn new(virtual_mic_sink: &str) -> Result<Self> {
        let mut player = Self {
            virtual_mic_sink: virtual_mic_sink.to_string(),
            speakers_sink: None,
            mic_sink: None,
            active_tracks: HashMap::new(),
            play_to_speakers: true,
            loopback_id: None,
            real_mic_source: None,
        };

        // Инициализируем аудиовыходы
        player.init_speakers();
        player.init_mic_sink();

        Ok(player)
    }

    pub fn open_default() -> Result<Self> {
        Self::new(VIRTUAL_SINK_NAME)
    }

    /// Инициализирует или переподключает системный вывод (наушники/динамики)
    fn init_speakers(&mut self) {
        if self.speakers_sink.is_none() {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    sink.log_on_drop(false);
                    self.speakers_sink = Some(sink);
                }
                Err(err) => {
                    eprintln!("AudioPlayer: не удалось открыть default sink: {}", err);
                }
            }
        }
    }

    /// Инициализирует или ищет виртуальный sink SoundPad
    fn init_mic_sink(&mut self) {
        if self.mic_sink.is_none() {
            let host = cpal::default_host();
            let target_name = self.virtual_mic_sink.to_lowercase();

            if let Ok(devices) = host.output_devices() {
                for dev in devices {
                    if let Ok(desc) = dev.description() {
                        let name_lower = desc.name().to_lowercase();
                        if name_lower.contains("soundpad") || name_lower.contains(&target_name) {
                            match DeviceSinkBuilder::from_device(dev) {
                                Ok(builder) => match builder.open_stream() {
                                    Ok(mut sink) => {
                                        sink.log_on_drop(false);
                                        self.mic_sink = Some(sink);
                                        eprintln!(
                                            "AudioPlayer: подключен virtual mic sink: {}",
                                            desc.name()
                                        );
                                        break;
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "AudioPlayer: ошибка открытия stream для {}: {}",
                                            desc.name(),
                                            e
                                        );
                                    }
                                },
                                Err(e) => {
                                    eprintln!("AudioPlayer: ошибка DeviceSinkBuilder: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Воспроизводит трек с заданным volume (0.0 .. 1.0)
    pub fn play(&mut self, track_id: i64, file_path: &str, volume: impl IntoF32) -> Result<()> {
        let vol_f32 = volume.into_f32();
        self.stop(track_id);

        let path = Path::new(file_path);
        if !path.exists() {
            anyhow::bail!("Файл не найден: {}", file_path);
        }

        let mut file = File::open(path)
            .with_context(|| format!("Не удалось открыть аудиофайл: {}", file_path))?;
        let mut audio_bytes = Vec::new();
        file.read_to_end(&mut audio_bytes)
            .context("Не удалось прочитать содержимое аудиофайла")?;

        // Проверяем / переподключаем аудиовыходы при необходимости
        if self.play_to_speakers && self.speakers_sink.is_none() {
            self.init_speakers();
        }
        if self.mic_sink.is_none() {
            self.init_mic_sink();
        }

        let mut speaker_player = None;
        let mut mic_player = None;

        // 1. Вывод в наушники/динамики (если включено)
        if self.play_to_speakers {
            if let Some(sink) = &self.speakers_sink {
                let cursor = Cursor::new(audio_bytes.clone());
                let source = Decoder::new(cursor).context("Ошибка декодирования аудио для динамиков")?;
                let player = Player::connect_new(sink.mixer());
                player.set_volume(vol_f32);
                player.append(source);
                speaker_player = Some(player);
            }
        }

        // 2. Вывод в виртуальный микрофон
        if let Some(sink) = &self.mic_sink {
            let cursor = Cursor::new(audio_bytes);
            let source = Decoder::new(cursor).context("Ошибка декодирования аудио для микрофона")?;
            let player = Player::connect_new(sink.mixer());
            player.set_volume(vol_f32);
            player.append(source);
            mic_player = Some(player);
        } else {
            eprintln!("AudioPlayer: предупреждение: virtual mic sink не найден, звук пойдет только на динамики");
        }

        self.active_tracks.insert(
            track_id,
            ActiveTrack {
                speaker_player,
                mic_player,
                volume: vol_f32,
            },
        );

        Ok(())
    }

    /// Останавливает воспроизведение трека
    pub fn stop(&mut self, track_id: i64) {
        if let Some(track) = self.active_tracks.remove(&track_id) {
            if let Some(spk) = track.speaker_player {
                spk.stop();
            }
            if let Some(mic) = track.mic_player {
                mic.stop();
            }
        }
    }

    /// Останавливает все играющие звуки
    pub fn stop_all(&mut self) {
        for (_, track) in self.active_tracks.drain() {
            if let Some(spk) = track.speaker_player {
                spk.stop();
            }
            if let Some(mic) = track.mic_player {
                mic.stop();
            }
        }
    }

    /// Проверяет, играет ли данный трек в данный момент
    pub fn is_playing(&self, track_id: i64) -> bool {
        if let Some(track) = self.active_tracks.get(&track_id) {
            let spk_playing = track.speaker_player.as_ref().map_or(false, |p| !p.empty());
            let mic_playing = track.mic_player.as_ref().map_or(false, |p| !p.empty());

            spk_playing || mic_playing
        } else {
            false
        }
    }

    /// Изменяет громкость для активно воспроизводящегося трека
    pub fn set_volume(&mut self, track_id: i64, volume: impl IntoF32) {
        let vol_f32 = volume.into_f32();
        if let Some(track) = self.active_tracks.get_mut(&track_id) {
            track.volume = vol_f32;
            if let Some(spk) = &track.speaker_player {
                spk.set_volume(vol_f32);
            }
            if let Some(mic) = &track.mic_player {
                mic.set_volume(vol_f32);
            }
        }
    }

    /// Устанавливает флаг вывода в динамики/наушники
    pub fn set_play_to_speakers(&mut self, val: bool) {
        self.play_to_speakers = val;
    }

    pub fn play_to_speakers(&self) -> bool {
        self.play_to_speakers
    }

    pub fn virtual_mic_sink(&self) -> &str {
        &self.virtual_mic_sink
    }

    pub fn set_virtual_mic_sink(&mut self, sink: &str) {
        self.virtual_mic_sink = sink.to_string();
        self.mic_sink = None;
        self.init_mic_sink();
    }

    pub fn set_real_mic_source(&mut self, source: &str) {
        let is_active = self.is_mic_passthrough_active();
        self.real_mic_source = Some(source.to_string());
        if is_active && !source.is_empty() {
            let _ = self.start_mic_passthrough(source);
        }
    }

    /// Запускает passthrough реального микрофона в виртуальный микрофон через pactl loopback
    pub fn start_mic_passthrough(&mut self, real_mic: &str) -> Result<()> {
        self.stop_mic_passthrough();

        let mic_to_use: String = if real_mic.trim().is_empty() {
            self.real_mic_source.clone().unwrap_or_default()
        } else {
            real_mic.to_string()
        };

        if mic_to_use.trim().is_empty() {
            anyhow::bail!("Имя микрофона не может быть пустым");
        }

        let output = Command::new("pactl")
            .args([
                "load-module",
                "module-loopback",
                &format!("source={}", mic_to_use),
                &format!("sink={}", self.virtual_mic_sink),
                "latency_msec=1",
            ])
            .output()
            .context("Не удалось запустить pactl loopback для микрофона")?;

        if !output.status.success() {
            anyhow::bail!(
                "Ошибка запуска loopback: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let id: u32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .context("Не удалось распарсить ID модуля loopback")?;

        self.loopback_id = Some(id);
        self.real_mic_source = Some(mic_to_use.to_string());
        eprintln!(
            "AudioPlayer: mic passthrough активен (id={}, source={})",
            id, mic_to_use
        );
        Ok(())
    }

    /// Останавливает passthrough реального микрофона
    pub fn stop_mic_passthrough(&mut self) {
        if let Some(id) = self.loopback_id.take() {
            let _ = Command::new("pactl")
                .args(["unload-module", &id.to_string()])
                .output();
            eprintln!("AudioPlayer: mic passthrough остановлен (id={})", id);
        }
        self.real_mic_source = None;
    }

    /// Проверяет, активен ли mic passthrough
    pub fn is_mic_passthrough_active(&self) -> bool {
        self.loopback_id.is_some()
    }

    pub fn real_mic_source(&self) -> &str {
        self.real_mic_source.as_deref().unwrap_or("")
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop_all();
        self.stop_mic_passthrough();
    }
}
