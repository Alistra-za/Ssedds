//! Minimal reader for Qt QSettings INI files as MO2 writes them (plan 02 §1.3).
use std::collections::HashMap;

pub struct Ini(HashMap<(String, String), String>);

fn unquote(v: &str) -> String {
    let v = v.trim();
    let v = v
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(v);
    let Some(v) = v
        .strip_prefix("@ByteArray(")
        .and_then(|x| x.strip_suffix(')'))
    else {
        return v.replace("\\\\", "\\");
    };
    // Qt's reader: `\xHH` -> that byte, `\\` -> `\`, `""` is the separator Qt writes after an escape; every other
    // char is its own UTF-8. MO2 stores QString::toUtf8, so decode as UTF-8 (Latin-1 if the bytes are not).
    let (mut bytes, cs, mut i) = (
        Vec::new(),
        v.replace("\"\"", "").chars().collect::<Vec<_>>(),
        0,
    );
    while i < cs.len() {
        let hex = |k: usize| cs.get(i + k).and_then(|c| c.to_digit(16));
        let c = cs[i];
        let next = cs.get(i + 1).copied();
        if c == '\\' && next == Some('x') && hex(2).is_some() {
            let n = if hex(3).is_some() { 2 } else { 1 };
            bytes.push((1..=n).fold(0, |a, k| a * 16 + hex(1 + k).unwrap()) as u8);
            i += 2 + n;
        } else if c == '\\' && next == Some('\\') {
            bytes.push(b'\\');
            i += 2;
        } else {
            bytes.extend(c.encode_utf8(&mut [0; 4]).as_bytes());
            i += 1;
        }
    }
    String::from_utf8(bytes.clone()).unwrap_or_else(|_| bytes.iter().map(|&b| b as char).collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn value(raw: &str) -> String {
        Ini::parse(&format!("[General]\ngamePath={raw}\n"))
            .get("General", "gamePath")
            .unwrap()
    }

    /// Fixtures follow Qt's `iniEscapedString` / reader rules (qsettings.cpp): bytes >= 0x80 inside a byte array
    /// are `\xHH`, a backslash is doubled, and a hex digit right after an escape is split off with `""` (the
    /// value is then quoted).
    #[test]
    fn bytearray_decodes_like_qt() {
        assert_eq!(value(r"@ByteArray(C:\\Mods\\Cafe)"), r"C:\Mods\Cafe");
        assert_eq!(value(r"@ByteArray(C:\\Mods\\Caf\xc3\xa9)"), r"C:\Mods\Café");
        assert_eq!(value(r"@ByteArray(C:\\Mods\\Café)"), r"C:\Mods\Café");
        // raw text is never re-interpreted: this is a real folder name, not mojibake
        assert_eq!(value(r"@ByteArray(C:\\Mods\\Ã©)"), r"C:\Mods\Ã©");
        assert_eq!(
            value(r#""@ByteArray(C:\\Mods\\Caf\xc3\xa9""a)""#),
            r"C:\Mods\Caféa"
        );
        // bytes that are not UTF-8 fall back to Latin-1
        assert_eq!(value(r"@ByteArray(C:\\Caf\xe9)"), r"C:\Café");
    }
}
