use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::token::Span;

#[derive(Debug)]
pub struct VerifyResult {
    pub errors: Vec<VerifyError>,
    pub warnings: Vec<VerifyError>,
}

#[derive(Debug)]
pub struct VerifyError {
    pub kind: VerifyErrorKind,
    pub message: String,
    pub span: Span,
    pub hint: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum VerifyErrorKind {
    UndefinedShape,
    UndefinedSource,
    UndefinedRealm,
    UndefinedBinding,
    UndefinedService,
    DuplicateBinding,
    DuplicateShape,
    DuplicateSource,
    MissingOrClause,
    MissingElse,
    MissingAuth,
    MissingReturn,
    MissingCapability,
    MissingIndex,
    MissingTenantScope,
    MissingRequiredField,
    AutoFieldInInsert,
    OrderingViolation,
    ForwardReference,
    TypeMismatch,
    PolicyViolation,
    EmptyEnum,
    InvalidPath,
    UndefinedStorage,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "line {}: {:?}: {}",
            self.span.line, self.kind, self.message
        )?;
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {hint}")?;
        }
        Ok(())
    }
}

impl VerifyResult {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
}

#[allow(dead_code)]
struct ShapeInfo {
    fields: HashMap<String, FieldInfo>,
    span: Span,
}

#[allow(dead_code)]
struct FieldInfo {
    ty: TypeExpr,
    is_pk: bool,
    is_auto: bool,
    is_required: bool,
    has_default: bool,
}

#[allow(dead_code)]
struct SourceInfo {
    shape_name: String,
    source_type: SourceType,
    indexes: Vec<Vec<String>>,
    unique_indexes: Vec<Vec<String>>,
    span: Span,
}

