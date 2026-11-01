use anyhow::{Context, Result};
use std::process::Command;

pub const VIRTUAL_SINK_NAME: &str = "SoundPad_Sink";
pub const VIRTUAL_MIC_NAME: &str = "SoundPad_Mic";
pub const VIRTUAL_MIC_DESC: &str = "SoundPad Virtual Microphone";
pub const VIRTUAL_SINK_DESC: &str = "SoundPad Virtual Sink";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioServer {
    PipeWire,
    PulseAudio,
}

#[derive(Debug, Default)]
pub struct VirtualMic {
    server: Option<AudioServer>,
    sink_id: Option<u32>,
    remap_id: Option<u32>,
    loopback_id: Option<u32>,
    real_mic: Option<String>,
}

impl VirtualMic {
    pub fn detect_server() -> Result<AudioServer> {
        let output = Command::new("pactl")
            .args(["info"])
            .output()
            .context("Не удалось выполнить pactl info")?;

        let info = String::from_utf8_lossy(&output.stdout);
        if info.contains("PipeWire") {
            Ok(AudioServer::PipeWire)
        } else {
            Ok(AudioServer::PulseAudio)
        }
    }

    pub fn new() -> Result<Self> {
        let server = Self::detect_server().ok();
        Ok(Self {
            server,
            sink_id: None,
            remap_id: None,
            loopback_id: None,
            real_mic: None,
        })
    }

    pub fn server(&self) -> AudioServer {
        self.server.unwrap_or_else(|| Self::detect_server().unwrap_or(AudioServer::PipeWire))
    }

    pub fn sink_name(&self) -> &str {
        VIRTUAL_SINK_NAME
    }

    pub fn source_name(&self) -> &str {
        VIRTUAL_MIC_NAME
    }

    pub fn is_active(&self) -> bool {
        self.sink_id.is_some()
    }

    pub fn is_mic_passthrough_active(&self) -> bool {
        self.loopback_id.is_some()
    }

    pub fn real_mic(&self) -> Option<&str> {
        self.real_mic.as_deref()
    }

