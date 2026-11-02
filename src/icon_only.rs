//! Small, explicit line-break rules for the supported names; no language-wide promises.
fn attached(ch: char) -> bool {
    matches!(ch, '\u{0300}'..='\u{036f}' | '\u{3099}'..='\u{309a}' | '\u{fe00}'..='\u{fe0f}' | '\u{200d}')
}
fn forbidden_start(ch: char) -> bool {
    "、。,.・ー」』)〕】ぁぃぅぇぉっゃゅょゎゕゖァィゥェォッャュョヮヵヶ".contains(ch)
}
fn japanese(ch: char) -> bool {
    matches!(ch, '\u{3000}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{ff66}'..='\u{ff9f}')
}

fn clusters(text: &str) -> Vec<String> {
    let mut output: Vec<String> = Vec::new();
    let mut joined = false;
    for ch in text.chars() {
        if !output.is_empty() && (attached(ch) || joined) {
            output.last_mut().unwrap().push(ch);
        } else {
            output.push(ch.to_string());
        }
        joined = ch == '\u{200d}';
    }
    output
}

fn ellipsis(text: &str, width: i32, measure: &impl Fn(&str) -> i32) -> String {
    let mut pieces = clusters(text.trim_end());
    loop {
        let candidate = format!("{}…", pieces.concat());
        if measure(&candidate) <= width || pieces.is_empty() {
            return candidate;
        }
        pieces.pop();
    }
}

pub fn lines(text: &str, width: i32, maximum: usize, measure: impl Fn(&str) -> i32) -> Vec<String> {
    if text.is_empty() || maximum == 0 || width <= 0 {
        return Vec::new();
    }
    let mut tokens: Vec<(String, bool)> = Vec::new();
    let mut word = String::new();
    let mut space = false;
    let mut word_space = false;
    let flush = |word: &mut String, tokens: &mut Vec<(String, bool)>, before: bool| {
        if !word.is_empty() {
            tokens.push((std::mem::take(word), before));
        }
    };
    for cluster in clusters(text) {
        let ch = cluster.chars().next().unwrap();
        if ch.is_whitespace() {
            flush(&mut word, &mut tokens, word_space);
            space = true;
        } else if japanese(ch) && !forbidden_start(ch) {
            flush(&mut word, &mut tokens, word_space);
            tokens.push((cluster, space));
            space = false;
        } else if forbidden_start(ch) && word.is_empty() && !space && !tokens.is_empty() {
            tokens.last_mut().unwrap().0.push_str(&cluster);
        } else {
            if word.is_empty() {
                word_space = space;
                space = false;
            }
            word.push_str(&cluster);
            if ch == '_' || ch == '-' {
                flush(&mut word, &mut tokens, word_space);
            }
        }
    }
    flush(&mut word, &mut tokens, word_space);
    let mut output: Vec<String> = Vec::new();
    let mut current = String::new();
    for (token, before) in tokens {
        let candidate = format!(
            "{current}{}{token}",
            if before && !current.is_empty() {
                " "
            } else {
                ""
            }
        );
        if measure(&candidate) <= width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            if output.len() + 1 >= maximum {
                output.push(ellipsis(&candidate, width, &measure));
                return output;
            }
            output.push(std::mem::take(&mut current));
        }
        if measure(&token) > width {
            output.push(ellipsis(&token, width, &measure));
            return output;
        }
        current = token;
    }
    if !current.is_empty() {
        output.push(current);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn measure(text: &str) -> i32 {
        text.chars()
            .map(|ch| if japanese(ch) { 12 } else { 6 })
            .sum()
    }
    #[test]
    fn names_and_unbreakable_words() {
        for (name, expected) in [
            ("claude_launch", vec!["claude_", "launch"]),
            ("Python 3.13 (64-bit)", vec!["Python 3.13", "(64-bit)"]),
            ("Visual Studio Code", vec!["Visual", "Studio Code"]),
            ("コマンドプロンプト", vec!["コマンドプ", "ロンプト"]),
            ("QNAP Qfinder Pro", vec!["QNAP", "Qfinder Pro"]),
            ("Supercalifragilisticexpialidocious", vec!["Supercalif…"]),
        ] {
            assert_eq!(lines(name, 66, 4, measure), expected, "{name}");
        }
    }
    #[test]
    fn clusters_and_line_limit() {
        assert_eq!(
            clusters("a\u{301}👩\u{200d}💻か\u{3099}"),
            vec!["a\u{301}", "👩\u{200d}💻", "か\u{3099}"]
        );
        let result = lines("one two three four five six seven", 42, 2, measure);
        assert_eq!(result.len(), 2);
        assert!(result[1].ends_with('…'));
        assert!(lines("", 66, 4, measure).is_empty());
        assert_eq!(
            lines("あいうえお、か", 60, 4, measure),
            vec!["あいうえ", "お、か"]
        );
    }
}