#[allow(dead_code)]
struct RealmInfo {
    tenant_field: Option<String>,
    capabilities: HashSet<(CapKind, String)>,
    span: Span,
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
enum CapKind {
    Read,
    Write,
    Call,
    Effect,
    Admin,
}

#[allow(dead_code)]
struct PolicyInfo {
    name: String,
    applies_to: AppliesTo,
    requires: Vec<RequireClause>,
    span: Span,
}

#[allow(dead_code)]
struct ServiceInfo {
    methods: HashSet<String>,
    pure_methods: HashSet<String>,
    idempotent_methods: HashMap<String, String>,
    span: Span,
}

pub struct Verifier {
    shapes: HashMap<String, ShapeInfo>,
    sources: HashMap<String, SourceInfo>,
    realms: HashMap<String, RealmInfo>,
    policies: Vec<PolicyInfo>,
    services: HashMap<String, ServiceInfo>,
    storages: HashSet<String>,
    errors: Vec<VerifyError>,
    warnings: Vec<VerifyError>,
}

impl Default for Verifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Verifier {
    pub fn new() -> Self {
        Self {
            shapes: HashMap::new(),
            sources: HashMap::new(),
            realms: HashMap::new(),
            policies: Vec::new(),
            services: HashMap::new(),
            storages: HashSet::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn verify(mut self, program: &Program) -> VerifyResult {
        self.collect_declarations(program);
        self.verify_storages(program);
        self.verify_sources(program);
        self.verify_flows(program);
        self.verify_sagas(program);
        self.verify_policies(program);
        self.verify_surfaces(program);
        self.verify_migrations(program);

        VerifyResult {
            errors: self.errors,
            warnings: self.warnings,
        }
    }

    fn collect_declarations(&mut self, program: &Program) {
        for construct in &program.constructs {
            match construct {
                Construct::Shape(s) => self.register_shape(s),
                Construct::Source(s) => self.register_source(s),
                Construct::Realm(r) => self.register_realm(r),
                Construct::Policy(p) => self.register_policy(p),
                Construct::Service(s) => self.register_service(s),
                Construct::Storage(s) => {
                    self.storages.insert(s.name.clone());
                }
                Construct::Flow(_)
                | Construct::Saga(_)
                | Construct::Surface(_)
                | Construct::Migrate(_)
                | Construct::Stream(_)
                | Construct::Func(_) => {}
            }
        }
    }

    fn verify_storages(&mut self, program: &Program) {
        for construct in &program.constructs {
            let Construct::Storage(storage) = construct else {
                continue;
            };
            if storage.max_size.is_some_and(|max_size| max_size <= 0) {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::TypeMismatch,
                    message: format!(
                        "storage '{}': MAX_SIZE must be greater than zero",
                        storage.name
                    ),
                    span: storage.span,
                    hint: None,
                });
            }
            if storage.prefix.as_ref().is_some_and(|prefix| {
                prefix.starts_with('/')
                    || prefix.contains('\\')
                    || prefix
                        .split('/')
                        .any(|component| component.is_empty() || matches!(component, "." | ".."))
            }) {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::InvalidPath,
                    message: format!(
                        "storage '{}': PREFIX must be a relative path without empty, '.' or '..' components",
                        storage.name
                    ),
                    span: storage.span,
                    hint: Some("use a prefix such as user-uploads/avatars".into()),
                });
            }
            let mut types = HashSet::new();
            for allowed_type in &storage.types {
                if !types.insert(allowed_type.to_ascii_lowercase()) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::DuplicateBinding,
                        message: format!(
                            "storage '{}': duplicate allowed type '{}'",
                            storage.name, allowed_type
                        ),
                        span: storage.span,
                        hint: None,
                    });
                }
            }
        }
    }

    fn register_shape(&mut self, shape: &ShapeDef) {
        if self.shapes.contains_key(&shape.name) {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::DuplicateShape,
                message: format!("shape '{}' is already defined", shape.name),
                span: shape.span,
                hint: None,
            });
            return;
        }

        let mut fields = HashMap::new();
        let mut has_pk = false;

        for field in &shape.fields {
            let is_pk = field.modifiers.iter().any(|m| matches!(m, Modifier::Pk));
            let is_auto = field.modifiers.iter().any(|m| matches!(m, Modifier::Auto));
            let is_required = field
                .modifiers
                .iter()
                .any(|m| matches!(m, Modifier::Required));
            let has_default = field
                .modifiers
                .iter()
                .any(|m| matches!(m, Modifier::Default(_)));

            if is_pk {
                has_pk = true;
            }

            if let TypeExpr::Ref {
                shape: ref_shape,
                field: ref_field,
            } = &field.ty
            {
                if let Some(target) = self.shapes.get(ref_shape) {
                    if !target.fields.contains_key(ref_field) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::UndefinedBinding,
                            message: format!(
                                "in shape '{}': REF {}.{} — field '{}' does not exist in shape '{}'",
                                shape.name, ref_shape, ref_field, ref_field, ref_shape
                            ),
                            span: field.span,
                            hint: None,
                        });
                    }
                }
            }

            if let TypeExpr::Enum(variants) = &field.ty {
                if variants.is_empty() {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::EmptyEnum,
                        message: format!(
                            "in shape '{}': field '{}' has ENUM with no variants",
                            shape.name, field.name
                        ),
                        span: field.span,
                        hint: Some("add at least one variant to ENUM".into()),
                    });
                }
            }

            fields.insert(
                field.name.clone(),
                FieldInfo {
                    ty: field.ty.clone(),
                    is_pk,
                    is_auto,
                    is_required,
                    has_default,
                },
            );
        }

        if !has_pk {
            self.warnings.push(VerifyError {
                kind: VerifyErrorKind::MissingRequiredField,
                message: format!("shape '{}' has no PK field", shape.name),
                span: shape.span,
                hint: Some("add PK modifier to one field".into()),
            });
        }

        self.shapes.insert(
            shape.name.clone(),
            ShapeInfo {
                fields,
                span: shape.span,
            },
        );
    }

    fn register_source(&mut self, source: &SourceDef) {
        if self.sources.contains_key(&source.name) {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::DuplicateSource,
                message: format!("source '{}' is already defined", source.name),
                span: source.span,
                hint: None,
            });
            return;
        }

        let indexes: Vec<Vec<String>> = source
            .indexes
            .iter()
            .map(|idx| idx.fields.iter().map(|f| f.name.clone()).collect())
            .collect();
        let unique_indexes: Vec<Vec<String>> = source
            .indexes
            .iter()
            .filter(|idx| {
                idx.fields
                    .iter()
                    .any(|f| matches!(f.suffix, Some(IndexSuffix::Unique)))
            })
            .map(|idx| idx.fields.iter().map(|f| f.name.clone()).collect())
            .collect();

        self.sources.insert(
            source.name.clone(),
            SourceInfo {
                shape_name: source.shape.clone(),
                source_type: source.source_type,
                indexes,
                unique_indexes,
                span: source.span,
            },
        );
    }

    fn register_realm(&mut self, realm: &RealmDef) {
        let mut caps = HashSet::new();
        for cap in &realm.capabilities {
            let kind = match cap.kind {
                CapabilityKind::Read => CapKind::Read,
                CapabilityKind::Write => CapKind::Write,
                CapabilityKind::Call => CapKind::Call,
                CapabilityKind::Effect => CapKind::Effect,
                CapabilityKind::Admin => CapKind::Admin,
            };
            caps.insert((kind, cap.target.clone()));
        }

        self.realms.insert(
            realm.name.clone(),
            RealmInfo {
                tenant_field: realm.tenant.clone(),
                capabilities: caps,
                span: realm.span,
            },
        );
    }

    fn register_policy(&mut self, policy: &PolicyDef) {
        self.policies.push(PolicyInfo {
            name: policy.name.clone(),
            applies_to: policy.applies_to.clone(),
            requires: policy.requires.clone(),
            span: policy.span,
        });
    }

    fn register_service(&mut self, service: &ServiceDef) {
        let methods: HashSet<String> = service.methods.iter().map(|m| m.name.clone()).collect();
        let pure_methods = service
            .methods
            .iter()
            .filter(|method| method.pure)
            .map(|method| method.name.clone())
            .collect();
        let mut idempotent_methods = HashMap::new();
        for method in &service.methods {
            let Some(input_name) = &method.idempotency_input else {
                continue;
            };
            if method.pure {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::PolicyViolation,
                    message: format!(
                        "service '{}.{}': PURE and IDEMPOTENCY are mutually exclusive",
                        service.name, method.name
                    ),
                    span: service.span,
                    hint: Some("remove IDEMPOTENCY from a side-effect-free method".into()),
                });
                continue;
            }
            match method.inputs.iter().find(|(name, _)| name == input_name) {
                Some((_, TypeExpr::String(_) | TypeExpr::Text | TypeExpr::Uuid)) => {
                    idempotent_methods.insert(method.name.clone(), input_name.clone());
                }
                Some(_) => self.errors.push(VerifyError {
                    kind: VerifyErrorKind::TypeMismatch,
                    message: format!(
                        "service '{}.{}': IDEMPOTENCY input '{}' must be STRING, TEXT, or UUID",
                        service.name, method.name, input_name
                    ),
                    span: service.span,
                    hint: Some("use a stable scalar operation key".into()),
                }),
                None => self.errors.push(VerifyError {
                    kind: VerifyErrorKind::MissingRequiredField,
                    message: format!(
                        "service '{}.{}': IDEMPOTENCY references missing input '{}'",
                        service.name, method.name, input_name
                    ),
                    span: service.span,
                    hint: Some(format!("add `{input_name} STRING 255` to INPUT")),
                }),
            }
        }
        self.services.insert(
            service.name.clone(),
            ServiceInfo {
                methods,
                pure_methods,
                idempotent_methods,
                span: service.span,
            },
        );
    }

    fn verify_sources(&mut self, program: &Program) {
        for construct in &program.constructs {
            if let Construct::Source(source) = construct {
                if !self.shapes.contains_key(&source.shape) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::UndefinedShape,
                        message: format!(
                            "source '{}' references undefined shape '{}'",
                            source.name, source.shape
                        ),
                        span: source.span,
                        hint: Some(format!("add SHAPE {} definition", source.shape)),
                    });
                    continue;
                }

                let shape = &self.shapes[&source.shape];
                for index in &source.indexes {
                    for field in &index.fields {
                        if !shape.fields.contains_key(&field.name) {
                            self.errors.push(VerifyError {
                                kind: VerifyErrorKind::UndefinedBinding,
                                message: format!(
                                    "index on source '{}' references unknown field '{}'",
                                    source.name, field.name
                                ),
                                span: index.span,
                                hint: None,
                            });
                        }
                    }
                }
            }
        }
    }

    fn verify_flows(&mut self, program: &Program) {
        for construct in &program.constructs {
            if let Construct::Flow(flow) = construct {
                self.verify_flow(flow);
            }
        }
    }

    fn verify_sagas(&mut self, program: &Program) {
        for construct in &program.constructs {
            if let Construct::Saga(saga) = construct {
                self.verify_saga(saga);
            }
        }
    }

    fn verify_saga(&mut self, saga: &SagaDef) {
        if let Some(ref realm_name) = saga.realm {
            if !self.realms.contains_key(realm_name) {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::UndefinedRealm,
                    message: format!(
                        "saga '{}' references undefined realm '{}'",
                        saga.name, realm_name
                    ),
                    span: saga.span,
                    hint: None,
                });
            }
        }

        let mut yielded: HashSet<String> = HashSet::new();
        if saga.auth.is_some() {
            yielded.insert("auth".into());
        }
        if saga.body.is_some() {
            yielded.insert("body".into());
        }
        if saga.path.contains(':') {
            yielded.insert("path".into());
        }

        for (i, step) in saga.steps.iter().enumerate() {
            for flow_step in &step.flow_steps {
                match flow_step {
                    FlowStep::Let(l) => {
                        self.verify_saga_expr_sources(saga, &l.expr, step);
                    }
                    FlowStep::Insert(ins) => {
                        if !self.sources.contains_key(&ins.source) {
                            self.errors.push(VerifyError {
                                kind: VerifyErrorKind::UndefinedSource,
                                message: format!(
                                    "in saga '{}' step '{}': undefined source '{}'",
                                    saga.name, step.name, ins.source
                                ),
                                span: step.span,
                                hint: None,
                            });
                        }
                    }
                    FlowStep::Upsert(upsert) => {
                        if !self.sources.contains_key(&upsert.source) {
                            self.errors.push(VerifyError {
                                kind: VerifyErrorKind::UndefinedSource,
                                message: format!(
                                    "in saga '{}' step '{}': undefined source '{}'",
                                    saga.name, step.name, upsert.source
                                ),
                                span: step.span,
                                hint: None,
                            });
                        }
                    }
                    FlowStep::Update(upd) => {
                        if !self.sources.contains_key(&upd.source) {
                            self.errors.push(VerifyError {
                                kind: VerifyErrorKind::UndefinedSource,
                                message: format!(
                                    "in saga '{}' step '{}': undefined source '{}'",
                                    saga.name, step.name, upd.source
                                ),
                                span: step.span,
                                hint: None,
                            });
                        }
                    }
                    FlowStep::Delete(del) => {
                        if !self.sources.contains_key(&del.source) {
                            self.errors.push(VerifyError {
                                kind: VerifyErrorKind::UndefinedSource,
                                message: format!(
                                    "in saga '{}' step '{}': undefined source '{}'",
                                    saga.name, step.name, del.source
                                ),
                                span: step.span,
                                hint: None,
                            });
                        }
                    }
                    FlowStep::Set(s) => {
                        self.verify_saga_expr_sources(saga, &s.expr, step);
                    }
                    FlowStep::Each(e) => {
                        self.verify_saga_expr_sources(saga, &e.source, step);
                    }
                    FlowStep::Fanout(f) => {
                        self.verify_saga_expr_sources(saga, &f.source, step);
                    }
                    FlowStep::Try(_) => {}
                    FlowStep::Rule(_)
                    | FlowStep::Guard(_)
                    | FlowStep::Effect(_)
                    | FlowStep::Match(_) => {}
                    FlowStep::Upload(u) => {
                        if !self.storages.contains(&u.storage) {
                            self.errors.push(VerifyError {
                                kind: VerifyErrorKind::UndefinedStorage,
                                message: format!(
                                    "in saga '{}' step '{}': undefined storage '{}'",
                                    saga.name, step.name, u.storage
                                ),
                                span: step.span,
                                hint: None,
                            });
                        }
                    }
                }
            }

            let has_mutations = step.flow_steps.iter().any(|s| {
                matches!(
                    s,
                    FlowStep::Insert(_)
                        | FlowStep::Upsert(_)
                        | FlowStep::Update(_)
                        | FlowStep::Delete(_)
                        | FlowStep::Fanout(_)
                )
            });
            let has_calls = step
                .flow_steps
                .iter()
                .any(|s| matches!(s, FlowStep::Let(l) if matches!(l.expr, Expr::Call { .. })));
            if (has_mutations || has_calls) && matches!(step.compensate, Compensate::None) && i > 0
            {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::MissingRequiredField,
                    message: format!(
                        "in saga '{}': step '{}' has mutations/calls but COMPENSATE NONE — compensation required for rollback safety",
                        saga.name, step.name
                    ),
                    span: step.span,
                    hint: Some("add COMPENSATE block with rollback logic".into()),
                });
            }

            for name in &step.yields {
                yielded.insert(name.clone());
            }
        }
    }

    fn verify_saga_expr_sources(&mut self, saga: &SagaDef, expr: &Expr, step: &SagaStep) {
        match expr {
            Expr::Fetch { source, .. } | Expr::Query { source, .. } => {
                if !self.sources.contains_key(source) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::UndefinedSource,
                        message: format!(
                            "in saga '{}' step '{}': undefined source '{}'",
                            saga.name, step.name, source
                        ),
                        span: step.span,
                        hint: None,
                    });
                }
            }
            Expr::Call {
                service, method, ..
            } => {
                if let Some(svc) = self.services.get(service) {
                    if !svc.methods.contains(method) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::UndefinedService,
                            message: format!(
                                "in saga '{}' step '{}': service '{}' has no method '{}'",
                                saga.name, step.name, service, method
                            ),
                            span: step.span,
                            hint: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn verify_flow(&mut self, flow: &FlowDef) {
        let mut ctx = FlowContext::new(flow);

        if flow
            .body
            .as_ref()
            .is_some_and(|body| body.kind == BodyKind::Multipart)
            && matches!(flow.auth, Some(AuthDecl::WebhookSignature { .. }))
        {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::TypeMismatch,
                message: format!(
                    "flow '{}': webhook signature authentication requires a JSON body",
                    flow.name
                ),
                span: flow.span,
                hint: Some(
                    "use a JSON body so Axis can verify the exact request bytes before decoding"
                        .into(),
                ),
            });
        }

        if let Some(ref realm_name) = flow.realm {
            if !self.realms.contains_key(realm_name) {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::UndefinedRealm,
                    message: format!(
                        "flow '{}' references undefined realm '{}'",
                        flow.name, realm_name
                    ),
                    span: flow.span,
                    hint: None,
                });
            }
        }

        if let Some(ref auth) = flow.auth {
            ctx.has_auth = !matches!(auth, AuthDecl::None);
        }
        ctx.has_limit = !flow.limits.is_empty();
        ctx.has_scope = flow.scope.is_some();

        if let Some(ref body) = flow.body {
            ctx.bindings.insert("body".into(), BindingInfo::body());
            for field in &body.fields {
                ctx.body_fields.insert(field.name.clone());
            }
        }

        if flow.path.contains(':') {
            ctx.bindings.insert("path".into(), BindingInfo::path());
        }

        if !flow.params.is_empty() {
            ctx.bindings.insert("query".into(), BindingInfo::query());
        }

        if !flow.headers.is_empty() {
            ctx.bindings.insert("header".into(), BindingInfo::header());
        }

        if ctx.has_auth {
            ctx.bindings.insert("auth".into(), BindingInfo::auth());
        }

        if !flow.cache.is_empty() && !matches!(flow.method, HttpMethod::Get) {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::MissingRequiredField,
                message: format!("flow '{}': CACHE is only allowed on GET flows", flow.name),
                span: flow.span,
                hint: Some("remove CACHE or change method to GET".into()),
            });
        }

        if matches!(flow.scope, Some(ScopeDecl::TenantAny)) {
            if let Some(ref realm_name) = flow.realm {
                if let Some(realm) = self.realms.get(realm_name) {
                    let has_admin = realm.capabilities.iter().any(|(k, _)| *k == CapKind::Admin);
                    if !has_admin {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::MissingCapability,
                            message: format!(
                                "flow '{}': SCOPE TENANT ANY requires CAPABILITY admin in realm '{}'",
                                flow.name, realm_name
                            ),
                            span: flow.span,
                            hint: Some(format!("add CAPABILITY admin to REALM {}", realm_name)),
                        });
                    }
                }
            }
        }

        if let Some(idempotency) = &flow.idempotency {
            self.verify_dotpath_binding(flow, &idempotency.key, &ctx, idempotency.span);
            self.verify_dotpath_binding(flow, &idempotency.scope, &ctx, idempotency.span);
            if idempotency.ttl <= 0 {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::MissingRequiredField,
                    message: format!("flow '{}': IDEMPOTENCY TTL must be positive", flow.name),
                    span: idempotency.span,
                    hint: Some("use a TTL in seconds, for example TTL 86400".into()),
                });
            }
            if matches!(flow.method, HttpMethod::Get) {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::PolicyViolation,
                    message: format!("flow '{}': IDEMPOTENCY is not valid on GET", flow.name),
                    span: idempotency.span,
                    hint: Some("remove IDEMPOTENCY from read-only flows".into()),
                });
            }

            let mut reads = HashSet::new();
            let mut writes = HashSet::new();
            let mut calls = HashSet::new();
            let mut effects = HashSet::new();
            for step in &flow.steps {
                self.collect_capabilities_from_step(
                    step,
                    &mut reads,
                    &mut writes,
                    &mut calls,
                    &mut effects,
                );
            }
            if writes.is_empty() {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::PolicyViolation,
                    message: format!(
                        "flow '{}': IDEMPOTENCY requires at least one database mutation",
                        flow.name
                    ),
                    span: idempotency.span,
                    hint: Some("add INSERT, UPSERT, UPDATE, DELETE, or FANOUT".into()),
                });
            }
            let impure_calls: Vec<String> = calls
                .iter()
                .filter(|(service, method)| {
                    let Some(info) = self.services.get(service) else {
                        return true;
                    };
                    if info.pure_methods.contains(method) {
                        return false;
                    }
                    !info.idempotent_methods.get(method).is_some_and(|input| {
                        flow_service_calls_use_key(
                            &flow.steps,
                            service,
                            method,
                            input,
                            &idempotency.key,
                        )
                    })
                })
                .map(|(service, method)| format!("{service}.{method}"))
                .collect();
            if !impure_calls.is_empty() {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::PolicyViolation,
                    message: format!(
                        "flow '{}': IDEMPOTENCY cannot atomically include service calls [{}]",
                        flow.name,
                        impure_calls.join(", ")
                    ),
                    span: idempotency.span,
                    hint: Some("use EFFECT, a PURE method, or declare service-method IDEMPOTENCY and pass the exact flow key".into()),
                });
            }

            let mut transactional_sources = reads;
            transactional_sources.extend(writes);
            let mut source_type: Option<SourceType> = None;
            for source_name in transactional_sources {
                let Some(source) = self.sources.get(&source_name) else {
                    continue;
                };
                if !matches!(
                    source.source_type,
                    SourceType::Postgres | SourceType::Mysql | SourceType::Sqlite
                ) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::PolicyViolation,
                        message: format!(
                            "flow '{}': IDEMPOTENCY requires transactional SQL, but '{}' is {:?}",
                            flow.name, source_name, source.source_type
                        ),
                        span: idempotency.span,
                        hint: Some("move non-SQL work behind a transactional outbox".into()),
                    });
                    continue;
                }
                if source_type.is_some_and(|kind| kind != source.source_type) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::PolicyViolation,
                        message: format!(
                            "flow '{}': IDEMPOTENCY cannot span different SQL dialects",
                            flow.name
                        ),
                        span: idempotency.span,
                        hint: Some(
                            "use sources in one physical database and one SQL dialect".into(),
                        ),
                    });
                } else {
                    source_type = Some(source.source_type);
                }
            }

            fn has_unsafe_transaction_step(steps: &[FlowStep]) -> bool {
                steps.iter().any(|step| match step {
                    FlowStep::Upload(_) | FlowStep::Try(_) => true,
                    FlowStep::Match(step) => {
                        step.branches
                            .iter()
                            .any(|branch| has_unsafe_transaction_step(&branch.steps))
                            || step
                                .default
                                .as_ref()
                                .is_some_and(|steps| has_unsafe_transaction_step(steps))
                    }
                    FlowStep::Each(step) => has_unsafe_transaction_step(&step.steps),
                    _ => false,
                })
            }
            if has_unsafe_transaction_step(&flow.steps) {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::PolicyViolation,
                    message: format!(
                        "flow '{}': IDEMPOTENCY cannot contain TRY or UPLOAD",
                        flow.name
                    ),
                    span: idempotency.span,
                    hint: Some(
                        "move external file work after a transactional outbox boundary".into(),
                    ),
                });
            }
        }

        self.verify_flow_ordering(flow);
        self.verify_flow_steps(flow, &mut ctx);
        self.verify_return(flow, &ctx);
        self.verify_tenant_scope(flow);
        self.verify_capabilities(flow);
    }

    fn verify_return(&mut self, flow: &FlowDef, ctx: &FlowContext) {
        let span = flow.return_stmt.span;
        match &flow.return_stmt.body {
            Some(ReturnBody::Binding(name)) => {
                if !ctx.bindings.contains_key(name) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::UndefinedBinding,
                        message: format!(
                            "in flow '{}': RETURN references undefined binding '{}'",
                            flow.name, name
                        ),
                        span,
                        hint: None,
                    });
                }
            }
            Some(ReturnBody::Inline(fields)) => {
                self.verify_return_fields(flow, fields, ctx, span);
            }
            Some(ReturnBody::Paginated {
                items,
                total,
                cursor,
                has_more,
            }) => {
                self.verify_expr_bindings(flow, items, ctx, span);
                self.verify_expr_bindings(flow, total, ctx, span);
                self.verify_expr_bindings(flow, cursor, ctx, span);
                self.verify_expr_bindings(flow, has_more, ctx, span);
            }
            None => {}
        }

        for (_, expr) in &flow.return_stmt.headers {
            self.verify_expr_bindings(flow, expr, ctx, span);
            self.check_expr_types(flow, expr, ctx, span);
        }
    }

    fn verify_return_fields(
        &mut self,
        flow: &FlowDef,
        fields: &[ReturnField],
        ctx: &FlowContext,
        span: Span,
    ) {
        for field in fields {
            match &field.value {
                ReturnValue::Expr(expr) => {
                    self.verify_expr_bindings(flow, expr, ctx, span);
                    self.check_expr_types(flow, expr, ctx, span);
                }
                ReturnValue::Nested(sub) => {
                    self.verify_return_fields(flow, sub, ctx, span);
                }
            }
        }
    }

    fn verify_flow_ordering(&mut self, flow: &FlowDef) {
        #[derive(PartialOrd, Ord, PartialEq, Eq, Clone, Copy)]
        enum Phase {
            Declaration,
            Validation,
            Computation,
            Mutation,
            Effect,
        }

        let mut current_phase = Phase::Declaration;

        for step in &flow.steps {
            let step_phase = match step {
                FlowStep::Rule(_) | FlowStep::Guard(_) => Phase::Validation,
                FlowStep::Let(_) | FlowStep::Set(_) => Phase::Computation,
                FlowStep::Insert(_)
                | FlowStep::Upsert(_)
                | FlowStep::Update(_)
                | FlowStep::Delete(_)
                | FlowStep::Fanout(_) => Phase::Mutation,
                FlowStep::Effect(_) => Phase::Effect,
                FlowStep::Match(_) | FlowStep::Each(_) | FlowStep::Try(_) => Phase::Computation,
                FlowStep::Upload(_) => Phase::Mutation,
            };

            if step_phase < current_phase {
                let step_span = match step {
                    FlowStep::Rule(s) => s.span,
                    FlowStep::Guard(s) => s.span,
                    FlowStep::Let(s) => s.span,
                    FlowStep::Set(s) => s.span,
                    FlowStep::Insert(s) => s.span,
                    FlowStep::Upsert(s) => s.span,
                    FlowStep::Update(s) => s.span,
                    FlowStep::Delete(s) => s.span,
                    FlowStep::Fanout(s) => s.span,
                    FlowStep::Effect(s) => s.span,
                    FlowStep::Match(s) => s.span,
                    FlowStep::Each(s) => s.span,
                    FlowStep::Try(s) => s.span,
                    FlowStep::Upload(s) => s.span,
                };
                let phase_name = match current_phase {
                    Phase::Declaration => "declaration",
                    Phase::Validation => "validation",
                    Phase::Computation => "computation",
                    Phase::Mutation => "mutation",
                    Phase::Effect => "effect",
                };
                if matches!(
                    (step_phase, current_phase),
                    (Phase::Validation, Phase::Mutation | Phase::Effect)
                        | (Phase::Mutation, Phase::Effect)
                ) {
                    let step_name = match step_phase {
                        Phase::Declaration => "declaration",
                        Phase::Validation => "validation",
                        Phase::Computation => "computation",
                        Phase::Mutation => "mutation",
                        Phase::Effect => "effect",
                    };
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::OrderingViolation,
                        message: format!(
                            "in flow '{}': {} step appears after {} phase",
                            flow.name, step_name, phase_name
                        ),
                        span: step_span,
                        hint: Some("operations must follow order: declarations → validations → computations → mutations → effects → return".into()),
                    });
                }
            }

            if step_phase > current_phase {
                current_phase = step_phase;
            }
        }
    }

    fn verify_flow_steps(&mut self, flow: &FlowDef, ctx: &mut FlowContext) {
        for step in &flow.steps {
            match step {
                FlowStep::Rule(rule) => {
                    self.verify_rule_bindings(flow, rule, ctx);
                }
                FlowStep::Guard(guard) => {
                    self.verify_expr_bindings(flow, &guard.expr, ctx, guard.span);
                    self.check_expr_types(flow, &guard.expr, ctx, guard.span);
                    let gt = self.infer_type(&guard.expr, ctx, flow);
                    if gt != Ty::Unknown && gt != Ty::Bool {
                        self.type_error(
                            flow,
                            guard.span,
                            &format!("GUARD expression must be BOOL, got {gt}"),
                        );
                    }
                }
                FlowStep::Let(let_step) => {
                    self.verify_expr_bindings(flow, &let_step.expr, ctx, let_step.span);
                    self.verify_expr_totality(flow, &let_step.expr, let_step.span);
                    self.check_expr_types(flow, &let_step.expr, ctx, let_step.span);

                    if let Expr::Call {
                        or_code,
                        or_message,
                        service,
                        method,
                        ..
                    } = &let_step.expr
                    {
                        if *or_code == 0 && or_message.is_none() {
                            self.errors.push(VerifyError {
                                kind: VerifyErrorKind::MissingRequiredField,
                                message: format!(
                                    "in flow '{}': CALL {}.{} requires an OR clause",
                                    flow.name, service, method
                                ),
                                span: let_step.span,
                                hint: Some(
                                    "add OR <status_code> \"message\" for failure case".into(),
                                ),
                            });
                        }
                    }

                    let inferred = self.infer_type(&let_step.expr, ctx, flow);

                    if ctx.bindings.contains_key(&let_step.name) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::DuplicateBinding,
                            message: format!(
                                "in flow '{}': binding '{}' is already defined",
                                flow.name, let_step.name
                            ),
                            span: let_step.span,
                            hint: None,
                        });
                    } else {
                        ctx.bindings.insert(
                            let_step.name.clone(),
                            BindingInfo::typed(let_step.span, inferred),
                        );
                    }
                }
                FlowStep::Insert(insert) => {
                    self.verify_insert(flow, insert, ctx);
                }
                FlowStep::Upsert(upsert) => {
                    self.verify_upsert(flow, upsert, ctx);
                }
                FlowStep::Update(update) => {
                    self.verify_update(flow, update, ctx);
                }
                FlowStep::Delete(delete) => {
                    self.verify_delete(flow, delete, ctx);
                }
                FlowStep::Fanout(fanout) => {
                    self.verify_fanout(flow, fanout, ctx);
                }
                FlowStep::Effect(effect) => {
                    self.verify_effect_bindings(flow, effect, ctx);
                    ctx.accessed_effects.insert(match effect.kind {
                        EffectKind::Email => "email".into(),
                        EffectKind::PushNotification => "push_notification".into(),
                        EffectKind::Async => "async".into(),
                        EffectKind::Webhook => "webhook".into(),
                    });
                }
                FlowStep::Match(match_step) => {
                    self.verify_match(flow, match_step, ctx);
                }
                FlowStep::Set(set_step) => {
                    self.verify_expr_bindings(flow, &set_step.expr, ctx, set_step.span);
                    self.verify_expr_totality(flow, &set_step.expr, set_step.span);
                    self.check_expr_types(flow, &set_step.expr, ctx, set_step.span);

                    if !ctx.bindings.contains_key(&set_step.name) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::UndefinedBinding,
                            message: format!(
                                "in flow '{}': SET references undefined binding '{}'",
                                flow.name, set_step.name
                            ),
                            span: set_step.span,
                            hint: Some("binding must be declared with LET before SET".into()),
                        });
                    }
                }
                FlowStep::Each(each_step) => {
                    self.verify_expr_bindings(flow, &each_step.source, ctx, each_step.span);
                    self.check_expr_types(flow, &each_step.source, ctx, each_step.span);

                    let mut inner_ctx = FlowContext::new(flow);
                    inner_ctx.bindings = ctx.bindings.clone();
                    inner_ctx.body_fields = ctx.body_fields.clone();
                    inner_ctx.has_auth = ctx.has_auth;
                    inner_ctx.has_limit = ctx.has_limit;
                    inner_ctx.has_scope = ctx.has_scope;
                    inner_ctx.bindings.insert(
                        each_step.binding.clone(),
                        BindingInfo::computed(each_step.span),
                    );

                    self.verify_flow_steps_slice(flow, &each_step.steps, &mut inner_ctx);
                }
                FlowStep::Try(try_step) => {
                    self.verify_flow_steps_slice(flow, &try_step.body, ctx);
                    self.verify_flow_steps_slice(flow, &try_step.recover, ctx);
                }
                FlowStep::Upload(upload) => {
                    self.verify_expr_bindings(flow, &upload.file_expr, ctx, upload.span);
                    if !self.storages.contains(&upload.storage) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::UndefinedStorage,
                            message: format!(
                                "in flow '{}': UPLOAD references undefined storage '{}'",
                                flow.name, upload.storage
                            ),
                            span: upload.span,
                            hint: Some("define a STORAGE construct first".into()),
                        });
                    }
                    if ctx.bindings.contains_key(&upload.binding) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::DuplicateBinding,
                            message: format!(
                                "in flow '{}': UPLOAD binding '{}' is already defined",
                                flow.name, upload.binding
                            ),
                            span: upload.span,
                            hint: None,
                        });
                    } else {
                        ctx.bindings.insert(
                            upload.binding.clone(),
                            BindingInfo::typed(upload.span, Ty::String),
                        );
                    }
                }
            }
        }
    }

    fn verify_flow_steps_slice(
        &mut self,
        flow: &FlowDef,
        steps: &[FlowStep],
        ctx: &mut FlowContext,
    ) {
        for step in steps {
            match step {
                FlowStep::Rule(rule) => {
                    self.verify_rule_bindings(flow, rule, ctx);
                }
                FlowStep::Guard(guard) => {
                    self.verify_expr_bindings(flow, &guard.expr, ctx, guard.span);
                    self.check_expr_types(flow, &guard.expr, ctx, guard.span);
                }
                FlowStep::Let(let_step) => {
                    self.verify_expr_bindings(flow, &let_step.expr, ctx, let_step.span);
                    self.verify_expr_totality(flow, &let_step.expr, let_step.span);
                    self.check_expr_types(flow, &let_step.expr, ctx, let_step.span);
                    let inferred = self.infer_type(&let_step.expr, ctx, flow);
                    if !ctx.bindings.contains_key(&let_step.name) {
                        ctx.bindings.insert(
                            let_step.name.clone(),
                            BindingInfo::typed(let_step.span, inferred),
                        );
                    }
                }
                FlowStep::Set(set_step) => {
                    self.verify_expr_bindings(flow, &set_step.expr, ctx, set_step.span);
                    self.check_expr_types(flow, &set_step.expr, ctx, set_step.span);
                }
                FlowStep::Insert(insert) => {
                    self.verify_insert(flow, insert, ctx);
                }
                FlowStep::Upsert(upsert) => {
                    self.verify_upsert(flow, upsert, ctx);
                }
                FlowStep::Update(update) => {
                    self.verify_update(flow, update, ctx);
                }
                FlowStep::Delete(delete) => {
                    self.verify_delete(flow, delete, ctx);
                }
                FlowStep::Fanout(fanout) => {
                    self.verify_fanout(flow, fanout, ctx);
                }
                FlowStep::Effect(effect) => {
                    self.verify_effect_bindings(flow, effect, ctx);
                }
                FlowStep::Match(match_step) => {
                    self.verify_match(flow, match_step, ctx);
                }
                FlowStep::Each(each_step) => {
                    self.verify_expr_bindings(flow, &each_step.source, ctx, each_step.span);
                    self.check_expr_types(flow, &each_step.source, ctx, each_step.span);
                }
                FlowStep::Try(try_step) => {
                    self.verify_flow_steps_slice(flow, &try_step.body, ctx);
                    self.verify_flow_steps_slice(flow, &try_step.recover, ctx);
                }
                FlowStep::Upload(upload) => {
                    self.verify_expr_bindings(flow, &upload.file_expr, ctx, upload.span);
                    if !self.storages.contains(&upload.storage) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::UndefinedStorage,
                            message: format!(
                                "in flow '{}': UPLOAD references undefined storage '{}'",
                                flow.name, upload.storage
                            ),
                            span: upload.span,
                            hint: Some("define a STORAGE construct first".into()),
                        });
                    }
                    if !ctx.bindings.contains_key(&upload.binding) {
                        ctx.bindings.insert(
                            upload.binding.clone(),
                            BindingInfo::typed(upload.span, Ty::String),
                        );
                    }
                }
            }
        }
    }

    fn verify_rule_bindings(&mut self, flow: &FlowDef, rule: &RuleStep, ctx: &FlowContext) {
        for req in &rule.requires {
            self.verify_dotpath_binding(flow, &req.path, ctx, rule.span);
            self.verify_expr_bindings(flow, &req.value, ctx, rule.span);
            self.check_expr_types(flow, &req.value, ctx, rule.span);
            let lhs = self.resolve_dotpath_type(&req.path, ctx, flow);
            let rhs = self.infer_type(&req.value, ctx, flow);
            if lhs != Ty::Unknown && rhs != Ty::Unknown && !lhs.compatible_with(&rhs) {
                self.type_error(
                    flow,
                    rule.span,
                    &format!(
                        "RULE '{}': REQUIRE {} {:?} compares {} with {}",
                        rule.name,
                        req.path.as_str(),
                        req.op,
                        lhs,
                        rhs
                    ),
                );
            }
        }
    }

    fn verify_expr_bindings(&mut self, flow: &FlowDef, expr: &Expr, ctx: &FlowContext, span: Span) {
        match expr {
            Expr::DotPath(path) => {
                self.verify_dotpath_binding(flow, path, ctx, span);
            }
            Expr::Literal(_) => {}
            Expr::Unary { operand, .. } => {
                self.verify_expr_bindings(flow, operand, ctx, span);
            }
            Expr::Binary { left, right, .. } => {
                self.verify_expr_bindings(flow, left, ctx, span);
                self.verify_expr_bindings(flow, right, ctx, span);
            }
            Expr::Ternary { a, b, c, .. } => {
                self.verify_expr_bindings(flow, a, ctx, span);
                self.verify_expr_bindings(flow, b, ctx, span);
                self.verify_expr_bindings(flow, c, ctx, span);
            }
            Expr::If {
                cond, then, else_, ..
            } => {
                self.verify_expr_bindings(flow, cond, ctx, span);
                self.verify_expr_bindings(flow, then, ctx, span);
                self.verify_expr_bindings(flow, else_, ctx, span);
            }
            Expr::Fetch {
                source, filters, ..
            } => {
                self.verify_source_exists(flow, source, span);
                self.check_source_op(&flow.name, source, "FETCH", span);
                self.verify_index_coverage(flow, source, filters, span);
                for filter in filters {
                    self.verify_expr_bindings(flow, &filter.value, ctx, span);
                }
                ctx.accessed_sources.borrow_mut().insert(source.clone());
            }
            Expr::Query {
                source,
                filters,
                sorts: _,
                cursor,
                page_size,
                ..
            } => {
                self.verify_source_exists(flow, source, span);
                self.check_source_op(&flow.name, source, "QUERY", span);
                self.verify_index_coverage(flow, source, filters, span);
                for filter in filters {
                    self.verify_expr_bindings(flow, &filter.value, ctx, span);
                }
                if let Some(c) = cursor {
                    self.verify_expr_bindings(flow, c, ctx, span);
                }
                if let Some(ps) = page_size {
                    self.verify_expr_bindings(flow, ps, ctx, span);
                }
                ctx.accessed_sources.borrow_mut().insert(source.clone());
            }
            Expr::Call {
                service,
                method,
                args,
                ..
            } => {
                if let Some(svc) = self.services.get(service) {
                    if !svc.methods.contains(method) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::UndefinedService,
                            message: format!(
                                "in flow '{}': service '{}' has no method '{}'",
                                flow.name, service, method
                            ),
                            span,
                            hint: None,
                        });
                    }
                }
                for (_, val) in args {
                    self.verify_expr_bindings(flow, val, ctx, span);
                }
                ctx.accessed_services.borrow_mut().insert(service.clone());
            }
            Expr::Aggregate { source, .. } => {
                self.verify_expr_bindings(flow, source, ctx, span);
            }
            Expr::NowOffset { amount, .. } => {
                self.verify_expr_bindings(flow, amount, ctx, span);
            }
            Expr::Coalesce { value, default } => {
                self.verify_expr_bindings(flow, value, ctx, span);
                self.verify_expr_bindings(flow, default, ctx, span);
            }
            Expr::Cached { expr, .. } => {
                self.verify_expr_bindings(flow, expr, ctx, span);
            }
            Expr::WasmCall { .. } => {}
            Expr::MapExpr { source, .. } => {
                self.verify_expr_bindings(flow, source, ctx, span);
            }
            Expr::FilterExpr { source, condition } => {
                self.verify_expr_bindings(flow, source, ctx, span);
                self.verify_expr_bindings(flow, condition, ctx, span);
            }
            Expr::ReduceExpr { source, .. } => {
                self.verify_expr_bindings(flow, source, ctx, span);
            }
            Expr::SplitExpr { value, delimiter } => {
                self.verify_expr_bindings(flow, value, ctx, span);
                self.verify_expr_bindings(flow, delimiter, ctx, span);
            }
            Expr::ReplaceExpr { value, from, to } => {
                self.verify_expr_bindings(flow, value, ctx, span);
                self.verify_expr_bindings(flow, from, ctx, span);
                self.verify_expr_bindings(flow, to, ctx, span);
            }
            Expr::FormatExpr { args, .. } => {
                for arg in args {
                    self.verify_expr_bindings(flow, arg, ctx, span);
                }
            }
            Expr::FuncCall { args, .. } => {
                for arg in args {
                    self.verify_expr_bindings(flow, arg, ctx, span);
                }
            }
            Expr::Render { vars, .. } | Expr::Translate { vars, .. } => {
                for (_, val) in vars {
                    self.verify_expr_bindings(flow, val, ctx, span);
                }
            }
        }
    }

    fn resolve_shape_field_type(&self, shape_name: &str, field_name: &str) -> Ty {
        if let Some(shape) = self.shapes.get(shape_name) {
            if let Some(field) = shape.fields.get(field_name) {
                return Ty::from_type_expr(&field.ty);
            }
        }
        Ty::Unknown
    }

    fn source_shape_name(&self, source: &str) -> Option<&str> {
        self.sources.get(source).map(|s| s.shape_name.as_str())
    }

    fn resolve_dotpath_type(&self, path: &DotPath, ctx: &FlowContext, flow: &FlowDef) -> Ty {
        if path.segments.is_empty() {
            return Ty::Unknown;
        }
        let root = &path.segments[0];

        if path.segments.len() == 1 {
            if let Some(binding) = ctx.bindings.get(root) {
                return binding.ty.clone();
            }
            return Ty::Unknown;
        }

        if let Some(binding) = ctx.bindings.get(root) {
            let shape_name = match &binding.ty {
                Ty::Shape(name) => Some(name.clone()),
                Ty::List(inner) => {
                    if let Ty::Shape(name) = inner.as_ref() {
                        Some(name.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if path.segments.len() == 2 {
                if let Some(ref sn) = shape_name {
                    return self.resolve_shape_field_type(sn, &path.segments[1]);
                }
            }
        }

        if root == "body" && path.segments.len() == 2 {
            if let Some(ref body) = flow.body {
                if let Some(field) = body.fields.iter().find(|f| f.name == path.segments[1]) {
                    return Ty::from_type_expr(&field.ty);
                }
            }
        }

        if root == "query" && path.segments.len() == 2 {
            if let Some(param) = flow.params.iter().find(|p| p.name == path.segments[1]) {
                return Ty::from_type_expr(&param.ty);
            }
        }

        if root == "header" && path.segments.len() == 2 {
            if let Some(hdr) = flow.headers.iter().find(|h| h.name == path.segments[1]) {
                return Ty::from_type_expr(&hdr.ty);
            }
        }

        Ty::Unknown
    }

    fn infer_type(&self, expr: &Expr, ctx: &FlowContext, flow: &FlowDef) -> Ty {
        match expr {
            Expr::Literal(lit) => match lit {
                LiteralValue::Int(_) => Ty::Int,
                LiteralValue::Decimal(_) => Ty::Decimal,
                LiteralValue::String(_) => Ty::String,
                LiteralValue::Bool(_) => Ty::Bool,
                LiteralValue::Now => Ty::Timestamp,
                LiteralValue::None => Ty::Unknown,
                LiteralValue::Ident(_) => Ty::Unknown,
            },
            Expr::DotPath(path) => self.resolve_dotpath_type(path, ctx, flow),
            Expr::Unary { op, operand } => {
                let inner = self.infer_type(operand, ctx, flow);
                match op {
                    UnaryOp::Not => Ty::Bool,
                    UnaryOp::Empty | UnaryOp::Exists => Ty::Bool,
                    UnaryOp::Lower | UnaryOp::Upper | UnaryOp::Trim => Ty::String,
                    UnaryOp::Length => Ty::Int,
                    UnaryOp::Abs => inner,
                    UnaryOp::Ceil | UnaryOp::Floor => Ty::Int,
                    UnaryOp::ToInt => Ty::Int,
                    UnaryOp::ToDecimal => Ty::Decimal,
                    UnaryOp::ToString => Ty::String,
                    UnaryOp::Count => Ty::Int,
                    UnaryOp::First | UnaryOp::Last => {
                        if let Ty::List(inner) = &inner {
                            Ty::Maybe(inner.clone())
                        } else {
                            Ty::Maybe(Box::new(Ty::Unknown))
                        }
                    }
                }
            }
            Expr::Binary { op, left, .. } => {
                let lt = self.infer_type(left, ctx, flow);
                match op {
                    BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod => lt,
                    BinaryOp::And | BinaryOp::Or => Ty::Bool,
                    BinaryOp::Eq
                    | BinaryOp::Neq
                    | BinaryOp::Gt
                    | BinaryOp::Gte
                    | BinaryOp::Lt
                    | BinaryOp::Lte => Ty::Bool,
                    BinaryOp::Concat => Ty::String,
                    BinaryOp::StartsWith | BinaryOp::EndsWith | BinaryOp::Contains => Ty::Bool,
                    BinaryOp::DaysBetween | BinaryOp::HoursBetween | BinaryOp::MinutesBetween => {
                        Ty::Int
                    }
                    BinaryOp::Round => Ty::Decimal,
                    BinaryOp::Coalesce => lt.unwrap_maybe().clone(),
                    BinaryOp::FormatDate => Ty::String,
                }
            }
            Expr::Ternary { op, .. } => match op {
                TernaryOp::Substring => Ty::String,
                TernaryOp::Between => Ty::Bool,
            },
            Expr::If { then, .. } => self.infer_type(then, ctx, flow),
            Expr::Fetch { source, .. } => {
                if let Some(sn) = self.source_shape_name(source) {
                    Ty::Shape(sn.to_string())
                } else {
                    Ty::Unknown
                }
            }
            Expr::Query { source, .. } => {
                if let Some(sn) = self.source_shape_name(source) {
                    Ty::List(Box::new(Ty::Shape(sn.to_string())))
                } else {
                    Ty::List(Box::new(Ty::Unknown))
                }
            }
            Expr::Call { .. } | Expr::WasmCall { .. } => Ty::Unknown,
            Expr::Aggregate { op, source, .. } => match op {
                AggregateOp::Count => Ty::Int,
                AggregateOp::Avg => Ty::Decimal,
                AggregateOp::Sum | AggregateOp::Min | AggregateOp::Max => {
                    self.infer_type(source, ctx, flow)
                }
                AggregateOp::First | AggregateOp::Last => {
                    let inner = self.infer_type(source, ctx, flow);
                    if let Ty::List(elem) = inner {
                        Ty::Maybe(elem)
                    } else {
                        Ty::Maybe(Box::new(Ty::Unknown))
                    }
                }
            },
            Expr::NowOffset { .. } => Ty::Timestamp,
            Expr::Coalesce { value, .. } => {
                let vt = self.infer_type(value, ctx, flow);
                vt.unwrap_maybe().clone()
            }
            Expr::Cached { expr, .. } => self.infer_type(expr, ctx, flow),
            Expr::MapExpr { source, .. } => {
                // MAP always produces a list
                let inner = self.infer_type(source, ctx, flow);
                if let Ty::List(_) = inner {
                    inner
                } else {
                    Ty::List(Box::new(Ty::Unknown))
                }
            }
            Expr::FilterExpr { source, .. } => {
                // FILTER preserves the source type (list in, list out)
                self.infer_type(source, ctx, flow)
            }
            Expr::ReduceExpr { op, source, .. } => match op {
                AggregateOp::Count => Ty::Int,
                AggregateOp::Avg => Ty::Decimal,
                AggregateOp::Sum | AggregateOp::Min | AggregateOp::Max => {
                    self.infer_type(source, ctx, flow)
                }
                AggregateOp::First | AggregateOp::Last => {
                    let inner = self.infer_type(source, ctx, flow);
                    if let Ty::List(elem) = inner {
                        Ty::Maybe(elem)
                    } else {
                        Ty::Maybe(Box::new(Ty::Unknown))
                    }
                }
            },
            Expr::SplitExpr { .. } => Ty::List(Box::new(Ty::String)),
            Expr::ReplaceExpr { .. } => Ty::String,
            Expr::FormatExpr { .. } => Ty::String,
            Expr::Render { .. } => Ty::String,
            Expr::Translate { .. } => Ty::String,
            Expr::FuncCall { .. } => Ty::Unknown,
        }
    }

    fn check_expr_types(&mut self, flow: &FlowDef, expr: &Expr, ctx: &FlowContext, span: Span) {
        match expr {
            Expr::Unary { op, operand } => {
                self.check_expr_types(flow, operand, ctx, span);
                let inner = self.infer_type(operand, ctx, flow);
                if inner == Ty::Unknown {
                    return;
                }
                match op {
                    UnaryOp::Not => {
                        if inner != Ty::Bool {
                            self.type_error(flow, span, &format!("NOT requires BOOL, got {inner}"));
                        }
                    }
                    UnaryOp::Empty | UnaryOp::Exists => {
                        if !matches!(inner, Ty::List(_)) {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires LIST, got {inner}"),
                            );
                        }
                    }
                    UnaryOp::Lower | UnaryOp::Upper | UnaryOp::Trim | UnaryOp::Length => {
                        if !inner.is_stringlike() {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires STRING, got {inner}"),
                            );
                        }
                    }
                    UnaryOp::Abs => {
                        if !inner.is_numeric() {
                            self.type_error(
                                flow,
                                span,
                                &format!("ABS requires numeric type, got {inner}"),
                            );
                        }
                    }
                    UnaryOp::Ceil | UnaryOp::Floor => {
                        if inner != Ty::Decimal {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires DECIMAL, got {inner}"),
                            );
                        }
                    }
                    UnaryOp::ToInt => {
                        if !matches!(inner, Ty::Decimal | Ty::String | Ty::Text) {
                            self.type_error(
                                flow,
                                span,
                                &format!("TO_INT requires DECIMAL or STRING, got {inner}"),
                            );
                        }
                    }
                    UnaryOp::ToDecimal => {
                        if !matches!(inner, Ty::Int | Ty::String | Ty::Text) {
                            self.type_error(
                                flow,
                                span,
                                &format!("TO_DECIMAL requires INT or STRING, got {inner}"),
                            );
                        }
                    }
                    UnaryOp::ToString => {}
                    UnaryOp::Count | UnaryOp::First | UnaryOp::Last => {}
                }
            }
            Expr::Binary { op, left, right } => {
                self.check_expr_types(flow, left, ctx, span);
                self.check_expr_types(flow, right, ctx, span);
                let lt = self.infer_type(left, ctx, flow);
                let rt = self.infer_type(right, ctx, flow);
                if lt == Ty::Unknown || rt == Ty::Unknown {
                    return;
                }
                match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                        if matches!(lt, Ty::Maybe(_)) {
                            self.type_error(flow, span, &format!("{op:?} cannot operate on MAYBE — use COALESCE to unwrap, got {lt}"));
                        } else if matches!(rt, Ty::Maybe(_)) {
                            self.type_error(flow, span, &format!("{op:?} cannot operate on MAYBE — use COALESCE to unwrap, got {rt}"));
                        } else if !lt.is_numeric() {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires numeric types, got {lt}"),
                            );
                        } else if lt != rt {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires same numeric type, got {lt} and {rt}"),
                            );
                        }
                    }
                    BinaryOp::Mod => {
                        if lt != Ty::Int || rt != Ty::Int {
                            self.type_error(
                                flow,
                                span,
                                &format!("MOD requires INT, got {lt} and {rt}"),
                            );
                        }
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        if lt != Ty::Bool {
                            self.type_error(flow, span, &format!("{op:?} requires BOOL, got {lt}"));
                        }
                        if rt != Ty::Bool {
                            self.type_error(flow, span, &format!("{op:?} requires BOOL, got {rt}"));
                        }
                    }
                    BinaryOp::Eq | BinaryOp::Neq => {
                        if !lt.compatible_with(&rt) {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires same type, got {lt} and {rt}"),
                            );
                        }
                    }
                    BinaryOp::Gt | BinaryOp::Gte | BinaryOp::Lt | BinaryOp::Lte => {
                        if !lt.is_orderable() {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires orderable type, got {lt}"),
                            );
                        }
                        if !lt.compatible_with(&rt) {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires same type, got {lt} and {rt}"),
                            );
                        }
                    }
                    BinaryOp::Concat => {
                        if !lt.is_stringlike() {
                            self.type_error(
                                flow,
                                span,
                                &format!("CONCAT requires STRING, got {lt}"),
                            );
                        }
                    }
                    BinaryOp::StartsWith | BinaryOp::EndsWith | BinaryOp::Contains => {
                        if !lt.is_stringlike() || !rt.is_stringlike() {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires STRING, got {lt} and {rt}"),
                            );
                        }
                    }
                    BinaryOp::DaysBetween => {
                        if *lt.unwrap_maybe() != Ty::Date {
                            self.type_error(
                                flow,
                                span,
                                &format!("DAYS_BETWEEN requires DATE, got {lt} and {rt}"),
                            );
                        }
                    }
                    BinaryOp::HoursBetween | BinaryOp::MinutesBetween => {
                        if *lt.unwrap_maybe() != Ty::Timestamp {
                            self.type_error(
                                flow,
                                span,
                                &format!("{op:?} requires TIMESTAMP, got {lt} and {rt}"),
                            );
                        }
                    }
                    BinaryOp::Round => {
                        if *lt.unwrap_maybe() != Ty::Decimal {
                            self.type_error(
                                flow,
                                span,
                                &format!("ROUND requires DECIMAL, got {lt}"),
                            );
                        }
                        if *rt.unwrap_maybe() != Ty::Int {
                            self.type_error(
                                flow,
                                span,
                                &format!("ROUND scale requires INT, got {rt}"),
                            );
                        }
                    }
                    BinaryOp::Coalesce => {
                        if !matches!(lt, Ty::Maybe(_)) {
                            self.type_error(
                                flow,
                                span,
                                &format!("COALESCE first argument must be MAYBE, got {lt}"),
                            );
                        }
                    }
                    BinaryOp::FormatDate => {
                        if !matches!(lt.unwrap_maybe(), Ty::Date | Ty::Timestamp) {
                            self.type_error(
                                flow,
                                span,
                                &format!("FORMAT_DATE requires DATE or TIMESTAMP, got {lt}"),
                            );
                        }
                    }
                }
            }
            Expr::Ternary { op, a, b, c } => {
                self.check_expr_types(flow, a, ctx, span);
                self.check_expr_types(flow, b, ctx, span);
                self.check_expr_types(flow, c, ctx, span);
                let at = self.infer_type(a, ctx, flow);
                if at == Ty::Unknown {
                    return;
                }
                match op {
                    TernaryOp::Substring => {
                        if !at.is_stringlike() {
                            self.type_error(
                                flow,
                                span,
                                &format!("SUBSTRING requires STRING, got {at}"),
                            );
                        }
                    }
                    TernaryOp::Between => {
                        if !at.is_orderable() {
                            self.type_error(
                                flow,
                                span,
                                &format!("BETWEEN requires orderable type, got {at}"),
                            );
                        }
                    }
                }
            }
            Expr::If { cond, then, else_ } => {
                self.check_expr_types(flow, cond, ctx, span);
                self.check_expr_types(flow, then, ctx, span);
                self.check_expr_types(flow, else_, ctx, span);
                let ct = self.infer_type(cond, ctx, flow);
                if ct != Ty::Unknown && ct != Ty::Bool {
                    self.type_error(flow, span, &format!("IF condition must be BOOL, got {ct}"));
                }
                let tt = self.infer_type(then, ctx, flow);
                let et = self.infer_type(else_, ctx, flow);
                if tt != Ty::Unknown && et != Ty::Unknown && !tt.compatible_with(&et) {
                    self.type_error(
                        flow,
                        span,
                        &format!("IF branches must return same type, got {tt} and {et}"),
                    );
                }
            }
            Expr::Coalesce { value, default } => {
                self.check_expr_types(flow, value, ctx, span);
                self.check_expr_types(flow, default, ctx, span);
                let vt = self.infer_type(value, ctx, flow);
                if vt != Ty::Unknown && !matches!(vt, Ty::Maybe(_)) {
                    self.type_error(
                        flow,
                        span,
                        &format!("COALESCE first argument must be MAYBE, got {vt}"),
                    );
                }
            }
            Expr::Fetch {
                source, filters, ..
            } => {
                let sn = self.source_shape_name(source).map(|s| s.to_string());
                for filter in filters {
                    self.check_expr_types(flow, &filter.value, ctx, span);
                    if let Some(ref shape_name) = sn {
                        let field_ty = self.resolve_shape_field_type(shape_name, &filter.field);
                        let val_ty = self.infer_type(&filter.value, ctx, flow);
                        if field_ty != Ty::Unknown
                            && val_ty != Ty::Unknown
                            && !field_ty.compatible_with(&val_ty)
                        {
                            self.type_error(
                                flow,
                                span,
                                &format!(
                                    "FILTER '{}' expects {}, got {}",
                                    filter.field, field_ty, val_ty
                                ),
                            );
                        }
                    }
                }
            }
            Expr::Query {
                source,
                filters,
                cursor,
                page_size,
                ..
            } => {
                let sn = self.source_shape_name(source).map(|s| s.to_string());
                for filter in filters {
                    self.check_expr_types(flow, &filter.value, ctx, span);
                    if let Some(ref shape_name) = sn {
                        let field_ty = self.resolve_shape_field_type(shape_name, &filter.field);
                        let val_ty = self.infer_type(&filter.value, ctx, flow);
                        if field_ty != Ty::Unknown
                            && val_ty != Ty::Unknown
                            && !field_ty.compatible_with(&val_ty)
                        {
                            self.type_error(
                                flow,
                                span,
                                &format!(
                                    "FILTER '{}' expects {}, got {}",
                                    filter.field, field_ty, val_ty
                                ),
                            );
                        }
                    }
                }
                if let Some(c) = cursor {
                    self.check_expr_types(flow, c, ctx, span);
                }
                if let Some(ps) = page_size {
                    self.check_expr_types(flow, ps, ctx, span);
                }
            }
            Expr::Call { args, .. } => {
                for (_, val) in args {
                    self.check_expr_types(flow, val, ctx, span);
                }
            }
            Expr::Aggregate { source, .. } => {
                self.check_expr_types(flow, source, ctx, span);
            }
            Expr::NowOffset { amount, .. } => {
                self.check_expr_types(flow, amount, ctx, span);
                let at = self.infer_type(amount, ctx, flow);
                if at != Ty::Unknown && at != Ty::Int {
                    self.type_error(
                        flow,
                        span,
                        &format!("NOW_PLUS/NOW_MINUS amount must be INT, got {at}"),
                    );
                }
            }
            Expr::Cached { expr, .. } => {
                self.check_expr_types(flow, expr, ctx, span);
            }
            Expr::Literal(_) | Expr::DotPath(_) | Expr::WasmCall { .. } => {}
            Expr::MapExpr { source, .. } => {
                self.check_expr_types(flow, source, ctx, span);
            }
            Expr::FilterExpr { source, condition } => {
                self.check_expr_types(flow, source, ctx, span);
                self.check_expr_types(flow, condition, ctx, span);
            }
            Expr::ReduceExpr { source, .. } => {
                self.check_expr_types(flow, source, ctx, span);
            }
            Expr::SplitExpr { value, delimiter } => {
                self.check_expr_types(flow, value, ctx, span);
                self.check_expr_types(flow, delimiter, ctx, span);
                let vt = self.infer_type(value, ctx, flow);
                if vt != Ty::Unknown && !vt.is_stringlike() {
                    self.type_error(flow, span, &format!("SPLIT requires STRING, got {vt}"));
                }
                let dt = self.infer_type(delimiter, ctx, flow);
                if dt != Ty::Unknown && !dt.is_stringlike() {
                    self.type_error(
                        flow,
                        span,
                        &format!("SPLIT delimiter requires STRING, got {dt}"),
                    );
                }
            }
            Expr::ReplaceExpr { value, from, to } => {
                self.check_expr_types(flow, value, ctx, span);
                self.check_expr_types(flow, from, ctx, span);
                self.check_expr_types(flow, to, ctx, span);
                let vt = self.infer_type(value, ctx, flow);
                if vt != Ty::Unknown && !vt.is_stringlike() {
                    self.type_error(flow, span, &format!("REPLACE requires STRING, got {vt}"));
                }
            }
            Expr::FormatExpr { args, .. } => {
                for arg in args {
                    self.check_expr_types(flow, arg, ctx, span);
                }
            }
            Expr::FuncCall { args, .. } => {
                for arg in args {
                    self.check_expr_types(flow, arg, ctx, span);
                }
            }
            Expr::Render { vars, .. } | Expr::Translate { vars, .. } => {
                for (_, val) in vars {
                    self.check_expr_types(flow, val, ctx, span);
                }
            }
        }
    }

    fn type_error(&mut self, flow: &FlowDef, span: Span, msg: &str) {
        self.errors.push(VerifyError {
            kind: VerifyErrorKind::TypeMismatch,
            message: format!("in flow '{}': {}", flow.name, msg),
            span,
            hint: None,
        });
    }

    fn verify_dotpath_binding(
        &mut self,
        flow: &FlowDef,
        path: &DotPath,
        ctx: &FlowContext,
        span: Span,
    ) {
        if path.segments.is_empty() {
            return;
        }
        let root = &path.segments[0];
        if path.segments.len() == 1 && !ctx.bindings.contains_key(root) {
            // single-segment path not in bindings — bare enum literal (e.g. "pending", "active")
            return;
        }
        if path.segments.len() > 1 && !ctx.bindings.contains_key(root) {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::UndefinedBinding,
                message: format!("in flow '{}': undefined binding '{}'", flow.name, root),
                span,
                hint: Some("bindings must be declared with LET before use".into()),
            });
        }
    }

    fn check_source_op(&mut self, flow_name: &str, source: &str, op: &str, span: Span) {
        if let Some(info) = self.sources.get(source) {
            let allowed = match (&info.source_type, op) {
                (SourceType::Postgres | SourceType::Mysql | SourceType::Sqlite, _) => true,
                (SourceType::Redis, "FETCH" | "INSERT" | "DELETE") => true,
                (SourceType::Redis, _) => false,
                (SourceType::Elasticsearch, "QUERY") => true,
                (SourceType::Elasticsearch, _) => false,
                (SourceType::Dynamodb, "FETCH" | "QUERY" | "INSERT" | "UPDATE" | "DELETE") => true,
                (SourceType::Dynamodb, _) => false,
            };
            if !allowed {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::TypeMismatch,
                    message: format!(
                        "in flow '{}': {} on source '{}' not allowed for {:?}",
                        flow_name, op, source, info.source_type
                    ),
                    span,
                    hint: None,
                });
            }
        }
    }

    fn verify_source_exists(&mut self, flow: &FlowDef, source: &str, span: Span) {
        if !self.sources.contains_key(source) {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::UndefinedSource,
                message: format!("in flow '{}': undefined source '{}'", flow.name, source),
                span,
                hint: Some(format!("add SOURCE {source} definition")),
            });
        }
    }

    fn verify_expr_totality(&mut self, flow: &FlowDef, expr: &Expr, span: Span) {
        match expr {
            Expr::If { else_, .. } => {
                if matches!(**else_, Expr::Literal(LiteralValue::None)) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::MissingElse,
                        message: format!("in flow '{}': IF expression must have ELSE", flow.name),
                        span,
                        hint: None,
                    });
                }
            }
            Expr::Fetch { .. } => {
                // OR is mandatory in the parser for FETCH.
                // OR 0 is valid: means "nullable fetch, return null if not found".
            }
            _ => {}
        }
    }

    fn verify_index_coverage(
        &mut self,
        flow: &FlowDef,
        source: &str,
        filters: &[FilterClause],
        span: Span,
    ) {
        let source_info = match self.sources.get(source) {
            Some(s) => s,
            None => return,
        };

        if filters.is_empty() {
            return;
        }

        let filter_fields: HashSet<&str> = filters.iter().map(|f| f.field.as_str()).collect();
        let covered = source_info.indexes.iter().any(|index| {
            // index covers query if the leading columns of the index appear in filters
            index
                .first()
                .is_some_and(|first| filter_fields.contains(first.as_str()))
        });

        // Also check if filtering by PK
        let filtering_by_pk = if let Some(shape) = self.shapes.get(&source_info.shape_name) {
            filter_fields
                .iter()
                .any(|f| shape.fields.get(*f).is_some_and(|info| info.is_pk))
        } else {
            false
        };

        if !covered && !filtering_by_pk {
            let suggested: String = filter_fields.iter().copied().collect::<Vec<_>>().join(" ");
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::MissingIndex,
                message: format!(
                    "in flow '{}': QUERY on '{}' with filters [{}] is not covered by any index",
                    flow.name, source, suggested
                ),
                span,
                hint: Some(format!("add INDEX {} to SOURCE {}", suggested, source)),
            });
        }
    }

    fn verify_insert(&mut self, flow: &FlowDef, insert: &InsertStep, ctx: &mut FlowContext) {
        self.check_source_op(&flow.name, &insert.source, "INSERT", insert.span);

        struct FieldSnapshot {
            ty: Ty,
            is_auto: bool,
            is_required: bool,
            has_default: bool,
        }

        let shape_snapshot: Option<(String, HashMap<String, FieldSnapshot>)> =
            self.sources.get(&insert.source).and_then(|si| {
                self.shapes.get(&si.shape_name).map(|shape| {
                    let fields = shape
                        .fields
                        .iter()
                        .map(|(name, fi)| {
                            (
                                name.clone(),
                                FieldSnapshot {
                                    ty: Ty::from_type_expr(&fi.ty),
                                    is_auto: fi.is_auto,
                                    is_required: fi.is_required,
                                    has_default: fi.has_default,
                                },
                            )
                        })
                        .collect();
                    (si.shape_name.clone(), fields)
                })
            });

        if let Some((shape_name, fields)) = shape_snapshot {
            let provided: HashSet<&str> = insert.fields.iter().map(|f| f.0.as_str()).collect();

            for (name, fs) in &fields {
                if fs.is_auto && provided.contains(name.as_str()) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::AutoFieldInInsert,
                        message: format!(
                            "in flow '{}': AUTO field '{}' cannot be in INSERT",
                            flow.name, name
                        ),
                        span: insert.span,
                        hint: Some(
                            "remove AUTO fields from INSERT — runtime generates them".into(),
                        ),
                    });
                }

                if fs.is_required
                    && !fs.is_auto
                    && !fs.has_default
                    && !provided.contains(name.as_str())
                {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::MissingRequiredField,
                        message: format!(
                            "in flow '{}': INSERT into '{}' missing REQUIRED field '{}'",
                            flow.name, insert.source, name
                        ),
                        span: insert.span,
                        hint: None,
                    });
                }
            }

            for (field_name, value) in &insert.fields {
                if let Some(fs) = fields.get(field_name) {
                    let expected = &fs.ty;
                    let actual = self.infer_type(value, ctx, flow);
                    if actual != Ty::Unknown && !expected.compatible_with(&actual) {
                        self.type_error(
                            flow,
                            insert.span,
                            &format!(
                                "INSERT field '{}' expects {}, got {}",
                                field_name, expected, actual
                            ),
                        );
                    }
                } else {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::UndefinedBinding,
                        message: format!(
                            "in flow '{}': INSERT field '{}' does not exist in shape '{}'",
                            flow.name, field_name, shape_name
                        ),
                        span: insert.span,
                        hint: None,
                    });
                }
            }
        } else {
            self.verify_source_exists(flow, &insert.source, insert.span);
        }

        for (_, value) in &insert.fields {
            self.verify_expr_bindings(flow, value, ctx, insert.span);
        }

        if let Some(ref binding) = insert.binding {
            let ty = self
                .source_shape_name(&insert.source)
                .map(|s| Ty::Shape(s.to_string()))
                .unwrap_or(Ty::Unknown);
            ctx.bindings
                .insert(binding.clone(), BindingInfo::typed(insert.span, ty));
        }

        ctx.accessed_sources
            .borrow_mut()
            .insert(insert.source.clone());
        ctx.writes_sources.insert(insert.source.clone());
    }

    fn verify_upsert(&mut self, flow: &FlowDef, upsert: &UpsertStep, ctx: &mut FlowContext) {
        if upsert.keys.is_empty() {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::MissingRequiredField,
                message: format!(
                    "in flow '{}': UPSERT on '{}' requires at least one KEY",
                    flow.name, upsert.source
                ),
                span: upsert.span,
                hint: Some("add KEY fields matching a PK or UNIQUE index".into()),
            });
        }
        if upsert.sets.is_empty() {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::MissingRequiredField,
                message: format!(
                    "in flow '{}': UPSERT on '{}' requires at least one SET",
                    flow.name, upsert.source
                ),
                span: upsert.span,
                hint: None,
            });
        }

        let key_names: Vec<String> = upsert.keys.iter().map(|(name, _)| name.clone()).collect();
        let unique_key_names: HashSet<&str> = key_names.iter().map(String::as_str).collect();
        if unique_key_names.len() != key_names.len() {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::PolicyViolation,
                message: format!(
                    "in flow '{}': UPSERT on '{}' contains duplicate KEY fields",
                    flow.name, upsert.source
                ),
                span: upsert.span,
                hint: Some("declare each conflict key exactly once".into()),
            });
        }
        let set_names: Vec<&str> = upsert.sets.iter().map(|set| set.field.as_str()).collect();
        let unique_set_names: HashSet<&str> = set_names.iter().copied().collect();
        if unique_set_names.len() != set_names.len() {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::PolicyViolation,
                message: format!(
                    "in flow '{}': UPSERT on '{}' contains duplicate SET fields",
                    flow.name, upsert.source
                ),
                span: upsert.span,
                hint: Some("assign each updated field exactly once".into()),
            });
        }
        if let Some(overlap) = set_names
            .iter()
            .find(|name| unique_key_names.contains(**name))
        {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::PolicyViolation,
                message: format!(
                    "in flow '{}': UPSERT field '{}' cannot be both KEY and SET",
                    flow.name, overlap
                ),
                span: upsert.span,
                hint: Some("remove the SET for conflict-key fields".into()),
            });
        }
        if let Some(source) = self.sources.get(&upsert.source) {
            let pk_match = self.shapes.get(&source.shape_name).is_some_and(|shape| {
                key_names.len() == 1
                    && shape
                        .fields
                        .get(&key_names[0])
                        .is_some_and(|field| field.is_pk)
            });
            let unique_match = source
                .unique_indexes
                .iter()
                .any(|index| index == &key_names);
            if !pk_match && !unique_match {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::MissingIndex,
                    message: format!(
                        "in flow '{}': UPSERT keys [{}] on '{}' must match a PK or UNIQUE index",
                        flow.name,
                        key_names.join(" "),
                        upsert.source
                    ),
                    span: upsert.span,
                    hint: Some(format!(
                        "add INDEX {} UNIQUE to SOURCE {}",
                        key_names.join(" "),
                        upsert.source
                    )),
                });
            }
        }

        let mut fields = upsert.keys.clone();
        fields.extend(
            upsert
                .sets
                .iter()
                .map(|set| (set.field.clone(), set.value.clone())),
        );
        let insert = InsertStep {
            source: upsert.source.clone(),
            fields,
            binding: upsert.binding.clone(),
            span: upsert.span,
        };
        self.verify_insert(flow, &insert, ctx);
    }

    fn verify_fanout(&mut self, flow: &FlowDef, fanout: &FanoutStep, ctx: &mut FlowContext) {
        self.verify_expr_bindings(flow, &fanout.source, ctx, fanout.span);
        self.check_expr_types(flow, &fanout.source, ctx, fanout.span);
        let source_ty = self.infer_type(&fanout.source, ctx, flow);
        if !matches!(source_ty, Ty::List(_) | Ty::Unknown) {
            self.type_error(
                flow,
                fanout.span,
                &format!("FANOUT source must be LIST, got {source_ty}"),
            );
        }
        if fanout.insert.binding.is_some() {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::PolicyViolation,
                message: format!(
                    "in flow '{}': FANOUT INSERT cannot declare AS binding",
                    flow.name
                ),
                span: fanout.span,
                hint: Some("remove AS; FANOUT is one atomic bulk mutation".into()),
            });
        }
        if ctx.bindings.contains_key(&fanout.binding) {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::DuplicateBinding,
                message: format!(
                    "in flow '{}': FANOUT binding '{}' is already defined",
                    flow.name, fanout.binding
                ),
                span: fanout.span,
                hint: Some("choose a unique item binding".into()),
            });
        }
        if let Some(source) = self.sources.get(&fanout.insert.source) {
            if !matches!(
                source.source_type,
                SourceType::Postgres | SourceType::Mysql | SourceType::Sqlite
            ) {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::PolicyViolation,
                    message: format!(
                        "in flow '{}': FANOUT target '{}' must be transactional SQL",
                        flow.name, fanout.insert.source
                    ),
                    span: fanout.span,
                    hint: Some("use POSTGRES, MYSQL, or SQLITE".into()),
                });
            }
        }

        let mut inner_ctx = FlowContext::new(flow);
        inner_ctx.bindings = ctx.bindings.clone();
        inner_ctx.body_fields = ctx.body_fields.clone();
        inner_ctx.has_auth = ctx.has_auth;
        inner_ctx.has_limit = ctx.has_limit;
        inner_ctx.has_scope = ctx.has_scope;
        inner_ctx
            .bindings
            .insert(fanout.binding.clone(), BindingInfo::computed(fanout.span));
        self.verify_insert(flow, &fanout.insert, &mut inner_ctx);
        ctx.accessed_sources
            .borrow_mut()
            .insert(fanout.insert.source.clone());
        ctx.writes_sources.insert(fanout.insert.source.clone());
    }

    fn verify_update(&mut self, flow: &FlowDef, update: &UpdateStep, ctx: &mut FlowContext) {
        self.verify_source_exists(flow, &update.source, update.span);
        self.check_source_op(&flow.name, &update.source, "UPDATE", update.span);

        if update.or_code == 0 && update.or_message.is_none() {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::MissingRequiredField,
                message: format!(
                    "in flow '{}': UPDATE on '{}' requires an OR clause",
                    flow.name, update.source
                ),
                span: update.span,
                hint: Some("add OR <status_code> \"message\" for zero-row case".into()),
            });
        }

        let shape_name = self
            .source_shape_name(&update.source)
            .map(|s| s.to_string());

        for wh in &update.wheres {
            self.verify_expr_bindings(flow, &wh.value, ctx, update.span);
            self.check_expr_types(flow, &wh.value, ctx, update.span);
        }
        for set in &update.sets {
            self.verify_expr_bindings(flow, &set.value, ctx, update.span);
            self.check_expr_types(flow, &set.value, ctx, update.span);
            if let Some(ref sn) = shape_name {
                let expected = self.resolve_shape_field_type(sn, &set.field);
                let actual = self.infer_type(&set.value, ctx, flow);
                if expected != Ty::Unknown
                    && actual != Ty::Unknown
                    && !expected.compatible_with(&actual)
                {
                    self.type_error(
                        flow,
                        update.span,
                        &format!(
                            "UPDATE SET '{}' expects {}, got {}",
                            set.field, expected, actual
                        ),
                    );
                }
            }
        }

        if let Some(ref binding) = update.binding {
            let (name, ty) = match binding {
                UpdateBinding::As(n) => {
                    let ty = self
                        .source_shape_name(&update.source)
                        .map(|s| Ty::Shape(s.to_string()))
                        .unwrap_or(Ty::Unknown);
                    (n.clone(), ty)
                }
                UpdateBinding::Count(n) => (n.clone(), Ty::Int),
            };
            ctx.bindings
                .insert(name, BindingInfo::typed(update.span, ty));
        }

        ctx.accessed_sources
            .borrow_mut()
            .insert(update.source.clone());
        ctx.writes_sources.insert(update.source.clone());
    }

    fn verify_delete(&mut self, flow: &FlowDef, delete: &DeleteStep, ctx: &mut FlowContext) {
        self.verify_source_exists(flow, &delete.source, delete.span);
        self.check_source_op(&flow.name, &delete.source, "DELETE", delete.span);

        if delete.or_code == 0 && delete.or_message.is_none() {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::MissingRequiredField,
                message: format!(
                    "in flow '{}': DELETE from '{}' requires an OR clause",
                    flow.name, delete.source
                ),
                span: delete.span,
                hint: Some("add OR <status_code> \"message\" for zero-row case".into()),
            });
        }

        if delete.wheres.is_empty() {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::MissingRequiredField,
                message: format!(
                    "in flow '{}': DELETE from '{}' must have at least one WHERE clause",
                    flow.name, delete.source
                ),
                span: delete.span,
                hint: Some("unqualified DELETE is not allowed".into()),
            });
        }

        for wh in &delete.wheres {
            self.verify_expr_bindings(flow, &wh.value, ctx, delete.span);
        }

        ctx.accessed_sources
            .borrow_mut()
            .insert(delete.source.clone());
        ctx.writes_sources.insert(delete.source.clone());
    }

    fn verify_effect_bindings(&mut self, flow: &FlowDef, effect: &EffectStep, ctx: &FlowContext) {
        for field in &effect.fields {
            match field {
                EffectField::To(expr) => {
                    self.verify_expr_bindings(flow, expr, ctx, effect.span);
                }
                EffectField::Data(exprs) => {
                    for expr in exprs {
                        self.verify_expr_bindings(flow, expr, ctx, effect.span);
                    }
                }
                EffectField::Url(expr) => {
                    self.verify_expr_bindings(flow, expr, ctx, effect.span);
                }
                EffectField::Template(_) | EffectField::Event(_) | EffectField::Task(_) => {}
            }
        }
    }

    fn verify_match(&mut self, flow: &FlowDef, match_step: &MatchStep, ctx: &mut FlowContext) {
        if match_step.default.is_none() {
            self.errors.push(VerifyError {
                kind: VerifyErrorKind::MissingElse,
                message: format!("in flow '{}': MATCH requires a DEFAULT branch", flow.name),
                span: match_step.span,
                hint: Some("add a DEFAULT branch for exhaustiveness".into()),
            });
        }

        let mut branch_bindings: Vec<HashSet<String>> = Vec::new();

        for branch in &match_step.branches {
            self.verify_expr_bindings(flow, &branch.condition, ctx, match_step.span);
            let bindings = collect_step_bindings(&branch.steps);
            for step in &branch.steps {
                if matches!(step, FlowStep::Match(_)) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::OrderingViolation,
                        message: format!(
                            "in flow '{}': MATCH inside MATCH is not allowed",
                            flow.name
                        ),
                        span: match_step.span,
                        hint: Some("flatten nested MATCH into separate flows or use IF".into()),
                    });
                }
            }
            branch_bindings.push(bindings);
        }
        if let Some(default) = &match_step.default {
            let bindings = collect_step_bindings(default);
            for step in default {
                if matches!(step, FlowStep::Match(_)) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::OrderingViolation,
                        message: format!(
                            "in flow '{}': MATCH inside MATCH is not allowed",
                            flow.name
                        ),
                        span: match_step.span,
                        hint: Some("flatten nested MATCH into separate flows or use IF".into()),
                    });
                }
            }
            branch_bindings.push(bindings);
        }

        if branch_bindings.len() >= 2 {
            let first = &branch_bindings[0];
            for (i, other) in branch_bindings.iter().enumerate().skip(1) {
                if first != other {
                    let missing_from_other: Vec<_> = first.difference(other).collect();
                    let missing_from_first: Vec<_> = other.difference(first).collect();
                    let mut diffs = Vec::new();
                    if !missing_from_other.is_empty() {
                        diffs.push(format!(
                            "branch {} missing: {}",
                            i + 1,
                            missing_from_other
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    if !missing_from_first.is_empty() {
                        diffs.push(format!(
                            "branch 1 missing: {}",
                            missing_from_first
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::TypeMismatch,
                        message: format!(
                            "in flow '{}': MATCH branches produce inconsistent bindings — {}",
                            flow.name,
                            diffs.join("; ")
                        ),
                        span: match_step.span,
                        hint: Some("all branches must produce the same set of bindings".into()),
                    });
                    break;
                }
            }
            if let Some(first) = branch_bindings.first() {
                for name in first {
                    ctx.bindings
                        .insert(name.clone(), BindingInfo::computed(match_step.span));
                }
            }
        }
    }

    fn verify_tenant_scope(&mut self, flow: &FlowDef) {
        let realm_name = match &flow.realm {
            Some(r) => r,
            None => return,
        };

        let realm = match self.realms.get(realm_name) {
            Some(r) => r,
            None => return,
        };

        if realm.tenant_field.is_none() {
            return;
        }

        if flow.scope.is_none() {
            let accesses_tenanted = flow.steps.iter().any(|step| match step {
                FlowStep::Let(l) => self.expr_accesses_source(&l.expr),
                FlowStep::Set(s) => self.expr_accesses_source(&s.expr),
                FlowStep::Insert(i) => self.sources.contains_key(&i.source),
                FlowStep::Upsert(u) => self.sources.contains_key(&u.source),
                FlowStep::Update(u) => self.sources.contains_key(&u.source),
                FlowStep::Delete(d) => self.sources.contains_key(&d.source),
                FlowStep::Fanout(f) => self.sources.contains_key(&f.insert.source),
                FlowStep::Guard(g) => self.expr_accesses_source(&g.expr),
                FlowStep::Each(e) => self.expr_accesses_source(&e.source),
                FlowStep::Try(_)
                | FlowStep::Rule(_)
                | FlowStep::Effect(_)
                | FlowStep::Match(_)
                | FlowStep::Upload(_) => false,
            });

            if accesses_tenanted {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::MissingTenantScope,
                    message: format!(
                        "flow '{}' accesses tenanted sources in realm '{}' but has no SCOPE TENANT",
                        flow.name, realm_name
                    ),
                    span: flow.span,
                    hint: Some("add SCOPE TENANT auth.user_id".into()),
                });
            }
        }
    }

    fn expr_accesses_source(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Fetch { source, .. } | Expr::Query { source, .. } => {
                self.sources.contains_key(source)
            }
            Expr::Unary { operand, .. } => self.expr_accesses_source(operand),
            Expr::Binary { left, right, .. } => {
                self.expr_accesses_source(left) || self.expr_accesses_source(right)
            }
            Expr::If { cond, then, else_ } => {
                self.expr_accesses_source(cond)
                    || self.expr_accesses_source(then)
                    || self.expr_accesses_source(else_)
            }
            Expr::MapExpr { source, .. } | Expr::ReduceExpr { source, .. } => {
                self.expr_accesses_source(source)
            }
            Expr::FilterExpr { source, condition } => {
                self.expr_accesses_source(source) || self.expr_accesses_source(condition)
            }
            Expr::SplitExpr { value, delimiter } => {
                self.expr_accesses_source(value) || self.expr_accesses_source(delimiter)
            }
            Expr::ReplaceExpr { value, from, to } => {
                self.expr_accesses_source(value)
                    || self.expr_accesses_source(from)
                    || self.expr_accesses_source(to)
            }
            Expr::FormatExpr { args, .. } | Expr::FuncCall { args, .. } => {
                args.iter().any(|a| self.expr_accesses_source(a))
            }
            Expr::Render { vars, .. } | Expr::Translate { vars, .. } => {
                vars.iter().any(|(_, v)| self.expr_accesses_source(v))
            }
            Expr::Coalesce { value, default } => {
                self.expr_accesses_source(value) || self.expr_accesses_source(default)
            }
            Expr::Cached { expr, .. } => self.expr_accesses_source(expr),
            Expr::Aggregate { source, .. } => self.expr_accesses_source(source),
            Expr::NowOffset { amount, .. } => self.expr_accesses_source(amount),
            Expr::Ternary { a, b, c, .. } => {
                self.expr_accesses_source(a)
                    || self.expr_accesses_source(b)
                    || self.expr_accesses_source(c)
            }
            Expr::Literal(_) | Expr::DotPath(_) | Expr::Call { .. } | Expr::WasmCall { .. } => {
                false
            }
        }
    }

    fn verify_capabilities(&mut self, flow: &FlowDef) {
        let realm_name = match &flow.realm {
            Some(r) => r,
            None => return,
        };

        let realm = match self.realms.get(realm_name) {
            Some(r) => r,
            None => return,
        };

        let mut read_sources = HashSet::new();
        let mut write_sources = HashSet::new();
        let mut call_services = HashSet::new();
        let mut effects = HashSet::new();

        for step in &flow.steps {
            self.collect_capabilities_from_step(
                step,
                &mut read_sources,
                &mut write_sources,
                &mut call_services,
                &mut effects,
            );
        }

        for source in &read_sources {
            if !realm
                .capabilities
                .contains(&(CapKind::Read, source.clone()))
            {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::MissingCapability,
                    message: format!(
                        "flow '{}' reads source '{}' but realm '{}' lacks CAPABILITY read {}",
                        flow.name, source, realm_name, source
                    ),
                    span: flow.span,
                    hint: Some(format!(
                        "add CAPABILITY read {} to REALM {}",
                        source, realm_name
                    )),
                });
            }
        }

        for source in &write_sources {
            if !realm
                .capabilities
                .contains(&(CapKind::Write, source.clone()))
            {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::MissingCapability,
                    message: format!(
                        "flow '{}' writes source '{}' but realm '{}' lacks CAPABILITY write {}",
                        flow.name, source, realm_name, source
                    ),
                    span: flow.span,
                    hint: Some(format!(
                        "add CAPABILITY write {} to REALM {}",
                        source, realm_name
                    )),
                });
            }
        }

        for (service, _) in &call_services {
            if !realm
                .capabilities
                .contains(&(CapKind::Call, service.clone()))
            {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::MissingCapability,
                    message: format!(
                        "flow '{}' calls service '{}' but realm '{}' lacks CAPABILITY call {}",
                        flow.name, service, realm_name, service
                    ),
                    span: flow.span,
                    hint: Some(format!(
                        "add CAPABILITY call {} to REALM {}",
                        service, realm_name
                    )),
                });
            }
        }

        for effect_type in &effects {
            if !realm
                .capabilities
                .contains(&(CapKind::Effect, effect_type.clone()))
            {
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::MissingCapability,
                    message: format!(
                        "flow '{}' emits effect '{}' but realm '{}' lacks CAPABILITY effect {}",
                        flow.name, effect_type, realm_name, effect_type
                    ),
                    span: flow.span,
                    hint: Some(format!(
                        "add CAPABILITY effect {} to REALM {}",
                        effect_type, realm_name
                    )),
                });
            }
        }
    }

    fn collect_capabilities_from_step(
        &self,
        step: &FlowStep,
        reads: &mut HashSet<String>,
        writes: &mut HashSet<String>,
        calls: &mut HashSet<(String, String)>,
        effects: &mut HashSet<String>,
    ) {
        match step {
            FlowStep::Let(l) => self.collect_capabilities_from_expr(&l.expr, reads, calls),
            FlowStep::Guard(g) => self.collect_capabilities_from_expr(&g.expr, reads, calls),
            FlowStep::Insert(step) => {
                writes.insert(step.source.clone());
                for (_, expression) in &step.fields {
                    self.collect_capabilities_from_expr(expression, reads, calls);
                }
            }
            FlowStep::Upsert(step) => {
                writes.insert(step.source.clone());
                for (_, expression) in &step.keys {
                    self.collect_capabilities_from_expr(expression, reads, calls);
                }
                for set in &step.sets {
                    self.collect_capabilities_from_expr(&set.value, reads, calls);
                }
            }
            FlowStep::Update(step) => {
                writes.insert(step.source.clone());
                for filter in &step.wheres {
                    self.collect_capabilities_from_expr(&filter.value, reads, calls);
                }
                for set in &step.sets {
                    self.collect_capabilities_from_expr(&set.value, reads, calls);
                }
            }
            FlowStep::Delete(step) => {
                writes.insert(step.source.clone());
                for filter in &step.wheres {
                    self.collect_capabilities_from_expr(&filter.value, reads, calls);
                }
            }
            FlowStep::Fanout(f) => {
                self.collect_capabilities_from_expr(&f.source, reads, calls);
                writes.insert(f.insert.source.clone());
            }
            FlowStep::Effect(e) => {
                let kind = match e.kind {
                    EffectKind::Email => "email",
                    EffectKind::PushNotification => "push_notification",
                    EffectKind::Async => "async",
                    EffectKind::Webhook => "webhook",
                };
                effects.insert(kind.into());
                for field in &e.fields {
                    match field {
                        EffectField::To(expression) | EffectField::Url(expression) => {
                            self.collect_capabilities_from_expr(expression, reads, calls);
                        }
                        EffectField::Data(expressions) => {
                            for expression in expressions {
                                self.collect_capabilities_from_expr(expression, reads, calls);
                            }
                        }
                        EffectField::Template(_) | EffectField::Event(_) | EffectField::Task(_) => {
                        }
                    }
                }
            }
            FlowStep::Match(m) => {
                for branch in &m.branches {
                    self.collect_capabilities_from_expr(&branch.condition, reads, calls);
                    for s in &branch.steps {
                        self.collect_capabilities_from_step(s, reads, writes, calls, effects);
                    }
                }
                if let Some(default) = &m.default {
                    for s in default {
                        self.collect_capabilities_from_step(s, reads, writes, calls, effects);
                    }
                }
            }
            FlowStep::Set(s) => self.collect_capabilities_from_expr(&s.expr, reads, calls),
            FlowStep::Each(e) => {
                self.collect_capabilities_from_expr(&e.source, reads, calls);
                for s in &e.steps {
                    self.collect_capabilities_from_step(s, reads, writes, calls, effects);
                }
            }
            FlowStep::Try(t) => {
                for s in &t.body {
                    self.collect_capabilities_from_step(s, reads, writes, calls, effects);
                }
                for s in &t.recover {
                    self.collect_capabilities_from_step(s, reads, writes, calls, effects);
                }
            }
            FlowStep::Rule(rule) => {
                for requirement in &rule.requires {
                    self.collect_capabilities_from_expr(&requirement.value, reads, calls);
                }
            }
            FlowStep::Upload(upload) => {
                self.collect_capabilities_from_expr(&upload.file_expr, reads, calls);
            }
        }
    }

    fn collect_capabilities_from_expr(
        &self,
        expr: &Expr,
        reads: &mut HashSet<String>,
        calls: &mut HashSet<(String, String)>,
    ) {
        match expr {
            Expr::Fetch { source, .. } | Expr::Query { source, .. } => {
                reads.insert(source.clone());
            }
            Expr::Call {
                service, method, ..
            } => {
                calls.insert((service.clone(), method.clone()));
            }
            Expr::Unary { operand, .. } => {
                self.collect_capabilities_from_expr(operand, reads, calls);
            }
            Expr::Binary { left, right, .. } => {
                self.collect_capabilities_from_expr(left, reads, calls);
                self.collect_capabilities_from_expr(right, reads, calls);
            }
            Expr::If { cond, then, else_ } => {
                self.collect_capabilities_from_expr(cond, reads, calls);
                self.collect_capabilities_from_expr(then, reads, calls);
                self.collect_capabilities_from_expr(else_, reads, calls);
            }
            Expr::Ternary { a, b, c, .. } => {
                self.collect_capabilities_from_expr(a, reads, calls);
                self.collect_capabilities_from_expr(b, reads, calls);
                self.collect_capabilities_from_expr(c, reads, calls);
            }
            Expr::Aggregate { source, .. } => {
                self.collect_capabilities_from_expr(source, reads, calls);
            }
            Expr::Coalesce { value, default } => {
                self.collect_capabilities_from_expr(value, reads, calls);
                self.collect_capabilities_from_expr(default, reads, calls);
            }
            Expr::Cached { expr, .. } => {
                self.collect_capabilities_from_expr(expr, reads, calls);
            }
            Expr::MapExpr { source, .. } => {
                self.collect_capabilities_from_expr(source, reads, calls);
            }
            Expr::FilterExpr { source, condition } => {
                self.collect_capabilities_from_expr(source, reads, calls);
                self.collect_capabilities_from_expr(condition, reads, calls);
            }
            Expr::ReduceExpr { source, .. } => {
                self.collect_capabilities_from_expr(source, reads, calls);
            }
            Expr::SplitExpr { value, delimiter } => {
                self.collect_capabilities_from_expr(value, reads, calls);
                self.collect_capabilities_from_expr(delimiter, reads, calls);
            }
            Expr::ReplaceExpr { value, from, to } => {
                self.collect_capabilities_from_expr(value, reads, calls);
                self.collect_capabilities_from_expr(from, reads, calls);
                self.collect_capabilities_from_expr(to, reads, calls);
            }
            Expr::FormatExpr { args, .. } => {
                for arg in args {
                    self.collect_capabilities_from_expr(arg, reads, calls);
                }
            }
            Expr::FuncCall { args, .. } => {
                for arg in args {
                    self.collect_capabilities_from_expr(arg, reads, calls);
                }
            }
            Expr::Render { vars, .. } | Expr::Translate { vars, .. } => {
                for (_, val) in vars {
                    self.collect_capabilities_from_expr(val, reads, calls);
                }
            }
            Expr::NowOffset { amount, .. } => {
                self.collect_capabilities_from_expr(amount, reads, calls);
            }
            Expr::Literal(_) | Expr::DotPath(_) | Expr::WasmCall { .. } => {}
        }
    }

    fn verify_policies(&mut self, program: &Program) {
        let policies = std::mem::take(&mut self.policies);

        for construct in &program.constructs {
            if let Construct::Flow(flow) = construct {
                for policy in &policies {
                    if self.policy_applies_to(flow, &policy.applies_to) {
                        self.check_policy_requirements(flow, policy);
                    }
                }
            }
        }

        self.policies = policies;
    }

    fn policy_applies_to(&self, flow: &FlowDef, applies_to: &AppliesTo) -> bool {
        if applies_to.filters.is_empty() {
            return true;
        }
        let matches = |filter: &PolicyFilter| self.policy_filter_matches(flow, filter);
        match applies_to.mode {
            PolicyMatchMode::All => applies_to.filters.iter().all(matches),
            PolicyMatchMode::Any => applies_to.filters.iter().any(matches),
        }
    }

    fn policy_filter_matches(&self, flow: &FlowDef, filter: &PolicyFilter) -> bool {
        match filter {
            PolicyFilter::MethodIn(methods) => {
                let flow_method = match flow.method {
                    HttpMethod::Get => "get",
                    HttpMethod::Post => "post",
                    HttpMethod::Put => "put",
                    HttpMethod::Patch => "patch",
                    HttpMethod::Delete => "delete",
                    HttpMethod::Webhook => "webhook",
                };
                methods.iter().any(|m| m == flow_method)
            }
            PolicyFilter::Reads(source) => self.steps_read_source(&flow.steps, source),
            PolicyFilter::Writes(source) => self.steps_write_source(&flow.steps, source),
            PolicyFilter::PathStartsWith(prefix) => flow.path.starts_with(prefix),
            PolicyFilter::Not(inner) => !self.policy_filter_matches(flow, inner),
        }
    }

    fn steps_read_source(&self, steps: &[FlowStep], source: &str) -> bool {
        steps.iter().any(|step| match step {
            FlowStep::Let(step) => self.expr_references_source(&step.expr, source),
            FlowStep::Set(step) => self.expr_references_source(&step.expr, source),
            FlowStep::Guard(step) => self.expr_references_source(&step.expr, source),
            FlowStep::Rule(step) => step
                .requires
                .iter()
                .any(|requirement| self.expr_references_source(&requirement.value, source)),
            FlowStep::Insert(step) => step
                .fields
                .iter()
                .any(|(_, expression)| self.expr_references_source(expression, source)),
            FlowStep::Upsert(step) => {
                step.keys
                    .iter()
                    .any(|(_, expression)| self.expr_references_source(expression, source))
                    || step
                        .sets
                        .iter()
                        .any(|set| self.expr_references_source(&set.value, source))
            }
            FlowStep::Update(step) => {
                step.wheres
                    .iter()
                    .any(|filter| self.expr_references_source(&filter.value, source))
                    || step
                        .sets
                        .iter()
                        .any(|set| self.expr_references_source(&set.value, source))
            }
            FlowStep::Delete(step) => step
                .wheres
                .iter()
                .any(|filter| self.expr_references_source(&filter.value, source)),
            FlowStep::Fanout(step) => {
                self.expr_references_source(&step.source, source)
                    || step
                        .insert
                        .fields
                        .iter()
                        .any(|(_, expression)| self.expr_references_source(expression, source))
            }
            FlowStep::Effect(step) => step.fields.iter().any(|field| match field {
                EffectField::To(expression) | EffectField::Url(expression) => {
                    self.expr_references_source(expression, source)
                }
                EffectField::Data(expressions) => expressions
                    .iter()
                    .any(|expression| self.expr_references_source(expression, source)),
                EffectField::Template(_) | EffectField::Event(_) | EffectField::Task(_) => false,
            }),
            FlowStep::Match(step) => {
                step.branches.iter().any(|branch| {
                    self.expr_references_source(&branch.condition, source)
                        || self.steps_read_source(&branch.steps, source)
                }) || step
                    .default
                    .as_ref()
                    .is_some_and(|steps| self.steps_read_source(steps, source))
            }
            FlowStep::Each(step) => {
                self.expr_references_source(&step.source, source)
                    || self.steps_read_source(&step.steps, source)
            }
            FlowStep::Try(step) => {
                self.steps_read_source(&step.body, source)
                    || self.steps_read_source(&step.recover, source)
            }
            FlowStep::Upload(step) => self.expr_references_source(&step.file_expr, source),
        })
    }

    fn steps_write_source(&self, steps: &[FlowStep], source: &str) -> bool {
        let _ = self;
        steps.iter().any(|step| match step {
            FlowStep::Insert(step) => step.source == source,
            FlowStep::Upsert(step) => step.source == source,
            FlowStep::Update(step) => step.source == source,
            FlowStep::Delete(step) => step.source == source,
            FlowStep::Fanout(step) => step.insert.source == source,
            FlowStep::Match(step) => {
                step.branches
                    .iter()
                    .any(|branch| self.steps_write_source(&branch.steps, source))
                    || step
                        .default
                        .as_ref()
                        .is_some_and(|steps| self.steps_write_source(steps, source))
            }
            FlowStep::Each(step) => self.steps_write_source(&step.steps, source),
            FlowStep::Try(step) => {
                self.steps_write_source(&step.body, source)
                    || self.steps_write_source(&step.recover, source)
            }
            _ => false,
        })
    }

    fn steps_have_fanout(&self, steps: &[FlowStep]) -> bool {
        let _ = self;
        steps.iter().any(|step| match step {
            FlowStep::Fanout(_) => true,
            FlowStep::Match(step) => {
                step.branches
                    .iter()
                    .any(|branch| self.steps_have_fanout(&branch.steps))
                    || step
                        .default
                        .as_ref()
                        .is_some_and(|steps| self.steps_have_fanout(steps))
            }
            FlowStep::Each(step) => self.steps_have_fanout(&step.steps),
            FlowStep::Try(step) => {
                self.steps_have_fanout(&step.body) || self.steps_have_fanout(&step.recover)
            }
            _ => false,
        })
    }

    fn expr_references_source(&self, expr: &Expr, source: &str) -> bool {
        match expr {
            Expr::Fetch { source: s, .. } | Expr::Query { source: s, .. } => s == source,
            Expr::Unary { operand, .. } => self.expr_references_source(operand, source),
            Expr::Binary { left, right, .. } => {
                self.expr_references_source(left, source)
                    || self.expr_references_source(right, source)
            }
            Expr::If { cond, then, else_ } => {
                self.expr_references_source(cond, source)
                    || self.expr_references_source(then, source)
                    || self.expr_references_source(else_, source)
            }
            Expr::MapExpr { source: s, .. } | Expr::ReduceExpr { source: s, .. } => {
                self.expr_references_source(s, source)
            }
            Expr::FilterExpr {
                source: s,
                condition,
            } => {
                self.expr_references_source(s, source)
                    || self.expr_references_source(condition, source)
            }
            Expr::SplitExpr { value, delimiter } => {
                self.expr_references_source(value, source)
                    || self.expr_references_source(delimiter, source)
            }
            Expr::ReplaceExpr { value, from, to } => {
                self.expr_references_source(value, source)
                    || self.expr_references_source(from, source)
                    || self.expr_references_source(to, source)
            }
            Expr::FormatExpr { args, .. } | Expr::FuncCall { args, .. } => {
                args.iter().any(|a| self.expr_references_source(a, source))
            }
            Expr::Render { vars, .. } | Expr::Translate { vars, .. } => vars
                .iter()
                .any(|(_, v)| self.expr_references_source(v, source)),
            Expr::Coalesce { value, default } => {
                self.expr_references_source(value, source)
                    || self.expr_references_source(default, source)
            }
            Expr::Cached { expr, .. } => self.expr_references_source(expr, source),
            Expr::Aggregate { source: s, .. } => self.expr_references_source(s, source),
            Expr::NowOffset { amount, .. } => self.expr_references_source(amount, source),
            Expr::Ternary { a, b, c, .. } => {
                self.expr_references_source(a, source)
                    || self.expr_references_source(b, source)
                    || self.expr_references_source(c, source)
            }
            Expr::Literal(_) | Expr::DotPath(_) | Expr::Call { .. } | Expr::WasmCall { .. } => {
                false
            }
        }
    }

    fn check_policy_requirements(&mut self, flow: &FlowDef, policy: &PolicyInfo) {
        for req in &policy.requires {
            let satisfied = match req {
                RequireClause::Auth(_) => {
                    flow.auth.is_some() && !matches!(flow.auth, Some(AuthDecl::None))
                }
                RequireClause::Limit => !flow.limits.is_empty(),
                RequireClause::Scope => flow.scope.is_some(),
                RequireClause::Rule(name) => flow
                    .steps
                    .iter()
                    .any(|s| matches!(s, FlowStep::Rule(r) if r.name == *name)),
                RequireClause::Guard(name) => flow
                    .steps
                    .iter()
                    .any(|s| matches!(s, FlowStep::Guard(g) if g.name == *name)),
                RequireClause::Idempotency => flow.idempotency.is_some(),
                RequireClause::Fanout => self.steps_have_fanout(&flow.steps),
            };

            if !satisfied {
                let what = match req {
                    RequireClause::Auth(_) => "AUTH".into(),
                    RequireClause::Limit => "LIMIT".into(),
                    RequireClause::Scope => "SCOPE".into(),
                    RequireClause::Rule(n) => format!("RULE {n}"),
                    RequireClause::Guard(n) => format!("GUARD {n}"),
                    RequireClause::Idempotency => "IDEMPOTENCY".into(),
                    RequireClause::Fanout => "FANOUT".into(),
                };
                self.errors.push(VerifyError {
                    kind: VerifyErrorKind::PolicyViolation,
                    message: format!(
                        "flow '{}' violates policy '{}': missing {}",
                        flow.name, policy.name, what
                    ),
                    span: flow.span,
                    hint: Some(format!("add {} to flow '{}'", what, flow.name)),
                });
            }
        }
    }

    fn verify_surfaces(&mut self, program: &Program) {
        let flows: HashMap<String, &FlowDef> = program
            .constructs
            .iter()
            .filter_map(|c| {
                if let Construct::Flow(f) = c {
                    Some((f.name.clone(), f))
                } else {
                    None
                }
            })
            .collect();
        let flow_names: HashSet<String> = flows.keys().cloned().collect();

        for construct in &program.constructs {
            if let Construct::Surface(surface) = construct {
                if let Some(ref realm) = surface.realm {
                    if !self.realms.contains_key(realm) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::UndefinedRealm,
                            message: format!(
                                "surface '{}' references undefined realm '{}'",
                                surface.name, realm
                            ),
                            span: surface.span,
                            hint: None,
                        });
                    }
                }
                for route in &surface.routes {
                    if !flow_names.contains(&route.target) {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::UndefinedBinding,
                            message: format!(
                                "surface '{}' route targets undefined flow '{}'",
                                surface.name, route.target
                            ),
                            span: surface.span,
                            hint: Some(format!("define FLOW {}", route.target)),
                        });
                    }
                    if let Some(flow) = flows.get(&route.target) {
                        if let Some(ReturnBody::Binding(binding)) = &flow.return_stmt.body {
                            for expose in &surface.exposes {
                                if let Some(shape) = self.shapes.get(&expose.shape) {
                                    if shape.fields.contains_key(binding)
                                        || binding == &expose.shape.to_lowercase()
                                        || binding.contains(&expose.shape.to_lowercase())
                                    {
                                        continue;
                                    }
                                }
                            }
                        }
                        if let Some(ReturnBody::Inline(fields)) = &flow.return_stmt.body {
                            for expose in &surface.exposes {
                                let _exposed_names: HashSet<String> = expose
                                    .fields
                                    .iter()
                                    .filter_map(|ef| match ef {
                                        ExposeField::Field { name, .. } => Some(name.clone()),
                                        ExposeField::Rename { to, .. } => Some(to.clone()),
                                        ExposeField::Hide(_) => None,
                                    })
                                    .collect();
                                let hidden_names: HashSet<String> = expose
                                    .fields
                                    .iter()
                                    .filter_map(|ef| {
                                        if let ExposeField::Hide(name) = ef {
                                            Some(name.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                for field in fields {
                                    if hidden_names.contains(&field.name) {
                                        self.errors.push(VerifyError {
                                            kind: VerifyErrorKind::TypeMismatch,
                                            message: format!(
                                                "surface '{}': flow '{}' returns field '{}' which is hidden by EXPOSE {} AS {}",
                                                surface.name, route.target, field.name, expose.shape, expose.alias.as_deref().unwrap_or(&expose.shape)
                                            ),
                                            span: flow.return_stmt.span,
                                            hint: Some("remove the field from RETURN or unhide it in EXPOSE".into()),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                for expose in &surface.exposes {
                    if let Some(shape) = self.shapes.get(&expose.shape) {
                        for ef in &expose.fields {
                            match ef {
                                ExposeField::Field { name, ty } => {
                                    if let Some(field_info) = shape.fields.get(name) {
                                        if !types_compatible(ty, &field_info.ty) {
                                            self.errors.push(VerifyError {
                                                kind: VerifyErrorKind::TypeMismatch,
                                                message: format!(
                                                    "surface '{}': EXPOSE field '{}' type does not match shape '{}' field type",
                                                    surface.name, name, expose.shape
                                                ),
                                                span: surface.span,
                                                hint: None,
                                            });
                                        }
                                    } else {
                                        self.errors.push(VerifyError {
                                            kind: VerifyErrorKind::UndefinedBinding,
                                            message: format!(
                                                "surface '{}': EXPOSE field '{}' does not exist in shape '{}'",
                                                surface.name, name, expose.shape
                                            ),
                                            span: surface.span,
                                            hint: None,
                                        });
                                    }
                                }
                                ExposeField::Hide(name)
                                | ExposeField::Rename { from: name, .. } => {
                                    if !shape.fields.contains_key(name) {
                                        self.errors.push(VerifyError {
                                            kind: VerifyErrorKind::UndefinedBinding,
                                            message: format!(
                                                "surface '{}': field '{}' does not exist in shape '{}'",
                                                surface.name, name, expose.shape
                                            ),
                                            span: surface.span,
                                            hint: None,
                                        });
                                    }
                                }
                            }
                        }
                    } else {
                        self.errors.push(VerifyError {
                            kind: VerifyErrorKind::UndefinedShape,
                            message: format!(
                                "surface '{}' exposes undefined shape '{}'",
                                surface.name, expose.shape
                            ),
                            span: surface.span,
                            hint: None,
                        });
                    }
                }
            }
        }
    }

    fn verify_migrations(&mut self, program: &Program) {
        for construct in &program.constructs {
            if let Construct::Migrate(migrate) = construct {
                if !self.shapes.contains_key(&migrate.shape) {
                    self.errors.push(VerifyError {
                        kind: VerifyErrorKind::UndefinedShape,
                        message: format!("MIGRATE references undefined shape '{}'", migrate.shape),
                        span: migrate.span,
                        hint: None,
                    });
                }
            }
        }
    }
}

fn flow_service_calls_use_key(
    steps: &[FlowStep],
    service: &str,
    method: &str,
    input: &str,
    key: &DotPath,
) -> bool {
    let mut found = false;
    let mut valid = true;
    visit_service_calls_in_steps(steps, &mut |called_service, called_method, args| {
        if called_service == service && called_method == method {
            found = true;
            valid &= args.iter().any(|(name, value)| {
                name == input
                    && matches!(value, Expr::DotPath(path) if path.segments == key.segments)
            });
        }
    });
    found && valid
}

fn visit_service_calls_in_steps<'a>(
    steps: &'a [FlowStep],
    visit: &mut impl FnMut(&'a str, &'a str, &'a [(String, Expr)]),
) {
    for step in steps {
        match step {
            FlowStep::Rule(step) => {
                for requirement in &step.requires {
                    visit_service_calls_in_expr(&requirement.value, visit);
                }
            }
            FlowStep::Guard(step) => visit_service_calls_in_expr(&step.expr, visit),
            FlowStep::Let(step) => visit_service_calls_in_expr(&step.expr, visit),
            FlowStep::Set(step) => visit_service_calls_in_expr(&step.expr, visit),
            FlowStep::Insert(step) => {
                for (_, value) in &step.fields {
                    visit_service_calls_in_expr(value, visit);
                }
            }
            FlowStep::Upsert(step) => {
                for (_, value) in &step.keys {
                    visit_service_calls_in_expr(value, visit);
                }
                for set in &step.sets {
                    visit_service_calls_in_expr(&set.value, visit);
                }
            }
            FlowStep::Update(step) => {
                for filter in &step.wheres {
                    visit_service_calls_in_expr(&filter.value, visit);
                }
                for set in &step.sets {
                    visit_service_calls_in_expr(&set.value, visit);
                }
            }
            FlowStep::Delete(step) => {
                for filter in &step.wheres {
                    visit_service_calls_in_expr(&filter.value, visit);
                }
            }
            FlowStep::Fanout(step) => {
                visit_service_calls_in_expr(&step.source, visit);
                for (_, value) in &step.insert.fields {
                    visit_service_calls_in_expr(value, visit);
                }
            }
            FlowStep::Effect(step) => {
                for field in &step.fields {
                    match field {
                        EffectField::To(value) | EffectField::Url(value) => {
                            visit_service_calls_in_expr(value, visit);
                        }
                        EffectField::Data(values) => {
                            for value in values {
                                visit_service_calls_in_expr(value, visit);
                            }
                        }
                        EffectField::Template(_) | EffectField::Event(_) | EffectField::Task(_) => {
                        }
                    }
                }
            }
            FlowStep::Match(step) => {
                for branch in &step.branches {
                    visit_service_calls_in_expr(&branch.condition, visit);
                    visit_service_calls_in_steps(&branch.steps, visit);
                }
                if let Some(default) = &step.default {
                    visit_service_calls_in_steps(default, visit);
                }
            }
            FlowStep::Each(step) => {
                visit_service_calls_in_expr(&step.source, visit);
                visit_service_calls_in_steps(&step.steps, visit);
            }
            FlowStep::Try(step) => {
                visit_service_calls_in_steps(&step.body, visit);
                visit_service_calls_in_steps(&step.recover, visit);
            }
            FlowStep::Upload(step) => visit_service_calls_in_expr(&step.file_expr, visit),
        }
    }
}

fn visit_service_calls_in_expr<'a>(
    expr: &'a Expr,
    visit: &mut impl FnMut(&'a str, &'a str, &'a [(String, Expr)]),
) {
    match expr {
        Expr::Call {
            service,
            method,
            args,
            ..
        } => {
            visit(service, method, args);
            for (_, value) in args {
                visit_service_calls_in_expr(value, visit);
            }
        }
        Expr::Unary { operand, .. }
        | Expr::Aggregate {
            source: operand, ..
        }
        | Expr::NowOffset {
            amount: operand, ..
        }
        | Expr::Cached { expr: operand, .. }
        | Expr::MapExpr {
            source: operand, ..
        }
        | Expr::ReduceExpr {
            source: operand, ..
        } => visit_service_calls_in_expr(operand, visit),
        Expr::Binary { left, right, .. }
        | Expr::Coalesce {
            value: left,
            default: right,
        }
        | Expr::FilterExpr {
            source: left,
            condition: right,
        }
        | Expr::SplitExpr {
            value: left,
            delimiter: right,
        } => {
            visit_service_calls_in_expr(left, visit);
            visit_service_calls_in_expr(right, visit);
        }
        Expr::ReplaceExpr { value, from, to }
        | Expr::Ternary {
            a: value,
            b: from,
            c: to,
            ..
        } => {
            visit_service_calls_in_expr(value, visit);
            visit_service_calls_in_expr(from, visit);
            visit_service_calls_in_expr(to, visit);
        }
        Expr::If { cond, then, else_ } => {
            visit_service_calls_in_expr(cond, visit);
            visit_service_calls_in_expr(then, visit);
            visit_service_calls_in_expr(else_, visit);
        }
        Expr::Fetch { filters, .. } => {
            for filter in filters {
                visit_service_calls_in_expr(&filter.value, visit);
            }
        }
        Expr::Query {
            filters,
            cursor,
            page_size,
            ..
        } => {
            for filter in filters {
                visit_service_calls_in_expr(&filter.value, visit);
            }
            if let Some(cursor) = cursor {
                visit_service_calls_in_expr(cursor, visit);
            }
            if let Some(page_size) = page_size {
                visit_service_calls_in_expr(page_size, visit);
            }
        }
        Expr::FormatExpr { args, .. } | Expr::FuncCall { args, .. } => {
            for value in args {
                visit_service_calls_in_expr(value, visit);
            }
        }
        Expr::Render { vars, .. } | Expr::Translate { vars, .. } => {
            for (_, value) in vars {
                visit_service_calls_in_expr(value, visit);
            }
        }
        Expr::Literal(_) | Expr::DotPath(_) | Expr::WasmCall { .. } => {}
    }
}

fn collect_step_bindings(steps: &[FlowStep]) -> HashSet<String> {
    let mut bindings = HashSet::new();
    for step in steps {
        match step {
            FlowStep::Let(l) => {
                bindings.insert(l.name.clone());
            }
            FlowStep::Insert(i) => {
                if let Some(b) = &i.binding {
                    bindings.insert(b.clone());
                }
            }
            FlowStep::Upsert(u) => {
                if let Some(b) = &u.binding {
                    bindings.insert(b.clone());
                }
            }
            FlowStep::Update(u) => {
                if let Some(b) = &u.binding {
                    let name = match b {
                        UpdateBinding::As(n) | UpdateBinding::Count(n) => n.clone(),
                    };
                    bindings.insert(name);
                }
            }
            FlowStep::Each(e) => {
                bindings.extend(collect_step_bindings(&e.steps));
            }
            FlowStep::Fanout(_) => {}
            FlowStep::Try(t) => {
                bindings.extend(collect_step_bindings(&t.body));
                bindings.extend(collect_step_bindings(&t.recover));
            }
            FlowStep::Upload(u) => {
                bindings.insert(u.binding.clone());
            }
            FlowStep::Set(_)
            | FlowStep::Rule(_)
            | FlowStep::Guard(_)
            | FlowStep::Delete(_)
            | FlowStep::Effect(_)
            | FlowStep::Match(_) => {}
        }
    }
    bindings
}

fn unwrap_maybe(t: &TypeExpr) -> &TypeExpr {
    match t {
        TypeExpr::Maybe(inner) => inner,
        other => other,
    }
}

fn types_compatible(expose_ty: &TypeExpr, shape_ty: &TypeExpr) -> bool {
    let a = unwrap_maybe(expose_ty);
    let b = unwrap_maybe(shape_ty);
    matches!(
        (a, b),
        (TypeExpr::Uuid, TypeExpr::Uuid)
            | (TypeExpr::Bool, TypeExpr::Bool)
            | (TypeExpr::Date, TypeExpr::Date)
            | (TypeExpr::Timestamp, TypeExpr::Timestamp)
            | (TypeExpr::Text, TypeExpr::Text)
            | (TypeExpr::String(_), TypeExpr::String(_))
            | (TypeExpr::String(_), TypeExpr::Text)
            | (TypeExpr::Text, TypeExpr::String(_))
            | (TypeExpr::Int { .. }, TypeExpr::Int { .. })
            | (TypeExpr::Decimal { .. }, TypeExpr::Decimal { .. })
            | (TypeExpr::Enum(_), TypeExpr::Enum(_))
            | (TypeExpr::Enum(_), TypeExpr::String(_))
            | (TypeExpr::String(_), TypeExpr::Enum(_))
            | (TypeExpr::Json, TypeExpr::Json)
            | (TypeExpr::List(_), TypeExpr::List(_))
            | (TypeExpr::Map(_, _), TypeExpr::Map(_, _))
            | (TypeExpr::Ref { .. }, TypeExpr::Ref { .. })
            | (TypeExpr::Ref { .. }, TypeExpr::Uuid)
            | (TypeExpr::Uuid, TypeExpr::Ref { .. })
            | (TypeExpr::Blob, TypeExpr::Blob)
    )
}

use std::cell::RefCell;

struct FlowContext {
    bindings: HashMap<String, BindingInfo>,
    body_fields: HashSet<String>,
    has_auth: bool,
    has_limit: bool,
    has_scope: bool,
    accessed_sources: RefCell<HashSet<String>>,
    writes_sources: HashSet<String>,
    accessed_services: RefCell<HashSet<String>>,
    accessed_effects: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Ty {
    Uuid,
    Bool,
    Date,
    Timestamp,
    Text,
    String,
    Int,
    Decimal,
    Enum(Vec<std::string::String>),
    Json,
    Maybe(Box<Ty>),
    List(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Blob,
    Shape(std::string::String),
    Unknown,
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Uuid => write!(f, "UUID"),
            Ty::Bool => write!(f, "BOOL"),
            Ty::Date => write!(f, "DATE"),
            Ty::Timestamp => write!(f, "TIMESTAMP"),
            Ty::Text => write!(f, "TEXT"),
            Ty::String => write!(f, "STRING"),
            Ty::Int => write!(f, "INT"),
            Ty::Decimal => write!(f, "DECIMAL"),
            Ty::Enum(_) => write!(f, "ENUM"),
            Ty::Json => write!(f, "JSON"),
            Ty::Maybe(t) => write!(f, "MAYBE {t}"),
            Ty::List(t) => write!(f, "LIST {t}"),
            Ty::Map(k, v) => write!(f, "MAP {k} {v}"),
            Ty::Blob => write!(f, "BLOB"),
            Ty::Shape(name) => write!(f, "{name}"),
            Ty::Unknown => write!(f, "?"),
        }
    }
}

impl Ty {
    fn from_type_expr(te: &TypeExpr) -> Self {
        match te {
            TypeExpr::Uuid => Ty::Uuid,
            TypeExpr::Bool => Ty::Bool,
            TypeExpr::Date => Ty::Date,
            TypeExpr::Timestamp => Ty::Timestamp,
            TypeExpr::Text => Ty::Text,
            TypeExpr::String(_) => Ty::String,
            TypeExpr::Int { .. } => Ty::Int,
            TypeExpr::Decimal { .. } => Ty::Decimal,
            TypeExpr::Enum(variants) => Ty::Enum(variants.clone()),
            TypeExpr::Ref { .. } => Ty::Uuid,
            TypeExpr::List(inner) => Ty::List(Box::new(Ty::from_type_expr(inner))),
            TypeExpr::Map(k, v) => Ty::Map(
                Box::new(Ty::from_type_expr(k)),
                Box::new(Ty::from_type_expr(v)),
            ),
            TypeExpr::Json => Ty::Json,
            TypeExpr::Blob => Ty::Blob,
            TypeExpr::Maybe(inner) => Ty::Maybe(Box::new(Ty::from_type_expr(inner))),
        }
    }

    fn is_numeric(&self) -> bool {
        matches!(self, Ty::Int | Ty::Decimal)
    }

    fn is_orderable(&self) -> bool {
        matches!(
            self,
            Ty::Int | Ty::Decimal | Ty::Date | Ty::Timestamp | Ty::String | Ty::Text
        )
    }

    fn is_stringlike(&self) -> bool {
        matches!(self, Ty::String | Ty::Text)
    }

    fn unwrap_maybe(&self) -> &Ty {
        match self {
            Ty::Maybe(inner) => inner,
            other => other,
        }
    }

    fn compatible_with(&self, other: &Ty) -> bool {
        if *self == Ty::Unknown || *other == Ty::Unknown {
            return true;
        }
        match (self.unwrap_maybe(), other.unwrap_maybe()) {
            (a, b) if a == b => true,
            (Ty::String, Ty::Text) | (Ty::Text, Ty::String) => true,
            (Ty::Enum(a), Ty::Enum(b)) => {
                b.iter().all(|v| a.contains(v)) || a.iter().all(|v| b.contains(v))
            }
            (Ty::Enum(_), Ty::String) | (Ty::String, Ty::Enum(_)) => true,
            (Ty::Uuid, Ty::Text) | (Ty::Text, Ty::Uuid) => true,
            (Ty::Uuid, Ty::String) | (Ty::String, Ty::Uuid) => true,
            _ => false,
        }
    }
}

#[derive(Clone)]
struct BindingInfo {
    _span: Option<Span>,
    ty: Ty,
}

impl BindingInfo {
    fn auto() -> Self {
        Self {
            _span: None,
            ty: Ty::Unknown,
        }
    }
    fn body() -> Self {
        Self::auto()
    }
    fn path() -> Self {
        Self::auto()
    }
    fn query() -> Self {
        Self::auto()
    }
    fn header() -> Self {
        Self::auto()
    }
    fn auth() -> Self {
        Self::auto()
    }
    fn computed(span: Span) -> Self {
        Self {
            _span: Some(span),
            ty: Ty::Unknown,
        }
    }
    fn typed(span: Span, ty: Ty) -> Self {
        Self {
            _span: Some(span),
            ty,
        }
    }
}

impl FlowContext {
    fn new(_flow: &FlowDef) -> Self {
        Self {
            bindings: HashMap::new(),
            body_fields: HashSet::new(),
            has_auth: false,
            has_limit: false,
            has_scope: false,
            accessed_sources: RefCell::new(HashSet::new()),
            writes_sources: HashSet::new(),
            accessed_services: RefCell::new(HashSet::new()),
            accessed_effects: HashSet::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn verify(input: &str) -> VerifyResult {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().unwrap();
        let verifier = Verifier::new();
        verifier.verify(&program)
    }

    #[test]
    fn test_valid_simple_flow() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM user_api
  CAPABILITY read users

FLOW get_user get /users/:id
  REALM user_api
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let result = verify(input);
        assert!(result.is_ok(), "errors: {:?}", result.errors);
    }

    #[test]
    fn test_undefined_source() {
        let input = r#"SHAPE User
  id UUID PK AUTO

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH nonexistent
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::UndefinedSource)
        );
    }

    #[test]
    fn test_undefined_binding() {
        let input = r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  GUARD check 403
    EQ undefined_var.field auth.user_id
  RETURN 200 user
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::UndefinedBinding)
        );
    }

    #[test]
    fn test_duplicate_binding() {
        let input = r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::DuplicateBinding)
        );
    }

    #[test]
    fn test_missing_capability() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM user_api
  CAPABILITY read users

FLOW create_user post /users
  REALM user_api
  AUTH session
  BODY UserCreate
    name STRING 100 REQUIRED
  INSERT users
    name body.name
  AS user
  RETURN 201 user
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::MissingCapability),
            "expected MissingCapability error, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_missing_tenant_scope() {
        let input = r#"SHAPE Booking
  id UUID PK AUTO
  user_id UUID REQUIRED

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX user_id

REALM booking_api
  TENANT user_id
  CAPABILITY read bookings

FLOW list_bookings get /bookings
  REALM booking_api
  AUTH session
  LET bookings
    QUERY bookings
      FILTER user_id EQ auth.user_id
  RETURN 200 bookings
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::MissingTenantScope),
            "expected MissingTenantScope, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_missing_index() {
        let input = r#"SHAPE Booking
  id UUID PK AUTO
  status ENUM pending confirmed REQUIRED
  created_at TIMESTAMP AUTO

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX id

FLOW list_by_status get /bookings
  AUTH session
  LET bookings
    QUERY bookings
      FILTER status EQ query.status
      FILTER created_at GTE query.from
  RETURN 200 bookings
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::MissingIndex),
            "expected MissingIndex, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_insert_auto_field() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW create_user post /users
  AUTH session
  BODY UserCreate
    name STRING 100 REQUIRED
  INSERT users
    id body.name
    name body.name
  AS user
  RETURN 201 user
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::AutoFieldInInsert),
            "expected AutoFieldInInsert, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_insert_missing_required() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  email STRING 255 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW create_user post /users
  AUTH session
  BODY UserCreate
    name STRING 100 REQUIRED
  INSERT users
    name body.name
  AS user
  RETURN 201 user
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::MissingRequiredField),
            "expected MissingRequiredField for email, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_undefined_realm() {
        let input = r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW get_user get /users/:id
  REALM nonexistent
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::UndefinedRealm),
            "expected UndefinedRealm, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_source_references_undefined_shape() {
        let input = r#"SOURCE bookings POSTGRES
  SHAPE NonExistent
  INDEX id
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::UndefinedShape),
            "expected UndefinedShape, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_surface_expose_unknown_field() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user

