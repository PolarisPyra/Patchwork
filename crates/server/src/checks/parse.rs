//! Reading a check command's output: what failed, and how many tests it says ran.
//!
//! Every runner prints failing tests its own way, so this knows the shapes of the common ones —
//! cargo's `failures:` list, pytest's `FAILED` lines, Node's, Jest's and Vitest's `✖`/`FAIL`
//! lines, Go's `--- FAIL:` — and falls back to the last lines that look like an error. It names
//! what failed and, when the output says why on the next line, keeps that too; the whole output is
//! kept beside it, so a person can see the rest.

use std::collections::HashSet;

use crate::threads::CheckFailure;

/// Most failures read out of one run.
const MOST: usize = 40;
/// How long a failure's name may be, in characters.
const LONGEST: usize = 200;
/// How far below a failure's line its reason may be and still be its own (Node prints one line
/// under it, others a few).
const REASON_WITHIN: usize = 3;

/// The failures `output` names, in the order it names them, each once.
pub fn failures(output: &str) -> Vec<CheckFailure> {
    let lines: Vec<&str> = output.lines().map(str::trim_end).collect();
    let mut found: Vec<CheckFailure> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // The line a failure was named on, while no reason has been read under it yet.
    let mut named_at: Option<usize> = None;
    let mut i = 0;
    while i < lines.len() {
        // Whether this line named a failure, whether or not it had been named before: the reason
        // under a repeated name belongs to it all the same.
        let mut names_failure = false;
        let line = lines[i];
        let trimmed = line.trim_start();
        let indented = line.len() != trimmed.len();

        if trimmed == "failures:" && !indented {
            // cargo: the `failures:` list, then the bodies under `---- name stdout ----`.
            let mut j = i + 1;
            while j < lines.len() {
                let candidate = lines[j].trim();
                if candidate.is_empty() || candidate.starts_with("test result:") {
                    break;
                }
                let name = candidate
                    .strip_prefix("---- ")
                    .and_then(|rest| rest.strip_suffix(" ----"))
                    .unwrap_or(candidate);
                push(&mut found, &mut seen, name);
                names_failure = true;
                j += 1;
            }
        } else if let Some(rest) = trimmed.strip_prefix("FAILED ") {
            // pytest: `FAILED tests/test_x.py::test_y - assert 1 == 2`.
            match rest.split_once(" - ") {
                Some((name, why)) => {
                    push_with(&mut found, &mut seen, name, Some(why.to_string()));
                }
                None => {
                    push(&mut found, &mut seen, rest);
                }
            }
            names_failure = true;
        } else if let Some(rest) = ["FAIL ", "✗ ", "× ", "✖ "]
            .iter()
            .find_map(|prefix| trimmed.strip_prefix(prefix))
        {
            // Node's own runner, Jest and Vitest: `✖ adds up prices (0.4ms)`, `FAIL src/app.test.ts`.
            // `✖ failing tests:` is the heading over the list, not a failure.
            let rest = rest.trim();
            if !rest.ends_with(':') {
                push(&mut found, &mut seen, rest);
                names_failure = true;
            }
        } else if let Some(rest) = trimmed.strip_prefix("--- FAIL: ") {
            // Go: `--- FAIL: TestThing (0.00s)`.
            push(
                &mut found,
                &mut seen,
                rest.split_whitespace().next().unwrap_or(rest),
            );
            names_failure = true;
        } else if let Some(name) = trimmed
            .strip_prefix("test ")
            .and_then(|rest| rest.strip_suffix(" ... FAILED"))
        {
            // cargo's progress line for a failing test: `test math::subtracts ... FAILED`.
            push(&mut found, &mut seen, name);
            names_failure = true;
        } else if let Some(rest) = trimmed.strip_prefix("thread '")
            && let Some((name, _)) = rest.split_once("' panicked at ")
        {
            // Rust's panic line names the test whose thread it was; the line under it says why.
            let why = lines.get(i + 1).map(|l| cut(l));
            push_with(&mut found, &mut seen, name, why.clone());
            if let (Some(why), Some(failure)) = (why, found.iter_mut().find(|f| f.name == name))
                && failure.detail.is_empty()
            {
                failure.detail = why;
            }
        } else if looks_like_error(trimmed) && !summary(trimmed) {
            // The reason for the failure named just above it, when that one has none yet. Other
            // error lines are left to the fallback below, so a runner's own wrap-up does not read
            // as a second failure.
            let recent = named_at.is_some_and(|at| i - at <= REASON_WITHIN);
            if let Some(failure) = found.last_mut().filter(|f| recent && f.detail.is_empty()) {
                failure.detail = cut(trimmed);
                named_at = None;
            }
        }
        if names_failure {
            named_at = Some(i);
        }
        i += 1;
        if found.len() >= MOST {
            break;
        }
    }
    if found.is_empty() {
        // No runner named a failure, as with a type check or a plain script: the lines that read
        // as errors are all there is, in order.
        for line in &lines {
            let trimmed = line.trim();
            if looks_like_error(trimmed) && !summary(trimmed) {
                push(&mut found, &mut seen, trimmed);
            }
            if found.len() >= MOST {
                break;
            }
        }
    }
    found
}

