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
        BinaryOperator, CollectionOperation, Expr, Literal, MatchPattern, PipelineStep, Program,
        Stmt, StructField, UnaryOperator, is_parallel_safe_expression,
    },
    error::{DiagnosticCode, SimplyError, Span},
    evaluator::closure_dependencies,
    runtime::limits,
    types::{DeclarationIdentity, DeclarationKind, Type},
};

#[derive(Clone)]
struct FunctionSignature {
    parameters: Vec<Option<Type>>,
    ref_parameters: Vec<bool>,
    mut_ref_parameters: Vec<bool>,
    return_type: Option<Type>,
}

#[derive(Clone)]
struct MessageSignature {
    parameters: Vec<Option<Type>>,
    ref_parameters: Vec<bool>,
    mut_ref_parameters: Vec<bool>,
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
    ref_parameter_scopes: Vec<HashSet<String>>,
    mut_ref_parameter_scopes: Vec<HashSet<String>>,
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
    ref_modes: HashMap<String, Vec<Option<bool>>>,
    vector_provenance: HashMap<String, Vec<VectorProvenance>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VectorProvenance {
    TupleBacked,
    ArrayBacked,
    Unknown,
}

type VectorProvenanceSnapshot = HashMap<String, Vec<VectorProvenance>>;

impl VectorProvenance {
    fn join(self, other: Self) -> Self {
        if self == other { self } else { Self::Unknown }
    }
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
            ref_modes: HashMap::new(),
            vector_provenance: HashMap::new(),
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
        self.mutability
            .entry(name.clone())
            .or_default()
            .push(mutable);
        self.ref_modes.entry(name.clone()).or_default().push(None);
        self.vector_provenance
            .entry(name.clone())
            .or_default()
            .push(VectorProvenance::Unknown);
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
        self.mutability
            .entry(name.clone())
            .or_default()
            .push(mutable);
        self.ref_modes.entry(name.clone()).or_default().push(None);
        self.vector_provenance
            .entry(name.clone())
            .or_default()
            .push(VectorProvenance::Unknown);
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

    fn is_reference(&self, name: &str) -> bool {
        self.ref_modes
            .get(name)
            .and_then(|modes| modes.last())
            .is_some_and(Option::is_some)
    }

    fn is_exclusive_reference(&self, name: &str) -> bool {
        self.ref_modes
            .get(name)
            .and_then(|modes| modes.last())
            .copied()
            .flatten()
            .unwrap_or(false)
    }

    fn mark_reference(&mut self, name: &str, exclusive: bool) {
        if let Some(mode) = self
            .ref_modes
            .get_mut(name)
            .and_then(|modes| modes.last_mut())
        {
            *mode = Some(exclusive);
        }
    }

    fn vector_provenance(&self, name: &str) -> VectorProvenance {
        self.vector_provenance
            .get(name)
            .and_then(|values| values.last())
            .copied()
            .unwrap_or(VectorProvenance::Unknown)
    }

    fn set_vector_provenance(&mut self, name: &str, provenance: VectorProvenance) {
        if let Some(value) = self
            .vector_provenance
            .get_mut(name)
            .and_then(|values| values.last_mut())
        {
            *value = provenance;
        }
    }

    fn snapshot_vector_provenance(&self) -> VectorProvenanceSnapshot {
        self.vector_provenance.clone()
    }

    fn restore_vector_provenance(&mut self, snapshot: &VectorProvenanceSnapshot) {
        self.vector_provenance.clone_from(snapshot);
    }

    fn merge_vector_provenance(&mut self, snapshots: &[VectorProvenanceSnapshot]) {
        for (name, values) in &mut self.vector_provenance {
            for (index, value) in values.iter_mut().enumerate() {
                *value = snapshots.iter().fold(*value, |merged, snapshot| {
                    merged.join(
                        snapshot
                            .get(name)
                            .and_then(|values| values.get(index))
                            .copied()
                            .unwrap_or(VectorProvenance::Unknown),
                    )
                });
            }
        }
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
                if let Some(values) = self.ref_modes.get_mut(name) {
                    values.pop();
                    if values.is_empty() {
                        self.ref_modes.remove(name);
                    }
                }
                if let Some(values) = self.vector_provenance.get_mut(name) {
                    values.pop();
                    if values.is_empty() {
                        self.vector_provenance.remove(name);
                    }
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
                        if let Some(values) = self.ref_modes.get_mut(name) {
                            values.pop();
                            if values.is_empty() {
                                self.ref_modes.remove(name);
                            }
                        }
                        if let Some(values) = self.vector_provenance.get_mut(name) {
                            values.pop();
                            if values.is_empty() {
                                self.vector_provenance.remove(name);
                            }
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

    fn expression_vector_provenance(
        &self,
        expression: &Expr,
        actual: &Type,
        expected: &Type,
    ) -> VectorProvenance {
        if !matches!(expected, Type::Vector(_, _)) {
            return VectorProvenance::Unknown;
        }
        match expression {
            Expr::Identifier(name) => self.variables.vector_provenance(name),
            Expr::Array(_) | Expr::List(_) => VectorProvenance::ArrayBacked,
            Expr::Tuple(_) => VectorProvenance::TupleBacked,
            _ if matches!(actual, Type::Tuple(_)) => VectorProvenance::TupleBacked,
            _ => VectorProvenance::Unknown,
        }
    }

    fn pattern_matches_known_bool(pattern: &MatchPattern, value: bool) -> bool {
        match pattern {
            MatchPattern::Literal(Literal::Bool(pattern_value)) => *pattern_value == value,
            MatchPattern::Wildcard | MatchPattern::Identifier(_) => true,
            MatchPattern::Alias { pattern, .. } => Self::pattern_matches_known_bool(pattern, value),
            MatchPattern::Or(patterns) => patterns
                .iter()
                .any(|pattern| Self::pattern_matches_known_bool(pattern, value)),
            _ => false,
        }
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
            Type::Vector(element, length) => {
                Type::Vector(Box::new(self.resolve_type_identity(element)), *length)
            }
            Type::TypedMatrix(element, rows, columns) => Type::TypedMatrix(
                Box::new(self.resolve_type_identity(element)),
                *rows,
                *columns,
            ),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|element| self.resolve_type_identity(element))
                    .collect(),
            ),
            Type::HashValues(element) => {
                Type::HashValues(Box::new(self.resolve_type_identity(element)))
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
}

mod declarations;
mod expressions;
mod imports;
mod patterns;
mod pipelines;
mod statements;
mod types;