SURFACE api v1
  BASE_PATH /api/v1
  ROUTE GET /users/:id -> get_user
  EXPOSE User AS UserResponse
    FIELD id UUID
    FIELD nonexistent STRING
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::UndefinedBinding
                    && e.message.contains("nonexistent")),
            "expected UndefinedBinding for nonexistent field, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_surface_expose_type_mismatch() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user

SURFACE api v1
  BASE_PATH /api/v1
  ROUTE GET /users/:id -> get_user
  EXPOSE User AS UserResponse
    FIELD id UUID
    FIELD name INT
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch),
            "expected TypeMismatch, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_valid_booking_flow() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  email_verified BOOL DEFAULT FALSE
  account_status ENUM active suspended banned DEFAULT active

SHAPE Listing
  id UUID PK AUTO
  host_id UUID REQUIRED
  price_per_night DECIMAL PRECISION 10 SCALE 2 REQUIRED
  max_guests INT MIN 1 MAX 16 REQUIRED
  weekly_discount BOOL DEFAULT FALSE
  host_status ENUM active inactive REQUIRED

SHAPE Booking
  id UUID PK AUTO
  user_id UUID REQUIRED
  listing_id UUID REQUIRED
  check_in DATE REQUIRED
  check_out DATE REQUIRED
  status ENUM pending confirmed cancelled REQUIRED
  total_price DECIMAL PRECISION 10 SCALE 2 REQUIRED
  guest_count INT MIN 1 MAX 16 DEFAULT 1
  created_at TIMESTAMP AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id