/// How many tests the output says ran, or None when it does not say.
///
/// `Some(0)` is the case that matters: a command can exit zero having run nothing at all, as
/// `cargo test` does in a crate without tests and `node --test` in a folder without test files.
/// That is not evidence, so it is not a pass. A command that says nothing about tests, such as a
/// type check, is None: nobody asked it to run tests, so there is nothing missing.
pub fn tests_ran(output: &str) -> Option<u32> {
    let mut ran = 0;
    let mut said = false;
    for line in output.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("ℹ tests ") {
            // Node's own runner: `ℹ tests 3`.
            ran += number(rest).unwrap_or(0);
            said = true;
        } else if let Some(rest) = line.strip_prefix("test result: ") {
            // cargo, once per target (lib, bins, doctests): `test result: ok. 3 passed; 1 failed`.
            // Both ran; an ignored test did not.
            if let Some(passed) = count_in(rest, " passed") {
                ran += passed + count_in(rest, " failed").unwrap_or(0);
                said = true;
            }
        } else if let Some(rest) = line.strip_prefix("Tests:") {
            // Jest: `Tests:       1 failed, 2 passed, 3 total`.
            if let Some(total) = rest.split_whitespace().rev().skip(1).find_map(number)
                && rest.trim_end().ends_with("total")
            {
                ran += total;
                said = true;
            }
        } else if let Some(rest) = line.strip_prefix("Tests ") {
            // Vitest: `Tests  3 passed (3)`.
            if let Some(open) = rest.rfind('(')
                && let Some(total) = number(&rest[open + 1..])
            {
                ran += total;
                said = true;
            }
        } else if line.contains("no tests ran") || line.contains("collected 0 items") {
            // pytest.
            said = true;
        } else if line == "0 passing" {
            // Mocha.
            said = true;
        } else if line.starts_with("? ") && line.contains("[no test files]") {
            // Go, for a package without tests.
            said = true;
        } else if line.starts_with("ok ") && line.contains('\t') {
            // Go, for a package whose tests passed. It does not count them, so this says some ran
            // without inventing a number.
            ran += 1;
            said = true;
        }
    }
    said.then_some(ran)
}

