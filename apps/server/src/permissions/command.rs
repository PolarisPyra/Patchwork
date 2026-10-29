//! Reading a shell command line the way a rule names it: its program and subcommand, past assignments and wrappers.

/// Programs whose first argument names what they do, so a rule names both.
const SUBCOMMANDS: [&str; 16] = [
    "git",
    "cargo",
    "npm",
    "pnpm",
    "yarn",
    "bun",
    "uv",
    "go",
    "docker",
    "kubectl",
    "gh",
    "pip",
    "rustup",
    "systemctl",
    "brew",
    "pacman",
];

/// The leading words a rule for `words` names: the program, and its subcommand for the programs
/// that have one.
pub(super) fn command_prefix(words: &[String]) -> String {
    let Some(program) = words.first() else {
        return String::new();
    };
    let program = program.rsplit('/').next().unwrap_or(program);
    match words.get(1) {
        Some(sub)
            if SUBCOMMANDS.contains(&program)
                && sub
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':')
                && !sub.starts_with('-') =>
        {
            format!("{program} {sub}")
        }
        _ => program.into(),
    }
}

/// A shell line as its simple commands' words, split at `&&`, `||`, `;`, `|`, `&`, and new lines,
/// with leading `NAME=value` assignments dropped and `sh -c '…'` unwrapped. The flag is false
/// when the line does more than its commands show: a substitution, a subshell, or a redirect
/// other than to /dev/null or another descriptor.
pub(super) fn split_command(line: &str) -> (Vec<Vec<String>>, bool) {
    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut safe = true;
    let mut chars = line.chars().peekable();
    let end_word = |word: &mut String, in_word: &mut bool, words: &mut Vec<String>| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    word.push(c);
                }
            }
            '"' => {
                in_word = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => word.extend(chars.next()),
                        '`' => {
                            safe = false;
                            word.push(c);
                        }
                        '$' if chars.peek() == Some(&'(') => {
                            safe = false;
                            word.push(c);
                        }
                        _ => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.extend(chars.next());
            }
            '`' | '(' | ')' => {
                safe = false;
                in_word = true;
                word.push(c);
            }
            '$' if chars.peek() == Some(&'(') => {
                safe = false;
                in_word = true;
                word.push(c);
            }
            '>' | '<' => {
                // `2>` names a descriptor, not a word.
                if in_word && word.chars().all(|c| c.is_ascii_digit()) {
                    word.clear();
                    in_word = false;
                }
                end_word(&mut word, &mut in_word, &mut words);
                if c == '<' {
                    safe = false;
                    continue;
                }
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                if chars.peek() == Some(&'&') {
                    chars.next();
                    if chars
                        .peek()
                        .is_some_and(|c| c.is_ascii_digit() || *c == '-')
                    {
                        chars.next();
                        continue;
                    }
                }
                while chars.peek().is_some_and(|c| *c == ' ' || *c == '\t') {
                    chars.next();
                }
                let mut file = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || matches!(c, ';' | '&' | '|') {
                        break;
                    }
                    file.push(c);
                    chars.next();
                }
                if file != "/dev/null" {
                    safe = false;
                }
            }
            ';' | '&' | '|' | '\n' => {
                end_word(&mut word, &mut in_word, &mut words);
                if matches!(c, '&' | '|') && chars.peek() == Some(&c) {
                    chars.next();
                }
                segments.push(std::mem::take(&mut words));
            }
            c if c.is_whitespace() => end_word(&mut word, &mut in_word, &mut words),
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    end_word(&mut word, &mut in_word, &mut words);
    segments.push(words);

    let mut out = Vec::new();
    for mut words in segments {
        let assignments = words.iter().take_while(|w| is_assignment(w)).count();
        words.drain(..assignments);
        if words.is_empty() {
            continue;
        }
        let shell = words[0].rsplit('/').next().unwrap_or_default();
        if words.len() == 3
            && matches!(shell, "sh" | "bash" | "zsh" | "dash")
            && matches!(words[1].as_str(), "-c" | "-lc" | "-ic")
        {
            let (inner, inner_safe) = split_command(&words[2]);
            safe &= inner_safe;
            out.extend(inner);
        } else {
            out.push(words);
        }
    }
    (out, safe)
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}