SOURCE listings POSTGRES
  SHAPE Listing
  INDEX id
  INDEX host_id

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX user_id created_at DESC
  INDEX listing_id check_in check_out
  INDEX status

REALM booking_api
  TENANT user_id
  CAPABILITY read users
  CAPABILITY read listings
  CAPABILITY read bookings
  CAPABILITY write bookings
  CAPABILITY effect email

FLOW create_booking post /bookings
  REALM booking_api
  AUTH session
  SCOPE TENANT auth.user_id
  BODY BookingCreate
    listing_id UUID REQUIRED
    check_in DATE REQUIRED
    check_out DATE REQUIRED
    guest_count INT MIN 1 MAX 16 DEFAULT 1
  RULE user_may_book
    REQUIRE auth.email_verified EQ TRUE
    REQUIRE auth.account_status EQ active
  GUARD valid_dates 400
    GT body.check_out body.check_in
  GUARD no_overlap 409
    EMPTY
      QUERY bookings
        FILTER listing_id EQ body.listing_id
        FILTER status IN pending
        FILTER check_in LT body.check_out
        FILTER check_out GT body.check_in
  LET listing
    FETCH listings
      FILTER id EQ body.listing_id
    OR 404
  GUARD host_active 400
    EQ listing.host_status active
  GUARD guest_capacity 400
    LTE body.guest_count listing.max_guests
  LET nights
    DAYS_BETWEEN body.check_in body.check_out
  LET nights_decimal
    TO_DECIMAL nights
  LET total
    MUL listing.price_per_night nights_decimal
  INSERT bookings
    user_id auth.user_id
    listing_id body.listing_id
    check_in body.check_in
    check_out body.check_out
    status pending
    total_price total
    guest_count body.guest_count
  AS booking
  EFFECT email
    TEMPLATE booking_request
    TO listing.host_id
    DATA booking
  RETURN 201 booking