/// The number at the start of `text`, as in `3` or `3 passed;`.
fn number(text: &str) -> Option<u32> {
    let digits: String = text
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The count before `label`, out of cargo's `ok. 0 passed; 0 failed; …`.
fn count_in(rest: &str, label: &str) -> Option<u32> {
    let start = rest.find(label)?;
    rest[..start].rsplit([' ', '.']).next().and_then(number)
}

/// Adds a failure, unless it was named already. Returns whether it was added.
fn push(found: &mut Vec<CheckFailure>, seen: &mut HashSet<String>, name: &str) -> bool {
    push_with(found, seen, name, None)
}

/// [`push`], with the reason the output gave for it. Returns whether it was added.
fn push_with(
    found: &mut Vec<CheckFailure>,
    seen: &mut HashSet<String>,
    name: &str,
    detail: Option<String>,
) -> bool {
    let name = cut(strip_duration(name));
    if name.is_empty() || !seen.insert(name.clone()) {
        return false;
    }
    found.push(CheckFailure {
        name,
        detail: detail.map(|d| cut(&d)).unwrap_or_default(),
    });
    true
}

/// A runner's wrap-up line, which repeats that something failed without saying what: cargo's
/// `test result: FAILED.` and `error: test failed`, npm's and pnpm's lifecycle errors.
fn summary(line: &str) -> bool {
    line.starts_with("test result:")
        || line.starts_with("error: test failed")
        || line.starts_with("npm error")
        || line.starts_with("npm ERR!")
        || line.contains("ELIFECYCLE")
}

/// A runner's timing after a test's name: `adds up prices (0.4ms)`.
fn strip_duration(name: &str) -> &str {
    let name = name.trim();
    let Some((head, tail)) = name.rsplit_once(" (") else {
        return name;
    };
    let tail = tail.strip_suffix(')').unwrap_or("");
    let timed = tail.ends_with("ms") || tail.ends_with('s');
    let digits = tail
        .trim_end_matches("ms")
        .trim_end_matches('s')
        .trim()
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.');
    if timed && digits && !head.trim().is_empty() {
        head.trim()
    } else {
        name
    }
}

/// Whether a line reads as an error: a named error, or a runner's own failure line.
fn looks_like_error(line: &str) -> bool {
    const WORDS: [&str; 9] = [
        "Error",
        "error:",
        "AssertionError",
        "assertion failed",
        "FAILED",
        "FAIL ",
        "make: ***",
        "*** [",
        "panicked at",
    ];
    WORDS.iter().any(|word| line.contains(word))
}

/// `text` cut to [`LONGEST`] characters, at a character boundary.
fn cut(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(LONGEST) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(output: &str) -> Vec<String> {
        failures(output).into_iter().map(|f| f.name).collect()
    }

    /// Node's own runner, captured from a real failing `npm test`.
    const NODE: &str = "\
✔ adds up prices (0.415386ms)
✖ counts a discount (0.463617ms)
✔ is zero when empty (0.131108ms)
ℹ tests 3
ℹ fail 1

✖ failing tests:

test at total.test.js:9:1
✖ counts a discount (0.463617ms)
  AssertionError [ERR_ASSERTION]: Expected values to be strictly equal:
  
  8 !== 9
      at TestContext.<anonymous> (file:///x/total.test.js:10:10)
";

    #[test]
    fn node_names_the_failing_test_once_without_its_time() {
        let found = failures(NODE);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].name, "counts a discount");
        assert!(
            found[0].detail.starts_with("AssertionError"),
            "{:?}",
            found[0].detail
        );
    }

    #[test]
    fn a_passing_node_run_names_nothing() {
        assert!(failures("✔ adds up prices (0.4ms)\nℹ pass 1\n").is_empty());
    }

    #[test]
    fn cargo_lists_each_failing_test_once() {
        let output = "\
running 3 tests
test math::adds works ... ok
test math::subtracts ... FAILED

failures:

---- math::subtracts stdout ----
thread 'math::subtracts' panicked at src/lib.rs:12:5:
assertion `left == right` failed

failures:
    math::subtracts

test result: FAILED. 2 passed; 1 failed
";
        let names = names(output);
        assert!(names.contains(&"math::subtracts".to_string()), "{names:?}");
        assert_eq!(
            names.iter().filter(|n| *n == "math::subtracts").count(),
            1,
            "named once: {names:?}"
        );
    }

    #[test]
    fn pytest_failures_keep_the_reason_apart_from_the_name() {
        let output = "\
FAILED tests/test_app.py::test_total - assert 3 == 4
1 failed, 2 passed in 0.10s
";
        let found = failures(output);
        assert_eq!(found[0].name, "tests/test_app.py::test_total");
        assert_eq!(found[0].detail, "assert 3 == 4");
    }

    #[test]
    fn jest_fail_lines_are_failures() {
        let output = "\
FAIL src/app.test.ts
  ● app › adds
    expect(received).toBe(expected)

× app › adds
";
        let names = names(output);
        assert!(names.contains(&"src/app.test.ts".to_string()), "{names:?}");
        assert!(names.contains(&"app › adds".to_string()), "{names:?}");
    }

    #[test]
    fn go_names_the_test_that_failed() {
        let output = "--- FAIL: TestAdd (0.00s)\n    add_test.go:9: got 3\n";
        assert_eq!(names(output), ["TestAdd"]);
    }

    #[test]
    fn output_that_names_nothing_falls_back_to_the_last_error_lines() {
        let names = names("building\nerror: could not compile `app`\nmore chatter\n");
        assert_eq!(names, ["error: could not compile `app`"]);
    }

    #[test]
    fn a_pass_names_nothing() {
        assert!(failures("test result: ok. 3 passed; 0 failed\n").is_empty());
    }

    #[test]
    fn node_says_how_many_tests_ran() {
        // The real run above had three tests and one of them failed.
        assert_eq!(tests_ran(NODE), Some(3));
        assert_eq!(tests_ran("ℹ tests 12\nℹ pass 12\n"), Some(12));
    }

    /// A folder with no test files: `node --test` exits zero and says it ran none.
    #[test]
    fn node_says_when_it_ran_nothing() {
        assert_eq!(
            tests_ran("ℹ tests 0\nℹ suites 0\nℹ pass 0\nℹ fail 0\n"),
            Some(0)
        );
    }

    #[test]
    fn cargo_counts_every_target_it_ran() {
        let output = "\
running 2 tests
test result: ok. 2 passed; 0 failed; 0 ignored

running 1 test
test result: ok. 1 passed; 0 failed
";
        assert_eq!(tests_ran(output), Some(3));
    }

    #[test]
    fn a_crate_without_tests_ran_none() {
        assert_eq!(
            tests_ran("running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored\n"),
            Some(0)
        );
    }

    #[test]
    fn pytest_says_how_many_passed_and_when_none_ran() {
        assert_eq!(
            tests_ran("3 passed in 0.12s\n"),
            None,
            "a count with no label"
        );
        assert_eq!(tests_ran("no tests ran in 0.01s\n"), Some(0));
        assert_eq!(tests_ran("collected 0 items\n"), Some(0));
    }

    #[test]
    fn jest_and_vitest_report_their_totals() {
        assert_eq!(
            tests_ran("Tests:       1 failed, 2 passed, 3 total\n"),
            Some(3)
        );
        assert_eq!(tests_ran("Tests  4 passed (4)\n"), Some(4));
    }

    #[test]
    fn go_reports_packages_and_says_when_one_has_no_tests() {
        assert_eq!(tests_ran("ok  \texample.com/app\t0.002s\n"), Some(1));
        assert_eq!(
            tests_ran("?   \texample.com/app\t[no test files]\n"),
            Some(0)
        );
    }

    /// A command that says nothing about tests, like a type check, is not a claim of zero.
    #[test]
    fn a_command_that_never_mentions_tests_says_nothing() {
        assert_eq!(
            tests_ran("Found 0 errors. Watching for file changes.\n"),
            None
        );
    }

    #[test]
    fn a_long_name_is_cut_at_a_character_boundary() {
        let long = "é".repeat(LONGEST + 10);
        let name = &failures(&format!("FAILED {long}"))[0].name;
        assert_eq!(name.chars().count(), LONGEST + 1, "cut, with an ellipsis");
        assert!(name.ends_with('…'));
    }
}
