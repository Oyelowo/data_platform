use std::fs;
use std::path::{Path, PathBuf};

use yelang_ast::parse_program_strict_with_file_id;
use yelang_interner::Interner;
use yelang_lexer::FileId;

fn collect_yed_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(root)
        .map_err(|err| format!("failed to read directory {}: {err}", root.display()))?;

    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let path = entry.path();

        if path.is_dir() {
            collect_yed_files(&path, out)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) == Some("yed") {
            out.push(path);
        }
    }

    Ok(())
}

#[test]
#[ignore]
fn strict_parser_accepts_repo_yed_corpus() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lang_root = crate_root.join("..");
    let repo_root = crate_root.join("../../..");
    let mut files = Vec::new();

    collect_yed_files(&lang_root.join("stdlib"), &mut files).expect("collect stdlib .yed files");
    collect_yed_files(&repo_root.join("examples"), &mut files)
        .expect("collect examples .yed files");

    files.sort();
    assert!(
        !files.is_empty(),
        "expected at least one .yed file in corpus"
    );

    let mut interner = Interner::new();
    let mut failures = Vec::new();

    for file in files {
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", file.display()));

        if let Err(error) =
            parse_program_strict_with_file_id(&source, &mut interner, FileId::default())
        {
            failures.push(format!("{} => {}", file.display(), error));
        }
    }

    assert!(
        failures.is_empty(),
        "strict parser failed for corpus files:\n{}",
        failures.join("\n")
    );
}