"#;
        let result = verify(input);
        for e in &result.errors {
            eprintln!("  ERROR: {e}");
        }
        for w in &result.warnings {
            eprintln!("  WARN: {w}");
        }
        assert!(
            result.is_ok(),
            "expected no errors, got {} errors",
            result.error_count()
        );
    }

    #[test]
    fn test_redis_no_query() {
        let input = r#"SHAPE Session
  id UUID PK AUTO
  token STRING 255 REQUIRED

SOURCE sessions REDIS
  SHAPE Session
  TTL 3600

FLOW list_sessions get /sessions
  AUTH session
  LET results
    QUERY sessions
      FILTER token EQ auth.user_id
  RETURN 200 results
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("QUERY")
                    && e.message.contains("Redis")),
            "expected Redis QUERY restriction error, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_elasticsearch_no_insert() {
        let input = r#"SHAPE Listing
  id UUID PK AUTO
  title STRING 200 REQUIRED

SOURCE listing_search ELASTICSEARCH
  SHAPE Listing
  INDEX title TEXT

FLOW create_listing post /listings
  AUTH session
  BODY ListingCreate
    title STRING 200 REQUIRED
  INSERT listing_search
    title body.title
  AS listing
  RETURN 201 listing
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("INSERT")
                    && e.message.contains("Elasticsearch")),
            "expected ES INSERT restriction error, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_match_inconsistent_bindings() {
        let input = r#"SHAPE Booking
  id UUID PK AUTO
  status STRING 20 REQUIRED
  refund_amount DECIMAL PRECISION 10 SCALE 2

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX id

