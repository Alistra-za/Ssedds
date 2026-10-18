//! A8: schema keys as readable labels. The key itself stays on hover.

/// Words kept upper case (compared lower case).
const INITIALISMS: [&str; 5] = ["ao", "uv", "rgb", "sss", "f0"];

/// A schema key as a label. CamelCase and snake_case split into words, first letter upper, the rest lower, known
/// initialisms kept upper ("AO", "UV", "RGB", "SSS", "F0").
pub fn readable(key: &str) -> String {
    let mut words: Vec<String> = vec![];
    for part in key.split('_').filter(|p| !p.is_empty()) {
        let cs: Vec<char> = part.chars().collect();
        let mut cur = String::new();
        for (i, &c) in cs.iter().enumerate() {
            let prev = i.checked_sub(1).map(|j| cs[j]);
            let next = cs.get(i + 1).copied();
            let split = c.is_uppercase()
                && prev.is_some_and(|p| {
                    p.is_lowercase() || (p.is_uppercase() && next.is_some_and(char::is_lowercase))
                });
            if split {
                words.push(std::mem::take(&mut cur));
            }
            cur.push(c);
        }
        words.push(cur);
    }
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        let lw = w.to_lowercase();
        let word = if INITIALISMS.contains(&lw.as_str()) {
            lw.to_uppercase()
        } else if i == 0 {
            let mut c = lw.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        } else {
            lw
        };
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&word);
    }
    out
}
