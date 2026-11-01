use anyhow::{Context, Result};
use scraper::{Html, Selector};
use std::path::{Path, PathBuf};

use crate::db::models::ShopSound;

const BASE_URL: &str = "https://www.myinstants.com";

#[allow(dead_code)]
pub struct MyInstantsClient {
    client: reqwest::blocking::Client,
}

#[allow(dead_code)]
impl MyInstantsClient {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .context("Не удалось создать HTTP клиент reqwest")?;
        Ok(Self { client })
    }

    /// Получает популярные (топовые) звуки
    pub fn get_top(&self) -> Result<Vec<ShopSound>> {
        self.get_sounds("/en/index/us/")
    }

    /// Получает звуки по произвольному пути
    pub fn get_sounds(&self, path: &str) -> Result<Vec<ShopSound>> {
        let html = self.fetch_page(path)?;
        self.parse_sound_list(&html)
    }

    /// Поиск звуков по запросу
    pub fn search(&self, query: &str) -> Result<Vec<ShopSound>> {
        let path = format!("/en/search/?name={}", urlencoding::encode(query));
        let html = self.fetch_page(&path)?;
        self.parse_sound_list(&html)
    }

    /// Скачивает звук по прямой ссылке в указанный файл
    pub fn download_sound(&self, url: &str, dest: &Path) -> Result<()> {
        let response = self.client.get(url).send()
            .with_context(|| format!("Не удалось отправить запрос на скачивание: {}", url))?;

        if !response.status().is_success() {
            anyhow::bail!("HTTP ошибка при скачивании ({}): {}", response.status(), url);
        }

        let bytes = response.bytes()
            .context("Не удалось прочитать байты звукового файла")?;

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(dest, &bytes)
            .with_context(|| format!("Не удалось сохранить файл: {}", dest.display()))?;

        Ok(())
    }

    /// Скачивает звук в кэш-директорию ~/.local/share/soundpaad/sounds/ и возвращает путь
    pub fn download_to_cache(&self, sound_name: &str, url: &str) -> Result<PathBuf> {
        let sounds_dir = dirs::data_local_dir()
            .or_else(dirs::data_dir)
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                PathBuf::from(home).join(".local").join("share")
            })
            .join("soundpaad")
            .join("sounds");

        std::fs::create_dir_all(&sounds_dir)
            .with_context(|| format!("Не удалось создать директорию кэша: {}", sounds_dir.display()))?;

        // Очищаем имя файла от недопустимых символов
        let safe_name: String = sound_name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
            .collect();
        let safe_name = safe_name.trim();
        let filename = if safe_name.is_empty() {
            format!("sound_{}.mp3", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis())
        } else {
            format!("{}.mp3", safe_name)
        };

        let target_path = sounds_dir.join(filename);

        // Если файл уже скачан и не пуст, возвращаем сразу
        if target_path.exists() && std::fs::metadata(&target_path).map(|m| m.len() > 0).unwrap_or(false) {
            return Ok(target_path);
        }

        // Если URL относительный или ведет на страницу, пробуем получить прямой mp3
        let direct_url = if url.ends_with(".mp3") {
            url.to_string()
        } else {
            self.get_sound_mp3_url(url).unwrap_or_else(|_| url.to_string())
        };

        self.download_sound(&direct_url, &target_path)?;
        Ok(target_path)
    }

    /// Находит прямой URL mp3 на странице отдельного звука
    pub fn get_sound_mp3_url(&self, instant_path: &str) -> Result<String> {
        let html = self.fetch_page(instant_path)?;
        let doc = Html::parse_document(&html);

        // 1. Ищем ссылку a[href*='/media/sounds/']
        let link_selector = Selector::parse("a[href*='/media/sounds/']").unwrap();
        if let Some(link) = doc.select(&link_selector).next() {
            if let Some(href) = link.value().attr("href") {
                let url = if href.starts_with("http") {
                    href.to_string()
                } else {
                    format!("{}{}", BASE_URL, href)
                };
                return Ok(url);
            }
        }

        // 2. Ищем в кнопке button[onclick]
        let btn_selector = Selector::parse("button[onclick]").unwrap();
        for el in doc.select(&btn_selector) {
            if let Some(onclick) = el.value().attr("onclick") {
                if let Some(url) = Self::extract_media_url(onclick) {
                    return Ok(if url.starts_with("http") {
                        url
                    } else {
                        format!("{}{}", BASE_URL, url)
                    });
                }
            }
        }

        anyhow::bail!("Не удалось найти прямую ссылку на MP3 на странице {}", instant_path)
    }

    fn fetch_page(&self, path: &str) -> Result<String> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{}", BASE_URL, path)
        };

        let response = self.client.get(&url).send()
            .with_context(|| format!("Ошибка GET запроса к {}", url))?;

        if !response.status().is_success() {
            anyhow::bail!("HTTP ошибка {} при загрузке {}", response.status(), url);
        }

        Ok(response.text()?)
    }

    fn parse_sound_list(&self, html: &str) -> Result<Vec<ShopSound>> {
        let doc = Html::parse_document(html);
        let mut sounds = Vec::new();

        let instant_selector = Selector::parse(".instant").unwrap();
        let button_selector = Selector::parse("button.small-button, button[onclick*='/media/sounds/']").unwrap();
        let link_selector = Selector::parse("a.instant-link").unwrap();

        for el in doc.select(&instant_selector) {
            let mut mp3_url = None;
            if let Some(btn) = el.select(&button_selector).next() {
                if let Some(onclick) = btn.value().attr("onclick") {
                    if let Some(extracted) = Self::extract_media_url(onclick) {
                        let full_mp3 = if extracted.starts_with("http") {
                            extracted
                        } else {
                            format!("{}{}", BASE_URL, extracted)
                        };
                        mp3_url = Some(full_mp3);
                    }
                }
            }

            if let Some(link) = el.select(&link_selector).next() {
                let name = link.text().collect::<String>().trim().to_string();
                let href = link.value().attr("href").unwrap_or("").to_string();

                if !name.is_empty() {
                    let url = mp3_url.unwrap_or_else(|| {
                        if href.starts_with("http") {
                            href
                        } else {
                            format!("{}{}", BASE_URL, href)
                        }
                    });

                    sounds.push(ShopSound {
                        name,
                        url,
                        category: None,
                    });
                }
            }
        }

        // Удаляем дубликаты по имени
        let mut seen = std::collections::HashSet::new();
        sounds.retain(|s| seen.insert(s.name.clone()));

        Ok(sounds)
    }

    fn extract_media_url(onclick: &str) -> Option<String> {
        let start = onclick.find("/media/")?;
        let sub = &onclick[start..];
        let end = sub.find(|c| c == '\'' || c == '"' || c == ',' || c == ')' || c == ' ');
        end.map(|e| sub[..e].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_media_url() {
        let onclick = "play('/media/sounds/fahhhhhhhhhhhhhh.mp3', 'loader-507030', 'fahhhhhhhhhhhhhh-3525')";
        let url = MyInstantsClient::extract_media_url(onclick);
        assert_eq!(url.as_deref(), Some("/media/sounds/fahhhhhhhhhhhhhh.mp3"));
    }

    #[test]
    fn test_parse_sound_list() {
        let html = r#"
            <div class="instant">
                <button class="small-button" onclick="play('/media/sounds/test.mp3')"></button>
                <a href="/en/instant/test-sound/" class="instant-link">Test Sound</a>
            </div>
        "#;
        let client = MyInstantsClient::new().expect("create client");
        let sounds = client.parse_sound_list(html).expect("parse html");
        assert_eq!(sounds.len(), 1);
        assert_eq!(sounds[0].name, "Test Sound");
        assert_eq!(sounds[0].url, "https://www.myinstants.com/media/sounds/test.mp3");
    }
}
