//! The `boundedWaits` check. It refuses a shell command that polls in a loop (`until` or `while`
//! with a `sleep` in it) with no time limit, and one that polls for a word cargo prints in colour.
//! Only what runs counts, so a loop quoted as data, such as an argument to `printf` or `jq`, is no
//! loop. The scripts `sh -c` and its kin, `eval`, `$(…)` and a shell's heredoc run are read too,
//! and one that `timeout` runs has its limit.

use super::shell::{self, Command, Token};

const UNBOUNDED: &str = "This wait/poll loop has no time limit. Wrap it in timeout N (for example \
timeout 300 sh -c ...) and make it also end on failure.";

const CARGO_WORDS: &str = "This wait greps cargo status words. Cargo colours them (Running\\e[0m), \
so the pattern never matches and the wait hangs. Wait on the process instead (pgrep -x NAME, then \
readlink /proc/PID/exe) or on the port (ss -ltn), and start the build with CARGO_TERM_COLOR=never.";

/// The words cargo prints in colour before a step, which a pattern waiting for them never matches.
const CARGO_STATUS: [&str; 4] = ["Running", "Compiling", "Finished", "Building"];

/// The shells whose `-c` script, or heredoc, is read as a script too.
const SHELLS: [&str; 7] = ["sh", "bash", "zsh", "dash", "ksh", "mksh", "ash"];

/// Programs that run the rest of their words as the command, and their options that take a value.
const WRAPPERS: [(&str, &[&str]); 9] = [
    ("env", &["-u", "-C", "-S", "--unset", "--chdir"]),
    ("exec", &["-a"]),
    ("nohup", &[]),
    ("setsid", &[]),
    ("command", &[]),
    ("builtin", &[]),
    ("time", &["-f", "-o"]),
    ("nice", &["-n"]),
    ("stdbuf", &["-i", "-o", "-e"]),
];

/// Nested scripts are read this deep at most.
const MAX_DEPTH: usize = 8;

/// Why `command` is refused, or None to let it run.
pub fn check(command: &str) -> Option<String> {
    script(command, false, 0).map(String::from)
}

/// Checks a script that runs with a time limit when `bounded`.
fn script(text: &str, bounded: bool, depth: usize) -> Option<&'static str> {
    if depth > MAX_DEPTH {
        return None;
    }
    let tokens = shell::tokens(text);
    if let Some(refusal) = loops(&tokens, bounded) {
        return Some(refusal);
    }
    for command in shell::commands(&tokens) {
        // A substitution runs before its command, under whatever limit the script has.
        for token in &command.words {
            if let Token::Word { subs, .. } = token {
                for sub in subs {
                    if let Some(refusal) = script(sub, bounded, depth + 1) {
                        return Some(refusal);
                    }
                }
            }
        }
        if let Some(refusal) = runs_script(&command, bounded, depth) {
            return Some(refusal);
        }
    }
    None
}

/// The loops in `tokens` that poll: refused when they have no limit, or wait for cargo's words.
fn loops(tokens: &[Token], bounded: bool) -> Option<&'static str> {
    let mut command_start = true;
    for (at, token) in tokens.iter().enumerate() {
        let opens = shell::is_keyword(token, "until") || shell::is_keyword(token, "while");
        if command_start && opens {
            let end = loop_end(tokens, at);
            let inside = shell::commands(&tokens[at + 1..end]);
            if inside.iter().any(|c| program(c, false).0 == Some("sleep")) {
                if inside.iter().any(greps_cargo_words) {
                    return Some(CARGO_WORDS);
                }
                if !bounded {
                    return Some(UNBOUNDED);
                }
            }
        }
        command_start = match token {
            Token::Op(_) => true,
            Token::Word { .. } => [
                "then", "do", "else", "elif", "if", "while", "until", "{", "!",
            ]
            .iter()
            .any(|word| shell::is_keyword(token, word)),
            Token::Redirect { .. } => command_start,
        };
    }
    None
}

/// Where the loop opened at `at` ends: its `done`, past any loops inside it.
fn loop_end(tokens: &[Token], at: usize) -> usize {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(at + 1) {
        if shell::is_keyword(token, "do") {
            depth += 1;
        } else if shell::is_keyword(token, "done") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return i;
            }
        }
    }
    tokens.len()
}

