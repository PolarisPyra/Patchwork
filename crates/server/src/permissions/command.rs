//! Reading a shell command line the way a rule names it: its program and subcommand, past assignments and wrappers.

// Rust guideline compliant 2026-10-09

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

/// Words the shell reads before a command without their being one: `{ rm x; }`, `then rm x`,
/// `done`.
const RESERVED: [&str; 18] = [
    "{", "}", "!", "if", "then", "elif", "else", "fi", "while", "until", "for", "select", "do",
    "done", "case", "esac", "time", "function",
];

/// Whether the shell reads `word` before a command without its being one.
fn is_reserved(word: &str) -> bool {
    RESERVED.contains(&word)
}

/// Variables that change only how a program reports, never what it runs, so setting one before a
/// command leaves a rule for the command answering for it.
const HARMLESS_VARIABLES: [&str; 14] = [
    "RUST_LOG",
    "RUST_BACKTRACE",
    "CARGO_TERM_COLOR",
    "NO_COLOR",
    "FORCE_COLOR",
    "CLICOLOR",
    "TERM",
    "COLUMNS",
    "LANG",
    "LC_ALL",
    "TZ",
    "CI",
    "NODE_ENV",
    "DEBUG",
];

/// Programs that run the command named after them, so a rule for that command covers them too.
const WRAPPERS: [&str; 20] = [
    "env", "command", "builtin", "exec", "eval", "nohup", "nice", "xargs", "sudo", "doas",
    "timeout", "setsid", "stdbuf", "ionice", "chrt", "taskset", "flock", "watch", "unbuffer",
    "busybox",
];

/// Whether a word after a wrapper is one of its own settings and not the command it runs: a
/// flag, or a number, duration, range or mask such as `5`, `2.5s`, `0-3` and `0x3`.
fn is_setting(word: &str) -> bool {
    let value = word.strip_suffix(['s', 'm', 'h', 'd']).unwrap_or(word);
    word.starts_with('-')
        || (value.starts_with(|c: char| c.is_ascii_digit())
            && value
                .chars()
                .all(|c| c.is_ascii_hexdigit() || matches!(c, '.' | ',' | '-' | ':' | 'x')))
}

/// The flags of a wrapper that are followed by a value of their own, which is then not the command
/// it runs: `sudo -u bob rm`, `env -u HOME rm`, `timeout -s KILL 5 rm`, `xargs -I {} rm {}`.
const VALUE_FLAGS: [(&str, &[&str]); 8] = [
    (
        "sudo",
        &[
            "-u",
            "-g",
            "-h",
            "-p",
            "-C",
            "-D",
            "-R",
            "-T",
            "-U",
            "-r",
            "-t",
            "--user",
            "--group",
            "--host",
            "--prompt",
            "--chdir",
            "--chroot",
            "--role",
            "--type",
            "--other-user",
            "--close-from",
            "--command-timeout",
        ],
    ),
    ("doas", &["-u", "-C"]),
    ("env", &["-u", "-C", "--unset", "--chdir"]),
    ("timeout", &["-s", "-k", "--signal", "--kill-after"]),
    (
        "xargs",
        &[
            "-I",
            "-d",
            "-a",
            "-E",
            "-n",
            "-P",
            "-L",
            "-s",
            "--delimiter",
            "--arg-file",
            "--max-args",
            "--max-procs",
            "--max-lines",
            "--max-chars",
        ],
    ),
    ("stdbuf", &["-i", "-o", "-e"]),
    ("exec", &["-a"]),
    ("flock", &["-w", "-E", "--timeout", "--conflict-exit-code"]),
];

