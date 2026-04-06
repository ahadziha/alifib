use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run_alifib(example_path: &Path) -> (i32, String, String) {
    let exe = std::env::var("CARGO_BIN_EXE_alifib").unwrap_or_else(|_| {
        let root = repo_root();
        root.join("target")
            .join("debug")
            .join("alifib")
            .to_string_lossy()
            .to_string()
    });

    let output = Command::new(exe)
        .arg(example_path)
        .output()
        .expect("failed to run alifib");

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stdout, stderr)
}

fn sort_csv(csv: &str) -> String {
    let mut parts: Vec<String> = csv
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    parts.sort();
    parts.join(", ")
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();

    while let Some(ch) = it.next() {
        if ch == '\u{1b}' {
            if it.peek().is_some_and(|c| *c == '[') {
                it.next();
                while let Some(c) = it.next() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(ch);
    }

    out
}

fn drop_advice_blocks(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        if lines[i].starts_with("Advice:") {
            i += 1;
            while i < lines.len() {
                if lines[i].trim().is_empty() {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        out.push(lines[i].clone());
        i += 1;
    }

    out
}

fn canonicalize_output(raw: &str) -> String {
    let mut lines: Vec<String> = strip_ansi(raw)
        .replace("\r\n", "\n")
        .lines()
        .map(|l| l.to_string())
        .collect();

    lines = drop_advice_blocks(lines);

    // Normalize list ordering within Diagrams/Maps lines (but NOT module paths yet —
    // we sort module sections by their raw path first for a stable order).
    for line in &mut lines {
        if let Some(rest) = line.strip_prefix("  Diagrams: ") {
            *line = format!("  Diagrams: {}", sort_csv(rest));
        } else if let Some(rest) = line.strip_prefix("  Maps: ") {
            *line = format!("  Maps: {}", sort_csv(rest));
        }
    }

    // Split into module sections, sort type blocks within each, sort modules by
    // raw path for determinism, then normalize paths to <MODULE>.
    if lines.is_empty() {
        return String::new();
    }

    let mut out: Vec<String> = Vec::new();
    out.push(lines[0].clone()); // summary line: "N cells, M types, K modules"
    out.push(String::new());

    // Collect all module sections: (raw_header, sorted_type_blocks)
    let mut modules: Vec<(String, Vec<Vec<String>>)> = Vec::new();

    let mut i = 1usize;
    while i < lines.len() {
        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }
        if i >= lines.len() {
            break;
        }

        if !lines[i].starts_with("* Module ") {
            out.push(lines[i].clone());
            i += 1;
            continue;
        }

        let module_line = lines[i].clone();
        i += 1;

        let mut blocks: Vec<Vec<String>> = Vec::new();
        let mut current: Vec<String> = Vec::new();

        while i < lines.len() && !lines[i].starts_with("* Module ") {
            let line = lines[i].clone();
            i += 1;
            if line.trim().is_empty() {
                if !current.is_empty() {
                    blocks.push(current);
                    current = Vec::new();
                }
            } else {
                current.push(line);
            }
        }
        if !current.is_empty() {
            blocks.push(current);
        }

        blocks.sort_by_key(|b| b.first().cloned().unwrap_or_default());
        modules.push((module_line, blocks));
    }

    // Sort by raw path so module order is independent of insertion order.
    modules.sort_by_key(|(header, _)| header.clone());

    for (_, blocks) in modules {
        out.push("* Module <MODULE>".to_string());
        for block in blocks {
            out.push(block.join("\n"));
            out.push(String::new());
        }
    }

    while out.last().is_some_and(|s| s.is_empty()) {
        out.pop();
    }

    out.join("\n") + "\n"
}

fn assert_example_matches_golden(example_name: &str) {
    let root = repo_root();
    let example = root.join("examples").join(example_name);
    let golden = root
        .join("tests")
        .join("golden")
        .join(format!("{}.out", example_name));

    let expected_raw = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("failed to read golden file {}: {}", golden.display(), e));

    let (code, stdout, stderr) = run_alifib(&example);

    let actual_raw = if code == 0 {
        stdout
    } else {
        format!("{}{}", stdout, stderr)
    };

    let expected = canonicalize_output(&expected_raw);
    let actual = canonicalize_output(&actual_raw);

    assert_eq!(
        expected, actual,
        "golden mismatch for {} (exit code {})",
        example_name, code
    );
}

#[test]
fn golden_examples() {
    let examples = [
        "Category.ali",
        "Empty.ali",
        "Empty2.ali",
        "Frobenius.ali",
        "Hole.ali",
        "Magma.ali",
        "Semigroup.ali",
        "Theory.ali",
        "Total.ali",
        "Tutorial.ali",
        "YangBaxter.ali",
    ];

    for example in examples {
        assert_example_matches_golden(example);
    }
}
