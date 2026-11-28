//! A sheet quilt's CSV read into cells, with its formulas worked out.
//!
//! A cell that starts with `=` is a formula: numbers, cells such as `B2`, the operators
//! `+ - * / ^`, brackets, and `SUM`, `AVERAGE`, `MIN`, `MAX`, `COUNT`, `ROUND` and `ABS` over
//! cells and ranges such as `B2:B9`. A formula that cannot be worked out shows `#ERROR`, and one
//! that reaches itself `#CYCLE`.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

/// How deep formulas may reach through other formulas before it counts as a cycle.
const MAX_DEPTH: usize = 64;

/// A cell as shown: its text, whether it is a number, and the formula it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub text: String,
    pub number: bool,
    pub formula: Option<String>,
}

/// The rows of `csv`, each a list of fields. Fields in double quotes may hold commas, line
/// breaks and doubled quotes.
#[must_use]
pub fn parse_csv(csv: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = csv.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            ('"', true) => quoted = false,
            ('"', false) if field.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (c, _) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// The column letters of column `index` from 0: A, B, … Z, AA.
#[must_use]
pub fn column_name(index: usize) -> String {
    let mut name = Vec::new();
    let mut n = index + 1;
    while n > 0 {
        let rem = (n - 1) % 26;
        name.push(b'A' + u8::try_from(rem).unwrap_or(0));
        n = (n - 1) / 26;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

/// The rows of `csv` as cells, with each formula's result.
#[must_use]
pub fn cells(csv: &str) -> Vec<Vec<Cell>> {
    let raw = parse_csv(csv);
    let mut sheet = Sheet {
        raw: &raw,
        done: HashMap::new(),
    };
    (0..raw.len())
        .map(|r| {
            (0..raw[r].len())
                .map(|c| {
                    let field = raw[r][c].trim();
                    match field.strip_prefix('=') {
                        Some(formula) => {
                            let value = sheet.value(r, c, 0);
                            Cell {
                                number: value.is_ok(),
                                text: match value {
                                    Ok(n) => format_number(n),
                                    Err(e) => e.to_owned(),
                                },
                                formula: Some(format!("={formula}")),
                            }
                        }
                        None => Cell {
                            number: number(field).is_some(),
                            text: raw[r][c].clone(),
                            formula: None,
                        },
                    }
                })
                .collect()
        })
        .collect()
}

/// A number as a sheet shows it: whole numbers without a point, others to at most six places.
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        let text = format!("{n:.6}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// The number a plain cell holds, allowing a currency sign, thousands commas and a percent.
fn number(text: &str) -> Option<f64> {
    let t = text.trim();
    let (t, percent) = match t.strip_suffix('%') {
        Some(t) => (t, true),
        None => (t, false),
    };
    let cleaned: String = t
        .trim_start_matches(['$', '€', '£'])
        .chars()
        .filter(|c| *c != ',')
        .collect();
    let n: f64 = cleaned.parse().ok()?;
    Some(if percent { n / 100.0 } else { n })
}

/// The sheet's fields and the formulas worked out so far.
struct Sheet<'a> {
    raw: &'a [Vec<String>],
    done: HashMap<(usize, usize), Result<f64, &'static str>>,
}

impl Sheet<'_> {
    /// The number in the cell at row `r`, column `c`: an empty cell is 0.
    fn value(&mut self, r: usize, c: usize, depth: usize) -> Result<f64, &'static str> {
        if let Some(done) = self.done.get(&(r, c)) {
            return *done;
        }
        if depth > MAX_DEPTH {
            return Err("#CYCLE");
        }
        let field = self
            .raw
            .get(r)
            .and_then(|row| row.get(c))
            .map_or("", |f| f.trim());
        let value = match field.strip_prefix('=') {
            Some(formula) => {
                let tokens = tokenize(formula).ok_or("#ERROR")?;
                let mut parser = Parser {
                    tokens: &tokens,
                    at: 0,
                    sheet: self,
                    depth: depth + 1,
                };
                let v = parser.expression()?;
                if parser.at != tokens.len() {
                    return Err("#ERROR");
                }
                v
            }
            None if field.is_empty() => 0.0,
            None => number(field).ok_or("#VALUE")?,
        };
        if depth == 0 || field.starts_with('=') {
            self.done.insert((r, c), Ok(value));
        }
        Ok(value)
    }

    /// The numbers in a range, skipping cells that hold text.
    fn range(
        &mut self,
        from: (usize, usize),
        to: (usize, usize),
        depth: usize,
    ) -> Result<Vec<f64>, &'static str> {
        let (r0, r1) = (from.0.min(to.0), from.0.max(to.0));
        let (c0, c1) = (from.1.min(to.1), from.1.max(to.1));
        let mut out = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                let field = self
                    .raw
                    .get(r)
                    .and_then(|row| row.get(c))
                    .map_or("", |f| f.trim());
                if field.is_empty() || (!field.starts_with('=') && number(field).is_none()) {
                    continue;
                }
                out.push(self.value(r, c, depth)?);
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Cell(usize, usize),
    Name(String),
    Op(char),
}

/// A cell reference such as `B12` or `$B$12` as its row and column from 0.
fn cell_ref(text: &str) -> Option<(usize, usize)> {
    let text = text.replace('$', "");
    let split = text.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = text.split_at(split);
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let col = letters
        .to_ascii_uppercase()
        .bytes()
        .fold(0usize, |n, b| n * 26 + usize::from(b - b'A' + 1));
    let row: usize = digits.parse().ok()?;
    Some((row.checked_sub(1)?, col - 1))
}

fn tokenize(formula: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = formula.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            tokens.push(Token::Number(text.parse().ok()?));
        } else if c.is_ascii_alphabetic() || c == '$' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '$') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            tokens.push(match cell_ref(&word) {
                Some((r, c)) => Token::Cell(r, c),
                None => Token::Name(word.to_ascii_uppercase()),
            });
        } else if "+-*/^(),:".contains(c) {
            tokens.push(Token::Op(c));
            i += 1;
        } else {
            return None;
        }
    }
    Some(tokens)
}

