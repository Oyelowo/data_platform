use yelang_interner::Interner;
use yelang_lexer::FileId;

/// Strict AST parse for a full program.
///
/// This is the canonical non-legacy strict parser entrypoint used by the compiler/IDE.
pub fn parse_program_strict_with_file_id(
    source: &str,
    interner: &mut Interner,
    file_id: FileId,
) -> Result<crate::Program, String> {
    let mut stream = crate::TokenKind::tokenize_with_file_id(source, interner, file_id)
        .map_err(|e| e.to_string())?;

    stream.parse::<crate::Program>().map_err(|e| e.to_string())
}
