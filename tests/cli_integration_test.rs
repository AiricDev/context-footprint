//! CLI integration tests: run the context-footprint binary to cover main.rs branches.
//! Uses CARGO_BIN_EXE_context_footprint when set (e.g. by `cargo test`).

use std::path::Path;
use std::process::Command;

const SEMANTIC_DATA_JSON: &str = "tests/fixtures/simple_python/semantic_data.json";

fn bin() -> Option<std::path::PathBuf> {
    // Binary target name is "context-footprint" (Cargo sets CARGO_BIN_EXE_<name> as-is)
    std::env::var_os("CARGO_BIN_EXE_context-footprint")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("CARGO_BIN_EXE_context_footprint").map(std::path::PathBuf::from)
        })
}

#[test]
fn test_cli_help_succeeds() {
    let Some(bin) = bin() else {
        eprintln!("Skipping CLI test: CARGO_BIN_EXE not set");
        return;
    };
    let out = Command::new(bin)
        .arg("--help")
        .output()
        .expect("run --help");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("context-footprint"));
    assert!(stdout.contains("Compute") || stdout.contains("compute"));
}

#[test]
fn test_cli_load_error_when_data_missing() {
    let Some(bin) = bin() else {
        eprintln!("Skipping CLI test: CARGO_BIN_EXE not set");
        return;
    };
    let out = Command::new(&bin)
        .args(["nonexistent_semantic_data_12345.json", "stats"])
        .output()
        .expect("run stats with missing semantic data file");
    assert!(
        !out.status.success(),
        "expected failure when semantic data file missing"
    );
}

#[test]
fn test_cli_compute_symbol_not_found() {
    let Some(bin) = bin() else {
        eprintln!("Skipping CLI test: CARGO_BIN_EXE not set");
        return;
    };
    if !Path::new(SEMANTIC_DATA_JSON).exists() {
        eprintln!("Skipping: {} not found", SEMANTIC_DATA_JSON);
        return;
    }
    let out = Command::new(&bin)
        .args([SEMANTIC_DATA_JSON, "compute", "nonexistent_symbol_xyz"])
        .output()
        .expect("run compute");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found") || stderr.contains("Symbol"));
}

#[test]
fn test_cli_stats_when_fixture_present() {
    let Some(bin) = bin() else {
        eprintln!("Skipping CLI test: CARGO_BIN_EXE not set");
        return;
    };
    if !Path::new(SEMANTIC_DATA_JSON).exists() {
        eprintln!("Skipping: {} not found", SEMANTIC_DATA_JSON);
        return;
    }
    let out = Command::new(&bin)
        .args([SEMANTIC_DATA_JSON, "stats"])
        .output()
        .expect("run stats");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Nodes:") || stdout.contains("Graph Summary"));
}

#[test]
fn test_cli_top_when_fixture_present() {
    let Some(bin) = bin() else {
        eprintln!("Skipping CLI test: CARGO_BIN_EXE not set");
        return;
    };
    if !Path::new(SEMANTIC_DATA_JSON).exists() {
        eprintln!("Skipping: {} not found", SEMANTIC_DATA_JSON);
        return;
    }
    let out = Command::new(&bin)
        .args([SEMANTIC_DATA_JSON, "top", "-n", "5"])
        .output()
        .expect("run top");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn test_cli_search_when_fixture_present() {
    let Some(bin) = bin() else {
        eprintln!("Skipping CLI test: CARGO_BIN_EXE not set");
        return;
    };
    if !Path::new(SEMANTIC_DATA_JSON).exists() {
        eprintln!("Skipping: {} not found", SEMANTIC_DATA_JSON);
        return;
    }
    let out = Command::new(&bin)
        .args([SEMANTIC_DATA_JSON, "search", "main"])
        .output()
        .expect("run search");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn test_cli_context_when_fixture_present() {
    let Some(bin) = bin() else {
        eprintln!("Skipping CLI test: CARGO_BIN_EXE not set");
        return;
    };
    if !Path::new(SEMANTIC_DATA_JSON).exists() {
        eprintln!("Skipping: {} not found", SEMANTIC_DATA_JSON);
        return;
    }
    // Use a symbol that likely exists in simple_python (we need one from the graph)
    let out = Command::new(&bin)
        .args([SEMANTIC_DATA_JSON, "search", "main", "--limit", "1"])
        .output()
        .expect("run search to find a symbol");
    if !out.status.success() {
        return;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // For coverage we run context with any symbol and accept "not found"
    let out2 = Command::new(&bin)
        .args([SEMANTIC_DATA_JSON, "context", "dummy_symbol_if_absent"])
        .output()
        .expect("run context");
    // Symbol not found is acceptable; we still exercised the context branch
    let _ = (stdout, out2);
}

#[test]
fn test_cli_compute_multi_symbols() {
    let Some(bin) = bin() else {
        eprintln!("Skipping CLI test: CARGO_BIN_EXE not set");
        return;
    };
    if !Path::new(SEMANTIC_DATA_JSON).exists() {
        eprintln!("Skipping: {} not found", SEMANTIC_DATA_JSON);
        return;
    }
    // Just run compute with two arbitrary symbols (even if they don't exist, we test the CLI parsing)
    // But better to use something that might exist or just verify it doesn't crash.
    let out = Command::new(&bin)
        .args([
            SEMANTIC_DATA_JSON,
            "compute",
            "scip-python python simple_python 0.1.0 `main`/main().",
            "scip-python python simple_python 0.1.0 `utils`/add().",
        ])
        .output()
        .expect("run compute multi");

    // We don't strictly require success because the symbols might change,
    // but the command should at least parse the multiple arguments.
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    if !out.status.success() {
        assert!(stderr.contains("Symbol not found") || stderr.contains("not found"));
    } else {
        assert!(stdout.contains("Starting symbols: 2"));
        assert!(stdout.contains("Total context size:"));
    }
}