/// Whether `command` greps for a word cargo prints in colour.
fn greps_cargo_words(command: &Command) -> bool {
    let (name, at, _) = program(command, false);
    let texts = command.texts();
    matches!(name, Some("grep" | "egrep" | "fgrep" | "rg"))
        && texts[at + 1..]
            .iter()
            .any(|word| CARGO_STATUS.iter().any(|status| word.contains(status)))
}

/// Checks the script `command` runs, if it runs one: a shell's `-c` script or heredoc, or what
/// `eval` is given.
fn runs_script(command: &Command, bounded: bool, depth: usize) -> Option<&'static str> {
    let (name, at, bounded) = program(command, bounded);
    let texts = command.texts();
    let args = texts.get(at + 1..).unwrap_or_default();
    match name? {
        "eval" => script(&args.join(" "), bounded, depth + 1),
        name if SHELLS.contains(&name) => {
            let flag = args.iter().position(|arg| {
                arg.starts_with('-') && !arg.starts_with("--") && arg.contains('c')
            });
            match flag {
                Some(flag) => {
                    let rest = &args[flag + 1..];
                    let rest = rest.strip_prefix(&["--"]).unwrap_or(rest);
                    script(rest.first()?, bounded, depth + 1)
                }
                None => command
                    .heredocs
                    .iter()
                    .find_map(|body| script(body, bounded, depth + 1)),
            }
        }
        _ => None,
    }
}

/// The program `command` runs, past its assignments, wrappers like `env` and `nohup`, and
/// `timeout`: its name, the index of its word, and whether it runs with a time limit, which
/// `timeout` gives it.
fn program<'a>(command: &'a Command, bounded: bool) -> (Option<&'a str>, usize, bool) {
    let texts = command.texts();
    let mut bounded = bounded;
    let mut at = 0;
    while let Some(word) = texts.get(at) {
        let name = word.rsplit('/').next().unwrap_or(word);
        if is_assignment(word) {
            at += 1;
        } else if let Some((_, takes_value)) = WRAPPERS.iter().find(|(w, _)| *w == name) {
            at = past_options(&texts, at + 1, takes_value);
        } else if name == "timeout" || name == "gtimeout" {
            let at_duration =
                past_options(&texts, at + 1, &["-s", "-k", "--signal", "--kill-after"]);
            bounded = true;
            at = at_duration + 1;
        } else {
            return (Some(name), at, bounded);
        }
    }
    (None, at, bounded)
}

/// The index of the first word from `at` that is not an option, skipping the value of each
/// option in `takes_value` and the `--` that ends them.
fn past_options(texts: &[&str], mut at: usize, takes_value: &[&str]) -> usize {
    while let Some(word) = texts.get(at) {
        if *word == "--" {
            return at + 1;
        }
        if is_assignment(word) {
            at += 1;
        } else if word.starts_with('-') && word.len() > 1 {
            at += if takes_value.contains(word) { 2 } else { 1 };
        } else {
            break;
        }
    }
    at
}

/// Whether `word` is `NAME=value`.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::check;

    const LOOP: &str = "while true; do sleep 1; done";

    #[test]
    fn refuses_an_executed_wait_loop() {
        assert!(check(LOOP).is_some());
        assert!(check(&format!("sh -c '{LOOP}'")).is_some());
        assert!(check(&format!("printf x; eval '{LOOP}'")).is_some());
    }

    #[test]
    fn allows_a_loop_quoted_as_data() {
        assert_eq!(check(&format!("printf '%s\\n' '{LOOP}'")), None);
        assert_eq!(
            check(&format!("jq -n --arg script '{LOOP}' '$script'")),
            None
        );
        assert_eq!(check(&format!("echo \"{LOOP}\"")), None);
        assert_eq!(check(&format!("rg -F '{LOOP}' script.sh")), None);
        assert_eq!(
            check(&format!("cat >script.sh <<'SCRIPT'\n{LOOP}\nSCRIPT")),
            None
        );
    }

    #[test]
    fn allows_an_executed_loop_with_a_timeout() {
        assert_eq!(check(&format!("timeout 10 sh -c '{LOOP}'")), None);
    }

    #[test]
    fn catches_command_substitution_and_heredoc_scripts() {
        assert!(check(&format!("printf '%s' \"$( {LOOP} )\"")).is_some());
        assert!(check(&format!("sh <<'SCRIPT'\n{LOOP}\nSCRIPT")).is_some());
    }
}