/// How many of the words after the wrapper `program` are its own settings and their values.
fn own_words(program: &str, rest: &[String]) -> usize {
    let valued = VALUE_FLAGS
        .iter()
        .find(|(wrapper, _)| *wrapper == program)
        .map_or(&[][..], |(_, flags)| *flags);
    let takes_value = |word: &str| {
        valued.contains(&word)
            // Short flags written together, the last of which takes the value: `sudo -Eu bob`.
            || (word.len() > 2
                && !word.starts_with("--")
                && valued.iter().any(|flag| {
                    flag.len() == 2 && word.ends_with(flag.trim_start_matches('-'))
                }))
    };
    let mut own = 0;
    while let Some(word) = rest.get(own).filter(|word| is_setting(word)) {
        own += 1;
        if takes_value(word) && own < rest.len() {
            own += 1;
        }
    }
    own
}

/// The options git takes before its subcommand that are followed by a value of their own.
const GIT_VALUE_OPTIONS: [&str; 7] = [
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--super-prefix",
];

/// `git` and its subcommand without the options written between them, so a rule for `git push`
/// covers `git -C . push`. None when there are no such options.
fn git_without_options(words: &[String]) -> Option<Vec<String>> {
    let mut at = 1;
    while let Some(word) = words.get(at).filter(|word| word.starts_with('-')) {
        at += if GIT_VALUE_OPTIONS.contains(&word.as_str()) {
            2
        } else {
            1
        };
    }
    (at > 1 && at < words.len()).then(|| {
        let mut plain = vec![words[0].clone()];
        plain.extend_from_slice(&words[at..]);
        plain
    })
}

/// Whether a program is written so that it could be any program: with a pattern the shell fills
/// in, or a variable. A rule that refuses or asks covers it, as it cannot be told it is not
/// the rule's command.
pub(super) fn any_program(program: &str) -> bool {
    program.contains(['*', '?', '[', '$', '`'])
}

/// A shell line as its simple commands' words, split at `&&`, `||`, `;`, `|`, `&`, new lines and
/// the ends of a subshell or substitution, with comments, reserved words and leading `NAME=value`
/// assignments dropped, `sh -c '…'` unwrapped, and the command a wrapper such as `env` or `xargs`
/// runs added as one of its own. The flag is false when the line does more than its commands
/// show: a substitution, a subshell, an assignment that may change what runs, a quote that never
/// closes, or a redirect other than to /dev/null or another descriptor.
pub(super) fn split_command(line: &str) -> (Vec<Vec<String>>, bool) {
    let (segments, safe, writes) = split_writes(line);
    (segments, safe && writes.is_empty())
}