FLOW cancel get /bookings/:id/cancel
  AUTH session
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  MATCH
    WHEN EQ booking.status confirmed
      LET refund
        MUL booking.refund_amount 1.00
      UPDATE bookings
        WHERE id EQ path.id
        SET status cancelled
      AS updated
      OR 500
    DEFAULT
      UPDATE bookings
        WHERE id EQ path.id
        SET status cancelled
      AS cancelled
      OR 500
  RETURN 200 booking
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("inconsistent bindings")),
            "expected inconsistent bindings error, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_match_consistent_bindings() {
        let input = r#"SHAPE Booking
  id UUID PK AUTO
  status STRING 20 REQUIRED

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX id

FLOW cancel get /bookings/:id/cancel
  AUTH session
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  MATCH
    WHEN EQ booking.status confirmed
      UPDATE bookings
        WHERE id EQ path.id
        SET status cancelled
      AS updated
      OR 500
    DEFAULT
      UPDATE bookings
        WHERE id EQ path.id
        SET status cancelled
      AS updated
      OR 500
  RETURN 200 booking
"#;
        let result = verify(input);
        assert!(
            !result
                .errors
                .iter()
                .any(|e| e.message.contains("inconsistent bindings")),
            "did not expect inconsistent bindings error, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_saga_undefined_source() {
        let input = r#"SHAPE Booking
  id UUID PK AUTO
  status STRING 20 REQUIRED

SAGA process post /bookings
  AUTH session
  BODY BookingCreate
    status STRING 20 REQUIRED
  STEP check
    LET existing
      QUERY nonexistent_source
        FILTER status EQ body.status
    VERIFY
      EMPTY existing
    YIELD existing
    COMPENSATE NONE
  ON_FAILURE RUN_COMPENSATIONS
  ON_SUCCESS
    RETURN 201 existing
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::UndefinedSource),
            "expected UndefinedSource, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_saga_mutation_without_compensate_errors() {
        let input = r#"SHAPE Booking
  id UUID PK AUTO
  user_id UUID REQUIRED
  status STRING 20 REQUIRED

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX id