/// Works out a formula's tokens by precedence: `+ -`, then `* /`, then `^`, then a single value.
struct Parser<'t, 's, 'a> {
    tokens: &'t [Token],
    at: usize,
    sheet: &'s mut Sheet<'a>,
    depth: usize,
}

impl Parser<'_, '_, '_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, op: char) -> bool {
        if self.peek() == Some(&Token::Op(op)) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expression(&mut self) -> Result<f64, &'static str> {
        let mut v = self.term()?;
        loop {
            if self.eat('+') {
                v += self.term()?;
            } else if self.eat('-') {
                v -= self.term()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn term(&mut self) -> Result<f64, &'static str> {
        let mut v = self.power()?;
        loop {
            if self.eat('*') {
                v *= self.power()?;
            } else if self.eat('/') {
                let d = self.power()?;
                if d == 0.0 {
                    return Err("#DIV/0");
                }
                v /= d;
            } else {
                return Ok(v);
            }
        }
    }

    fn power(&mut self) -> Result<f64, &'static str> {
        let base = self.unary()?;
        if self.eat('^') {
            return Ok(base.powf(self.power()?));
        }
        Ok(base)
    }

    fn unary(&mut self) -> Result<f64, &'static str> {
        if self.eat('-') {
            return Ok(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<f64, &'static str> {
        let token = self.peek().cloned().ok_or("#ERROR")?;
        self.at += 1;
        match token {
            Token::Number(n) => Ok(n),
            Token::Cell(r, c) => self.sheet.value(r, c, self.depth),
            Token::Op('(') => {
                let v = self.expression()?;
                if self.eat(')') { Ok(v) } else { Err("#ERROR") }
            }
            Token::Name(name) => {
                if !self.eat('(') {
                    return Err("#NAME");
                }
                let args = self.arguments()?;
                apply(&name, &args)
            }
            Token::Op(_) => Err("#ERROR"),
        }
    }

    /// A function's arguments up to its closing bracket, each range spread into its numbers.
    fn arguments(&mut self) -> Result<Vec<f64>, &'static str> {
        let mut out = Vec::new();
        if self.eat(')') {
            return Ok(out);
        }
        loop {
            if let (Some(Token::Cell(r0, c0)), Some(Token::Op(':')), Some(Token::Cell(r1, c1))) = (
                self.tokens.get(self.at).cloned(),
                self.tokens.get(self.at + 1).cloned(),
                self.tokens.get(self.at + 2).cloned(),
            ) {
                self.at += 3;
                out.extend(self.sheet.range((r0, c0), (r1, c1), self.depth)?);
            } else {
                out.push(self.expression()?);
            }
            if self.eat(')') {
                return Ok(out);
            }
            if !self.eat(',') {
                return Err("#ERROR");
            }
        }
    }
}

/// The function `name` over `args`.
#[expect(
    clippy::cast_precision_loss,
    reason = "a count of cells is far below where f64 loses whole numbers"
)]
fn apply(name: &str, args: &[f64]) -> Result<f64, &'static str> {
    let sum: f64 = args.iter().sum();
    match name {
        "SUM" => Ok(sum),
        "AVERAGE" | "AVG" if !args.is_empty() => Ok(sum / args.len() as f64),
        "AVERAGE" | "AVG" => Err("#DIV/0"),
        "MIN" | "MAX" if args.is_empty() => Ok(0.0),
        "MIN" => Ok(args.iter().copied().fold(f64::INFINITY, f64::min)),
        "MAX" => Ok(args.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "COUNT" => Ok(args.len() as f64),
        "ABS" => args.first().map(|n| n.abs()).ok_or("#ERROR"),
        "ROUND" => {
            let n = args.first().ok_or("#ERROR")?;
            let places = args.get(1).copied().unwrap_or(0.0);
            let scale = 10f64.powf(places.trunc());
            Ok((n * scale).round() / scale)
        }
        _ => Err("#NAME"),
    }
}
