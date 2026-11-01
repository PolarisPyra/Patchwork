//! Reading a shell command line the way the shell does, as far as a hook needs it: words with their
//! quotes and escapes removed, the operators between commands, comments, heredocs, and the scripts
//! that `$(…)` and backticks run.

/// A piece of a command line.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A word, with its quotes and escapes removed. `subs` are the scripts its `$(…)` and
    /// backticks run, which the shell runs before the command.
    Word {
        text: String,
        quoted: bool,
        subs: Vec<String>,
    },
    /// A redirection, with the body of its heredoc when it has one.
    Redirect { heredoc: Option<String> },
    /// `;`, `&`, `&&`, `||`, `|`, `(`, `)`, or a newline, which is `;`.
    Op(&'static str),
}

/// One simple command: its words, and the bodies of the heredocs it reads.
#[derive(Debug, Default)]
pub struct Command<'a> {
    pub words: Vec<&'a Token>,
    pub heredocs: Vec<&'a str>,
}

impl Command<'_> {
    /// The text of each word.
    pub fn texts(&self) -> Vec<&str> {
        self.words
            .iter()
            .filter_map(|token| match token {
                Token::Word { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// Words the shell reads as syntax when they start a command.
const RESERVED: [&str; 17] = [
    "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "for", "case", "esac", "{",
    "}", "!", "select", "function",
];

/// Whether `token` is the reserved word `word` where a command starts.
pub fn is_keyword(token: &Token, word: &str) -> bool {
    matches!(token, Token::Word { text, quoted: false, .. } if text == word)
}

/// The simple commands in `tokens`, in order, with the reserved words that open them left out.
pub fn commands(tokens: &[Token]) -> Vec<Command<'_>> {
    let mut all = Vec::new();
    let mut current = Command::default();
    for token in tokens {
        match token {
            Token::Op(_) => {
                if !current.words.is_empty() || !current.heredocs.is_empty() {
                    all.push(std::mem::take(&mut current));
                }
            }
            Token::Redirect { heredoc } => {
                if let Some(body) = heredoc {
                    current.heredocs.push(body);
                }
            }
            Token::Word { .. }
                if current.words.is_empty()
                    && RESERVED.iter().any(|word| is_keyword(token, word)) => {}
            Token::Word { .. } => current.words.push(token),
        }
    }
    if !current.words.is_empty() || !current.heredocs.is_empty() {
        all.push(current);
    }
    all
}

/// The tokens of `line`.
pub fn tokens(line: &str) -> Vec<Token> {
    let mut lexer = Lexer {
        chars: line.chars().collect(),
        at: 0,
        tokens: Vec::new(),
        heredocs: Vec::new(),
    };
    lexer.run();
    lexer.tokens
}

/// A heredoc whose body starts after the line it is named on: the redirection it belongs to, its
/// end word, and whether the body's leading tabs are dropped (`<<-`).
struct PendingHeredoc {
    token: usize,
    end: String,
    strip_tabs: bool,
}

struct Lexer {
    chars: Vec<char>,
    at: usize,
    tokens: Vec<Token>,
    heredocs: Vec<PendingHeredoc>,
}

impl Lexer {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    fn starts(&self, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(i, c)| self.peek(i) == Some(c))
    }

    fn run(&mut self) {
        while let Some(c) = self.peek(0) {
            match c {
                ' ' | '\t' => self.at += 1,
                '\n' => {
                    self.at += 1;
                    self.tokens.push(Token::Op(";"));
                    self.read_heredocs();
                }
                '#' => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.at += 1;
                    }
                }
                '\\' if self.peek(1) == Some('\n') => self.at += 2,
                '<' | '>' => self.redirect(),
                '&' if matches!(self.peek(1), Some('>')) => self.redirect(),
                ';' | '&' | '|' | '(' | ')' => self.operator(),
                _ => {
                    let start = self.at;
                    let word = self.word();
                    // A file descriptor right before a redirection belongs to it: `2>&1`.
                    let fd = self.chars[start..self.at].iter().all(char::is_ascii_digit);
                    if fd && matches!(self.peek(0), Some('<' | '>')) {
                        self.redirect();
                    } else {
                        self.tokens.push(word);
                    }
                }
            }
        }
    }

    fn operator(&mut self) {
        for (text, op) in [
            ("&&", "&&"),
            ("||", "||"),
            (";;", ";"),
            ("|&", "|"),
            (";", ";"),
            ("&", "&"),
            ("|", "|"),
            ("(", "("),
            (")", ")"),
        ] {
            if self.starts(text) {
                self.at += text.len();
                self.tokens.push(Token::Op(op));
                return;
            }
        }
    }

    /// A redirection and its target, which is not a command and so is dropped. A heredoc's body is
    /// read once its line ends.
    fn redirect(&mut self) {
        if self.starts("<<<") {
            self.at += 3;
            self.skip_blanks();
            self.word();
            self.tokens.push(Token::Redirect { heredoc: None });
            return;
        }
        if self.starts("<<") {
            self.at += 2;
            let strip_tabs = self.peek(0) == Some('-');
            if strip_tabs {
                self.at += 1;
            }
            self.skip_blanks();
            let end = match self.word() {
                Token::Word { text, .. } => text,
                _ => String::new(),
            };
            self.heredocs.push(PendingHeredoc {
                token: self.tokens.len(),
                end,
                strip_tabs,
            });
            self.tokens.push(Token::Redirect {
                heredoc: Some(String::new()),
            });
            return;
        }
        while matches!(self.peek(0), Some('<' | '>' | '&' | '|')) {
            self.at += 1;
        }
        self.skip_blanks();
        if self.peek(0) == Some('-') {
            self.at += 1;
        } else {
            self.word();
        }
        self.tokens.push(Token::Redirect { heredoc: None });
    }

    fn skip_blanks(&mut self) {
        while matches!(self.peek(0), Some(' ' | '\t')) {
            self.at += 1;
        }
    }

    /// The bodies of the heredocs named on the line that just ended.
    fn read_heredocs(&mut self) {
        for pending in std::mem::take(&mut self.heredocs) {
            let mut body = String::new();
            while self.peek(0).is_some() {
                let start = self.at;
                while self.peek(0).is_some_and(|c| c != '\n') {
                    self.at += 1;
                }
                let line: String = self.chars[start..self.at].iter().collect();
                if self.peek(0) == Some('\n') {
                    self.at += 1;
                }
                let compared = if pending.strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if compared == pending.end {
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            if let Some(Token::Redirect { heredoc }) = self.tokens.get_mut(pending.token) {
                *heredoc = Some(body);
            }
        }
    }

    /// One word, up to the next blank or operator.
    fn word(&mut self) -> Token {
        let mut text = String::new();
        let mut quoted = false;
        let mut subs = Vec::new();
        while let Some(c) = self.peek(0) {
            match c {
                ' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>' => break,
                '\'' => {
                    quoted = true;
                    self.at += 1;
                    while let Some(c) = self.peek(0) {
                        self.at += 1;
                        if c == '\'' {
                            break;
                        }
                        text.push(c);
                    }
                }
                '"' => {
                    quoted = true;
                    self.at += 1;
                    while let Some(c) = self.peek(0) {
                        match c {
                            '"' => {
                                self.at += 1;
                                break;
                            }
                            '\\' if matches!(self.peek(1), Some('"' | '\\' | '$' | '`')) => {
                                text.extend(self.peek(1));
                                self.at += 2;
                            }
                            '\\' if self.peek(1) == Some('\n') => self.at += 2,
                            '$' if self.starts("$((") => self.arithmetic(&mut text),
                            '$' if self.peek(1) == Some('(') => {
                                let script = self.substitution();
                                text.push_str(&format!("$({script})"));
                                subs.push(script);
                            }
                            '`' => {
                                let script = self.backtick();
                                text.push_str(&format!("`{script}`"));
                                subs.push(script);
                            }
                            _ => {
                                text.push(c);
                                self.at += 1;
                            }
                        }
                    }
                }
                '\\' => {
                    if let Some(next) = self.peek(1)
                        && next != '\n'
                    {
                        text.push(next);
                    }
                    self.at += 2;
                }
                '$' if self.starts("$((") => self.arithmetic(&mut text),
                '$' if self.peek(1) == Some('(') => {
                    let script = self.substitution();
                    text.push_str(&format!("$({script})"));
                    subs.push(script);
                }
                '$' if self.peek(1) == Some('{') => {
                    while let Some(c) = self.peek(0) {
                        text.push(c);
                        self.at += 1;
                        if c == '}' {
                            break;
                        }
                    }
                }
                '`' => {
                    let script = self.backtick();
                    text.push_str(&format!("`{script}`"));
                    subs.push(script);
                }
                _ => {
                    text.push(c);
                    self.at += 1;
                }
            }
        }
        Token::Word { text, quoted, subs }
    }

    /// `$((…))`, which only does arithmetic, kept as text.
    fn arithmetic(&mut self, text: &mut String) {
        let mut depth = 0usize;
        while let Some(c) = self.peek(0) {
            text.push(c);
            self.at += 1;
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    /// The script of a `$(…)` starting here, with its quotes left in for the script to read.
    fn substitution(&mut self) -> String {
        self.at += 2;
        let mut script = String::new();
        let mut depth = 1usize;
        while let Some(c) = self.peek(0) {
            self.at += 1;
            match c {
                '\'' => {
                    script.push(c);
                    while let Some(c) = self.peek(0) {
                        self.at += 1;
                        script.push(c);
                        if c == '\'' {
                            break;
                        }
                    }
                }
                '"' => {
                    script.push(c);
                    while let Some(c) = self.peek(0) {
                        self.at += 1;
                        script.push(c);
                        if c == '\\' {
                            script.extend(self.peek(0));
                            self.at += 1;
                        } else if c == '"' {
                            break;
                        }
                    }
                }
                '\\' => {
                    script.push(c);
                    script.extend(self.peek(0));
                    self.at += 1;
                }
                '(' => {
                    depth += 1;
                    script.push(c);
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    script.push(c);
                }
                _ => script.push(c),
            }
        }
        script
    }

    /// The script between backticks starting here.
    fn backtick(&mut self) -> String {
        self.at += 1;
        let mut script = String::new();
        while let Some(c) = self.peek(0) {
            self.at += 1;
            match c {
                '`' => break,
                '\\' if self.peek(0) == Some('`') => {
                    script.push('`');
                    self.at += 1;
                }
                _ => script.push(c),
            }
        }
        script
    }
}
