use crate::{ast::Program, error::SimplyError};

pub(crate) fn analyze(
    analyzer: &mut super::SemanticAnalyzer,
    program: &Program,
) -> Result<(), SimplyError> {
    analyzer.ensure_global_function_scope();
    analyzer.module_identity = "memory://root".into();
    analyzer.module_return_allowed = false;
    analyzer.module_return_type = None;
    analyzer.imported_types.clear();
    analyzer.module_export_names.clear();
    analyzer.collect_structs_and_messages(&program.statements)?;
    analyzer.collect_functions(&program.statements)?;
    analyzer.analyze_statements(&program.statements)
}
