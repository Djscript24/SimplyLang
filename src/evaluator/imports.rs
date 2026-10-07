use super::*;

impl Evaluator {
    pub(super) fn load_import(&mut self, path: &str) -> Result<ImportedModule, SimplyError> {
        let base = self
            .current_file
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        let requested = Path::new(path);
        let resolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            base.join(requested)
        };
        let resolved =
            fs::canonicalize(&resolved).map_err(|error| self.file_error(&resolved, error))?;
        if let Some(cycle_start) = self
            .import_stack
            .iter()
            .position(|imported| imported == &resolved)
        {
            let mut chain = self.import_stack[cycle_start..]
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            chain.push(resolved.display().to_string());
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeImport,
                format!("cyclic import: {}", chain.join(" -> ")),
            ));
        }
        let (program, source) = if let Some(cached) = self.import_cache.borrow().get(&resolved) {
            (Arc::clone(&cached.0), cached.1.clone())
        } else {
            let source =
                fs::read_to_string(&resolved).map_err(|error| self.file_error(&resolved, error))?;
            let tokens = Lexer::new(&source)
                .tokenize()
                .map_err(|error| error.in_source(resolved.display().to_string(), source.clone()))?;
            let program = Parser::new(tokens)
                .parse()
                .map_err(|error| error.in_source(resolved.display().to_string(), source.clone()))?;
            let program = Arc::new(program);
            let source = self.heap.insert_source(SourceText { text: source });
            self.import_cache
                .borrow_mut()
                .insert(resolved.clone(), (Arc::clone(&program), source.clone()));
            (program, source)
        };
        let source_contents = source.with(|source| source.text.clone()).ok_or_else(|| {
            self.runtime_error_with_code(
                DiagnosticCode::RuntimeImport,
                "source text handle is no longer valid",
            )
        })?;
        let mut module = Self {
            current_file: Some(resolved.clone()),
            module_identity: resolved.display().to_string(),
            current_source: Some(source.clone()),
            heap: self.heap.clone(),
            import_stack: self
                .import_stack
                .iter()
                .cloned()
                .chain(std::iter::once(resolved.clone()))
                .collect(),
            module_is_imported: true,
            import_cache: self.import_cache.clone(),
            ..Self::new()
        };
        let mut export_names = Vec::new();
        let mut seen_exports = HashSet::new();
        for statement in &program.statements {
            let statement = match statement {
                Stmt::Located { statement, .. } => statement.as_ref(),
                statement => statement,
            };
            if let Stmt::Export { names } = statement {
                for name in names {
                    if !seen_exports.insert(name.clone()) {
                        return Err(module.runtime_error_with_code(
                            DiagnosticCode::RuntimeImport,
                            format!("`{name}` is exported more than once"),
                        ));
                    }
                    export_names.push(name.clone());
                }
            }
        }
        let result = (|| {
            module.register_declarations(&program.statements)?;
            let returned_value = match module.execute_statements(&program.statements)? {
                Flow::Return(value) => value,
                Flow::None => {
                    return Err(module.runtime_error_with_code(
                        DiagnosticCode::RuntimeImport,
                        format!("imported file `{path}` must return a value"),
                    ));
                }
                Flow::Break | Flow::Continue => {
                    return Err(module.runtime_error_with_code(
                        DiagnosticCode::RuntimeControl,
                        "control statement is outside its valid context",
                    ));
                }
            };
            let mut exports = HashMap::new();
            let mut exported_types = HashMap::new();
            for name in export_names {
                if let Some(value) = module.lookup(&name).cloned() {
                    if let Value::Function(function) = &value {
                        let function_value = module.tracked_function(*function)?;
                        if function_value.captures.is_empty() {
                            let mut dependencies = closure_dependencies(
                                &function_value.body,
                                &function_value.parameters,
                            );
                            if let Some(function_name) = &function_value.name {
                                dependencies.remove(function_name);
                            }
                            let captures = module.scopes.values_for(&dependencies);
                            module
                                .heap
                                .with_function_mut(*function, |function| {
                                    function.captures = captures;
                                })
                                .expect("tracked function remains in its heap");
                        }
                    }
                    exports.insert(name, value);
                    continue;
                }
                if let Some(definition) = module.lookup_struct(&name).cloned() {
                    let messages = module
                        .message_scopes
                        .iter()
                        .flat_map(|scope| scope.iter())
                        .filter(|((identity, _), _)| identity == &definition.identity)
                        .map(|(key, behavior)| {
                            let behavior = behavior.clone();
                            let function = module.tracked_function(behavior.function)?;
                            let mut dependencies =
                                closure_dependencies(&function.body, &function.parameters);
                            for (parameter, _, _, _) in &function.parameters {
                                dependencies.remove(parameter);
                            }
                            let captures = module.scopes.values_for(&dependencies);
                            module
                                .heap
                                .with_function_mut(behavior.function, |function| {
                                    function.captures = captures;
                                })
                                .expect("tracked message function remains in its heap");
                            Ok((key.clone(), behavior))
                        })
                        .collect::<Result<HashMap<_, _>, SimplyError>>()?;
                    exported_types.insert(
                        name,
                        ExportedRuntimeType::Struct {
                            definition,
                            messages,
                        },
                    );
                    continue;
                }
                if let Some(definition) = module.lookup_enum(&name).cloned() {
                    exported_types.insert(name, ExportedRuntimeType::Enum(definition));
                    continue;
                }
                return Err(module.runtime_error_with_code(
                    DiagnosticCode::RuntimeImport,
                    format!("cannot export unknown value `{name}` or type"),
                ));
            }
            Ok((returned_value, exports, exported_types))
        })();
        result.map_err(|error: SimplyError| {
            error
                .in_source(resolved.display().to_string(), source_contents)
                .with_context(format!("in imported module `{}`", resolved.display()))
        })
    }

    pub(super) fn file_error(&self, path: &Path, error: std::io::Error) -> SimplyError {
        self.runtime_error_with_code(
            DiagnosticCode::RuntimeImport,
            format!("could not open `{}`: {error}", path.display()),
        )
    }

    pub(super) fn function_source_context(&self) -> Option<SourceContext> {
        if self.import_stack.len() <= 1 {
            return None;
        }
        Some(SourceContext {
            filename: self.current_file.as_ref()?.display().to_string(),
            source: self.current_source.as_ref()?.clone(),
        })
    }
}