    /// Очищает все старые/зависшие модули SoundPad из pactl
    pub fn cleanup_duplicates() {
        let Ok(output) = Command::new("pactl")
            .args(["list", "short", "modules"])
            .output()
        else {
            return;
        };

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let lower = line.to_lowercase();
            if lower.contains("soundpad") {
                if let Some(id_str) = line.split_whitespace().next() {
                    if let Ok(id) = id_str.parse::<u32>() {
                        eprintln!("VirtualMic: выгружаем старый модуль {}", id);
                        let _ = Command::new("pactl")
                            .args(["unload-module", &id.to_string()])
                            .output();
                    }
                }
            }
        }
    }

    /// Создает виртуальный sink и remap-source микрофон
    pub fn create(&mut self) -> Result<()> {
        Self::cleanup_duplicates();

        let sink_arg = format!("sink_name={}", VIRTUAL_SINK_NAME);
        let sink_desc = format!("sink_properties=device.description=\"{}\"", VIRTUAL_SINK_DESC);

        // 1. Создаем null sink: 48000Hz, stereo
        let output = Command::new("pactl")
            .args([
                "load-module",
                "module-null-sink",
                &sink_arg,
                "sink_format=s16le",
                "sink_rate=48000",
                "sink_channels=2",
                &sink_desc,
            ])
            .output()
            .context("Не удалось выполнить команду pactl для создания null-sink")?;

        if !output.status.success() {
            anyhow::bail!(
                "Ошибка создания null-sink: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let sink_id_str = String::from_utf8_lossy(&output.stdout);
        let sink_id: u32 = sink_id_str
            .trim()
            .parse()
            .context("Не удалось распарсить ID null-sink")?;
        self.sink_id = Some(sink_id);

        // 2. Создаем remap-source из SoundPad_Sink.monitor -> SoundPad_Mic
        let monitor_name = format!("{}.monitor", VIRTUAL_SINK_NAME);
        let remap_output = Command::new("pactl")
            .args([
                "load-module",
                "module-remap-source",
                &format!("source_name={}", VIRTUAL_MIC_NAME),
                &format!("master={}", monitor_name),
                &format!("source_properties=device.description=\"{}\"", VIRTUAL_MIC_DESC),
            ])
            .output()
            .context("Не удалось выполнить команду pactl для создания remap-source")?;

        if !remap_output.status.success() {
            anyhow::bail!(
                "Ошибка создания remap-source: {}",
                String::from_utf8_lossy(&remap_output.stderr)
            );
        }

        let remap_id_str = String::from_utf8_lossy(&remap_output.stdout);
        let remap_id: u32 = remap_id_str
            .trim()
            .parse()
            .context("Не удалось распарсить ID remap-source")?;
        self.remap_id = Some(remap_id);

        eprintln!(
            "VirtualMic: успешно создан (sink_id={}, remap_id={})",
            sink_id, remap_id
        );

        // Если ранее был задан реальный микрофон, восстанавливаем passthrough
        if let Some(real_mic) = self.real_mic.clone() {
            let _ = self.start_mic_passthrough(&real_mic);
        }

        Ok(())
    }

    /// Запускает passthrough реального микрофона в виртуальный микрофон через module-loopback
    pub fn start_mic_passthrough(&mut self, real_mic_source: &str) -> Result<()> {
        self.stop_mic_passthrough().ok();

        if real_mic_source.trim().is_empty() {
            anyhow::bail!("Имя источника микрофона не может быть пустым");
        }

        let output = Command::new("pactl")
            .args([
                "load-module",
                "module-loopback",
                &format!("source={}", real_mic_source),
                &format!("sink={}", VIRTUAL_SINK_NAME),
                "latency_msec=1",
            ])
            .output()
            .context("Не удалось создать module-loopback для реального микрофона")?;

        if !output.status.success() {
            anyhow::bail!(
                "Ошибка создания loopback: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let loopback_id: u32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .context("Не удалось распарсить ID loopback модуля")?;

        self.loopback_id = Some(loopback_id);
        self.real_mic = Some(real_mic_source.to_string());
        eprintln!(
            "VirtualMic: mic passthrough запущен (id={}, source={})",
            loopback_id, real_mic_source
        );
        Ok(())
    }

    /// Останавливает passthrough реального микрофона
    pub fn stop_mic_passthrough(&mut self) -> Result<()> {
        if let Some(id) = self.loopback_id.take() {
            let _ = Command::new("pactl")
                .args(["unload-module", &id.to_string()])
                .output();
            eprintln!("VirtualMic: mic passthrough остановлен (id={})", id);
        }
        self.real_mic = None;
        Ok(())
    }

    /// Уничтожает созданные модули и очищает ресурсы
    pub fn destroy(&mut self) -> Result<()> {
        let _ = self.stop_mic_passthrough();

        if let Some(remap_id) = self.remap_id.take() {
            let _ = Command::new("pactl")
                .args(["unload-module", &remap_id.to_string()])
                .output();
        }

        if let Some(sink_id) = self.sink_id.take() {
            let _ = Command::new("pactl")
                .args(["unload-module", &sink_id.to_string()])
                .output();
        }

        Self::cleanup_duplicates();
        Ok(())
    }

    /// Возвращает список доступных источников звука (микрофонов), исключая мониторы и SoundPad
    pub fn list_sources() -> Result<Vec<String>> {
        let output = Command::new("pactl")
            .args(["list", "short", "sources"])
            .output()
            .context("Не удалось получить список sources через pactl")?;

        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text
            .lines()
            .filter_map(|l| l.split_whitespace().nth(1).map(String::from))
            .filter(|s| !s.ends_with(".monitor"))
            .filter(|s| !s.to_lowercase().contains("soundpad"))
            .collect())
    }

    /// Возвращает список доступных аудиовыходов (sinks)
    pub fn list_sinks() -> Result<Vec<String>> {
        let output = Command::new("pactl")
            .args(["list", "short", "sinks"])
            .output()
            .context("Не удалось получить список sinks через pactl")?;

        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text
            .lines()
            .filter_map(|l| l.split_whitespace().nth(1).map(String::from))
            .collect())
    }

    /// Возвращает текущий системный выход звука по умолчанию
    pub fn get_default_sink() -> Result<String> {
        let output = Command::new("pactl")
            .args(["get-default-sink"])
            .output()
            .context("Не удалось получить default sink через pactl")?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Возвращает текущий системный микрофон по умолчанию
    pub fn get_default_source() -> Result<String> {
        let output = Command::new("pactl")
            .args(["get-default-source"])
            .output()
            .context("Не удалось получить default source через pactl")?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl Drop for VirtualMic {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_mic_queries() {
        if let Ok(sink) = VirtualMic::get_default_sink() {
            assert!(!sink.is_empty());
        }
        if let Ok(source) = VirtualMic::get_default_source() {
            assert!(!source.is_empty());
        }
        if let Ok(sinks) = VirtualMic::list_sinks() {
            assert!(!sinks.is_empty());
        }
        let _ = VirtualMic::list_sources();
    }
}