SAGA process post /bookings
  AUTH session
  BODY BookingCreate
    user_id UUID REQUIRED
    status STRING 20 REQUIRED
  STEP first
    LET booking
      FETCH bookings
        FILTER id EQ body.user_id
      OR 404
    YIELD booking
    COMPENSATE NONE
  STEP mutate
    INSERT bookings
      user_id body.user_id
      status body.status
    AS created
    YIELD created
    COMPENSATE NONE
  ON_FAILURE RUN_COMPENSATIONS
  ON_SUCCESS
    RETURN 201 created
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.message.contains("COMPENSATE NONE")),
            "expected COMPENSATE NONE error on mutation step, got errors: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_type_mul_mixed_numeric() {
        let input = r#"SHAPE Item
  id UUID PK AUTO
  price DECIMAL PRECISION 10 SCALE 2 REQUIRED
  stock INT REQUIRED

SOURCE items POSTGRES
  SHAPE Item
  INDEX id

FLOW bad_mul get /items/:id/total
  AUTH session
  PARAM id UUID
  LET item
    FETCH items
      FILTER id EQ path.id
    OR 404
  LET total
    MUL item.price item.stock
  RETURN 200 total
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("same numeric type")),
            "expected type error for MUL DECIMAL * INT"
        );
    }

    #[test]
    fn test_type_not_requires_bool() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW bad_not get /users/:id
  AUTH session
  PARAM id UUID
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  GUARD check 400
    NOT user.name
  RETURN 200 user
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("NOT requires BOOL")),
            "expected type error for NOT on STRING"
        );
    }

    #[test]
    fn test_type_days_between_requires_date() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW bad_days get /users/:id
  AUTH session
  PARAM id UUID
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  LET diff
    DAYS_BETWEEN user.name user.id
  RETURN 200 diff
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("DAYS_BETWEEN requires DATE")),
            "expected type error for DAYS_BETWEEN on non-DATE"
        );
    }

    #[test]
    fn test_type_if_branch_mismatch() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  active BOOL REQUIRED
  name STRING 100 REQUIRED
  age INT REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW branch_mismatch get /users/:id
  AUTH session
  PARAM id UUID
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  LET result
    IF user.active
      THEN user.name
      ELSE user.age
  RETURN 200 result
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("IF branches must return same type")),
            "expected type error for IF branches returning different types, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_type_valid_arithmetic() {
        let input = r#"SHAPE Product
  id UUID PK AUTO
  price DECIMAL PRECISION 10 SCALE 2 REQUIRED
  tax_rate DECIMAL PRECISION 4 SCALE 2 REQUIRED

