use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AudioServer {
    PipeWire,
    PulseAudio,
}

const VIRTUAL_MIC_NAME: &str = "SoundPad_Mic";
const VIRTUAL_SINK_NAME: &str = "SoundPad_Sink";
const VIRTUAL_MIC_DESC: &str = "SoundPad Virtual Microphone";

pub struct VirtualMic {
    server: AudioServer,
    sink_id: Option<u32>,
    loopback_id: Option<u32>,
}

impl VirtualMic {
    pub fn detect_server() -> Result<AudioServer> {
        let output = Command::new("pactl")
            .args(["info"])
            .output()
            .context("Не удалось выполнить pactl info. Установлен ли PulseAudio/PipeWire?")?;

        let info = String::from_utf8_lossy(&output.stdout);
        if info.contains("PipeWire") {
            Ok(AudioServer::PipeWire)
        } else if info.contains("PulseAudio") {
            Ok(AudioServer::PulseAudio)
        } else {
            Ok(AudioServer::PipeWire)
        }
    }

    pub fn new() -> Result<Self> {
        let server = Self::detect_server()?;
        Ok(Self {
            server,
            sink_id: None,
            loopback_id: None,
        })
    }

    pub fn server(&self) -> AudioServer {
        self.server
    }

    pub fn is_active(&self) -> bool {
        self.sink_id.is_some()
    }

    pub fn sink_name(&self) -> &str {
        VIRTUAL_SINK_NAME
    }

    pub fn source_name(&self) -> &str {
        VIRTUAL_MIC_NAME
    }

    pub fn create(&mut self) -> Result<()> {
        // Убиваем все старые дубликаты
        self.cleanup_duplicates();

        let sink_arg = format!("sink_name={}", VIRTUAL_SINK_NAME);
        let sink_desc = format!("sink_properties=device.description=\"{}\"", VIRTUAL_MIC_DESC);

        // Создаём null sink
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
            .context("Не удалось создать null sink")?;

        if !output.status.success() {
            anyhow::bail!(
                "Ошибка создания null sink: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let id_str = String::from_utf8_lossy(&output.stdout);
        let sink_id: u32 = id_str.trim().parse().context("Не удалось распарсить ID sink")?;
        self.sink_id = Some(sink_id);

        // Создаём remap-source из monitor — чтобы голосовой чат видел его как настоящий микрофон
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
            .context("Не удалось создать remap-source")?;

        if remap_output.status.success() {
            let id = String::from_utf8_lossy(&remap_output.stdout).trim().to_string();
            if let Ok(lb_id) = id.parse::<u32>() {
                self.loopback_id = Some(lb_id);
            }
            eprintln!("VirtualMic: микрофон создан (ID: {}, Name: {})", id, VIRTUAL_MIC_NAME);
        } else {
            let err = String::from_utf8_lossy(&remap_output.stderr);
            eprintln!("VirtualMic: ошибка создания микрофона: {}", err);
        }

        Ok(())
    }

    pub fn destroy(&mut self) -> Result<()> {
        if let Some(lb_id) = self.loopback_id.take() {
            let _ = Command::new("pactl")
                .args(["unload-module", &lb_id.to_string()])
                .output();
        }

        if let Some(sink_id) = self.sink_id.take() {
            let output = Command::new("pactl")
                .args(["unload-module", &sink_id.to_string()])
                .output()
                .context("Не удалось выгрузить null sink")?;

            if !output.status.success() {
                anyhow::bail!(
                    "Ошибка выгрузки null sink: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(())
    }

    pub fn get_default_sink() -> Result<String> {
        let output = Command::new("pactl")
            .args(["get-default-sink"])
            .output()
            .context("Не удалось получить default sink")?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn cleanup_duplicates(&self) {
        // Находим и выгружаем все старые модули с SoundPad
        let Ok(output) = Command::new("pactl")
            .args(["list", "short", "modules"])
            .output()
        else {
            return;
        };

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains("SoundPad") {
                if let Some(id_str) = line.split_whitespace().next() {
                    if let Ok(id) = id_str.parse::<u32>() {
                        eprintln!("VirtualMic: выгружаем модуль {}", id);
                        let _ = Command::new("pactl")
                            .args(["unload-module", &id.to_string()])
                            .output();
                    }
                }
            }
        }
    }

    pub fn list_sources() -> Result<Vec<String>> {
        let output = Command::new("pactl")
            .args(["list", "short", "sources"])
            .output()
            .context("Не удалось получить список sources")?;

        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text
            .lines()
            .filter_map(|l| l.split_whitespace().nth(1).map(String::from))
            .filter(|s| !s.ends_with(".monitor"))
            .filter(|s| !s.contains("SoundPad"))
            .collect())
    }
}

impl Drop for VirtualMic {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}
