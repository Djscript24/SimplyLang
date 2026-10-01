//! semantic.rs — static program analysis
//! Checks declarations, scopes, types, functions, and control-flow requirements before evaluation.
//! Key components: SemanticAnalyzer, FunctionSignature, and ScopeStack.
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    ast::{
        BinaryOperator, Expr, Literal, MatchPattern, PipelineStep, Program, Stmt, StructField,
        UnaryOperator, is_parallel_safe_expression,
    },
    error::{DiagnosticCode, SimplyError, Span},
    runtime::limits,
    types::{DeclarationIdentity, DeclarationKind, Type},
};

#[derive(Clone)]
struct FunctionSignature {
    parameters: Vec<Option<Type>>,
    return_type: Option<Type>,
}

#[derive(Clone)]
struct MessageSignature {
    parameters: Vec<Option<Type>>,
    return_type: Option<Type>,
}

#[derive(Clone)]
struct ModuleInterface {
    return_type: Type,
    exports: HashMap<String, Type>,
    types: HashMap<String, ExportedType>,
}

#[derive(Clone)]
enum ExportedType {
    Struct {
        identity: DeclarationIdentity,
        fields: Vec<StructField>,
        messages: HashMap<String, MessageSignature>,
    },
    Enum {
        identity: DeclarationIdentity,
        variants: Vec<crate::ast::EnumVariant>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CoverageConstructor {
    Enum {
        type_identity: DeclarationIdentity,
        variant: String,
    },
    Tuple(usize),
    Struct(DeclarationIdentity),
    Bool(bool),
}

#[derive(Default)]
pub struct SemanticAnalyzer {
    variables: ScopeStack,
    function_scopes: Vec<HashMap<String, FunctionSignature>>,
    following_functions: Vec<HashSet<String>>,
    ordered_function_references: Vec<HashSet<(usize, String)>>,
    blocked_function_references: Vec<HashSet<(usize, String)>>,
    structs: HashMap<String, Vec<StructField>>,
    struct_identities: HashMap<String, DeclarationIdentity>,
    enums: HashMap<String, Vec<crate::ast::EnumVariant>>,
    enum_identities: HashMap<String, DeclarationIdentity>,
    messages: HashMap<(DeclarationIdentity, String), MessageSignature>,
    current_span: Option<Span>,
    loop_depth: usize,
    function_depth: usize,
    function_capture_frames: Vec<(usize, bool)>,
    function_return: Option<Type>,
    inferred_return: Option<Type>,
    saw_return: bool,
    module_return_allowed: bool,
    module_return_type: Option<Type>,
    imported_types: HashMap<String, Type>,
    module_export_names: HashSet<String>,
    module_identity: String,
}

#[derive(Clone)]
struct ScopeStack {
    scopes: Vec<HashMap<String, Type>>,
    bindings: HashMap<String, Vec<usize>>,
    mutability: HashMap<String, Vec<bool>>,
}

fn resolve_import_path(current_file: Option<&Path>, path: &str) -> Result<PathBuf, SimplyError> {
    let base = current_file
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };
    fs::canonicalize(&resolved).map_err(|error| SimplyError::Runtime {
        span: Span::new(0, 0),
        code: DiagnosticCode::RuntimeImport,
        message: format!("could not open `{path}`: {error}"),
    })
}

impl ScopeStack {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            bindings: HashMap::new(),
            mutability: HashMap::new(),
        }
    }

    fn insert(
        &mut self,
        name: String,
        typ: Type,
        mutable: bool,
    ) -> Result<Option<Type>, SimplyError> {
        self.insert_at(name, typ, mutable, Span::new(0, 0))
    }

    fn insert_at(
        &mut self,
        name: String,
        typ: Type,
        mutable: bool,
        span: Span,
    ) -> Result<Option<Type>, SimplyError> {
        let current = self
            .scopes
            .last_mut()
            .expect("semantic analyzer always has a global scope");
        if current.contains_key(&name) {
            return Err(SimplyError::Semantic {
                span,
                code: DiagnosticCode::DuplicateDeclaration,
                message: format!(
                    "variable `{name}` is already declared in this scope; declare it with `mut` to reassign it"
                ),
            });
        }
        let previous = current.insert(name.clone(), typ);
        self.bindings
            .entry(name.clone())
            .or_default()
            .push(self.scopes.len() - 1);
        self.mutability.entry(name).or_default().push(mutable);
        Ok(previous)
    }

    fn define(&mut self, name: String, typ: Type, mutable: bool) -> Result<(), SimplyError> {
        self.define_at(name, typ, mutable, Span::new(0, 0))
    }

    fn define_at(
        &mut self,
        name: String,
        typ: Type,
        mutable: bool,
        span: Span,
    ) -> Result<(), SimplyError> {
        if self
            .scopes
            .last()
            .map(|scope| scope.contains_key(&name))
            .unwrap_or(false)
        {
            return Err(SimplyError::Semantic {
                span,
                code: DiagnosticCode::DuplicateDeclaration,
                message: format!(
                    "variable `{name}` is already declared in this scope; declare it with `mut` to reassign it"
                ),
            });
        }
        self.scopes
            .last_mut()
            .expect("semantic analyzer always has a global scope")
            .insert(name.clone(), typ);
        self.bindings
            .entry(name.clone())
            .or_default()
            .push(self.scopes.len() - 1);
        self.mutability.entry(name).or_default().push(mutable);
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&Type> {
        let scope_index = self.bindings.get(name)?.last().copied()?;
        self.scopes.get(scope_index)?.get(name)
    }

    fn replace_visible(&mut self, name: &str, typ: Type) {
        let Some(scope_index) = self
            .bindings
            .get(name)
            .and_then(|indices| indices.last())
            .copied()
        else {
            return;
        };
        if let Some(scope) = self.scopes.get_mut(scope_index) {
            scope.insert(name.to_owned(), typ);
        }
    }

    fn is_mutable(&self, name: &str) -> bool {
        self.mutability
            .get(name)
            .and_then(|values| values.last())
            .copied()
            .unwrap_or(false)
    }

    fn binding_scope(&self, name: &str) -> Option<usize> {
        self.bindings.get(name)?.last().copied()
    }

    fn remove(&mut self, name: &str) -> Option<Type> {
        let value = self
            .scopes
            .last_mut()
            .expect("semantic analyzer always has a global scope")
            .remove(name)?;
        if let Some(scope_indices) = self.bindings.get_mut(name) {
            scope_indices.pop();
            if scope_indices.is_empty() {
                self.bindings.remove(name);
            }
            if let Some(values) = self.mutability.get_mut(name) {
                values.pop();
                if values.is_empty() {
                    self.mutability.remove(name);
                }
            }
        }
        Some(value)
    }

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn truncate(&mut self, depth: usize) {
        while self.scopes.len() > depth && self.scopes.len() > 1 {
            let scope = self.scopes.pop().expect("scope exists above global frame");
            for name in scope.keys() {
                if let Some(scope_indices) = self.bindings.get_mut(name) {
                    scope_indices.pop();
                    if scope_indices.is_empty() {
                        self.bindings.remove(name);
                    }
                    if let Some(values) = self.mutability.get_mut(name) {
                        values.pop();
                        if values.is_empty() {
                            self.mutability.remove(name);
                        }
                    }
                }
            }
        }
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        Self {
            variables: ScopeStack::new(),
            function_scopes: vec![HashMap::new()],
            module_identity: "memory://root".into(),
            ..Self::default()
        }
    }

    fn identity(&self, name: &str, kind: DeclarationKind) -> DeclarationIdentity {
        DeclarationIdentity::new(self.module_identity.clone(), name, kind)
    }

    fn enum_name_for_identity(&self, identity: &DeclarationIdentity) -> Option<&String> {
        self.enum_identities
            .iter()
            .find_map(|(name, found)| (found == identity).then_some(name))
    }

    fn struct_name_for_identity(&self, identity: &DeclarationIdentity) -> Option<&String> {
        self.struct_identities
            .iter()
            .find_map(|(name, found)| (found == identity).then_some(name))
    }

    fn resolve_type_identity(&self, typ: &Type) -> Type {
        match typ {
            Type::Struct(identity) if identity.is_unresolved() => {
                if let Some(identity) = self.struct_identities.get(&identity.local_name) {
                    Type::Struct(identity.clone())
                } else if let Some(identity) = self.enum_identities.get(&identity.local_name) {
                    Type::Enum(identity.clone())
                } else {
                    typ.clone()
                }
            }
            Type::Enum(identity) if identity.is_unresolved() => self
                .enum_identities
                .get(&identity.local_name)
                .cloned()
                .map(Type::Enum)
                .unwrap_or_else(|| typ.clone()),
            Type::Array(element) => Type::Array(Box::new(self.resolve_type_identity(element))),
            Type::List(element) => Type::List(Box::new(self.resolve_type_identity(element))),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|element| self.resolve_type_identity(element))
                    .collect(),
            ),
            Type::HashValues(element) => {
                Type::HashValues(Box::new(self.resolve_type_identity(element)))
            }
            Type::TreeValues(element) => {
                Type::TreeValues(Box::new(self.resolve_type_identity(element)))
            }
            Type::Function {
                parameters,
                return_type,
            } => Type::Function {
                parameters: parameters
                    .iter()
                    .map(|parameter| {
                        parameter
                            .as_ref()
                            .map(|typ| Box::new(self.resolve_type_identity(typ)))
                    })
                    .collect(),
                return_type: return_type
                    .as_ref()
                    .map(|typ| Box::new(self.resolve_type_identity(typ))),
            },
            _ => typ.clone(),
        }
    }

    fn is_snapshot_capture(&self, name: &str) -> bool {
        let Some((frame_start, captures_outer)) = self.function_capture_frames.last() else {
            return false;
        };
        *captures_outer
            && self
                .variables
                .binding_scope(name)
                .is_some_and(|scope| scope < *frame_start)
    }

    fn ensure_global_function_scope(&mut self) {
        if self.function_scopes.is_empty() {
            self.function_scopes.push(HashMap::new());
        }
    }

    fn check_function_reference_order(&self, name: &str) -> Result<(), SimplyError> {
        for (scope_index, scope) in self.function_scopes.iter().enumerate().rev() {
            if scope.contains_key(name) {
                if self
                    .blocked_function_references
                    .iter()
                    .any(|blocked| blocked.contains(&(scope_index, name.to_owned())))
                {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!(
                            "function `{name}` is declared later and is not captured by this closure"
                        ),
                    ));
                }
                if (self.function_scopes.len() > 1 || self.module_return_allowed)
                    && self
                        .ordered_function_references
                        .iter()
                        .any(|blocked| blocked.contains(&(scope_index, name.to_owned())))
                {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("function `{name}` is declared later and is not available yet"),
                    ));
                }
                break;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn analyze(&mut self, program: &Program) -> Result<(), SimplyError> {
        self.ensure_global_function_scope();
        self.module_identity = "memory://root".into();
        self.module_return_allowed = false;
        self.module_return_type = None;
        self.imported_types.clear();
        self.module_export_names.clear();
        self.collect_structs_and_messages(&program.statements)?;
        self.collect_functions(&program.statements)?;
        self.analyze_statements(&program.statements)
    }

    pub fn analyze_file(&mut self, path: &Path) -> Result<(), SimplyError> {
        let resolved = fs::canonicalize(path).map_err(|error| SimplyError::Runtime {
            span: Span::new(0, 0),
            code: DiagnosticCode::RuntimeImport,
            message: format!("could not open `{}`: {error}", path.display()),
        })?;
        let source = fs::read_to_string(&resolved).map_err(|error| SimplyError::Runtime {
            span: Span::new(0, 0),
            code: DiagnosticCode::RuntimeImport,
            message: format!("could not read `{}`: {error}", path.display()),
        })?;
        let tokens = crate::lexer::Lexer::new(&source).tokenize()?;
        let program = crate::parser::Parser::new(tokens).parse()?;
        self.analyze_file_program(&resolved, &program)
    }

    pub fn analyze_file_program(
        &mut self,
        path: &Path,
        program: &Program,
    ) -> Result<(), SimplyError> {
        self.analyze_file_program_kind(path, program, true)
    }

    pub fn analyze_module_file_program(
        &mut self,
        path: &Path,
        program: &Program,
    ) -> Result<(), SimplyError> {
        self.analyze_file_program_kind(path, program, false)
    }

    fn analyze_file_program_kind(
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

    fn analyze_imports(
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

    fn collect_structs_and_messages(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
        let statements = statements.iter().map(|statement| match statement {
            Stmt::Located { span, statement } => (Some(span.clone()), statement.as_ref()),
            statement => (None, statement),
        });
        let statements = statements.collect::<Vec<_>>();
        for (span, statement) in &statements {
            if let Some(span) = span {
                self.current_span = Some(span.clone());
            }
            if let Stmt::Enum { name, variants } = statement {
                if self.enums.contains_key(name) || self.structs.contains_key(name) {
                    return Err(self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("type `{name}` is already declared"),
                    ));
                }
                if variants.is_empty() {
                    return Err(self.error(
                        DiagnosticCode::UnexpectedToken,
                        format!("enum `{name}` must declare at least one variant"),
                    ));
                }
                self.enum_identities
                    .insert(name.clone(), self.identity(name, DeclarationKind::Enum));
                self.enums.insert(
                    name.clone(),
                    variants
                        .iter()
                        .cloned()
                        .map(|mut variant| {
                            variant.payload_type = variant
                                .payload_type
                                .as_ref()
                                .map(|typ| self.resolve_type_identity(typ));
                            variant
                        })
                        .collect(),
                );
            }
        }
        for (span, statement) in &statements {
            if let Some(span) = span {
                self.current_span = Some(span.clone());
            }
            if let Stmt::Struct { name, fields } = statement {
                if self.structs.contains_key(name) || self.enums.contains_key(name) {
                    return Err(self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("type `{name}` is already declared"),
                    ));
                }
                self.struct_identities
                    .insert(name.clone(), self.identity(name, DeclarationKind::Struct));
                self.structs.insert(
                    name.clone(),
                    fields
                        .iter()
                        .cloned()
                        .map(|mut field| {
                            field.field_type = self.resolve_type_identity(&field.field_type);
                            field
                        })
                        .collect(),
                );
            }
        }
        let enum_definitions = self
            .enums
            .iter()
            .map(|(name, variants)| (name.clone(), variants.clone()))
            .collect::<Vec<_>>();
        for (name, variants) in enum_definitions {
            self.enums.insert(
                name,
                variants
                    .into_iter()
                    .map(|mut variant| {
                        variant.payload_type = variant
                            .payload_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ));
                        variant
                    })
                    .collect(),
            );
        }
        let struct_definitions = self
            .structs
            .iter()
            .map(|(name, fields)| (name.clone(), fields.clone()))
            .collect::<Vec<_>>();
        for (name, fields) in struct_definitions {
            self.structs.insert(
                name,
                fields
                    .into_iter()
                    .map(|mut field| {
                        field.field_type = self.resolve_type_identity(&field.field_type);
                        field
                    })
                    .collect(),
            );
        }
        for (span, statement) in statements {
            if let Some(span) = span {
                self.current_span = Some(span);
            }
            match statement {
                Stmt::Struct { fields, .. } => {
                    for field in fields {
                        self.validate_declared_type(&field.field_type)?;
                    }
                }
                Stmt::Enum { variants, .. } => {
                    for variant in variants {
                        if let Some(payload_type) = &variant.payload_type {
                            self.validate_declared_type(payload_type)?;
                        }
                    }
                }
                Stmt::Message {
                    receiver_type,
                    name,
                    parameters,
                    ..
                } => {
                    if !self.structs.contains_key(receiver_type) {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            format!("unknown receiver type `{receiver_type}` for message `{name}`"),
                        ));
                    }
                    let key = (self.struct_identities[receiver_type].clone(), name.clone());
                    if self.messages.contains_key(&key) {
                        return Err(self.error(
                            DiagnosticCode::DuplicateDeclaration,
                            format!("message `{name}` is already defined for `{receiver_type}`"),
                        ));
                    }
                    let fields = &self.structs[receiver_type];
                    for (parameter, parameter_type, _) in parameters {
                        if fields.iter().any(|field| &field.name == parameter) {
                            return Err(self.error(
                                DiagnosticCode::DuplicateDeclaration,
                                format!(
                                    "message parameter `{parameter}` conflicts with a field of `{receiver_type}`"
                                ),
                            ));
                        }
                        if let Some(parameter_type) = parameter_type {
                            self.validate_declared_type(parameter_type)?;
                        }
                    }
                    self.messages.insert(
                        key,
                        MessageSignature {
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter_type, _)| {
                                    parameter_type
                                        .as_ref()
                                        .map(|typ| self.resolve_type_identity(typ))
                                })
                                .collect(),
                            return_type: None,
                        },
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_destructure_assignment_pattern(
        &self,
        pattern: &MatchPattern,
        actual: &Type,
        targets: &mut Vec<String>,
    ) -> Result<(), SimplyError> {
        match pattern {
            MatchPattern::Identifier(name) => {
                self.validate_destructure_assignment_identifier(name, actual, targets)?;
            }
            MatchPattern::Wildcard => {}
            MatchPattern::Tuple(patterns) => match actual {
                Type::Tuple(types) if types.len() == patterns.len() => {
                    for (pattern, typ) in patterns.iter().zip(types) {
                        self.validate_destructure_assignment_pattern(pattern, typ, targets)?;
                    }
                }
                Type::Tuple(types) => {
                    return Err(self.error(
                        DiagnosticCode::SemanticDestructure,
                        format!(
                            "tuple has {} values, but destructuring assignment target has {} elements",
                            types.len(),
                            patterns.len()
                        ),
                    ));
                }
                Type::Unknown => {
                    for pattern in patterns {
                        self.validate_destructure_assignment_pattern(
                            pattern,
                            &Type::Unknown,
                            targets,
                        )?;
                    }
                }
                _ => {
                    return Err(self.error(
                        DiagnosticCode::SemanticDestructure,
                        "tuple destructuring assignment requires a tuple value",
                    ));
                }
            },
            MatchPattern::Sequence { patterns, rest } => {
                let (element_type, rest_type) = match actual {
                    Type::Array(element) => {
                        (element.as_ref().clone(), Type::Array(element.clone()))
                    }
                    Type::List(element) => (element.as_ref().clone(), Type::List(element.clone())),
                    Type::Unknown => (Type::Unknown, Type::Unknown),
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticDestructure,
                            format!(
                                "sequence destructuring assignment requires an Array or List value, found {}",
                                actual.name()
                            ),
                        ));
                    }
                };
                for pattern in patterns {
                    self.validate_destructure_assignment_pattern(pattern, &element_type, targets)?;
                }
                if let Some(name) = rest {
                    self.validate_destructure_assignment_identifier(name, &rest_type, targets)?;
                }
            }
            MatchPattern::Literal(_)
            | MatchPattern::Range { .. }
            | MatchPattern::Or(_)
            | MatchPattern::Hash(_)
            | MatchPattern::Alias { .. }
            | MatchPattern::EnumVariant { .. }
            | MatchPattern::Struct { .. }
            | MatchPattern::NamedStruct { .. } => {
                return Err(self.error(
                    DiagnosticCode::SemanticDestructure,
                    "destructuring assignment targets support only identifiers, `_`, tuples, sequences, and rest bindings",
                ));
            }
        }
        Ok(())
    }

    fn validate_destructure_assignment_identifier(
        &self,
        name: &str,
        actual: &Type,
        targets: &mut Vec<String>,
    ) -> Result<(), SimplyError> {
        let expected = self.variables.get(name).ok_or_else(|| {
            self.error(
                DiagnosticCode::UndefinedVariable,
                format!("cannot destructure-assign unknown variable `{name}`"),
            )
        })?;
        if self.is_snapshot_capture(name) || !self.variables.is_mutable(name) {
            return Err(self.error(
                DiagnosticCode::InvalidReassignment,
                format!("cannot reassign immutable variable `{name}`; declare it with `mut`"),
            ));
        }
        if targets.iter().any(|target| target == name) {
            return Err(self.error(
                DiagnosticCode::SemanticDestructure,
                format!("duplicate destructuring assignment target `{name}`"),
            ));
        }
        self.require_type(expected, actual)?;
        targets.push(name.to_owned());
        Ok(())
    }

    fn validate_declared_type(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Struct(identity)
                if !identity.is_unresolved()
                    && !self
                        .struct_identities
                        .values()
                        .any(|known| known == identity) =>
            {
                Err(self.error(
                    DiagnosticCode::UndefinedVariable,
                    format!("unknown type `{}`", identity.local_name),
                ))
            }
            Type::Struct(identity) if !self.structs.contains_key(&identity.local_name) => Err(self
                .error(
                    DiagnosticCode::UndefinedVariable,
                    format!("unknown type `{}`", identity.local_name),
                )),
            Type::Enum(identity)
                if !identity.is_unresolved()
                    && !self.enum_identities.values().any(|known| known == identity) =>
            {
                Err(self.error(
                    DiagnosticCode::UndefinedVariable,
                    format!("unknown enum type `{}`", identity.local_name),
                ))
            }
            Type::Enum(identity) if !self.enums.contains_key(&identity.local_name) => Err(self
                .error(
                    DiagnosticCode::UndefinedVariable,
                    format!("unknown enum type `{}`", identity.local_name),
                )),
            Type::Array(element) | Type::List(element) => self.validate_declared_type(element),
            Type::Tuple(elements) => {
                for element in elements {
                    self.validate_declared_type(element)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn define_variable(
        &mut self,
        name: String,
        typ: Type,
        mutable: bool,
    ) -> Result<(), SimplyError> {
        let span = self.current_span.clone().unwrap_or_else(|| Span::new(0, 0));
        self.variables.define_at(name, typ, mutable, span)
    }

    fn validate_destructure_pattern(
        &self,
        pattern: &MatchPattern,
        expected: &Type,
        bindings: &mut Vec<(String, Type)>,
    ) -> Result<(), SimplyError> {
        match pattern {
            MatchPattern::Identifier(name) => bindings.push((name.clone(), expected.clone())),
            MatchPattern::Wildcard => {}
            MatchPattern::Tuple(patterns) => match expected {
                Type::Tuple(types) if types.len() == patterns.len() => {
                    for (pattern, typ) in patterns.iter().zip(types) {
                        self.validate_destructure_pattern(pattern, typ, bindings)?;
                    }
                }
                Type::Tuple(types) => {
                    return Err(self.error(
                        DiagnosticCode::SemanticDestructure,
                        format!(
                            "tuple has {} values, but destructuring target has {} elements",
                            types.len(),
                            patterns.len()
                        ),
                    ));
                }
                Type::Unknown => {
                    for pattern in patterns {
                        self.validate_destructure_pattern(pattern, &Type::Unknown, bindings)?;
                    }
                }
                _ => {
                    return Err(self.error(
                        DiagnosticCode::SemanticDestructure,
                        "tuple destructuring requires a tuple value",
                    ));
                }
            },
            MatchPattern::Sequence { patterns, rest } => {
                let (element_type, rest_type) = match expected {
                    Type::Array(element) => {
                        (element.as_ref().clone(), Type::Array(element.clone()))
                    }
                    Type::List(element) => (element.as_ref().clone(), Type::List(element.clone())),
                    Type::Range => (Type::Int, Type::Range),
                    Type::Unknown => (Type::Unknown, Type::Unknown),
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticDestructure,
                            format!(
                                "sequence destructuring requires an Array or List value, found {}",
                                expected.name()
                            ),
                        ));
                    }
                };
                for pattern in patterns {
                    self.validate_destructure_pattern(pattern, &element_type, bindings)?;
                }
                if let Some(name) = rest {
                    bindings.push((name.clone(), rest_type));
                }
            }
            MatchPattern::Literal(_)
            | MatchPattern::Range { .. }
            | MatchPattern::Or(_)
            | MatchPattern::Hash(_)
            | MatchPattern::Alias { .. }
            | MatchPattern::EnumVariant { .. }
            | MatchPattern::Struct { .. }
            | MatchPattern::NamedStruct { .. } => {
                return Err(self.error(
                    DiagnosticCode::SemanticDestructure,
                    "destructuring targets support only identifiers, `_`, tuples, sequences, and rest bindings",
                ));
            }
        }
        Ok(())
    }

    fn collect_functions(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
        for statement in statements {
            let statement_span = match statement {
                Stmt::Located { span, .. } => Some(span.clone()),
                _ => None,
            };
            let previous_span = self.current_span.clone();
            if let Some(span) = statement_span {
                self.current_span = Some(span);
            }
            let statement = match statement {
                Stmt::Located { statement, .. } => statement.as_ref(),
                statement => statement,
            };
            if let Stmt::Function {
                name,
                parameters,
                return_type,
                body: _,
            } = statement
            {
                if self.structs.contains_key(name) || self.enums.contains_key(name) {
                    return Err(self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("function `{name}` conflicts with a struct type"),
                    ));
                }
                for (_, parameter_type, _) in parameters {
                    if let Some(parameter_type) = parameter_type {
                        self.validate_declared_type(parameter_type)?;
                    }
                }
                if let Some(return_type) = return_type {
                    self.validate_declared_type(return_type)?;
                }
                if self
                    .function_scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
                {
                    return Err(self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("function `{name}` is already declared in this scope"),
                    ));
                }
                let resolved_parameters: Vec<Option<Type>> = parameters
                    .iter()
                    .map(|(_, parameter_type, _)| {
                        parameter_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ))
                    })
                    .collect();
                let resolved_return_type = return_type
                    .as_ref()
                    .map(|typ| self.resolve_type_identity(typ));
                self.function_scopes
                    .last_mut()
                    .expect("semantic function scope has a global frame")
                    .insert(
                        name.clone(),
                        FunctionSignature {
                            parameters: resolved_parameters.clone(),
                            return_type: resolved_return_type.clone(),
                        },
                    );
                self.variables.define(
                    name.clone(),
                    Type::Function {
                        parameters: resolved_parameters
                            .into_iter()
                            .map(|typ| typ.map(Box::new))
                            .collect(),
                        return_type: resolved_return_type.map(Box::new),
                    },
                    false,
                )?;
            }
            self.current_span = previous_span;
        }
        Ok(())
    }

    fn analyze_statements(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
        for (index, statement) in statements.iter().enumerate() {
            let following: HashSet<String> = statements[index + 1..]
                .iter()
                .filter_map(|statement| match statement {
                    Stmt::Located { statement, .. } => match statement.as_ref() {
                        Stmt::Function { name, .. } => Some(name.clone()),
                        _ => None,
                    },
                    Stmt::Function { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect();
            let parent_scope = self.function_scopes.len().saturating_sub(1);
            let ordered = if self.function_scopes.len() > 1 || self.module_return_allowed {
                following
                    .iter()
                    .map(|name| (parent_scope, name.clone()))
                    .collect()
            } else {
                HashSet::new()
            };
            self.ordered_function_references.push(ordered);
            self.following_functions.push(following);
            let result = self.analyze_statement(statement);
            self.following_functions.pop();
            self.ordered_function_references.pop();
            result?;
        }
        Ok(())
    }

    fn analyze_statement(&mut self, statement: &Stmt) -> Result<(), SimplyError> {
        match statement {
            Stmt::Located { span, statement } => {
                self.current_span = Some(span.clone());
                self.analyze_statement(statement)?;
            }
            Stmt::Say(expression) | Stmt::Sayln(expression) | Stmt::Expression(expression) => {
                self.analyze_expression(expression)?;
            }
            Stmt::Import {
                alias, exposing, ..
            } => {
                if let Some(alias) = alias {
                    self.define_variable(
                        alias.clone(),
                        self.imported_types
                            .get(alias)
                            .cloned()
                            .unwrap_or(Type::Unknown),
                        false,
                    )?;
                } else {
                    for (exported, local) in exposing {
                        let Some(typ) = self.imported_types.get(local).cloned() else {
                            if self.struct_identities.contains_key(local)
                                || self.enum_identities.contains_key(local)
                            {
                                continue;
                            }
                            return Err(self.error(
                                DiagnosticCode::UndefinedVariable,
                                format!("module does not export `{exported}`"),
                            ));
                        };
                        self.define_variable(local.clone(), typ, false)?;
                    }
                }
            }
            Stmt::Export { names } => {
                if !self.module_return_allowed
                    || self.function_depth > 0
                    || self.variables.scopes.len() != 1
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidReturn,
                        "`export` is only allowed at the top level of an imported module",
                    ));
                }
                if let Some(name) = names
                    .iter()
                    .find(|name| !self.module_export_names.contains(*name))
                {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("invalid exported name `{name}`"),
                    ));
                }
            }
            Stmt::Struct { .. } => {}
            Stmt::Enum { .. } => {}
            Stmt::Message {
                receiver_type,
                name,
                parameters,
                body,
            } => {
                let fields = self.structs[receiver_type].clone();
                let saved_function_depth = self.function_depth;
                let saved_return = self.function_return.clone();
                let saved_inferred_return = self.inferred_return.clone();
                let saved_saw_return = self.saw_return;
                let saved_loop_depth = self.loop_depth;
                let frame_start = self.variables.scopes.len();
                self.variables.push();
                self.function_scopes.push(HashMap::new());
                self.function_return = None;
                self.inferred_return = None;
                self.function_depth += 1;
                self.loop_depth = 0;
                self.saw_return = false;
                for field in &fields {
                    self.define_variable(field.name.clone(), field.field_type.clone(), true)?;
                }
                for (parameter, parameter_type, mutable) in parameters {
                    self.define_variable(
                        parameter.clone(),
                        parameter_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ))
                            .unwrap_or(Type::Unknown),
                        *mutable,
                    )?;
                }
                let body_result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                let inferred_return = self.inferred_return.clone();
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                self.function_depth = saved_function_depth;
                self.loop_depth = saved_loop_depth;
                self.function_return = saved_return;
                self.inferred_return = saved_inferred_return;
                self.saw_return = saved_saw_return;
                body_result?;
                let key = (self.struct_identities[receiver_type].clone(), name.clone());
                if let Some(signature) = self.messages.get_mut(&key) {
                    signature.return_type = inferred_return;
                }
            }
            Stmt::Assign {
                name,
                mutable,
                declared_type,
                value,
            } => {
                let actual = self.analyze_expression(value)?;
                let expected = declared_type
                    .as_ref()
                    .map(|typ| self.resolve_type_identity(typ))
                    .unwrap_or_else(|| actual.clone());
                self.require_type(&expected, &actual)?;
                self.define_variable(name.clone(), expected, *mutable)?;
            }
            Stmt::Flow {
                name,
                source,
                steps,
            } => {
                let actual = self.analyze_expression(&Expr::Pipeline {
                    source: Box::new(source.clone()),
                    steps: steps.clone(),
                })?;
                self.define_variable(name.clone(), actual, false)?;
            }
            Stmt::Reassign { name, value } => {
                if self.is_snapshot_capture(name) || !self.variables.is_mutable(name) {
                    return Err(self.error(
                        DiagnosticCode::InvalidReassignment,
                        format!(
                            "cannot reassign immutable variable `{name}`; declare it with `mut`"
                        ),
                    ));
                }
                let expected = self.variables.get(name).cloned().ok_or_else(|| {
                    self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown variable `{name}`"),
                    )
                })?;
                let actual = self.analyze_expression(value)?;
                self.require_type(&expected, &actual)?;
            }
            Stmt::DestructureReassign { pattern, value } => {
                let actual = self.analyze_expression(value)?;
                let mut targets = Vec::new();
                self.validate_destructure_assignment_pattern(pattern, &actual, &mut targets)?;
            }
            Stmt::SetIndex { name, index, value } => {
                if self.is_snapshot_capture(name) || !self.variables.is_mutable(name) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollectionOperation,
                        format!("cannot mutate immutable variable `{name}`; declare it with `mut`"),
                    ));
                }
                let target = self.variables.get(name).cloned().ok_or_else(|| {
                    self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown variable `{name}`"),
                    )
                })?;
                let index_type = self.analyze_expression(index)?;
                let value_type = self.analyze_expression(value)?;
                match target {
                    Type::Array(element) | Type::List(element) => {
                        self.require_type(&Type::Int, &index_type)?;
                        self.require_type(&element, &value_type)?;
                    }
                    Type::Hash => self.require_type(&Type::String, &index_type)?,
                    Type::HashValues(element) => {
                        self.require_type(&Type::String, &index_type)?;
                        self.require_type(&element, &value_type)?;
                    }
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollectionOperation,
                            format!("`{name}` is not mutable"),
                        ));
                    }
                }
            }
            Stmt::Destructure {
                pattern,
                mutable,
                value,
            } => {
                let value_type = self.analyze_expression(value)?;
                let mut bindings = Vec::new();
                self.validate_destructure_pattern(pattern, &value_type, &mut bindings)?;

                let mut names = HashSet::new();
                for (name, _) in &bindings {
                    if !names.insert(name.as_str())
                        || self
                            .variables
                            .scopes
                            .last()
                            .is_some_and(|scope| scope.contains_key(name))
                    {
                        return Err(self.error(
                            DiagnosticCode::DuplicateDeclaration,
                            format!(
                                "variable `{name}` is already declared or bound more than once"
                            ),
                        ));
                    }
                }

                for (name, typ) in bindings {
                    self.define_variable(name, typ, *mutable)?;
                }
            }
            Stmt::CollectionOp {
                name,
                operation: _,
                value,
            } => {
                if self.is_snapshot_capture(name) || !self.variables.is_mutable(name) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollectionOperation,
                        format!("cannot mutate immutable variable `{name}`; declare it with `mut`"),
                    ));
                }
                let target = self.variables.get(name).cloned().ok_or_else(|| {
                    self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown variable `{name}`"),
                    )
                })?;
                let value_type = self.analyze_expression(value)?;
                match target {
                    Type::List(element) if *element == Type::Unknown => {
                        self.variables
                            .replace_visible(name, Type::List(Box::new(value_type)));
                    }
                    Type::List(element) => self.require_type(&element, &value_type)?,
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollectionOperation,
                            format!("`{name}` is not a list"),
                        ));
                    }
                }
            }
            Stmt::Function {
                name,
                parameters,
                return_type,
                body,
            } => {
                let saved_function_depth = self.function_depth;
                let saved_return = self.function_return.clone();
                let saved_inferred_return = self.inferred_return.clone();
                let saved_saw_return = self.saw_return;
                let saved_loop_depth = self.loop_depth;
                let captures_outer = self.function_scopes.len() > 1 || self.module_return_allowed;
                let parent_function_scope = self.function_scopes.len().saturating_sub(1);
                let frame_start = self.variables.scopes.len();
                self.variables.push();
                self.function_scopes.push(HashMap::new());
                self.function_capture_frames
                    .push((frame_start, captures_outer));
                self.function_return = return_type
                    .as_ref()
                    .map(|typ| self.resolve_type_identity(typ));
                self.inferred_return = None;
                self.function_depth += 1;
                self.loop_depth = 0;
                self.saw_return = false;
                let blocked = if captures_outer {
                    self.following_functions
                        .last()
                        .into_iter()
                        .flatten()
                        .map(|name| (parent_function_scope, name.clone()))
                        .collect()
                } else {
                    HashSet::new()
                };
                self.blocked_function_references.push(blocked);
                for (parameter, parameter_type, mutable) in parameters {
                    self.define_variable(
                        parameter.clone(),
                        parameter_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ))
                            .unwrap_or(Type::Unknown),
                        *mutable,
                    )?;
                }
                let body_result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                self.blocked_function_references.pop();
                if let Err(error) = body_result {
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.function_capture_frames.pop();
                    self.function_depth = saved_function_depth;
                    self.loop_depth = saved_loop_depth;
                    self.function_return = saved_return;
                    self.inferred_return = saved_inferred_return;
                    self.saw_return = saved_saw_return;
                    return Err(error);
                }
                if let Some(expected) = return_type
                    && !self.saw_return
                    && *expected != Type::Unknown
                {
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.function_capture_frames.pop();
                    self.function_depth = saved_function_depth;
                    self.loop_depth = saved_loop_depth;
                    self.function_return = saved_return;
                    self.inferred_return = saved_inferred_return;
                    self.saw_return = saved_saw_return;
                    return Err(self.error(
                        DiagnosticCode::SemanticMissingReturn,
                        format!("function `{name}` must return {}", expected.name()),
                    ));
                }
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                self.function_capture_frames.pop();
                self.function_depth = saved_function_depth;
                self.loop_depth = saved_loop_depth;
                self.function_return = saved_return;
                if return_type.is_none() {
                    let inferred_return = self.inferred_return.clone();
                    if let Some(signature) = self
                        .function_scopes
                        .iter_mut()
                        .rev()
                        .find_map(|scope| scope.get_mut(name))
                    {
                        signature.return_type = inferred_return.clone();
                    }
                    if let Some(Type::Function { parameters, .. }) =
                        self.variables.get(name).cloned()
                    {
                        self.variables.replace_visible(
                            name,
                            Type::Function {
                                parameters,
                                return_type: inferred_return.map(Box::new),
                            },
                        );
                    }
                }
                self.inferred_return = saved_inferred_return;
                self.saw_return = saved_saw_return;
            }
            Stmt::Return(expression) => {
                let actual = self.analyze_expression(expression)?;
                if self.function_depth == 0 && !self.module_return_allowed {
                    return Err(self.error(
                        DiagnosticCode::InvalidReturn,
                        "return used outside a function",
                    ));
                }
                if self.function_depth == 0 {
                    if let Some(previous) = self.module_return_type.clone() {
                        self.require_type(&previous, &actual)?;
                    } else {
                        self.module_return_type = Some(actual);
                    }
                    self.saw_return = true;
                    return Ok(());
                }
                if let Some(expected) = self.function_return.clone() {
                    self.require_type(&expected, &actual)?;
                }
                if let Some(previous) = self.inferred_return.clone() {
                    self.require_type(&previous, &actual)?;
                } else {
                    self.inferred_return = Some(actual);
                }
                self.saw_return = true;
            }
            Stmt::Throw(expression) => {
                let thrown_type = self.analyze_expression(expression)?;
                if !matches!(thrown_type, Type::Enum(_) | Type::Unknown) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "`throw` requires an enum value, found {}",
                            thrown_type.name()
                        ),
                    ));
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition_type = self.analyze_expression(condition)?;
                self.require_type(&Type::Bool, &condition_type)?;
                let saved_saw_return = self.saw_return;
                let frame_start = self.variables.scopes.len();
                self.variables.push();
                self.function_scopes.push(HashMap::new());
                self.saw_return = false;
                let branch_result = self
                    .collect_functions(then_branch)
                    .and_then(|()| self.analyze_statements(then_branch));
                if let Err(error) = branch_result {
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.saw_return = saved_saw_return;
                    return Err(error);
                }
                let then_returns = self.saw_return;
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                let frame_start = self.variables.scopes.len();
                self.variables.push();
                self.function_scopes.push(HashMap::new());
                self.saw_return = false;
                let branch_result = self
                    .collect_functions(else_branch)
                    .and_then(|()| self.analyze_statements(else_branch));
                if let Err(error) = branch_result {
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.saw_return = saved_saw_return;
                    return Err(error);
                }
                let else_returns = self.saw_return;
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                self.saw_return = saved_saw_return || (then_returns && else_returns);
            }
            Stmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                let saved_saw_return = self.saw_return;
                let try_returns = self.analyze_scoped_block(try_body)?;
                let mut catch_returns = true;
                for catch in catches {
                    if let Some(code) = catch.code.as_deref()
                        && DiagnosticCode::from_code(code).is_none()
                    {
                        return Err(self.error(
                            DiagnosticCode::InvalidCatchCode,
                            format!("unknown diagnostic code `{code}` in catch clause"),
                        ));
                    }
                    let frame_start = self.variables.scopes.len();
                    self.variables.push();
                    self.function_scopes.push(HashMap::new());
                    if let Some(name) = &catch.binding {
                        self.define_variable(name.clone(), Type::Unknown, false)?;
                    }
                    self.saw_return = false;
                    let result = self
                        .collect_functions(&catch.body)
                        .and_then(|()| self.analyze_statements(&catch.body));
                    catch_returns &= self.saw_return;
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    result?;
                }
                let finally_returns = self.analyze_scoped_block(finally_body)?;
                self.saw_return =
                    saved_saw_return || finally_returns || (try_returns && catch_returns);
            }
            Stmt::For {
                name,
                mutable,
                iterable,
                body,
            } => {
                let iterable_type = self.analyze_expression(iterable)?;
                let element = Self::element_type(&iterable_type).ok_or_else(|| {
                    self.error(
                        DiagnosticCode::SemanticCollection,
                        "for requires a collection",
                    )
                })?;
                let frame_start = self.variables.scopes.len();
                let saved_saw_return = self.saw_return;
                self.variables.push();
                self.function_scopes.push(HashMap::new());
                let span = self.current_span.clone().unwrap_or_else(|| Span::new(0, 0));
                self.variables
                    .insert_at(name.clone(), element, *mutable, span)?;
                self.loop_depth += 1;
                let body_result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                if let Err(error) = body_result {
                    self.loop_depth -= 1;
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.saw_return = saved_saw_return;
                    return Err(error);
                }
                self.loop_depth -= 1;
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                self.saw_return = saved_saw_return;
            }
            Stmt::While { condition, body } => {
                let condition_type = self.analyze_expression(condition)?;
                self.require_type(&Type::Bool, &condition_type)?;
                let saved_saw_return = self.saw_return;
                self.loop_depth += 1;
                let result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                self.loop_depth -= 1;
                self.saw_return = saved_saw_return;
                result?;
            }
            Stmt::Break | Stmt::Continue if self.loop_depth == 0 => {
                return Err(self.error(
                    DiagnosticCode::InvalidBreakContinue,
                    "break or continue used outside a loop",
                ));
            }
            Stmt::Break | Stmt::Continue => {}
        }
        Ok(())
    }

    fn analyze_scoped_block(&mut self, statements: &[Stmt]) -> Result<bool, SimplyError> {
        if statements.is_empty() {
            return Ok(false);
        }
        let frame_start = self.variables.scopes.len();
        let saved_saw_return = self.saw_return;
        self.variables.push();
        self.function_scopes.push(HashMap::new());
        self.saw_return = false;
        let result = self
            .collect_functions(statements)
            .and_then(|()| self.analyze_statements(statements));
        let returns = self.saw_return;
        self.variables.truncate(frame_start);
        self.function_scopes.pop();
        self.saw_return = saved_saw_return;
        result.map(|()| returns)
    }

    fn analyze_expression(&mut self, expression: &Expr) -> Result<Type, SimplyError> {
        match expression {
            Expr::Literal(Literal::String(_)) => Ok(Type::String),
            Expr::Literal(Literal::Int(_)) => Ok(Type::Int),
            Expr::Literal(Literal::Float(_)) => Ok(Type::Float),
            Expr::Literal(Literal::Bool(_)) => Ok(Type::Bool),
            Expr::Identifier(name) => {
                if let Some(typ) = self.variables.get(name).cloned() {
                    if matches!(typ, Type::Function { .. }) {
                        self.check_function_reference_order(name)?;
                    }
                    Ok(typ)
                } else if let Some(function) = self
                    .function_scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(name))
                {
                    self.check_function_reference_order(name)?;
                    Ok(Type::Function {
                        parameters: function
                            .parameters
                            .iter()
                            .map(|parameter| parameter.clone().map(Box::new))
                            .collect(),
                        return_type: function.return_type.clone().map(Box::new),
                    })
                } else {
                    Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown variable `{name}`"),
                    ))
                }
            }
            Expr::Array(values) => self.collection_type(values, true),
            Expr::List(values) => self.collection_type(values, false),
            Expr::Tuple(values) => Ok(Type::Tuple(
                values
                    .iter()
                    .map(|value| self.analyze_expression(value))
                    .collect::<Result<_, _>>()?,
            )),
            Expr::Matrix(values) => {
                for value in values {
                    self.analyze_expression(value)?;
                }
                Ok(Type::Matrix)
            }
            Expr::Hash(entries) | Expr::Tree(entries) => {
                let mut value_type = None;
                for (_, value) in entries {
                    let actual = self.analyze_expression(value)?;
                    value_type = Some(match value_type {
                        Some(current) => Self::merge_collection_type(&current, &actual),
                        None => actual,
                    });
                }
                let value_type = value_type.unwrap_or(Type::Unknown);
                Ok(if matches!(expression, Expr::Hash(_)) {
                    Type::HashValues(Box::new(value_type))
                } else {
                    Type::TreeValues(Box::new(value_type))
                })
            }
            Expr::Unary { operator, operand } => {
                let operand_type = self.analyze_expression(operand)?;
                match operator {
                    UnaryOperator::Not => {
                        self.require_type(&Type::Bool, &operand_type)?;
                        Ok(Type::Bool)
                    }
                    UnaryOperator::Negate => {
                        self.require_numeric(&operand_type)?;
                        Ok(operand_type)
                    }
                    UnaryOperator::Transpose => {
                        self.require_type(&Type::Matrix, &operand_type)?;
                        Ok(Type::Matrix)
                    }
                }
            }
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                let left_type = self.analyze_expression(left)?;
                if matches!(
                    (operator, left.as_ref()),
                    (BinaryOperator::And, Expr::Literal(Literal::Bool(false)))
                        | (BinaryOperator::Or, Expr::Literal(Literal::Bool(true)))
                ) {
                    return Ok(Type::Bool);
                }
                let right_type = self.analyze_expression(right)?;
                self.binary_type(operator, &left_type, &right_type)
            }
            Expr::Call { name, arguments } => self.call_type(name, arguments),
            Expr::MessageDispatch {
                receiver,
                message,
                arguments,
            } => self.message_type(receiver, message, arguments),
            Expr::EnumVariant {
                enum_name,
                variant_name,
                arguments,
            } => {
                if self.enums.contains_key(enum_name) {
                    self.enum_variant_type(enum_name, variant_name, arguments)
                } else if self.variables.get(enum_name).is_none() {
                    Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown enum type `{enum_name}`"),
                    ))
                } else {
                    self.message_type(
                        &Expr::Identifier(enum_name.clone()),
                        variant_name,
                        arguments,
                    )
                }
            }
            Expr::Match { value, arms } => self.match_type(value, arms),
            Expr::Index { target, index } => {
                let target_type = self.analyze_expression(target)?;
                let index_type = self.analyze_expression(index)?;
                self.index_type(&target_type, &index_type, index)
            }
            Expr::Field { target, name } => {
                let target_type = self.analyze_expression(target)?;
                match target_type {
                    Type::Hash | Type::Tree | Type::Unknown => Ok(Type::Unknown),
                    Type::HashValues(value) | Type::TreeValues(value) => Ok(*value),
                    _ => Err(self.error(
                        DiagnosticCode::SemanticField,
                        format!("value has no field `{name}`"),
                    )),
                }
            }
            Expr::Pipeline { source, steps } => self.pipeline_type(source, steps),
        }
    }

    fn collection_type(&mut self, values: &[Expr], array: bool) -> Result<Type, SimplyError> {
        let mut element = None;
        for value in values {
            let actual = self.analyze_expression(value)?;
            element = Some(match element {
                Some(current) if !actual.compatible_with(&current) => {
                    return Err(self.type_error(&current, &actual, "collection element"));
                }
                Some(current) => current,
                None => actual,
            });
        }
        let element = element.unwrap_or(Type::Unknown);
        Ok(if array {
            Type::Array(Box::new(element))
        } else {
            Type::List(Box::new(element))
        })
    }

    fn merge_collection_type(current: &Type, actual: &Type) -> Type {
        if current == &Type::Unknown || actual == &Type::Unknown {
            Type::Unknown
        } else if actual.compatible_with(current) {
            current.clone()
        } else if current.compatible_with(actual) {
            actual.clone()
        } else {
            Type::Unknown
        }
    }

    fn binary_type(
        &self,
        operator: &BinaryOperator,
        left: &Type,
        right: &Type,
    ) -> Result<Type, SimplyError> {
        use BinaryOperator::*;
        if left == &Type::Unknown || right == &Type::Unknown {
            return Ok(Type::Unknown);
        }
        match operator {
            Add if left == &Type::Matrix && right == &Type::Matrix => Ok(Type::Matrix),
            Add if left == &Type::String && right == &Type::String => Ok(Type::String),
            Add => self.numeric_result(left, right),
            Subtract | Multiply | Divide | Remainder => self.numeric_result(left, right),
            MatrixMultiply => {
                self.require_type(&Type::Matrix, left)?;
                self.require_type(&Type::Matrix, right)?;
                Ok(Type::Matrix)
            }
            Greater | GreaterEqual | Less | LessEqual => {
                self.require_numeric(left)?;
                self.require_numeric(right)?;
                Ok(Type::Bool)
            }
            Equal | NotEqual => Ok(Type::Bool),
            And | Or => {
                self.require_type(&Type::Bool, left)?;
                self.require_type(&Type::Bool, right)?;
                Ok(Type::Bool)
            }
        }
    }

    fn call_type(&mut self, name: &str, arguments: &[Expr]) -> Result<Type, SimplyError> {
        self.check_function_reference_order(name)?;
        if let Some(fields) = self.structs.get(name).cloned() {
            let argument_types = arguments
                .iter()
                .map(|argument| self.analyze_expression(argument))
                .collect::<Result<Vec<_>, _>>()?;
            if fields.len() != argument_types.len() {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "struct `{name}` expects {} fields, got {}",
                        fields.len(),
                        argument_types.len()
                    ),
                ));
            }
            for (field, actual) in fields.iter().zip(&argument_types) {
                self.require_type(&field.field_type, actual)?;
            }
            return Ok(Type::Struct(self.struct_identities[name].clone()));
        }
        if name == "Ask" {
            if !(1..=2).contains(&arguments.len()) {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    "`Ask` expects a prompt and an optional type (`Int`, `Float`, `String`, or `Bool`)",
                ));
            }
            let prompt_type = self.analyze_expression(&arguments[0])?;
            self.require_type(&Type::String, &prompt_type)?;
            return match arguments.get(1) {
                None => Ok(Type::String),
                Some(Expr::Identifier(type_name)) => match type_name.as_str() {
                    "String" => Ok(Type::String),
                    "Int" => Ok(Type::Int),
                    "Float" => Ok(Type::Float),
                    "Bool" => Ok(Type::Bool),
                    _ => Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!(
                            "unsupported `Ask` type `{type_name}`; expected `Int`, `Float`, `String`, or `Bool`"
                        ),
                    )),
                },
                Some(_) => Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    "`Ask` type must be `Int`, `Float`, `String`, or `Bool`",
                )),
            };
        }
        let argument_types = arguments
            .iter()
            .map(|argument| self.analyze_expression(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let user_function = self
            .function_scopes
            .iter()
            .rev()
            .any(|scope| scope.contains_key(name))
            || matches!(self.variables.get(name), Some(Type::Function { .. }));
        if name == "enumerate" && !user_function {
            self.expect_count(name, &argument_types, 1)?;
            let element_type = match &argument_types[0] {
                Type::Array(element) | Type::List(element) => (**element).clone(),
                Type::Range => Type::Int,
                Type::String => Type::String,
                Type::Tuple(elements) => elements
                    .iter()
                    .cloned()
                    .reduce(|current, next| Self::merge_collection_type(&current, &next))
                    .unwrap_or(Type::Unknown),
                Type::Unknown => Type::Unknown,
                other => {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`enumerate` requires a sequence or string, found {}",
                            other.name()
                        ),
                    ));
                }
            };
            return Ok(Type::Array(Box::new(Type::Tuple(vec![
                Type::Int,
                element_type,
            ]))));
        }
        if name == "zip" && !user_function {
            self.expect_count(name, &argument_types, 2)?;
            let element_type = |typ: &Type| match typ {
                Type::Array(element) | Type::List(element) => Some((**element).clone()),
                Type::Range => Some(Type::Int),
                Type::String => Some(Type::String),
                Type::Tuple(elements) => Some(
                    elements
                        .iter()
                        .cloned()
                        .reduce(|current, next| Self::merge_collection_type(&current, &next))
                        .unwrap_or(Type::Unknown),
                ),
                Type::Unknown => Some(Type::Unknown),
                _ => None,
            };
            let left = element_type(&argument_types[0]).ok_or_else(|| {
                self.error(
                    DiagnosticCode::SemanticCollection,
                    format!(
                        "`zip` requires sequences or strings, found {}",
                        argument_types[0].name()
                    ),
                )
            })?;
            let right = element_type(&argument_types[1]).ok_or_else(|| {
                self.error(
                    DiagnosticCode::SemanticCollection,
                    format!(
                        "`zip` requires sequences or strings, found {}",
                        argument_types[1].name()
                    ),
                )
            })?;
            return Ok(Type::Array(Box::new(Type::Tuple(vec![left, right]))));
        }
        match name {
            "print" => {
                self.expect_count(name, &argument_types, 1)?;
                Ok(Type::Unit)
            }
            "assert" => {
                if !(1..=2).contains(&argument_types.len()) {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        "`assert` expects a condition and an optional message",
                    ));
                }
                self.require_type(&Type::Bool, &argument_types[0])?;
                if let Some(message_type) = argument_types.get(1) {
                    self.require_type(&Type::String, message_type)?;
                }
                Ok(Type::Unit)
            }
            "type_of" => {
                self.expect_count(name, &argument_types, 1)?;
                Ok(Type::String)
            }
            "length" | "count" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_collection_or_string(&argument_types[0])?;
                Ok(Type::Int)
            }
            "contains" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_collection_or_string(&argument_types[0])?;
                if matches!(argument_types[0], Type::String) {
                    self.require_type(&Type::String, &argument_types[1])?;
                }
                Ok(Type::Bool)
            }
            "has_key" if !user_function => {
                self.expect_count(name, &argument_types, 2)?;
                if !matches!(
                    argument_types[0],
                    Type::Hash | Type::HashValues(_) | Type::Tree | Type::TreeValues(_)
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`has_key` requires a Hash or Tree, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::Bool)
            }
            "get" if !user_function => {
                self.expect_count(name, &argument_types, 3)?;
                let value_type = match &argument_types[0] {
                    Type::HashValues(value_type) | Type::TreeValues(value_type) => {
                        (**value_type).clone()
                    }
                    Type::Hash | Type::Tree | Type::Unknown => Type::Unknown,
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!(
                                "`get` requires a Hash or Tree, found {}",
                                argument_types[0].name()
                            ),
                        ));
                    }
                };
                self.require_type(&Type::String, &argument_types[1])?;
                if value_type == Type::Unknown {
                    Ok(argument_types[2].clone())
                } else {
                    self.require_type(&value_type, &argument_types[2])?;
                    Ok(value_type)
                }
            }
            "without_key" if !user_function => {
                self.expect_count(name, &argument_types, 2)?;
                if !matches!(
                    argument_types[0],
                    Type::Hash | Type::HashValues(_) | Type::Tree | Type::TreeValues(_)
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`without_key` requires a Hash or Tree, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(argument_types[0].clone())
            }
            "select_keys" if !user_function => {
                self.expect_count(name, &argument_types, 2)?;
                if !matches!(
                    argument_types[0],
                    Type::Hash | Type::HashValues(_) | Type::Tree | Type::TreeValues(_)
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`select_keys` requires a Hash or Tree, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                match &argument_types[1] {
                    Type::Array(element) | Type::List(element) => {
                        self.require_type(&Type::String, element)?;
                    }
                    Type::Tuple(elements) => {
                        for element in elements {
                            self.require_type(&Type::String, element)?;
                        }
                    }
                    Type::Unknown => {}
                    other => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!(
                                "`select_keys` expects an array, list, or tuple of strings, found {}",
                                other.name()
                            ),
                        ));
                    }
                }
                Ok(argument_types[0].clone())
            }
            "keys" if !user_function => {
                self.expect_count(name, &argument_types, 1)?;
                if !matches!(
                    argument_types[0],
                    Type::Hash | Type::HashValues(_) | Type::Tree | Type::TreeValues(_)
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`keys` requires a Hash or Tree, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                Ok(Type::Array(Box::new(Type::String)))
            }
            "values" if !user_function => {
                self.expect_count(name, &argument_types, 1)?;
                let value_type = match &argument_types[0] {
                    Type::HashValues(value_type) | Type::TreeValues(value_type) => {
                        (**value_type).clone()
                    }
                    Type::Hash | Type::Tree | Type::Unknown => Type::Unknown,
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!(
                                "`values` requires a Hash or Tree, found {}",
                                argument_types[0].name()
                            ),
                        ));
                    }
                };
                Ok(Type::Array(Box::new(value_type)))
            }
            "entries" if !user_function => {
                self.expect_count(name, &argument_types, 1)?;
                let value_type = match &argument_types[0] {
                    Type::HashValues(value_type) | Type::TreeValues(value_type) => {
                        (**value_type).clone()
                    }
                    Type::Hash | Type::Tree | Type::Unknown => Type::Unknown,
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!(
                                "`entries` requires a Hash or Tree, found {}",
                                argument_types[0].name()
                            ),
                        ));
                    }
                };
                Ok(Type::Array(Box::new(Type::Tuple(vec![
                    Type::String,
                    value_type,
                ]))))
            }
            "any" | "all" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_boolean_iterable(&argument_types[0])?;
                Ok(Type::Bool)
            }
            "join" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_string_iterable(&argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::String)
            }
            "total" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric_iterable(&argument_types[0])?;
                Ok(self.numeric_sum_type(&argument_types[0]))
            }
            "trim" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::String)
            }
            "read_file" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::String)
            }
            "write_file" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::Unit)
            }
            "substring" => {
                self.expect_count(name, &argument_types, 3)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::Int, &argument_types[1])?;
                self.require_type(&Type::Int, &argument_types[2])?;
                Ok(Type::String)
            }
            "characters" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Array(Box::new(Type::String)))
            }
            "is_ascii_alpha" | "is_ascii_digit" | "is_whitespace" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Bool)
            }
            "to_float" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                if let Expr::Literal(Literal::String(value)) = &arguments[0]
                    && !matches!(
                        value.trim().parse::<f64>(),
                        Ok(number) if number.is_finite()
                    )
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidNumericLiteral,
                        format!("cannot convert `{value}` to a finite Float"),
                    ));
                }
                Ok(Type::Float)
            }
            "to_int" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                if let Expr::Literal(Literal::String(value)) = &arguments[0]
                    && value.trim().parse::<i64>().is_err()
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidNumericLiteral,
                        format!("cannot convert `{value}` to an Int"),
                    ));
                }
                Ok(Type::Int)
            }
            "abs" | "round" => {
                self.expect_count(name, &argument_types, if name == "round" { 2 } else { 1 })?;
                self.require_numeric(&argument_types[0])?;
                if name == "round" {
                    self.require_type(&Type::Int, &argument_types[1])?;
                    if let Expr::Literal(Literal::Int(decimals)) = arguments[1]
                        && !(0..=15).contains(&decimals)
                    {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            "`round` decimal count must be an integer from 0 to 15",
                        ));
                    }
                }
                Ok(argument_types[0].clone())
            }
            "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan" | "floor" | "ceil" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric(&argument_types[0])?;
                Ok(Type::Float)
            }
            "sign" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric(&argument_types[0])?;
                Ok(Type::Int)
            }
            "pow" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric(&argument_types[0])?;
                self.require_numeric(&argument_types[1])?;
                Ok(Type::Float)
            }
            "vector_add" | "vector_subtract" => {
                self.expect_count(name, &argument_types, 2)?;
                let left = self.numeric_sequence_element(&argument_types[0])?;
                let right = self.numeric_sequence_element(&argument_types[1])?;
                let element = self.numeric_result(&left, &right)?;
                Ok(Type::Array(Box::new(element)))
            }
            "vector_scale" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric(&argument_types[1])?;
                let element = self.numeric_sequence_element(&argument_types[0])?;
                Ok(Type::Array(Box::new(
                    self.numeric_result(&element, &argument_types[1])?,
                )))
            }
            "dot" => {
                self.expect_count(name, &argument_types, 2)?;
                let left = self.numeric_sequence_element(&argument_types[0])?;
                let right = self.numeric_sequence_element(&argument_types[1])?;
                self.numeric_result(&left, &right)
            }
            "norm" => {
                self.expect_count(name, &argument_types, 1)?;
                self.numeric_sequence_element(&argument_types[0])?;
                Ok(Type::Float)
            }
            "distance" => {
                self.expect_count(name, &argument_types, 2)?;
                self.numeric_sequence_element(&argument_types[0])?;
                self.numeric_sequence_element(&argument_types[1])?;
                Ok(Type::Float)
            }
            "normalize" => {
                self.expect_count(name, &argument_types, 1)?;
                self.numeric_sequence_element(&argument_types[0])?;
                Ok(Type::Array(Box::new(Type::Float)))
            }
            "shape" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                Ok(Type::Tuple(vec![Type::Int, Type::Int]))
            }
            "transpose" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                Ok(Type::Matrix)
            }
            "matrix_add" | "matrix_subtract" | "multiply" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_matrix(&argument_types[0])?;
                self.require_matrix(&argument_types[1])?;
                Ok(Type::Matrix)
            }
            "matrix_scale" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_matrix(&argument_types[0])?;
                self.require_numeric(&argument_types[1])?;
                Ok(Type::Matrix)
            }
            "identity" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::Int, &argument_types[0])?;
                Ok(Type::Matrix)
            }
            "mean" | "median" | "variance" | "stddev" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric_iterable(&argument_types[0])?;
                Ok(Type::Float)
            }
            "percentile" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric_iterable(&argument_types[0])?;
                self.require_numeric(&argument_types[1])?;
                Ok(Type::Float)
            }
            "covariance" | "correlation" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric_iterable(&argument_types[0])?;
                self.require_numeric_iterable(&argument_types[1])?;
                Ok(Type::Float)
            }
            "clamp" => {
                self.expect_count(name, &argument_types, 3)?;
                for argument in &argument_types {
                    self.require_numeric(argument)?;
                }
                if argument_types.contains(&Type::Unknown) {
                    Ok(Type::Unknown)
                } else if argument_types.contains(&Type::Float) {
                    Ok(Type::Float)
                } else {
                    Ok(Type::Int)
                }
            }
            "split" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::List(Box::new(Type::String)))
            }
            "csv_rows" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::CsvStream)
            }
            "csv_row" => Ok(Type::List(Box::new(Type::Unknown))),
            "replace" => {
                self.expect_count(name, &argument_types, 3)?;
                for argument in &argument_types {
                    self.require_type(&Type::String, argument)?;
                }
                Ok(Type::String)
            }
            "starts_with" | "ends_with" => {
                self.expect_count(name, &argument_types, 2)?;
                for argument in &argument_types {
                    self.require_type(&Type::String, argument)?;
                }
                Ok(Type::Bool)
            }
            "is_empty" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_collection_or_string(&argument_types[0])?;
                Ok(Type::Bool)
            }
            "reverse" => {
                self.expect_count(name, &argument_types, 1)?;
                if !matches!(
                    argument_types[0],
                    Type::Array(_) | Type::List(_) | Type::Tuple(_) | Type::Range | Type::Unknown
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`reverse` requires a sequence, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                Ok(if argument_types[0] == Type::Range {
                    Type::Array(Box::new(Type::Int))
                } else {
                    argument_types[0].clone()
                })
            }
            "range" => {
                if !(2..=3).contains(&argument_types.len()) {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        "`range` expects start, end, and an optional step",
                    ));
                }
                for argument in &argument_types {
                    self.require_type(&Type::Int, argument)?;
                }
                if matches!(arguments.get(2), Some(Expr::Literal(Literal::Int(0)))) {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        "`range` step cannot be zero",
                    ));
                }
                Ok(Type::Range)
            }
            _ => {
                let Some(function) = self
                    .function_scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(name))
                    .cloned()
                else {
                    if let Some(Type::Function {
                        parameters,
                        return_type,
                    }) = self.variables.get(name)
                    {
                        if parameters.len() != argument_types.len() {
                            return Err(self.error(
                                DiagnosticCode::InvalidFunctionCall,
                                format!(
                                    "function `{name}` expects {} arguments, got {}",
                                    parameters.len(),
                                    argument_types.len()
                                ),
                            ));
                        }
                        for (expected, actual) in parameters.iter().zip(&argument_types) {
                            if let Some(expected) = expected {
                                self.require_type(expected, actual)?;
                            }
                        }
                        return Ok(return_type.as_deref().cloned().unwrap_or(Type::Unknown));
                    }
                    if matches!(self.variables.get(name), Some(&Type::Unknown)) {
                        return Ok(Type::Unknown);
                    }
                    if name.chars().next().is_some_and(char::is_uppercase) {
                        return Err(self.error(
                            DiagnosticCode::UndefinedVariable,
                            format!("unknown struct type `{name}`"),
                        ));
                    }
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!("unknown function `{name}`"),
                    ));
                };
                if function.parameters.len() != argument_types.len() {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!(
                            "function `{name}` expects {} arguments, got {}",
                            function.parameters.len(),
                            argument_types.len()
                        ),
                    ));
                }
                for (expected, actual) in function.parameters.iter().zip(argument_types.iter()) {
                    if let Some(expected) = expected {
                        self.require_type(expected, actual)?;
                    }
                }
                Ok(function.return_type.unwrap_or(Type::Unit))
            }
        }
    }

    fn message_type(
        &mut self,
        receiver: &Expr,
        message: &str,
        arguments: &[Expr],
    ) -> Result<Type, SimplyError> {
        let receiver_type = self.analyze_expression(receiver)?;
        if matches!(receiver_type, Type::Enum(_)) {
            return Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                "enum values do not support message dispatch",
            ));
        }
        let argument_types = arguments
            .iter()
            .map(|argument| self.analyze_expression(argument))
            .collect::<Result<Vec<_>, _>>()?;
        if let Type::Struct(type_identity) = &receiver_type {
            let Some(signature) = self
                .messages
                .get(&(type_identity.clone(), message.to_owned()))
            else {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "message `{message}` is not defined for `{}`",
                        type_identity.local_name
                    ),
                ));
            };
            if signature.parameters.len() != argument_types.len() {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "message `{message}` expects {} arguments, got {}",
                        signature.parameters.len(),
                        argument_types.len()
                    ),
                ));
            }
            for (expected, actual) in signature.parameters.iter().zip(&argument_types) {
                if let Some(expected) = expected {
                    self.require_type(expected, actual)?;
                }
            }
            return Ok(signature.return_type.clone().unwrap_or(Type::Unknown));
        }
        let (parameters, return_type) = if let Some(function) = self
            .function_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(message))
        {
            (
                function.parameters.clone(),
                function.return_type.clone().unwrap_or(Type::Unit),
            )
        } else if let Some(Type::Function {
            parameters,
            return_type,
        }) = self.variables.get(message)
        {
            (
                parameters
                    .iter()
                    .map(|parameter| parameter.as_deref().cloned())
                    .collect(),
                return_type.as_deref().cloned().unwrap_or(Type::Unknown),
            )
        } else if receiver_type == Type::Unknown
            || matches!(self.variables.get(message), Some(&Type::Unknown))
        {
            return Ok(Type::Unknown);
        } else {
            return Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                format!(
                    "unknown message `{message}` for a {} receiver",
                    receiver_type.name()
                ),
            ));
        };
        let actual_types = std::iter::once(&receiver_type)
            .chain(argument_types.iter())
            .collect::<Vec<_>>();
        if parameters.len() != actual_types.len() {
            return Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                format!(
                    "message `{message}` expects {} arguments, got {}",
                    parameters.len(),
                    actual_types.len()
                ),
            ));
        }
        for (expected, actual) in parameters.iter().zip(actual_types) {
            if let Some(expected) = expected {
                self.require_type(expected, actual)?;
            }
        }
        Ok(return_type)
    }

    fn enum_variant_type(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        arguments: &[Expr],
    ) -> Result<Type, SimplyError> {
        let Some(variants) = self.enums.get(enum_name).cloned() else {
            return Err(self.error(
                DiagnosticCode::UndefinedVariable,
                format!("unknown enum type `{enum_name}`"),
            ));
        };
        let Some(variant) = variants.iter().find(|variant| variant.name == variant_name) else {
            return Err(self.error(
                DiagnosticCode::UndefinedVariable,
                format!("unknown variant `{enum_name}::{variant_name}`"),
            ));
        };
        match (&variant.payload_type, arguments) {
            (Some(payload_type), [payload]) => {
                let actual = self.analyze_expression(payload)?;
                self.require_type(payload_type, &actual)?;
            }
            (Some(payload_type), _) => {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "variant `{enum_name}::{variant_name}` expects a payload of type {}",
                        payload_type.name()
                    ),
                ));
            }
            (None, []) => {}
            (None, _) => {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!("unit variant `{enum_name}::{variant_name}` does not accept a payload"),
                ));
            }
        }
        Ok(Type::Enum(self.enum_identities[enum_name].clone()))
    }

    fn match_type(
        &mut self,
        value: &Expr,
        arms: &[crate::ast::MatchArm],
    ) -> Result<Type, SimplyError> {
        let value_type = self.analyze_expression(value)?;
        let mut coverage_matrix = Vec::<Vec<MatchPattern>>::new();
        let mut result_type: Option<Type> = None;

        for arm in arms {
            let mut bindings = Vec::new();
            self.validate_match_pattern(&arm.pattern, &value_type, &mut bindings)?;
            if !self.pattern_is_useful(&coverage_matrix, &arm.pattern, &value_type) {
                return Err(
                    self.error(DiagnosticCode::UnexpectedToken, "unreachable match pattern")
                );
            }
            if arm.guard.is_none() {
                coverage_matrix.push(vec![arm.pattern.clone()]);
            }

            let frame_start = self.variables.scopes.len();
            self.variables.push();
            self.function_scopes.push(HashMap::new());
            let saved_saw_return = self.saw_return;
            let saved_inferred_return = self.inferred_return.clone();
            let saved_module_return_type = self.module_return_type.clone();
            let saved_loop_depth = self.loop_depth;
            self.saw_return = false;
            self.loop_depth = 0;
            let branch_result = (|| {
                for (binding, binding_type) in bindings {
                    self.define_variable(binding, binding_type, false)?;
                }
                if let Some(guard) = &arm.guard {
                    let guard_type = self.analyze_expression(guard)?;
                    self.require_type(&Type::Bool, &guard_type)?;
                }
                self.collect_functions(&arm.body)?;
                self.analyze_statements(&arm.body)?;
                if self.saw_return {
                    Ok(if self.function_depth == 0 {
                        self.module_return_type.clone()
                    } else {
                        self.inferred_return.clone()
                    }
                    .unwrap_or(Type::Unit))
                } else {
                    arm.result
                        .as_ref()
                        .map(|result| self.analyze_expression(result))
                        .unwrap_or(Ok(Type::Unit))
                }
            })();
            self.variables.truncate(frame_start);
            self.function_scopes.pop();
            self.loop_depth = saved_loop_depth;
            self.saw_return = saved_saw_return;
            self.inferred_return = saved_inferred_return;
            self.module_return_type = saved_module_return_type;
            let branch_type = branch_result?;
            match &result_type {
                None => result_type = Some(branch_type),
                Some(expected) if *expected == Type::Unknown => result_type = Some(branch_type),
                Some(_) if branch_type == Type::Unknown => {}
                Some(expected) => self.require_type(expected, &branch_type)?,
            }
        }

        if self.pattern_is_useful(&coverage_matrix, &MatchPattern::Wildcard, &value_type) {
            if let Type::Enum(enum_identity) = &value_type
                && let Some(enum_name) = self.enum_name_for_identity(enum_identity)
                && let Some(variants) = self.enums.get(enum_name)
            {
                for variant in variants {
                    let pattern = MatchPattern::EnumVariant {
                        enum_name: enum_name.clone(),
                        variant_name: variant.name.clone(),
                        payload: variant
                            .payload_type
                            .as_ref()
                            .map(|_| Box::new(MatchPattern::Wildcard)),
                    };
                    if self.pattern_is_useful(&coverage_matrix, &pattern, &value_type) {
                        let constructor = CoverageConstructor::Enum {
                            type_identity: enum_identity.clone(),
                            variant: variant.name.clone(),
                        };
                        let has_variant_arm = coverage_matrix.iter().any(|row| {
                            row.first()
                                .and_then(|pattern| self.pattern_constructor(pattern))
                                .is_some_and(|(found, _)| found == constructor)
                        });
                        if !has_variant_arm {
                            return Err(self.error(
                                DiagnosticCode::UnexpectedToken,
                                format!("missing variant `{}::{}`", enum_name, variant.name),
                            ));
                        }
                        break;
                    }
                }
            }
            return Err(self.error(
                DiagnosticCode::TypeMismatch,
                format!(
                    "non-exhaustive match: value of type {} is not fully covered",
                    value_type.name()
                ),
            ));
        }

        Ok(result_type.unwrap_or(Type::Unit))
    }

    fn pattern_is_useful(
        &self,
        matrix: &[Vec<MatchPattern>],
        pattern: &MatchPattern,
        typ: &Type,
    ) -> bool {
        self.pattern_vector_is_useful(
            matrix,
            std::slice::from_ref(pattern),
            std::slice::from_ref(typ),
        )
    }

    fn pattern_contains_alias(pattern: &MatchPattern) -> bool {
        match pattern {
            MatchPattern::Alias { .. } => true,
            MatchPattern::Or(patterns)
            | MatchPattern::Tuple(patterns)
            | MatchPattern::Struct {
                fields: patterns, ..
            } => patterns.iter().any(Self::pattern_contains_alias),
            MatchPattern::NamedStruct { fields, .. } => fields
                .iter()
                .any(|(_, pattern)| Self::pattern_contains_alias(pattern)),
            MatchPattern::Sequence { patterns, .. } => {
                patterns.iter().any(Self::pattern_contains_alias)
            }
            MatchPattern::Hash(entries) => entries
                .iter()
                .any(|(_, pattern)| Self::pattern_contains_alias(pattern)),
            MatchPattern::EnumVariant {
                payload: Some(pattern),
                ..
            } => Self::pattern_contains_alias(pattern),
            MatchPattern::Identifier(_)
            | MatchPattern::Literal(_)
            | MatchPattern::Range { .. }
            | MatchPattern::EnumVariant { payload: None, .. }
            | MatchPattern::Wildcard => false,
        }
    }

    fn without_aliases(pattern: &MatchPattern) -> MatchPattern {
        match pattern {
            MatchPattern::Alias { pattern, .. } => Self::without_aliases(pattern),
            MatchPattern::Or(patterns) => {
                MatchPattern::Or(patterns.iter().map(Self::without_aliases).collect())
            }
            MatchPattern::Tuple(patterns) => {
                MatchPattern::Tuple(patterns.iter().map(Self::without_aliases).collect())
            }
            MatchPattern::Sequence { patterns, rest } => MatchPattern::Sequence {
                patterns: patterns.iter().map(Self::without_aliases).collect(),
                rest: rest.clone(),
            },
            MatchPattern::Hash(entries) => MatchPattern::Hash(
                entries
                    .iter()
                    .map(|(key, pattern)| (key.clone(), Self::without_aliases(pattern)))
                    .collect(),
            ),
            MatchPattern::EnumVariant {
                enum_name,
                variant_name,
                payload,
            } => MatchPattern::EnumVariant {
                enum_name: enum_name.clone(),
                variant_name: variant_name.clone(),
                payload: payload
                    .as_ref()
                    .map(|pattern| Box::new(Self::without_aliases(pattern))),
            },
            MatchPattern::Struct { type_name, fields } => MatchPattern::Struct {
                type_name: type_name.clone(),
                fields: fields.iter().map(Self::without_aliases).collect(),
            },
            MatchPattern::NamedStruct { type_name, fields } => MatchPattern::NamedStruct {
                type_name: type_name.clone(),
                fields: fields
                    .iter()
                    .map(|(name, pattern)| (name.clone(), Self::without_aliases(pattern)))
                    .collect(),
            },
            MatchPattern::Identifier(_)
            | MatchPattern::Literal(_)
            | MatchPattern::Range { .. }
            | MatchPattern::Wildcard => pattern.clone(),
        }
    }

    fn pattern_vector_is_useful(
        &self,
        matrix: &[Vec<MatchPattern>],
        candidate: &[MatchPattern],
        types: &[Type],
    ) -> bool {
        if candidate.iter().any(Self::pattern_contains_alias)
            || matrix.iter().flatten().any(Self::pattern_contains_alias)
        {
            let normalized_matrix = matrix
                .iter()
                .map(|row| row.iter().map(Self::without_aliases).collect())
                .collect::<Vec<Vec<_>>>();
            let normalized_candidate = candidate
                .iter()
                .map(Self::without_aliases)
                .collect::<Vec<_>>();
            return self.pattern_vector_is_useful(&normalized_matrix, &normalized_candidate, types);
        }

        if candidate.is_empty() {
            return matrix.is_empty();
        }
        let Some((head, tail)) = candidate.split_first() else {
            return matrix.is_empty();
        };
        let Some(typ) = types.first() else {
            return false;
        };
        let rest_types = &types[1..];

        if let MatchPattern::Or(alternatives) = head {
            let mut alternatives_matrix = matrix.to_vec();
            let mut useful = false;
            for alternative in alternatives {
                let mut alternative_candidate = vec![alternative.clone()];
                alternative_candidate.extend_from_slice(tail);
                useful |= self.pattern_vector_is_useful(
                    &alternatives_matrix,
                    &alternative_candidate,
                    types,
                );
                alternatives_matrix.push(alternative_candidate);
            }
            return useful;
        }

        if typ == &Type::Int {
            let intervals = self.int_pattern_intervals(head);
            if !intervals.is_empty() {
                return self.int_pattern_vector_is_useful(matrix, &intervals, tail, rest_types);
            }
        }
        if typ == &Type::Unknown
            && (!self.int_pattern_intervals(head).is_empty())
            && (matches!(head, MatchPattern::Range { .. })
                || matches!(head, MatchPattern::Literal(Literal::Int(_))))
        {
            return self.int_pattern_vector_is_useful(
                matrix,
                &self.int_pattern_intervals(head),
                tail,
                rest_types,
            );
        }

        if let MatchPattern::Literal(literal) = head
            && (!matches!(literal, Literal::Bool(_)) || !matches!(typ, Type::Bool))
        {
            let specialized = self.specialize_literal_matrix(matrix, literal);
            return self.pattern_vector_is_useful(&specialized, tail, rest_types);
        }

        let sequence_element_type = match typ {
            Type::Array(element) | Type::List(element) => Some((**element).clone()),
            Type::CsvStream => Some(Type::List(Box::new(Type::String))),
            Type::Unknown if matches!(head, MatchPattern::Sequence { .. }) => Some(Type::Unknown),
            _ => None,
        };
        if let Some(element_type) = sequence_element_type {
            if let MatchPattern::Sequence { patterns, rest } = head {
                return self.sequence_pattern_vector_is_useful(
                    matrix,
                    patterns,
                    rest.is_some(),
                    tail,
                    rest_types,
                    &element_type,
                );
            }
            if matches!(head, MatchPattern::Wildcard | MatchPattern::Identifier(_)) {
                let max_length = self.max_sequence_prefix_length(matrix, 0);
                return (0..=max_length + 1).any(|length| {
                    let candidate = vec![MatchPattern::Wildcard; length];
                    self.sequence_pattern_vector_is_useful(
                        matrix,
                        &candidate,
                        false,
                        tail,
                        rest_types,
                        &element_type,
                    )
                });
            }
        }

        if matches!(typ, Type::Hash | Type::HashValues(_) | Type::Unknown) {
            if let MatchPattern::Hash(entries) = head {
                let candidate = self.expand_hash_pattern(entries, tail);
                let specialized = matrix
                    .iter()
                    .flat_map(|row| self.expand_hash_row(row, entries))
                    .collect::<Vec<_>>();
                let mut candidate_types = vec![Type::Unknown; entries.len()];
                candidate_types.extend_from_slice(rest_types);
                return self.pattern_vector_is_useful(&specialized, &candidate, &candidate_types);
            }
            if matches!(head, MatchPattern::Wildcard | MatchPattern::Identifier(_))
                && matches!(typ, Type::Hash | Type::HashValues(_))
            {
                let defaults = self.hash_default_matrix(matrix);
                return self.pattern_vector_is_useful(&defaults, tail, rest_types);
            }
        }

        if let Some((constructor, arguments)) = self.pattern_constructor(head) {
            let Some(argument_types) = self.constructor_argument_types(&constructor, typ) else {
                return false;
            };
            let specialized = self.specialize_matrix(matrix, &constructor, arguments.len());
            let mut specialized_candidate = arguments;
            specialized_candidate.extend_from_slice(tail);
            let mut specialized_types = argument_types;
            specialized_types.extend_from_slice(rest_types);
            return self.pattern_vector_is_useful(
                &specialized,
                &specialized_candidate,
                &specialized_types,
            );
        }

        if let Some(constructors) = self.finite_constructors(typ) {
            let complete = constructors.iter().all(|(constructor, _)| {
                matrix.iter().any(|row| {
                    row.first().is_some_and(|pattern| {
                        self.pattern_may_match_constructor(pattern, constructor)
                    })
                })
            });
            if complete {
                for (constructor, argument_types) in constructors {
                    let arity = argument_types.len();
                    let specialized = self.specialize_matrix(matrix, &constructor, arity);
                    let mut specialized_candidate = vec![MatchPattern::Wildcard; arity];
                    specialized_candidate.extend_from_slice(tail);
                    let mut specialized_types = argument_types;
                    specialized_types.extend_from_slice(rest_types);
                    if self.pattern_vector_is_useful(
                        &specialized,
                        &specialized_candidate,
                        &specialized_types,
                    ) {
                        return true;
                    }
                }
                false
            } else {
                let defaults = self.default_matrix(matrix);
                self.pattern_vector_is_useful(&defaults, tail, rest_types)
            }
        } else {
            let defaults = self.default_matrix(matrix);
            self.pattern_vector_is_useful(&defaults, tail, rest_types)
        }
    }

    fn int_pattern_intervals(&self, pattern: &MatchPattern) -> Vec<(i128, i128)> {
        match pattern {
            MatchPattern::Alias { pattern, .. } => self.int_pattern_intervals(pattern),
            MatchPattern::Literal(Literal::Int(value)) => {
                let value = i128::from(*value);
                vec![(value, value)]
            }
            MatchPattern::Range { start, end } => {
                let start = match start {
                    Some(Literal::Int(value)) => i128::from(*value),
                    Some(_) => return Vec::new(),
                    None => i128::from(i64::MIN),
                };
                let end = match end {
                    Some(Literal::Int(value)) => i128::from(*value),
                    Some(_) => return Vec::new(),
                    None => i128::from(i64::MAX),
                };
                (start <= end).then_some((start, end)).into_iter().collect()
            }
            MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                vec![(i128::from(i64::MIN), i128::from(i64::MAX))]
            }
            MatchPattern::Or(alternatives) => alternatives
                .iter()
                .flat_map(|alternative| self.int_pattern_intervals(alternative))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn int_pattern_vector_is_useful(
        &self,
        matrix: &[Vec<MatchPattern>],
        candidate_intervals: &[(i128, i128)],
        tail: &[MatchPattern],
        rest_types: &[Type],
    ) -> bool {
        let mut boundaries = Vec::new();
        for (start, end) in candidate_intervals {
            boundaries.push(*start);
            boundaries.push(*end + 1);
        }
        for row in matrix {
            let Some((head, _)) = row.split_first() else {
                continue;
            };
            for (start, end) in self.int_pattern_intervals(head) {
                boundaries.push(start);
                boundaries.push(end + 1);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        for window in boundaries.windows(2) {
            let start = window[0];
            let end = window[1] - 1;
            if !candidate_intervals
                .iter()
                .any(|(lower, upper)| *lower <= start && end <= *upper)
            {
                continue;
            }

            let specialized = matrix
                .iter()
                .filter_map(|row| {
                    let (head, tail) = row.split_first()?;
                    self.int_pattern_intervals(head)
                        .iter()
                        .any(|(lower, upper)| *lower <= start && end <= *upper)
                        .then(|| tail.to_vec())
                })
                .collect::<Vec<_>>();
            if self.pattern_vector_is_useful(&specialized, tail, rest_types) {
                return true;
            }
        }
        false
    }

    fn pattern_constructor(
        &self,
        pattern: &MatchPattern,
    ) -> Option<(CoverageConstructor, Vec<MatchPattern>)> {
        match pattern {
            MatchPattern::Alias { pattern, .. } => self.pattern_constructor(pattern),
            MatchPattern::EnumVariant {
                enum_name,
                variant_name,
                payload,
            } => Some((
                CoverageConstructor::Enum {
                    type_identity: self.enum_identities.get(enum_name)?.clone(),
                    variant: variant_name.clone(),
                },
                payload
                    .iter()
                    .map(|pattern| pattern.as_ref().clone())
                    .collect(),
            )),
            MatchPattern::Tuple(patterns) => {
                Some((CoverageConstructor::Tuple(patterns.len()), patterns.clone()))
            }
            MatchPattern::Struct { type_name, fields } => Some((
                CoverageConstructor::Struct(self.struct_identities.get(type_name)?.clone()),
                fields.clone(),
            )),
            MatchPattern::NamedStruct { type_name, fields } => {
                let struct_fields = self.structs.get(type_name)?;
                let mut positional = vec![MatchPattern::Wildcard; struct_fields.len()];
                for (field_name, pattern) in fields {
                    let index = struct_fields
                        .iter()
                        .position(|field| field.name == *field_name)?;
                    positional[index] = pattern.clone();
                }
                Some((
                    CoverageConstructor::Struct(self.struct_identities.get(type_name)?.clone()),
                    positional,
                ))
            }
            MatchPattern::Literal(Literal::Bool(value)) => {
                Some((CoverageConstructor::Bool(*value), Vec::new()))
            }
            MatchPattern::Range { .. }
            | MatchPattern::Literal(_)
            | MatchPattern::Sequence { .. }
            | MatchPattern::Hash(_)
            | MatchPattern::Wildcard
            | MatchPattern::Identifier(_)
            | MatchPattern::Or(_) => None,
        }
    }

    fn pattern_may_match_constructor(
        &self,
        pattern: &MatchPattern,
        constructor: &CoverageConstructor,
    ) -> bool {
        match pattern {
            MatchPattern::Alias { pattern, .. } => {
                self.pattern_may_match_constructor(pattern, constructor)
            }
            MatchPattern::Or(alternatives) => alternatives
                .iter()
                .any(|alternative| self.pattern_may_match_constructor(alternative, constructor)),
            MatchPattern::Literal(Literal::Bool(value)) => {
                *constructor == CoverageConstructor::Bool(*value)
            }
            MatchPattern::Wildcard | MatchPattern::Identifier(_) => true,
            _ => self
                .pattern_constructor(pattern)
                .is_some_and(|(found, _)| found == *constructor),
        }
    }

    fn finite_constructors(&self, typ: &Type) -> Option<Vec<(CoverageConstructor, Vec<Type>)>> {
        match typ {
            Type::Enum(identity) => {
                let name = self.enum_name_for_identity(identity)?;
                Some(
                    self.enums
                        .get(name)?
                        .iter()
                        .map(|variant| {
                            (
                                CoverageConstructor::Enum {
                                    type_identity: identity.clone(),
                                    variant: variant.name.clone(),
                                },
                                variant.payload_type.iter().cloned().collect(),
                            )
                        })
                        .collect(),
                )
            }
            Type::Tuple(types) => Some(vec![(
                CoverageConstructor::Tuple(types.len()),
                types.clone(),
            )]),
            Type::Struct(identity) => {
                let name = self.struct_name_for_identity(identity)?;
                Some(vec![(
                    CoverageConstructor::Struct(identity.clone()),
                    self.structs
                        .get(name)?
                        .iter()
                        .map(|field| field.field_type.clone())
                        .collect(),
                )])
            }
            Type::Bool => Some(vec![
                (CoverageConstructor::Bool(true), Vec::new()),
                (CoverageConstructor::Bool(false), Vec::new()),
            ]),
            _ => None,
        }
    }

    fn specialize_literal_matrix(
        &self,
        matrix: &[Vec<MatchPattern>],
        literal: &Literal,
    ) -> Vec<Vec<MatchPattern>> {
        let mut specialized = Vec::new();
        for row in matrix {
            let Some((head, tail)) = row.split_first() else {
                continue;
            };
            match head {
                MatchPattern::Literal(found) if found == literal => {
                    specialized.push(tail.to_vec());
                }
                MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                    specialized.push(tail.to_vec());
                }
                MatchPattern::Or(alternatives) => {
                    for alternative in alternatives {
                        let expanded = std::iter::once(alternative.clone())
                            .chain(tail.iter().cloned())
                            .collect::<Vec<_>>();
                        specialized.extend(
                            self.specialize_literal_matrix(
                                std::slice::from_ref(&expanded),
                                literal,
                            ),
                        );
                    }
                }
                _ => {}
            }
        }
        specialized
    }

    fn constructor_argument_types(
        &self,
        constructor: &CoverageConstructor,
        typ: &Type,
    ) -> Option<Vec<Type>> {
        if typ == &Type::Unknown {
            return Some(match constructor {
                CoverageConstructor::Tuple(arity) => vec![Type::Unknown; *arity],
                CoverageConstructor::Struct(identity) => self
                    .structs
                    .get(self.struct_name_for_identity(identity)?)?
                    .iter()
                    .map(|field| field.field_type.clone())
                    .collect(),
                CoverageConstructor::Enum {
                    type_identity,
                    variant,
                } => self
                    .enums
                    .get(self.enum_name_for_identity(type_identity)?)?
                    .iter()
                    .find(|definition| definition.name == *variant)?
                    .payload_type
                    .iter()
                    .cloned()
                    .collect(),
                CoverageConstructor::Bool(_) => Vec::new(),
            });
        }
        self.finite_constructors(typ)?
            .into_iter()
            .find_map(|(found, arguments)| (found == *constructor).then_some(arguments))
    }

    fn sequence_pattern_vector_is_useful(
        &self,
        matrix: &[Vec<MatchPattern>],
        patterns: &[MatchPattern],
        has_rest: bool,
        tail: &[MatchPattern],
        rest_types: &[Type],
        element_type: &Type,
    ) -> bool {
        let max_length = self.max_sequence_prefix_length(matrix, patterns.len());
        let lengths = if has_rest {
            (patterns.len()..=max_length.max(patterns.len()) + 1).collect::<Vec<_>>()
        } else {
            vec![patterns.len()]
        };
        for length in lengths {
            let mut candidate = patterns.to_vec();
            candidate.resize(length, MatchPattern::Wildcard);
            if !has_rest && length != patterns.len() {
                continue;
            }
            candidate.extend_from_slice(tail);
            let mut specialized = Vec::new();
            for row in matrix {
                let Some((head, row_tail)) = row.split_first() else {
                    continue;
                };
                for mut expanded in self.expand_sequence_pattern(head, length) {
                    expanded.extend_from_slice(row_tail);
                    specialized.push(expanded);
                }
            }
            let mut types = vec![element_type.clone(); length];
            types.extend_from_slice(rest_types);
            if self.pattern_vector_is_useful(&specialized, &candidate, &types) {
                return true;
            }
        }
        false
    }

    fn expand_hash_pattern(
        &self,
        entries: &[(String, MatchPattern)],
        tail: &[MatchPattern],
    ) -> Vec<MatchPattern> {
        let mut expanded = entries
            .iter()
            .map(|(_, pattern)| pattern.clone())
            .collect::<Vec<_>>();
        expanded.extend_from_slice(tail);
        expanded
    }

    fn expand_hash_row(
        &self,
        row: &[MatchPattern],
        candidate_entries: &[(String, MatchPattern)],
    ) -> Vec<Vec<MatchPattern>> {
        let Some((head, tail)) = row.split_first() else {
            return Vec::new();
        };
        match head {
            MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                let mut expanded = vec![MatchPattern::Wildcard; candidate_entries.len()];
                expanded.extend_from_slice(tail);
                vec![expanded]
            }
            MatchPattern::Hash(entries)
                if entries
                    .iter()
                    .all(|(key, _)| candidate_entries.iter().any(|(found, _)| found == key)) =>
            {
                let mut expanded = candidate_entries
                    .iter()
                    .map(|(key, _)| {
                        entries
                            .iter()
                            .find(|(found, _)| found == key)
                            .map(|(_, pattern)| pattern.clone())
                            .unwrap_or(MatchPattern::Wildcard)
                    })
                    .collect::<Vec<_>>();
                expanded.extend_from_slice(tail);
                vec![expanded]
            }
            MatchPattern::Or(alternatives) => alternatives
                .iter()
                .flat_map(|alternative| {
                    self.expand_hash_row(
                        &std::iter::once(alternative.clone())
                            .chain(tail.iter().cloned())
                            .collect::<Vec<_>>(),
                        candidate_entries,
                    )
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn hash_default_matrix(&self, matrix: &[Vec<MatchPattern>]) -> Vec<Vec<MatchPattern>> {
        let mut defaults = Vec::new();
        for row in matrix {
            let Some((head, tail)) = row.split_first() else {
                continue;
            };
            match head {
                MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                    defaults.push(tail.to_vec())
                }
                MatchPattern::Hash(entries) if entries.is_empty() => defaults.push(tail.to_vec()),
                MatchPattern::Or(alternatives)
                    if alternatives.iter().any(|alternative| {
                        matches!(
                            alternative,
                            MatchPattern::Wildcard | MatchPattern::Identifier(_)
                        ) || matches!(alternative, MatchPattern::Hash(entries) if entries.is_empty())
                    }) =>
                {
                    defaults.push(tail.to_vec());
                }
                _ => {}
            }
        }
        defaults
    }

    fn max_sequence_prefix_length(&self, matrix: &[Vec<MatchPattern>], current: usize) -> usize {
        matrix.iter().fold(current, |maximum, row| {
            row.first().map_or(maximum, |pattern| {
                self.sequence_pattern_prefix_max(pattern, maximum)
            })
        })
    }

    fn sequence_pattern_prefix_max(&self, pattern: &MatchPattern, current: usize) -> usize {
        match pattern {
            MatchPattern::Sequence { patterns, .. } => current.max(patterns.len()),
            MatchPattern::Or(alternatives) => {
                alternatives.iter().fold(current, |maximum, pattern| {
                    self.sequence_pattern_prefix_max(pattern, maximum)
                })
            }
            _ => current,
        }
    }

    fn expand_sequence_pattern(
        &self,
        pattern: &MatchPattern,
        length: usize,
    ) -> Vec<Vec<MatchPattern>> {
        match pattern {
            MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                vec![vec![MatchPattern::Wildcard; length]]
            }
            MatchPattern::Sequence { patterns, rest } => {
                if patterns.len() > length || (rest.is_none() && patterns.len() != length) {
                    return Vec::new();
                }
                let mut expanded = patterns.clone();
                expanded.resize(length, MatchPattern::Wildcard);
                vec![expanded]
            }
            MatchPattern::Or(alternatives) => alternatives
                .iter()
                .flat_map(|alternative| self.expand_sequence_pattern(alternative, length))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn specialize_matrix(
        &self,
        matrix: &[Vec<MatchPattern>],
        constructor: &CoverageConstructor,
        arity: usize,
    ) -> Vec<Vec<MatchPattern>> {
        let mut specialized = Vec::new();
        for row in matrix {
            let Some((head, tail)) = row.split_first() else {
                continue;
            };
            match self.pattern_constructor(head) {
                Some((found, arguments)) if found == *constructor => {
                    let mut new_row = arguments;
                    new_row.extend_from_slice(tail);
                    specialized.push(new_row);
                }
                None => {
                    if matches!(head, MatchPattern::Wildcard | MatchPattern::Identifier(_)) {
                        let mut new_row = vec![MatchPattern::Wildcard; arity];
                        new_row.extend_from_slice(tail);
                        specialized.push(new_row);
                    } else if let MatchPattern::Or(alternatives) = head {
                        for alternative in alternatives {
                            let expanded = self.specialize_matrix(
                                &[std::iter::once(alternative.clone())
                                    .chain(tail.iter().cloned())
                                    .collect()],
                                constructor,
                                arity,
                            );
                            specialized.extend(expanded);
                        }
                    }
                }
                _ => {}
            }
        }
        specialized
    }

    fn default_matrix(&self, matrix: &[Vec<MatchPattern>]) -> Vec<Vec<MatchPattern>> {
        let mut defaults = Vec::new();
        for row in matrix {
            let Some((head, tail)) = row.split_first() else {
                continue;
            };
            match head {
                MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                    defaults.push(tail.to_vec())
                }
                MatchPattern::Or(alternatives)
                    if alternatives.iter().any(|alternative| {
                        matches!(
                            alternative,
                            MatchPattern::Wildcard | MatchPattern::Identifier(_)
                        )
                    }) =>
                {
                    defaults.push(tail.to_vec());
                }
                _ => {}
            }
        }
        defaults
    }

    fn validate_match_pattern(
        &self,
        pattern: &MatchPattern,
        expected: &Type,
        bindings: &mut Vec<(String, Type)>,
    ) -> Result<bool, SimplyError> {
        match pattern {
            MatchPattern::Wildcard => Ok(true),
            MatchPattern::Identifier(name) => {
                bindings.push((name.clone(), expected.clone()));
                Ok(true)
            }
            MatchPattern::Alias { name, pattern } => {
                bindings.push((name.clone(), expected.clone()));
                self.validate_match_pattern(pattern, expected, bindings)
            }
            MatchPattern::Or(alternatives) => {
                if alternatives.len() < 2 {
                    return Err(self.error(
                        DiagnosticCode::UnexpectedToken,
                        "OR-pattern requires at least two alternatives",
                    ));
                }
                let mut canonical_bindings: Option<Vec<(String, Type)>> = None;
                let mut irrefutable = false;
                let mut alternatives_matrix = Vec::new();
                for alternative in alternatives {
                    if !self.pattern_is_useful(&alternatives_matrix, alternative, expected) {
                        return Err(self.error(
                            DiagnosticCode::UnexpectedToken,
                            "redundant OR-pattern alternative",
                        ));
                    }
                    let mut alternative_bindings = Vec::new();
                    irrefutable |= self.validate_match_pattern(
                        alternative,
                        expected,
                        &mut alternative_bindings,
                    )?;
                    alternative_bindings.sort_by(|left, right| left.0.cmp(&right.0));
                    if let Some(expected_bindings) = &mut canonical_bindings {
                        let same_names = expected_bindings.len() == alternative_bindings.len()
                            && expected_bindings
                                .iter()
                                .zip(&alternative_bindings)
                                .all(|((left_name, _), (right_name, _))| left_name == right_name);
                        let compatible_types = same_names
                            && expected_bindings.iter().zip(&alternative_bindings).all(
                                |((_, left_type), (_, right_type))| {
                                    left_type.compatible_with(right_type)
                                        && right_type.compatible_with(left_type)
                                },
                            );
                        if !compatible_types {
                            let message = if !same_names {
                                "OR-pattern alternatives must bind the same names"
                            } else {
                                "OR-pattern alternatives must bind compatible types"
                            };
                            return Err(self.error(DiagnosticCode::TypeMismatch, message));
                        }
                        for ((_, common_type), (_, alternative_type)) in
                            expected_bindings.iter_mut().zip(&alternative_bindings)
                        {
                            if *common_type == Type::Unknown {
                                *common_type = alternative_type.clone();
                            }
                        }
                    } else {
                        canonical_bindings = Some(alternative_bindings);
                    }
                    alternatives_matrix.push(vec![alternative.clone()]);
                }
                bindings.extend(canonical_bindings.unwrap_or_default());
                Ok(irrefutable)
            }
            MatchPattern::Hash(entries) => {
                if !matches!(expected, Type::Hash | Type::HashValues(_) | Type::Unknown) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "hash pattern cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                let irrefutable = entries.is_empty();
                let value_type = match expected {
                    Type::HashValues(value_type) => (**value_type).clone(),
                    _ => Type::Unknown,
                };
                for (_, pattern) in entries {
                    self.validate_match_pattern(pattern, &value_type, bindings)?;
                }
                Ok(irrefutable)
            }
            MatchPattern::Literal(literal) => {
                let literal_type = match literal {
                    Literal::String(_) => Type::String,
                    Literal::Int(_) => Type::Int,
                    Literal::Float(_) => Type::Float,
                    Literal::Bool(_) => Type::Bool,
                };
                if expected != &Type::Unknown && !literal_type.compatible_with(expected) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "literal pattern of type {} cannot match value of type {}",
                            literal_type.name(),
                            expected.name()
                        ),
                    ));
                }
                Ok(false)
            }
            MatchPattern::Range { start, end } => {
                for bound in start.iter().chain(end.iter()) {
                    if !matches!(bound, Literal::Int(_)) {
                        return Err(self.error(
                            DiagnosticCode::TypeMismatch,
                            "range pattern bounds must be Int",
                        ));
                    }
                }
                if expected != &Type::Int && expected != &Type::Unknown {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "range pattern cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                if let (Some(Literal::Int(start)), Some(Literal::Int(end))) = (start, end)
                    && start > end
                {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        "range pattern lower bound must not exceed upper bound",
                    ));
                }
                Ok(false)
            }
            MatchPattern::Tuple(patterns) => {
                let unknown_types;
                let types = if let Type::Tuple(types) = expected {
                    types
                } else if expected == &Type::Unknown {
                    unknown_types = vec![Type::Unknown; patterns.len()];
                    &unknown_types
                } else {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "tuple pattern cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                };
                if patterns.len() != types.len() {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "tuple pattern expects {} elements, found {}",
                            patterns.len(),
                            types.len()
                        ),
                    ));
                }
                let mut irrefutable = true;
                for (pattern, typ) in patterns.iter().zip(types) {
                    irrefutable &= self.validate_match_pattern(pattern, typ, bindings)?;
                }
                Ok(irrefutable)
            }
            MatchPattern::Sequence { patterns, rest } => {
                let (element_type, sequence_type) = match expected {
                    Type::Array(element) | Type::List(element) => {
                        ((**element).clone(), expected.clone())
                    }
                    Type::Range => (Type::Int, Type::Range),
                    Type::CsvStream => (Type::List(Box::new(Type::String)), Type::CsvStream),
                    Type::Unknown => (Type::Unknown, Type::Unknown),
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::TypeMismatch,
                            format!(
                                "sequence pattern cannot match value of type {}",
                                expected.name()
                            ),
                        ));
                    }
                };
                for pattern in patterns {
                    self.validate_match_pattern(pattern, &element_type, bindings)?;
                }
                if let Some(name) = rest {
                    bindings.push((name.clone(), sequence_type));
                }
                Ok(patterns.is_empty() && rest.is_some())
            }
            MatchPattern::Struct { type_name, fields } => {
                if let Type::Struct(actual_identity) = expected
                    && self.struct_identities.get(type_name) != Some(actual_identity)
                {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "struct pattern `{type_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                } else if expected != &Type::Unknown && !matches!(expected, Type::Struct(_)) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "struct pattern `{type_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                let Some(struct_fields) = self.structs.get(type_name) else {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown struct type `{type_name}`"),
                    ));
                };
                if fields.len() != struct_fields.len() {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!(
                            "struct pattern `{type_name}` expects {} fields, found {}",
                            struct_fields.len(),
                            fields.len()
                        ),
                    ));
                }
                let mut irrefutable = true;
                for (pattern, field) in fields.iter().zip(struct_fields) {
                    irrefutable &=
                        self.validate_match_pattern(pattern, &field.field_type, bindings)?;
                }
                Ok(irrefutable)
            }
            MatchPattern::NamedStruct { type_name, fields } => {
                if let Type::Struct(actual_identity) = expected
                    && self.struct_identities.get(type_name) != Some(actual_identity)
                {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "struct pattern `{type_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                } else if expected != &Type::Unknown && !matches!(expected, Type::Struct(_)) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "struct pattern `{type_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                let Some(struct_fields) = self.structs.get(type_name) else {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown struct type `{type_name}`"),
                    ));
                };
                let mut irrefutable = true;
                for (field_name, pattern) in fields {
                    let Some(field) = struct_fields.iter().find(|field| field.name == *field_name)
                    else {
                        return Err(self.error(
                            DiagnosticCode::UndefinedVariable,
                            format!("struct pattern `{type_name}` has no field `{field_name}`"),
                        ));
                    };
                    irrefutable &=
                        self.validate_match_pattern(pattern, &field.field_type, bindings)?;
                }
                Ok(irrefutable)
            }
            MatchPattern::EnumVariant {
                enum_name,
                variant_name,
                payload,
            } => {
                if expected
                    != &Type::Enum(self.enum_identities.get(enum_name).cloned().unwrap_or_else(
                        || DeclarationIdentity::unresolved(enum_name, DeclarationKind::Enum),
                    ))
                    && expected != &Type::Unknown
                {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "pattern `{enum_name}::{variant_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                let Some(variants) = self.enums.get(enum_name) else {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown enum type `{enum_name}`"),
                    ));
                };
                let Some(variant) = variants
                    .iter()
                    .find(|variant| variant.name == *variant_name)
                else {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown variant `{enum_name}::{variant_name}`"),
                    ));
                };
                let payload_irrefutable = match (&variant.payload_type, payload) {
                    (Some(payload_type), Some(payload_pattern)) => {
                        self.validate_match_pattern(payload_pattern, payload_type, bindings)?
                    }
                    (Some(payload_type), None) => {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            format!(
                                "pattern `{enum_name}::{variant_name}` requires a payload binding (pattern) of type {}",
                                payload_type.name()
                            ),
                        ));
                    }
                    (None, Some(_)) => {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            format!("unit variant `{enum_name}::{variant_name}` has no payload"),
                        ));
                    }
                    (None, None) => true,
                };
                Ok(variants.len() == 1 && payload_irrefutable)
            }
        }
    }

    fn pipeline_type(
        &mut self,
        source: &Expr,
        steps: &[PipelineStep],
    ) -> Result<Type, SimplyError> {
        let has_chunk = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_)));
        let has_parallel = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Parallel(_)));
        let has_checkpoint = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)));
        let has_take = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Take(_)));
        let has_skip = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Skip(_)));
        let has_step_by = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::StepBy(_)));
        let has_take_while = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::TakeWhile(_)));
        let has_drop_while = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::DropWhile(_)));
        let has_distinct = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Distinct));
        let writes_csv = matches!(steps.last(), Some(PipelineStep::WriteCsv(_)));
        let reads_csv_rows = matches!(source, Expr::Call { name, .. } if name == "csv_rows");
        if has_parallel && has_checkpoint {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`parallel` cannot be combined with `checkpoint`",
            ));
        }
        if (has_take || has_skip || has_step_by || has_take_while || has_drop_while || has_distinct)
            && has_parallel
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`take`, `skip`, `step_by`, `take_while`, `drop_while`, and `distinct` cannot be combined with `parallel` because they require ordered input",
            ));
        }
        if (has_take || has_skip || has_step_by || has_take_while || has_drop_while || has_distinct)
            && has_checkpoint
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`take`, `skip`, `step_by`, `take_while`, `drop_while`, and `distinct` cannot be combined with `checkpoint`",
            ));
        }
        if has_chunk && !has_parallel && !has_checkpoint {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`chunk` requires `parallel` or `checkpoint` in the same Flow",
            ));
        }
        if has_checkpoint && !writes_csv {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`checkpoint` requires a `write_csv` terminal",
            ));
        }
        if has_checkpoint && !matches!(source, Expr::Call { name, .. } if name == "csv_rows") {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`checkpoint` currently requires a `csv_rows` source",
            ));
        }
        if reads_csv_rows
            && !matches!(
                steps.last(),
                Some(
                    PipelineStep::Sum
                        | PipelineStep::Count
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                        | PipelineStep::Any
                        | PipelineStep::All
                        | PipelineStep::Partition { .. }
                        | PipelineStep::WriteCsv(_)
                )
            )
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`csv_rows` pipelines must end with an aggregate, `partition`, or `write_csv` terminal",
            ));
        }
        let source_type = self.analyze_expression(source)?;
        let mut item_type = match source_type {
            Type::Array(element) | Type::List(element) => *element,
            Type::Range => Type::Int,
            Type::CsvStream => Type::List(Box::new(Type::String)),
            Type::Unknown => Type::Unknown,
            _ => {
                return Err(self.error(
                    DiagnosticCode::SemanticCollection,
                    "pipeline source must be an array or list",
                ));
            }
        };
        if has_parallel
            && !matches!(
                item_type,
                Type::String | Type::Int | Type::Float | Type::Bool
            )
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`parallel` requires a scalar source item type",
            ));
        }
        if has_parallel
            && !matches!(
                steps.last(),
                Some(
                    PipelineStep::Sum
                        | PipelineStep::Count
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                )
            )
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`parallel` requires an aggregate terminal",
            ));
        }
        for step in steps {
            match step {
                PipelineStep::Where(expression) => {
                    if has_parallel && !is_parallel_safe_expression(expression) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`parallel` requires a parallel-safe `where` expression",
                        ));
                    }
                    let previous =
                        self.variables
                            .insert("item".into(), item_type.clone(), false)?;
                    let condition_type = self.analyze_expression(expression)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    Self::restore_item(&mut self.variables, "item", previous);
                }
                PipelineStep::Derive(expression) => {
                    if has_parallel && !is_parallel_safe_expression(expression) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`parallel` requires a parallel-safe `derive` expression",
                        ));
                    }
                    let previous =
                        self.variables
                            .insert("item".into(), item_type.clone(), false)?;
                    item_type = self.analyze_expression(expression)?;
                    Self::restore_item(&mut self.variables, "item", previous);
                }
                PipelineStep::Take(count) => {
                    if *count < 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`take` count must be a non-negative integer",
                        ));
                    }
                }
                PipelineStep::Skip(count) => {
                    if *count < 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`skip` count must be a non-negative integer",
                        ));
                    }
                }
                PipelineStep::StepBy(interval) => {
                    if *interval <= 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`step_by` interval must be a positive integer",
                        ));
                    }
                }
                PipelineStep::TakeWhile(expression) => {
                    let previous =
                        self.variables
                            .insert("item".into(), item_type.clone(), false)?;
                    let condition_type = self.analyze_expression(expression)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    Self::restore_item(&mut self.variables, "item", previous);
                }
                PipelineStep::DropWhile(expression) => {
                    let previous =
                        self.variables
                            .insert("item".into(), item_type.clone(), false)?;
                    let condition_type = self.analyze_expression(expression)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    Self::restore_item(&mut self.variables, "item", previous);
                }
                PipelineStep::Distinct => {}
                PipelineStep::Partition { item, rules } => {
                    if has_parallel {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`parallel` does not support `partition`",
                        ));
                    }
                    let mut categories = HashSet::new();
                    for rule in rules {
                        categories.insert(rule.category.as_str());
                        if let Some(condition) = &rule.condition {
                            let previous =
                                self.variables
                                    .insert(item.clone(), item_type.clone(), false)?;
                            let condition_type = self.analyze_expression(condition)?;
                            self.require_type(&Type::Bool, &condition_type)?;
                            Self::restore_item(&mut self.variables, item, previous);
                        }
                    }
                    if categories.len() < 2 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "partition must define at least two distinct categories",
                        ));
                    }
                    return Ok(Type::HashValues(Box::new(Type::List(Box::new(
                        item_type.clone(),
                    )))));
                }
                PipelineStep::Sum => {
                    self.require_numeric(&item_type)?;
                    return Ok(item_type);
                }
                PipelineStep::Count => return Ok(Type::Int),
                PipelineStep::Average => {
                    self.require_numeric(&item_type)?;
                    return Ok(Type::Float);
                }
                PipelineStep::Min | PipelineStep::Max => {
                    self.require_numeric(&item_type)?;
                    return Ok(item_type);
                }
                PipelineStep::Any | PipelineStep::All => {
                    self.require_type(&Type::Bool, &item_type)?;
                    return Ok(Type::Bool);
                }
                PipelineStep::WriteCsv(path) => {
                    if has_parallel {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`parallel` does not support `write_csv`",
                        ));
                    }
                    let path_type = self.analyze_expression(path)?;
                    self.require_type(&Type::String, &path_type)?;
                    let invalid_row_field = match &item_type {
                        Type::Array(fields) | Type::List(fields) => !matches!(
                            fields.as_ref(),
                            Type::String
                                | Type::Int
                                | Type::Float
                                | Type::Bool
                                | Type::Unit
                                | Type::Unknown
                        ),
                        Type::Tuple(fields) => fields.iter().any(|field| {
                            !matches!(
                                field,
                                Type::String
                                    | Type::Int
                                    | Type::Float
                                    | Type::Bool
                                    | Type::Unit
                                    | Type::Unknown
                            )
                        }),
                        Type::Unknown => false,
                        _ => {
                            return Err(self.error(
                                DiagnosticCode::SemanticCollection,
                                format!(
                                    "`write_csv` requires `derive` to produce a row collection, found {}",
                                    item_type.name()
                                ),
                            ));
                        }
                    };
                    if invalid_row_field {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`write_csv` row fields must be scalar values",
                        ));
                    }
                    if !reads_csv_rows {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`write_csv` requires a `csv_rows` source",
                        ));
                    }
                    return Ok(Type::Unit);
                }
                PipelineStep::Chunk(size) => {
                    if *size <= 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "chunk size must be a positive integer",
                        ));
                    }
                    if usize::try_from(*size).map_or(true, |size| size > limits::MAX_CHUNK_SIZE) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!("chunk size cannot exceed {}", limits::MAX_CHUNK_SIZE),
                        ));
                    }
                }
                PipelineStep::Parallel(workers) => {
                    if *workers <= 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "parallel worker count must be a positive integer",
                        ));
                    }
                    if usize::try_from(*workers)
                        .map_or(true, |workers| workers > limits::MAX_PARALLEL_WORKERS)
                    {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!(
                                "parallel worker count cannot exceed {}",
                                limits::MAX_PARALLEL_WORKERS
                            ),
                        ));
                    }
                }
                PipelineStep::Checkpoint(path) => {
                    let path_type = self.analyze_expression(path)?;
                    self.require_type(&Type::String, &path_type)?;
                }
            }
        }
        Ok(Type::List(Box::new(item_type)))
    }

    fn restore_item(variables: &mut ScopeStack, name: &str, previous: Option<Type>) {
        match previous {
            Some(value) => {
                variables.insert(name.into(), value, false).expect(
                    "restoring previous pipeline item should not collide with an in-scope binding",
                );
            }
            None => {
                variables.remove(name);
            }
        }
    }
    fn index_type(
        &self,
        target: &Type,
        index: &Type,
        index_expression: &Expr,
    ) -> Result<Type, SimplyError> {
        match target {
            Type::Array(element) | Type::List(element) => {
                self.require_type(&Type::Int, index)?;
                Ok((**element).clone())
            }
            Type::Range => {
                self.require_type(&Type::Int, index)?;
                Ok(Type::Int)
            }
            Type::Tuple(types) => {
                self.require_type(&Type::Int, index)?;
                match index_expression {
                    Expr::Literal(Literal::Int(value)) if *value >= 0 => usize::try_from(*value)
                        .ok()
                        .and_then(|index| types.get(index).cloned())
                        .ok_or_else(|| {
                            self.error(
                                DiagnosticCode::SemanticTupleIndex,
                                "tuple index out of bounds",
                            )
                        }),
                    Expr::Literal(Literal::Int(_)) => Err(self.error(
                        DiagnosticCode::SemanticTupleIndex,
                        "tuple index must be non-negative",
                    )),
                    _ => Ok(Type::Unknown),
                }
            }
            Type::Hash | Type::Tree | Type::HashValues(_) | Type::TreeValues(_) => {
                self.require_type(&Type::String, index)?;
                Ok(match target {
                    Type::HashValues(value) | Type::TreeValues(value) => (**value).clone(),
                    _ => Type::Unknown,
                })
            }
            Type::String => {
                self.require_type(&Type::Int, index)?;
                Ok(Type::String)
            }
            Type::Matrix => match index {
                Type::Tuple(types)
                    if types.len() == 2
                        && types.iter().all(|value| value.compatible_with(&Type::Int)) =>
                {
                    Ok(Type::Unknown)
                }
                Type::Unknown => Ok(Type::Unknown),
                _ => Err(self.error(
                    DiagnosticCode::SemanticMatrixIndex,
                    "matrix index requires a tuple of two integers",
                )),
            },
            Type::Unknown => Ok(Type::Unknown),
            _ => Err(self.error(DiagnosticCode::SemanticIndex, "value is not indexable")),
        }
    }
    fn element_type(typ: &Type) -> Option<Type> {
        match typ {
            Type::Array(element) | Type::List(element) => Some((**element).clone()),
            Type::Range => Some(Type::Int),
            Type::Tuple(types) => Some(types.first().cloned().unwrap_or(Type::Unknown)),
            Type::Hash | Type::Tree => Some(Type::Unknown),
            Type::HashValues(value) | Type::TreeValues(value) => Some((**value).clone()),
            Type::Unknown => Some(Type::Unknown),
            _ => None,
        }
    }

    fn require_collection_or_string(&self, typ: &Type) -> Result<(), SimplyError> {
        if matches!(
            typ,
            Type::String
                | Type::Range
                | Type::Array(_)
                | Type::List(_)
                | Type::Tuple(_)
                | Type::Hash
                | Type::Tree
                | Type::HashValues(_)
                | Type::TreeValues(_)
                | Type::Unknown
        ) {
            Ok(())
        } else {
            Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a collection or string, found {}", typ.name()),
            ))
        }
    }

    fn require_string_iterable(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Array(element) | Type::List(element) => self.require_type(&Type::String, element),
            Type::Hash | Type::Tree => Ok(()),
            Type::HashValues(element) | Type::TreeValues(element) => {
                self.require_type(&Type::String, element)
            }
            Type::Tuple(types) => {
                for element in types {
                    self.require_type(&Type::String, element)?;
                }
                Ok(())
            }
            Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a string collection, found {}", typ.name()),
            )),
        }
    }

    fn require_boolean_iterable(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Array(element) | Type::List(element) => self.require_type(&Type::Bool, element),
            Type::Tuple(types) => {
                for element in types {
                    self.require_type(&Type::Bool, element)?;
                }
                Ok(())
            }
            Type::Hash | Type::Tree | Type::Unknown => Ok(()),
            Type::HashValues(element) | Type::TreeValues(element) => {
                self.require_type(&Type::Bool, element)
            }
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a boolean collection, found {}", typ.name()),
            )),
        }
    }

    fn require_numeric_iterable(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Array(element) | Type::List(element) => self.require_numeric(element),
            Type::Range => Ok(()),
            Type::Tuple(types) => {
                for element in types {
                    self.require_numeric(element)?;
                }

                Ok(())
            }
            Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a numeric collection, found {}", typ.name()),
            )),
        }
    }

    fn numeric_sequence_element(&self, typ: &Type) -> Result<Type, SimplyError> {
        let element = self.numeric_sequence_element_option(typ).ok_or_else(|| {
            self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a numeric sequence, found {}", typ.name()),
            )
        })?;
        self.require_numeric(&element)?;
        Ok(element)
    }

    fn numeric_sequence_element_option(&self, typ: &Type) -> Option<Type> {
        match typ {
            Type::Array(element) | Type::List(element) => Some((**element).clone()),
            Type::Tuple(elements) => {
                let first = elements.first().cloned().unwrap_or(Type::Unknown);
                if elements
                    .iter()
                    .all(|element| element.compatible_with(&first))
                {
                    Some(first)
                } else {
                    Some(Type::Unknown)
                }
            }
            Type::Unknown => Some(Type::Unknown),
            _ => None,
        }
    }

    fn numeric_sum_type(&self, typ: &Type) -> Type {
        let elements: Vec<&Type> = match typ {
            Type::Array(element) | Type::List(element) => vec![element],
            Type::Tuple(elements) => elements.iter().collect(),
            Type::Range => return Type::Int,
            _ => return Type::Unknown,
        };
        if elements.iter().any(|element| **element == Type::Float) {
            Type::Float
        } else if elements.iter().any(|element| **element == Type::Unknown) {
            Type::Unknown
        } else {
            Type::Int
        }
    }

    fn require_matrix(&self, typ: &Type) -> Result<(), SimplyError> {
        let row_type = match typ {
            Type::Matrix => return Ok(()),
            Type::Array(row) | Type::List(row) => row,
            Type::Unknown => return Ok(()),
            _ => {
                return Err(self.error(
                    DiagnosticCode::SemanticCollection,
                    format!("expected a matrix, found {}", typ.name()),
                ));
            }
        };
        match row_type.as_ref() {
            Type::Array(element) | Type::List(element) => self.require_numeric(element),
            Type::HashValues(element) | Type::TreeValues(element) => self.require_numeric(element),
            Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                "matrix rows must be numeric sequences",
            )),
        }
    }

    fn numeric_result(&self, left: &Type, right: &Type) -> Result<Type, SimplyError> {
        self.require_numeric(left)?;
        self.require_numeric(right)?;
        Ok(if left == &Type::Float || right == &Type::Float {
            Type::Float
        } else {
            Type::Int
        })
    }
    fn require_numeric(&self, typ: &Type) -> Result<(), SimplyError> {
        if matches!(typ, Type::Int | Type::Float | Type::Unknown) {
            Ok(())
        } else {
            Err(self.error(
                DiagnosticCode::TypeMismatch,
                format!("expected a number, found {}", typ.name()),
            ))
        }
    }
    fn require_type(&self, expected: &Type, actual: &Type) -> Result<(), SimplyError> {
        if actual == &Type::Unknown {
            return Ok(());
        }
        if actual.compatible_with(expected) {
            Ok(())
        } else {
            Err(self.type_error(expected, actual, "expression"))
        }
    }
    fn expect_count(
        &self,
        name: &str,
        arguments: &[Type],
        expected: usize,
    ) -> Result<(), SimplyError> {
        if arguments.len() == expected {
            Ok(())
        } else {
            Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                format!(
                    "`{name}` expects {expected} arguments, got {}",
                    arguments.len()
                ),
            ))
        }
    }
    fn type_error(&self, expected: &Type, actual: &Type, subject: &str) -> SimplyError {
        self.error(
            DiagnosticCode::TypeMismatch,
            format!(
                "type mismatch for `{subject}`: expected {}, found {}",
                expected.name(),
                actual.name()
            ),
        )
    }
    fn error(&self, code: DiagnosticCode, message: impl Into<String>) -> SimplyError {
        SimplyError::Semantic {
            span: self.current_span.clone().unwrap_or_else(|| Span::new(0, 0)),
            code,
            message: message.into(),
        }
    }
}