SOURCE products POSTGRES
  SHAPE Product
  INDEX id

FLOW calc get /products/:id/total
  AUTH session
  PARAM id UUID
  LET product
    FETCH products
      FILTER id EQ path.id
    OR 404
  LET tax
    MUL product.price product.tax_rate
  LET total
    ADD product.price tax
  RETURN 200 total
"#;
        let result = verify(input);
        for e in &result.errors {
            eprintln!("  ERROR: {e}");
        }
        assert!(
            result.is_ok(),
            "expected no type errors for valid DECIMAL arithmetic"
        );
    }

    #[test]
    fn test_type_insert_field_mismatch() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  quantity INT REQUIRED
  status ENUM pending confirmed REQUIRED

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

FLOW bad_insert post /orders
  AUTH session
  BODY OrderCreate
    quantity INT REQUIRED
  INSERT orders
    quantity body.quantity
    status 42
  AS order
  RETURN 201 order
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("INSERT field 'status'")),
            "expected type error for INSERT status with INT value, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_type_filter_field_mismatch() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  age INT REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id
  INDEX age

FLOW bad_filter get /users
  AUTH session
  LET results
    QUERY users
      FILTER age EQ "not_a_number"
  RETURN 200 results
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("FILTER 'age'")),
            "expected type error for FILTER age with STRING value, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_type_update_set_mismatch() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  quantity INT REQUIRED
  status ENUM pending confirmed REQUIRED

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

FLOW bad_update put /orders/:id
  AUTH session
  PARAM id UUID
  UPDATE orders
    WHERE id EQ path.id
    SET quantity "not_a_number"
  OR 404
  RETURN 200
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch
                    && e.message.contains("UPDATE SET 'quantity'")),
            "expected type error for UPDATE SET quantity with STRING value, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_return_undefined_binding() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW bad_return get /users/:id
  AUTH session
  PARAM id UUID
  RETURN 200 nonexistent
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::UndefinedBinding
                    && e.message.contains("RETURN")
                    && e.message.contains("nonexistent")),
            "expected undefined binding error in RETURN, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_type_maybe_requires_coalesce() {
        let input = r#"SHAPE Item
  id UUID PK AUTO
  price DECIMAL PRECISION 10 SCALE 2 REQUIRED
  discount MAYBE DECIMAL

SOURCE items POSTGRES
  SHAPE Item
  INDEX id

FLOW bad_maybe get /items/:id
  AUTH session
  PARAM id UUID
  LET item
    FETCH items
      FILTER id EQ path.id
    OR 404
  LET total
    ADD item.price item.discount
  RETURN 200 total
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::TypeMismatch && e.message.contains("MAYBE")),
            "expected type error for ADD with MAYBE operand, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_type_coalesce_unwraps_maybe() {
        let input = r#"SHAPE Item
  id UUID PK AUTO
  price DECIMAL PRECISION 10 SCALE 2 REQUIRED
  discount MAYBE DECIMAL

SOURCE items POSTGRES
  SHAPE Item
  INDEX id

FLOW good_coalesce get /items/:id
  AUTH session
  PARAM id UUID
  LET item
    FETCH items
      FILTER id EQ path.id
    OR 404
  LET safe_discount
    COALESCE item.discount 0.0
  LET total
    ADD item.price safe_discount
  RETURN 200 total
"#;
        let result = verify(input);
        for e in &result.errors {
            eprintln!("  ERROR: {e}");
        }
        assert!(
            result.is_ok(),
            "expected no errors when MAYBE is properly unwrapped via COALESCE"
        );
    }

    #[test]
    fn test_forward_reference_rejected() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW bad get /users/:id
  AUTH session
  LET total
    ADD item.price item.discount
  LET item
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 total
"#;
        let result = verify(input);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.kind == VerifyErrorKind::UndefinedBinding && e.message.contains("item")),
            "expected UndefinedBinding for forward reference to 'item'"
        );
    }

    #[test]
    fn test_messenger_primitives_verify_cleanly() {
        let input = include_str!("../../examples/messenger-primitives.axis");
        let result = verify(input);
        assert!(result.is_ok(), "unexpected errors: {:?}", result.errors);
    }

    #[test]
    fn test_upsert_requires_primary_or_unique_key() {
        let input = r#"SHAPE Contact
  id UUID PK AUTO
  email STRING 255 REQUIRED
  name STRING 100 REQUIRED

SOURCE contacts POSTGRES
  SHAPE Contact
  INDEX email

FLOW save_contact post /contacts
  BODY ContactInput
    email STRING 255 REQUIRED
    name STRING 100 REQUIRED
  UPSERT contacts
    KEY email body.email
    SET name body.name
  RETURN 204
"#;
        let result = verify(input);
        assert!(
            result.errors.iter().any(|error| {
                let message = error.message.to_ascii_lowercase();
                message.contains("upsert keys") && message.contains("unique index")
            }),
            "expected a non-unique UPSERT key error, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_idempotency_rejects_non_transactional_service_calls() {
        let input = r#"SHAPE Job
  id UUID PK
  state STRING 30 REQUIRED

SOURCE jobs POSTGRES
  SHAPE Job
  INDEX id UNIQUE

SERVICE mailer
  ENDPOINT mailer_api
  AUTH bearer VAULT mailer_token
  METHOD send
    INPUT job_id UUID
    OUTPUT accepted BOOL

FLOW create_job post /jobs/:id
  HEADER idempotency_key STRING 255 REQUIRED
  BODY JobInput
    owner UUID REQUIRED
  IDEMPOTENCY header.idempotency_key SCOPE body.owner TTL 60
  INSERT jobs
    id path.id
    state "queued"
  LET sent
    CALL mailer.send
      job_id path.id
  RETURN 201
"#;
        let result = verify(input);
        assert!(
            result.errors.iter().any(|error| {
                error
                    .message
                    .contains("cannot atomically include service calls")
            }),
            "expected service-call idempotency error, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_idempotency_accepts_pure_service_calls() {
        let input = r#"SHAPE Job
  id UUID PK
  state STRING 30 REQUIRED

SOURCE jobs POSTGRES
  SHAPE Job
  INDEX id UNIQUE

SERVICE verifier
  ENDPOINT verifier_api
  AUTH bearer VAULT verifier_token
  METHOD check
    PURE
    INPUT job_id UUID
    OUTPUT valid BOOL

FLOW create_job post /jobs/:id
  HEADER idempotency_key STRING 255 REQUIRED
  BODY JobInput
    owner UUID REQUIRED
  IDEMPOTENCY header.idempotency_key SCOPE body.owner TTL 60
  LET checked
    CALL verifier.check
      job_id path.id
    OR 503
  INSERT jobs
    id path.id
    state "queued"
  RETURN 201 checked
"#;
        let result = verify(input);
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_idempotency_accepts_provider_deduplicated_service_calls() {
        let input = r#"SHAPE Job
  id UUID PK
  state STRING 30 REQUIRED

SOURCE jobs POSTGRES
  SHAPE Job
  INDEX id UNIQUE

SERVICE mailer
  ENDPOINT mailer_api
  AUTH bearer VAULT mailer_token
  METHOD send
    IDEMPOTENCY operation_id
    INPUT operation_id STRING 255 job_id UUID
    OUTPUT accepted BOOL

FLOW create_job post /jobs/:id
  HEADER idempotency_key STRING 255 REQUIRED
  BODY JobInput
    owner UUID REQUIRED
  IDEMPOTENCY header.idempotency_key SCOPE body.owner TTL 60
  LET sent
    CALL mailer.send
      operation_id header.idempotency_key
      job_id path.id
    OR 503
  INSERT jobs
    id path.id
    state "queued"
  RETURN 201 sent
"#;
        let result = verify(input);
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_idempotency_rejects_provider_key_not_bound_to_flow_key() {
        let input = r#"SHAPE Job
  id UUID PK
  state STRING 30 REQUIRED

SOURCE jobs POSTGRES
  SHAPE Job
  INDEX id UNIQUE

SERVICE mailer
  ENDPOINT mailer_api
  AUTH bearer VAULT mailer_token
  METHOD send
    IDEMPOTENCY operation_id
    INPUT operation_id STRING 255 job_id UUID
    OUTPUT accepted BOOL

FLOW create_job post /jobs/:id
  HEADER idempotency_key STRING 255 REQUIRED
  BODY JobInput
    owner UUID REQUIRED
    unrelated_key STRING 255 REQUIRED
  IDEMPOTENCY header.idempotency_key SCOPE body.owner TTL 60
  LET sent
    CALL mailer.send
      operation_id body.unrelated_key
      job_id path.id
    OR 503
  INSERT jobs
    id path.id
    state "queued"
  RETURN 201 sent
"#;
        let result = verify(input);
        assert!(result.errors.iter().any(|error| {
            error
                .message
                .contains("cannot atomically include service calls")
        }));
    }
}
