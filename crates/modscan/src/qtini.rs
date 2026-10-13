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
    // Qt 6 writes each byte as a Latin-1 char, Qt 5 writes non-ASCII bytes as `\xHH`; rebuild the bytes, read UTF-8.
    let (mut bytes, cs, mut i) = (Vec::new(), v.chars().collect::<Vec<_>>(), 0);
    while i < cs.len() {
        let hex = |k: usize| cs.get(i + k).and_then(|c| c.to_digit(16));
        match cs[i] {
            '\\' if cs.get(i + 1) == Some(&'\\') => {
                bytes.push(b'\\');
                i += 2;
            }
            '\\' if cs.get(i + 1) == Some(&'x') && hex(2).is_some() && hex(3).is_some() => {
                bytes.push((hex(2).unwrap() * 16 + hex(3).unwrap()) as u8);
                i += 4;
            }
            c => {
                match u8::try_from(c) {
                    Ok(b) => bytes.push(b),
                    Err(_) => bytes.extend(c.encode_utf8(&mut [0; 4]).as_bytes()),
                }
                i += 1;
            }
        }
    }
    // not valid UTF-8 once reduced to bytes: the file already held real text, keep it
    String::from_utf8(bytes).unwrap_or_else(|_| v.replace("\\\\", "\\"))
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

    fn game_path(raw: &str) -> String {
        Ini::parse(&format!("[General]\ngamePath=@ByteArray({raw})\n"))
            .get("General", "gamePath")
            .unwrap()
    }

    #[test]
    fn bytearray_decodes_both_qt_encodings() {
        // Qt 6: the UTF-8 bytes of the path are written as Latin-1 characters.
        assert_eq!(game_path(r"C:\\Mods\\CafÃ©"), r"C:\Mods\Café");
        // Qt 5: non-ASCII bytes are \xHH escapes.
        assert_eq!(game_path(r"C:\\Mods\\Caf\xc3\xa9"), r"C:\Mods\Café");
        // a value that is already real UTF-8 text (not valid once reduced to bytes) is kept
        assert_eq!(game_path(r"C:\\Mods\\Café"), r"C:\Mods\Café");
    }
}