/// `split_command`, with the files the line writes by `>` and `>>` apart, rather than counting
/// against it: an Edit rule, not a command rule, answers for them.
pub(super) fn split_writes(line: &str) -> (Vec<Vec<String>>, bool, Vec<String>) {
    let mut writes: Vec<String> = Vec::new();
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
            // A comment runs to the end of its line. Read as words, a quote inside it would
            // swallow the lines after it, which the shell runs.
            '#' if !in_word => {
                while chars.peek().is_some_and(|c| *c != '\n') {
                    chars.next();
                }
            }
            '\'' => {
                in_word = true;
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '\'' {
                        closed = true;
                        break;
                    }
                    word.push(c);
                }
                safe &= closed;
            }
            '"' => {
                in_word = true;
                let mut closed = false;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => {
                            closed = true;
                            break;
                        }
                        // The shell drops a backslash and the new line after it.
                        '\\' if chars.peek() == Some(&'\n') => {
                            chars.next();
                        }
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
                safe &= closed;
            }
            '\\' if chars.peek() == Some(&'\n') => {
                chars.next();
            }
            '\\' => {
                in_word = true;
                word.extend(chars.next());
            }
            // What a subshell or substitution runs is a command of its own, so a rule for it
            // still sees it.
            '`' | '(' | ')' => {
                safe = false;
                end_word(&mut word, &mut in_word, &mut words);
                segments.push(std::mem::take(&mut words));
            }
            // `$(…)` substitutes, and `$'…'` spells its text in escapes this does not read.
            '$' if matches!(chars.peek(), Some('(' | '\'')) => {
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
                // A name the shell would expand or quote is not the file it names.
                if file.is_empty()
                    || file.contains(['\'', '"', '$', '`', '~', '*', '?', '[', '{', '(', '\\'])
                {
                    safe = false;
                } else if file != "/dev/null" {
                    writes.push(file);
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
    for words in &segments {
        safe &= add_command(words, &mut out);
    }
    (out, safe, writes)
}

/// Adds the command `words` spell to `out`, and the commands it runs in turn. False when it does
/// more than they show.
fn add_command(words: &[String], out: &mut Vec<Vec<String>>) -> bool {
    let reserved = words.iter().take_while(|w| is_reserved(w)).count();
    // `for f in a b`, `select f in a b` and `case x in` name a variable and list words; none of
    // them is a program.
    if words[..reserved]
        .last()
        .is_some_and(|w| matches!(w.as_str(), "for" | "select" | "case"))
    {
        return true;
    }
    let words = &words[reserved..];
    // An assignment can change which program a name runs (`PATH`, `LD_PRELOAD`), or have it run
    // other code (`GIT_SSH_COMMAND`, `NODE_OPTIONS`), so a rule for the command answers only for
    // the ones that only change its output.
    let assignments = words.iter().take_while(|w| is_assignment(w)).count();
    let mut safe = words[..assignments].iter().all(|w| {
        w.split_once('=')
            .is_some_and(|(name, _)| HARMLESS_VARIABLES.contains(&name))
    });
    let words = &words[assignments..];
    let Some(program) = words.first() else {
        return safe;
    };
    let program = program.rsplit('/').next().unwrap_or_default();
    if matches!(
        program,
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "mksh" | "ash" | "fish"
    ) {
        // `-c`, and the forms that carry other flags with it: `-lc`, `-ec`.
        let flag = words[1..]
            .iter()
            .position(|w| w.starts_with('-') && !w.starts_with("--") && w.contains('c'));
        if let Some((at, script)) = flag.and_then(|at| Some((at, words.get(at + 2)?))) {
            // Past a word that is not a flag, as in `bash -o pipefail -c …`, the `-c` may be the
            // shell's or a script's own, so the command stands as written as well.
            if !words[1..=at].iter().all(|w| w.starts_with('-')) {
                out.push(words.to_vec());
            }
            let (inner, inner_safe) = split_command(script);
            out.extend(inner);
            return safe && inner_safe;
        }
    }
    out.push(words.to_vec());
    if WRAPPERS.contains(&program) {
        // Past the wrapper's own flags and their values: `nice -n 5 rm`, `timeout 5s rm`.
        let mut own = own_words(program, &words[1..]);
        // `flock` names the file it locks before the command.
        if program == "flock" && own + 1 < words.len() {
            own += 1;
        }
        safe &= add_command(&words[1 + own..], out);
    }
    if program == "git"
        && let Some(plain) = git_without_options(words)
    {
        out.push(plain);
    }
    // `find … -exec rm {} ;` runs the command it names for each file it finds.
    if program == "find" {
        for (at, word) in words.iter().enumerate() {
            if matches!(word.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
                let run: Vec<String> = words[at + 1..]
                    .iter()
                    .take_while(|w| !matches!(w.as_str(), ";" | "+"))
                    .cloned()
                    .collect();
                safe &= add_command(&run, out);
            }
        }
    }
    safe
}

/// The flags that hand each interpreter its code on the command line, by the interpreter's name
/// without a version: `python3.12 -c`, `node --eval`. The shells are here with none: `sh -c` is
/// read as the commands it runs, and what is left is a shell fed its script on its input.
const INTERPRETERS: [(&str, &[&str]); 22] = [
    ("python", &["-c"]),
    ("pypy", &["-c"]),
    ("perl", &["-e", "-E"]),
    ("ruby", &["-e"]),
    ("node", &["-e", "--eval", "-p", "--print"]),
    ("nodejs", &["-e", "--eval", "-p", "--print"]),
    ("bun", &["-e", "--eval", "-p", "--print"]),
    ("deno", &["eval"]),
    ("php", &["-r"]),
    ("lua", &["-e"]),
    ("Rscript", &["-e"]),
    ("osascript", &["-e"]),
    ("pwsh", &["-c", "-Command", "-command", "-EncodedCommand"]),
    (
        "powershell",
        &["-c", "-Command", "-command", "-EncodedCommand"],
    ),
    ("sh", &[]),
    ("bash", &[]),
    ("zsh", &[]),
    ("dash", &[]),
    ("ksh", &[]),
    ("mksh", &[]),
    ("ash", &[]),
    ("fish", &[]),
];

/// The code a command hands an interpreter, which runs it in place of a program a rule could
/// name: what follows `python -c` or `node -e`, or all of `line` when the code comes on the
/// interpreter's input, from a here-document or a pipe. None for a command that is no
/// interpreter, or that runs a script file, whose contents no rule reads either way.
pub(super) fn handed_code(words: &[String], line: &str) -> Option<String> {
    let program = words.first()?.rsplit('/').next().unwrap_or_default();
    let name = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    let (_, flags) = INTERPRETERS.iter().find(|(known, _)| *known == name)?;
    let arguments = &words[1..];
    for (at, word) in arguments.iter().enumerate() {
        if flags.contains(&word.as_str()) {
            return Some(arguments[at + 1..].join(" "));
        }
        if let Some((flag, code)) = word.split_once('=')
            && flags.contains(&flag)
        {
            return Some(code.to_string());
        }
    }
    // With no script named, or `-` for one, the code is whatever reaches the input.
    let script = arguments
        .iter()
        .find(|word| !word.starts_with('-') || *word == "-");
    script
        .is_none_or(|word| word == "-")
        .then(|| line.to_string())
}

/// What code names when it runs a command, in the languages of `INTERPRETERS`: Python's
/// `os.system` and `subprocess`, Node's `child_process` and `exec`, Perl's and Ruby's `system`
/// and backticks, a shell by its name. Code that names one can run any program.
const RUNS_COMMANDS: [&str; 16] = [
    "system",
    "subprocess",
    "popen",
    "Popen",
    "exec",
    "execSync",
    "execFile",
    "spawn",
    "spawnSync",
    "child_process",
    "Command",
    "eval",
    "sh",
    "bash",
    "zsh",
    "shell",
];

/// Whether `text` has `word` as a whole word: `rm` in `os.system('rm -rf x')`, not in `os.remove`.
fn mentions(text: &str, word: &str) -> bool {
    let part_of_word = |c: char| c.is_alphanumeric() || c == '_';
    !word.is_empty()
        && text.match_indices(word).any(|(at, found)| {
            !text[..at].chars().next_back().is_some_and(part_of_word)
                && !text[at + found.len()..]
                    .chars()
                    .next()
                    .is_some_and(part_of_word)
        })
}

/// Whether code handed to an interpreter may run `program`: it names it, or it names a way of
/// running commands, by which it could run anything. Code that builds a name piece by piece is
/// not seen through; a rule reads what is written.
pub(super) fn may_run(code: &str, program: &str) -> bool {
    mentions(code, program)
        || code.contains('`')
        || RUNS_COMMANDS.iter().any(|way| mentions(code, way))
}

/// Whether an argument makes a command that reads by its name run a program or write a file:
/// `rg --pre`, `git diff --output`, `tree -o`. A rule that allows the command does not allow that.
pub(super) fn does_more(program: &str, arguments: &[String]) -> bool {
    let program = program.rsplit('/').next().unwrap_or(program);
    arguments.iter().any(|word| {
        let flag = word.split_once('=').map_or(word.as_str(), |(flag, _)| flag);
        match program {
            "rg" => matches!(flag, "--pre" | "--hostname-bin"),
            // git takes any unambiguous start of `--output`.
            "git" => flag.len() >= 5 && "--output".starts_with(flag),
            "tree" => flag == "-o",
            _ => false,
        }
    })
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}
