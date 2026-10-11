//! Minimal reader for Qt QSettings INI files as MO2 writes them (plan 02 §1.3).
use std::collections::HashMap;

pub struct Ini(HashMap<(String, String), String>);

fn unquote(v: &str) -> String {
    let v = v.trim();
    let v = v
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(v);
    let v = v
        .strip_prefix("@ByteArray(")
        .and_then(|x| x.strip_suffix(')'))
        .unwrap_or(v);
    v.replace("\\\\", "\\")
}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        let mut map = HashMap::new();
        let mut section = String::from("General");
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(s) = line.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
                section = s.to_string();
            } else if let Some((k, v)) = line.split_once('=') {
                map.insert(
                    (section.to_ascii_lowercase(), k.trim().to_ascii_lowercase()),
                    unquote(v),
                );
            }
        }
        Ini(map)
    }
    pub fn get(&self, section: &str, key: &str) -> Option<String> {
        self.0
            .get(&(section.to_ascii_lowercase(), key.to_ascii_lowercase()))
            .cloned()
    }
}
