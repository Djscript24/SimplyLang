use super::*;

impl SemanticAnalyzer {
    pub(super) fn analyze_file_program_kind(
        &mut self,
        path: &Path,
        program: &Program,
        is_root: bool,
    ) -> Result<(), SimplyError> {
        self.ensure_global_function_scope();
        let resolved = fs::canonicalize(path).map_err(|error| SimplyError::Runtime {
            span: Span::new(0, 0),
            code: DiagnosticCode::RuntimeImport,
            message: format!("could not open `{}`: {error}", path.display()),
        })?;
        self.module_identity = resolved.display().to_string();
        self.module_return_allowed = false;
        self.module_return_type = None;
        self.imported_types.clear();
        self.module_export_names.clear();
        let mut import_stack = vec![resolved.clone()];
        let mut import_cache = HashMap::<PathBuf, ModuleInterface>::new();
        self.analyze_imports(
            program,
            &resolved,
            &mut import_stack,
            &mut import_cache,
            is_root,
        )?;
        Ok(())
    }

    pub(super) fn analyze_imports(
        &mut self,
        program: &Program,
        current_file: &Path,
        import_stack: &mut Vec<PathBuf>,
        import_cache: &mut HashMap<PathBuf, ModuleInterface>,
        is_root: bool,
    ) -> Result<ModuleInterface, SimplyError> {
        self.module_return_allowed = !is_root;
        let mut declared_exports = Vec::new();
        let mut seen_exports = HashSet::new();
        for statement in &program.statements {
            let statement = match statement {
                Stmt::Located { statement, .. } => statement.as_ref(),
                statement => statement,
            };
            if let Stmt::Export { names } = statement {
                for name in names {
                    if !seen_exports.insert(name.clone()) {
                        return Err(self.error(
                            DiagnosticCode::DuplicateDeclaration,
                            format!("`{name}` is exported more than once"),
                        ));
                    }
                    declared_exports.push(name.clone());
                }
            }
        }
        self.module_export_names = seen_exports;
        for statement in &program.statements {
            let (statement_span, statement) = match statement {
                Stmt::Located { span, statement } => (Some(span.clone()), statement.as_ref()),
                statement => (None, statement),
            };
            self.current_span = statement_span.clone();
            if let Stmt::Import { path, .. } = statement {
                let resolved = resolve_import_path(Some(current_file), path).map_err(|error| {
                    error
                        .with_span(statement_span.clone().unwrap_or_else(|| Span::new(0, 0)))
                        .with_context(format!("imported by `{}`", current_file.display()))
                })?;
                if let Some(cycle_start) = import_stack.iter().position(|item| item == &resolved) {
                    let mut chain = import_stack[cycle_start..]
                        .iter()
                        .map(|item| item.display().to_string())
                        .collect::<Vec<_>>();
                    chain.push(resolved.display().to_string());
                    return Err(SimplyError::Runtime {
                        span: statement_span.unwrap_or_else(|| Span::new(0, 0)),
                        code: DiagnosticCode::RuntimeImport,
                        message: format!("cyclic import: {}", chain.join(" -> ")),
                    });
                }
                let imported_module = if let Some(interface) = import_cache.get(&resolved) {
                    interface.clone()
                } else {
                    let source =
                        fs::read_to_string(&resolved).map_err(|error| SimplyError::Runtime {
                            span: self.current_span.clone().unwrap_or_else(|| Span::new(0, 0)),
                            code: DiagnosticCode::RuntimeImport,
                            message: format!("could not read `{}`: {error}", resolved.display()),
                        })?;
                    let imported = (|| {
                        let tokens = crate::lexer::Lexer::new(&source).tokenize()?;
                        crate::parser::Parser::new(tokens).parse()
                    })()
                    .map_err(|error: SimplyError| {
                        error
                            .in_source(resolved.display().to_string(), source.clone())
                            .with_context(format!("in imported module `{}`", resolved.display()))
                    })?;
                    import_stack.push(resolved.clone());
                    let mut module_analyzer = SemanticAnalyzer::new();
                    module_analyzer.module_identity = resolved.display().to_string();
                    let result = module_analyzer
                        .analyze_imports(&imported, &resolved, import_stack, import_cache, false)
                        .map_err(|error| {
                            error
                                .in_source(resolved.display().to_string(), source.clone())
                                .with_context(format!(
                                    "in imported module `{}`",
                                    resolved.display()
                                ))
                        });
                    import_stack.pop();
                    let interface = result?;
                    import_cache.insert(resolved.clone(), interface.clone());
                    interface
                };
                if let Stmt::Import {
                    alias, exposing, ..
                } = statement
                {
                    if let Some(alias) = alias {
                        self.imported_types
                            .insert(alias.clone(), imported_module.return_type);
                    } else {
                        let mut local_names = HashSet::new();
                        for (exported, local) in exposing {
                            if !local_names.insert(local) {
                                return Err(self.error(
                                    DiagnosticCode::DuplicateDeclaration,
                                    format!("`{local}` is imported more than once"),
                                ));
                            }
                            let Some(typ) = imported_module.exports.get(exported) else {
                                if let Some(exported_type) =
                                    imported_module.types.get(exported).cloned()
                                {
                                    match exported_type {
                                        ExportedType::Struct {
                                            identity,
                                            fields,
                                            messages,
                                        } => {
                                            self.struct_identities
                                                .insert(local.clone(), identity.clone());
                                            self.structs.insert(local.clone(), fields);
                                            for (message, signature) in messages {
                                                self.messages
                                                    .insert((identity.clone(), message), signature);
                                            }
                                        }
                                        ExportedType::Enum { identity, variants } => {
                                            self.enum_identities.insert(local.clone(), identity);
                                            self.enums.insert(local.clone(), variants);
                                        }
                                    }
                                    continue;
                                }
                                return Err(self.error(
                                    DiagnosticCode::UndefinedVariable,
                                    format!("module does not export `{exported}`"),
                                ));
                            };
                            self.imported_types.insert(local.clone(), typ.clone());
                        }
                    }
                }
            }
        }
        self.collect_structs_and_messages(&program.statements)?;
        self.collect_functions(&program.statements)?;
        self.analyze_statements(&program.statements)?;
        if !is_root && self.module_return_type.is_none() {
            return Err(self.error(
                DiagnosticCode::InvalidReturn,
                "imported module must return a value",
            ));
        }
        let mut exports = HashMap::new();
        let mut types = HashMap::new();
        for name in declared_exports {
            if let Some(typ) = self.variables.get(&name) {
                exports.insert(name.clone(), typ.clone());
                continue;
            }
            if let Some(identity) = self.struct_identities.get(&name) {
                let fields = self.structs.get(&name).cloned().ok_or_else(|| {
                    self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("cannot export unknown type `{name}`"),
                    )
                })?;
                let messages = self
                    .messages
                    .iter()
                    .filter(|((message_identity, _), _)| message_identity == identity)
                    .map(|((_, message), signature)| (message.clone(), signature.clone()))
                    .collect();
                types.insert(
                    name,
                    ExportedType::Struct {
                        identity: identity.clone(),
                        fields,
                        messages,
                    },
                );
                continue;
            }
            if let Some(identity) = self.enum_identities.get(&name) {
                let variants = self.enums.get(&name).cloned().ok_or_else(|| {
                    self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("cannot export unknown type `{name}`"),
                    )
                })?;
                types.insert(
                    name,
                    ExportedType::Enum {
                        identity: identity.clone(),
                        variants,
                    },
                );
                continue;
            }
            return Err(self.error(
                DiagnosticCode::UndefinedVariable,
                format!("cannot export unknown value `{name}` or type"),
            ));
        }
        Ok(ModuleInterface {
            return_type: self.module_return_type.clone().unwrap_or(Type::Unit),
            exports,
            types,
        })
    }
}
