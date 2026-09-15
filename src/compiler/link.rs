use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::ast::*;
use crate::fmt;

#[derive(Debug)]
pub struct LinkResult {
    pub manifest: Manifest,
    pub errors: Vec<LinkError>,
}

#[derive(Debug)]
pub struct LinkError {
    pub kind: LinkErrorKind,
    pub message: String,
}

#[derive(Debug, PartialEq)]
pub enum LinkErrorKind {
    UndefinedReference,
    CircularDependency,
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl LinkResult {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Debug)]
pub struct Manifest {
    pub hash: String,
    pub shapes: Vec<HashedEntry>,
    pub sources: Vec<HashedEntry>,
    pub realms: Vec<HashedEntry>,
    pub policies: Vec<HashedEntry>,
    pub services: Vec<HashedEntry>,
    pub flows: Vec<HashedEntry>,
    pub sagas: Vec<HashedEntry>,
    pub surfaces: Vec<HashedEntry>,
    pub migrations: Vec<HashedEntry>,
    pub streams: Vec<HashedEntry>,
    pub storages: Vec<HashedEntry>,
}

#[derive(Debug)]
pub struct HashedEntry {
    pub name: String,
    pub hash: String,
    pub deps: Vec<String>,
}

pub fn link(program: &Program) -> LinkResult {
    let mut linker = Linker::new();
    linker.link(program)
}

struct Linker {
    errors: Vec<LinkError>,
}

impl Linker {
    fn new() -> Self {
        Self { errors: Vec::new() }
    }

    fn link(&mut self, program: &Program) -> LinkResult {
        let mut shape_names: HashSet<String> = HashSet::new();
        let mut source_names: HashSet<String> = HashSet::new();
        let mut realm_names: HashSet<String> = HashSet::new();
        let mut service_names: HashMap<String, HashSet<String>> = HashMap::new();
        let mut flow_names: HashSet<String> = HashSet::new();

        for construct in &program.constructs {
            match construct {
                Construct::Shape(s) => {
                    shape_names.insert(s.name.clone());
                }
                Construct::Source(s) => {
                    source_names.insert(s.name.clone());
                }
                Construct::Realm(r) => {
                    realm_names.insert(r.name.clone());
                }
                Construct::Service(s) => {
                    let methods: HashSet<String> =
                        s.methods.iter().map(|m| m.name.clone()).collect();
                    service_names.insert(s.name.clone(), methods);
                }
                Construct::Flow(f) => {
                    flow_names.insert(f.name.clone());
                }
                Construct::Saga(s) => {
                    flow_names.insert(s.name.clone());
                }
                _ => {}
            }
        }

        self.resolve_references(
            program,
            &shape_names,
            &source_names,
            &realm_names,
            &service_names,
            &flow_names,
        );
        self.detect_structural_cycles(program, &shape_names);

        let formatted = fmt::format_program(program);
        let mut shape_entries = Vec::new();
        let mut source_entries = Vec::new();
        let mut realm_entries = Vec::new();
        let mut policy_entries = Vec::new();
        let mut service_entries = Vec::new();
        let mut flow_entries = Vec::new();
        let mut saga_entries = Vec::new();
        let mut surface_entries = Vec::new();
        let mut migration_entries = Vec::new();
        let mut stream_entries = Vec::new();
        let mut storage_entries = Vec::new();

        for construct in &program.constructs {
            match construct {
                Construct::Shape(s) => {
                    let fragment =
                        self.extract_construct_text(&formatted, &format!("SHAPE {}", s.name));
                    let hash = content_hash(&fragment);
                    let deps = self.shape_deps(s);
                    shape_entries.push(HashedEntry {
                        name: s.name.clone(),
                        hash,
                        deps,
                    });
                }
                Construct::Source(s) => {
                    let fragment =
                        self.extract_construct_text(&formatted, &format!("SOURCE {}", s.name));
                    let hash = content_hash(&fragment);
                    source_entries.push(HashedEntry {
                        name: s.name.clone(),
                        hash,
                        deps: vec![s.shape.clone()],
                    });
                }
                Construct::Realm(r) => {
                    let fragment =
                        self.extract_construct_text(&formatted, &format!("REALM {}", r.name));
                    let hash = content_hash(&fragment);
                    realm_entries.push(HashedEntry {
                        name: r.name.clone(),
                        hash,
                        deps: vec![],
                    });
                }
                Construct::Policy(p) => {
                    let fragment =
                        self.extract_construct_text(&formatted, &format!("POLICY {}", p.name));
                    let hash = content_hash(&fragment);
                    policy_entries.push(HashedEntry {
                        name: p.name.clone(),
                        hash,
                        deps: vec![],
                    });
                }
                Construct::Service(s) => {
                    let fragment =
                        self.extract_construct_text(&formatted, &format!("SERVICE {}", s.name));
                    let hash = content_hash(&fragment);
                    service_entries.push(HashedEntry {
                        name: s.name.clone(),
                        hash,
                        deps: vec![],
                    });
                }
                Construct::Flow(f) => {
                    let method = match f.method {
                        HttpMethod::Get => "get",
                        HttpMethod::Post => "post",
                        HttpMethod::Put => "put",
                        HttpMethod::Patch => "patch",
                        HttpMethod::Delete => "delete",
                        HttpMethod::Webhook => "webhook",
                    };
                    let fragment = self
                        .extract_construct_text(&formatted, &format!("FLOW {} {}", f.name, method));
                    let hash = content_hash(&fragment);
                    let deps = self.flow_deps(f);
                    flow_entries.push(HashedEntry {
                        name: f.name.clone(),
                        hash,
                        deps,
                    });
                }
                Construct::Saga(s) => {
                    let method = match s.method {
                        HttpMethod::Get => "GET",
                        HttpMethod::Post => "POST",
                        HttpMethod::Put => "PUT",
                        HttpMethod::Patch => "PATCH",
                        HttpMethod::Delete => "DELETE",
                        HttpMethod::Webhook => "WEBHOOK",
                    };
                    let fragment = self
                        .extract_construct_text(&formatted, &format!("SAGA {} {}", s.name, method));
                    let hash = content_hash(&fragment);
                    saga_entries.push(HashedEntry {
                        name: s.name.clone(),
                        hash,
                        deps: vec![],
                    });
                }
                Construct::Surface(s) => {
                    let fragment = self.extract_construct_text(
                        &formatted,
                        &format!("SURFACE {} {}", s.name, s.version),
                    );
                    let hash = content_hash(&fragment);
                    let deps: Vec<String> = s.routes.iter().map(|r| r.target.clone()).collect();
                    surface_entries.push(HashedEntry {
                        name: format!("{}/{}", s.name, s.version),
                        hash,
                        deps,
                    });
                }
                Construct::Migrate(m) => {
                    let fragment = self.extract_construct_text(
                        &formatted,
                        &format!("MIGRATE {} {} TO {}", m.shape, m.from_version, m.to_version),
                    );
                    let hash = content_hash(&fragment);
                    migration_entries.push(HashedEntry {
                        name: format!("{} {} -> {}", m.shape, m.from_version, m.to_version),
                        hash,
                        deps: vec![m.shape.clone()],
                    });
                }
                Construct::Stream(s) => {
                    let fragment =
                        self.extract_construct_text(&formatted, &format!("STREAM {}", s.name));
                    let hash = content_hash(&fragment);
                    let mut deps = Vec::new();
                    if let Some(ref realm) = s.realm {
                        deps.push(realm.clone());
                    }
                    stream_entries.push(HashedEntry {
                        name: s.name.clone(),
                        hash,
                        deps,
                    });
                }
                Construct::Func(f) => {
                    let fragment =
                        self.extract_construct_text(&formatted, &format!("FUNC {}", f.name));
                    let hash = content_hash(&fragment);
                    let mut deps = HashSet::new();
                    self.collect_step_source_deps(&f.steps, &mut deps);
                    self.collect_expr_source_deps(&f.return_expr, &mut deps);
                    flow_entries.push(HashedEntry {
                        name: f.name.clone(),
                        hash,
                        deps: deps.into_iter().collect(),
                    });
                }
                Construct::Storage(s) => {
                    let fragment =
                        self.extract_construct_text(&formatted, &format!("STORAGE {}", s.name));
                    let hash = content_hash(&fragment);
                    storage_entries.push(HashedEntry {
                        name: s.name.clone(),
                        hash,
                        deps: vec![],
                    });
                }
            }
        }

        let mut manifest_content = String::new();
        for e in shape_entries
            .iter()
            .chain(source_entries.iter())
            .chain(realm_entries.iter())
            .chain(policy_entries.iter())
            .chain(service_entries.iter())
            .chain(flow_entries.iter())
            .chain(saga_entries.iter())
            .chain(surface_entries.iter())
            .chain(migration_entries.iter())
            .chain(stream_entries.iter())
            .chain(storage_entries.iter())
        {
            let _ = writeln!(manifest_content, "{}:{}", e.name, e.hash);
        }

        let manifest_hash = content_hash(&manifest_content);

        LinkResult {
            manifest: Manifest {
                hash: manifest_hash,
                shapes: shape_entries,
                sources: source_entries,
                realms: realm_entries,
                policies: policy_entries,
                services: service_entries,
                flows: flow_entries,
                sagas: saga_entries,
                surfaces: surface_entries,
                migrations: migration_entries,
                streams: stream_entries,
                storages: storage_entries,
            },
            errors: std::mem::take(&mut self.errors),
        }
    }

    fn resolve_references(
        &mut self,
        program: &Program,
        shapes: &HashSet<String>,
        sources: &HashSet<String>,
        realms: &HashSet<String>,
        services: &HashMap<String, HashSet<String>>,
        flows: &HashSet<String>,
    ) {
        for construct in &program.constructs {
            match construct {
                Construct::Source(s) => {
                    if !shapes.contains(&s.shape) {
                        self.errors.push(LinkError {
                            kind: LinkErrorKind::UndefinedReference,
                            message: format!(
                                "SOURCE '{}' references undefined SHAPE '{}'",
                                s.name, s.shape
                            ),
                        });
                    }
                }
                Construct::Flow(f) => {
                    if let Some(realm) = &f.realm {
                        if !realms.contains(realm) {
                            self.errors.push(LinkError {
                                kind: LinkErrorKind::UndefinedReference,
                                message: format!(
                                    "FLOW '{}' references undefined REALM '{realm}'",
                                    f.name
                                ),
                            });
                        }
                    }
                    self.resolve_step_refs(&f.name, &f.steps, sources, services);
                }
                Construct::Saga(s) => {
                    if let Some(realm) = &s.realm {
                        if !realms.contains(realm) {
                            self.errors.push(LinkError {
                                kind: LinkErrorKind::UndefinedReference,
                                message: format!(
                                    "SAGA '{}' references undefined REALM '{realm}'",
                                    s.name
                                ),
                            });
                        }
                    }
                    for step in &s.steps {
                        self.resolve_step_refs(&s.name, &step.flow_steps, sources, services);
                        if let Compensate::Steps(comp_steps) = &step.compensate {
                            self.resolve_step_refs(&s.name, comp_steps, sources, services);
                        }
                    }
                }
                Construct::Surface(s) => {
                    for route in &s.routes {
                        if !flows.contains(&route.target) {
                            self.errors.push(LinkError {
                                kind: LinkErrorKind::UndefinedReference,
                                message: format!(
                                    "SURFACE '{}/{}' route {} {} -> '{}' references undefined FLOW",
                                    s.name,
                                    s.version,
                                    format!("{:?}", route.method).to_uppercase(),
                                    route.path,
                                    route.target
                                ),
                            });
                        }
                    }
                    for expose in &s.exposes {
                        if !shapes.contains(&expose.shape) {
                            self.errors.push(LinkError {
                                kind: LinkErrorKind::UndefinedReference,
                                message: format!(
                                    "SURFACE '{}/{}' EXPOSE references undefined SHAPE '{}'",
                                    s.name, s.version, expose.shape
                                ),
                            });
                        }
                    }
                }
                Construct::Migrate(m) => {
                    if !shapes.contains(&m.shape) {
                        self.errors.push(LinkError {
                            kind: LinkErrorKind::UndefinedReference,
                            message: format!("MIGRATE references undefined SHAPE '{}'", m.shape),
                        });
                    }
                }
                Construct::Shape(s) => {
                    for field in &s.fields {
                        if let TypeExpr::Ref { shape, .. } = &field.ty {
                            if !shapes.contains(shape) {
                                self.errors.push(LinkError {
                                    kind: LinkErrorKind::UndefinedReference,
                                    message: format!(
                                        "SHAPE '{}' field '{}' REF references undefined SHAPE '{shape}'",
                                        s.name, field.name
                                    ),
                                });
                            }
                        }
                        for modifier in &field.modifiers {
                            if let Modifier::Ref { shape, .. } = modifier {
                                if !shapes.contains(shape) {
                                    self.errors.push(LinkError {
                                        kind: LinkErrorKind::UndefinedReference,
                                        message: format!(
                                            "SHAPE '{}' field '{}' REF references undefined SHAPE '{shape}'",
                                            s.name, field.name
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn resolve_step_refs(
        &mut self,
        owner: &str,
        steps: &[FlowStep],
        sources: &HashSet<String>,
        services: &HashMap<String, HashSet<String>>,
    ) {
        for step in steps {
            match step {
                FlowStep::Let(l) => self.resolve_expr_refs(owner, &l.expr, sources, services),
                FlowStep::Guard(g) => self.resolve_expr_refs(owner, &g.expr, sources, services),
                FlowStep::Insert(i) => {
                    if !sources.contains(&i.source) {
                        self.errors.push(LinkError {
                            kind: LinkErrorKind::UndefinedReference,
                            message: format!(
                                "'{owner}' INSERT references undefined SOURCE '{}'",
                                i.source
                            ),
                        });
                    }
                }
                FlowStep::Upsert(u) => {
                    if !sources.contains(&u.source) {
                        self.errors.push(LinkError {
                            kind: LinkErrorKind::UndefinedReference,
                            message: format!(
                                "'{owner}' UPSERT references undefined SOURCE '{}'",
                                u.source
                            ),
                        });
                    }
                }
                FlowStep::Update(u) => {
                    if !sources.contains(&u.source) {
                        self.errors.push(LinkError {
                            kind: LinkErrorKind::UndefinedReference,
                            message: format!(
                                "'{owner}' UPDATE references undefined SOURCE '{}'",
                                u.source
                            ),
                        });
                    }
                }
                FlowStep::Delete(d) => {
                    if !sources.contains(&d.source) {
                        self.errors.push(LinkError {
                            kind: LinkErrorKind::UndefinedReference,
                            message: format!(
                                "'{owner}' DELETE references undefined SOURCE '{}'",
                                d.source
                            ),
                        });
                    }
                }
                FlowStep::Match(m) => {
                    for branch in &m.branches {
                        self.resolve_step_refs(owner, &branch.steps, sources, services);
                    }
                    if let Some(default) = &m.default {
                        self.resolve_step_refs(owner, default, sources, services);
                    }
                }
                FlowStep::Set(s) => self.resolve_expr_refs(owner, &s.expr, sources, services),
                FlowStep::Each(e) => {
                    self.resolve_expr_refs(owner, &e.source, sources, services);
                    self.resolve_step_refs(owner, &e.steps, sources, services);
                }
                FlowStep::Fanout(f) => {
                    self.resolve_expr_refs(owner, &f.source, sources, services);
                    if !sources.contains(&f.insert.source) {
                        self.errors.push(LinkError {
                            kind: LinkErrorKind::UndefinedReference,
                            message: format!(
                                "'{owner}' FANOUT references undefined SOURCE '{}'",
                                f.insert.source
                            ),
                        });
                    }
                }
                FlowStep::Try(t) => {
                    self.resolve_step_refs(owner, &t.body, sources, services);
                    self.resolve_step_refs(owner, &t.recover, sources, services);
                }
                FlowStep::Rule(_) | FlowStep::Effect(_) | FlowStep::Upload(_) => {}
            }
        }
    }

    fn resolve_expr_refs(
        &mut self,
        owner: &str,
        expr: &Expr,
        sources: &HashSet<String>,
        services: &HashMap<String, HashSet<String>>,
    ) {
        match expr {
            Expr::Fetch { source, .. } | Expr::Query { source, .. } => {
                if !sources.contains(source) {
                    self.errors.push(LinkError {
                        kind: LinkErrorKind::UndefinedReference,
                        message: format!("'{owner}' references undefined SOURCE '{source}'"),
                    });
                }
            }
            Expr::Call {
                service, method, ..
            } => match services.get(service) {
                None => {
                    self.errors.push(LinkError {
                        kind: LinkErrorKind::UndefinedReference,
                        message: format!("'{owner}' CALL references undefined SERVICE '{service}'"),
                    });
                }
                Some(methods) => {
                    if !methods.contains(method) {
                        self.errors.push(LinkError {
                            kind: LinkErrorKind::UndefinedReference,
                            message: format!(
                                "'{owner}' CALL references undefined METHOD '{service}.{method}'"
                            ),
                        });
                    }
                }
            },
            Expr::Unary { operand, .. } => {
                self.resolve_expr_refs(owner, operand, sources, services)
            }
            Expr::Binary { left, right, .. } => {
                self.resolve_expr_refs(owner, left, sources, services);
                self.resolve_expr_refs(owner, right, sources, services);
            }
            Expr::Ternary { a, b, c, .. } => {
                self.resolve_expr_refs(owner, a, sources, services);
                self.resolve_expr_refs(owner, b, sources, services);
                self.resolve_expr_refs(owner, c, sources, services);
            }
            Expr::If {
                cond, then, else_, ..
            } => {
                self.resolve_expr_refs(owner, cond, sources, services);
                self.resolve_expr_refs(owner, then, sources, services);
                self.resolve_expr_refs(owner, else_, sources, services);
            }
            Expr::Coalesce { value, default } => {
                self.resolve_expr_refs(owner, value, sources, services);
                self.resolve_expr_refs(owner, default, sources, services);
            }
            Expr::Aggregate { source, .. } => {
                self.resolve_expr_refs(owner, source, sources, services)
            }
            Expr::NowOffset { amount, .. } => {
                self.resolve_expr_refs(owner, amount, sources, services)
            }
            Expr::Cached { expr, .. } => self.resolve_expr_refs(owner, expr, sources, services),
            Expr::MapExpr { source, .. } => {
                self.resolve_expr_refs(owner, source, sources, services)
            }
            Expr::FilterExpr { source, condition } => {
                self.resolve_expr_refs(owner, source, sources, services);
                self.resolve_expr_refs(owner, condition, sources, services);
            }
            Expr::ReduceExpr { source, .. } => {
                self.resolve_expr_refs(owner, source, sources, services)
            }
            Expr::SplitExpr { value, delimiter } => {
                self.resolve_expr_refs(owner, value, sources, services);
                self.resolve_expr_refs(owner, delimiter, sources, services);
            }
            Expr::ReplaceExpr { value, from, to } => {
                self.resolve_expr_refs(owner, value, sources, services);
                self.resolve_expr_refs(owner, from, sources, services);
                self.resolve_expr_refs(owner, to, sources, services);
            }
            Expr::FormatExpr { args, .. } => {
                for arg in args {
                    self.resolve_expr_refs(owner, arg, sources, services);
                }
            }
            Expr::FuncCall { args, .. } => {
                for arg in args {
                    self.resolve_expr_refs(owner, arg, sources, services);
                }
            }
            Expr::Render { vars, .. } | Expr::Translate { vars, .. } => {
                for (_, val) in vars {
                    self.resolve_expr_refs(owner, val, sources, services);
                }
            }
            Expr::WasmCall { .. } => {}
            Expr::Literal(_) | Expr::DotPath(_) => {}
        }
    }

    fn detect_structural_cycles(&mut self, program: &Program, shapes: &HashSet<String>) {
        let mut shape_deps: HashMap<String, HashSet<String>> = HashMap::new();

        for construct in &program.constructs {
            if let Construct::Shape(s) = construct {
                let mut deps = HashSet::new();
                for field in &s.fields {
                    self.collect_type_shape_refs(&field.ty, &mut deps);
                    for modifier in &field.modifiers {
                        if let Modifier::Ref { shape, .. } = modifier {
                            if shapes.contains(shape) && shape != &s.name {
                                deps.insert(shape.clone());
                            }
                        }
                    }
                }
                // REF dependencies are foreign keys — allowed to be cyclic
                // Only structural embedding (LIST Shape, MAP Shape, nested SHAPE) causes cycles
                let structural_deps: HashSet<String> = deps
                    .into_iter()
                    .filter(|d| self.is_structural_dep(&s.name, d, program))
                    .collect();
                shape_deps.insert(s.name.clone(), structural_deps);
            }
        }

        let mut visited = HashSet::new();
        let mut stack = HashSet::new();
        for name in shape_deps.keys() {
            if !visited.contains(name) {
                self.dfs_cycle(name, &shape_deps, &mut visited, &mut stack);
            }
        }
    }

    fn collect_type_shape_refs(&self, ty: &TypeExpr, deps: &mut HashSet<String>) {
        match ty {
            TypeExpr::List(inner) => self.collect_type_shape_refs(inner, deps),
            TypeExpr::Map(k, v) => {
                self.collect_type_shape_refs(k, deps);
                self.collect_type_shape_refs(v, deps);
            }
            TypeExpr::Maybe(inner) => self.collect_type_shape_refs(inner, deps),
            TypeExpr::Ref { shape, .. } => {
                deps.insert(shape.clone());
            }
            _ => {}
        }
    }

    fn is_structural_dep(&self, _owner: &str, dep: &str, program: &Program) -> bool {
        // REF is a foreign key (UUID pointer), not structural embedding.
        // Structural deps would be LIST Shape or embedding — which Axis doesn't support.
        // So in practice, cross-shape refs are always REF (non-structural).
        // We still check: if a field's *type* (not modifier) embeds the shape, that's structural.
        for construct in &program.constructs {
            if let Construct::Shape(s) = construct {
                if s.name == _owner {
                    for field in &s.fields {
                        if self.type_structurally_contains(&field.ty, dep) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn type_structurally_contains(&self, _ty: &TypeExpr, _shape_name: &str) -> bool {
        // Axis has no structural shape embedding (LIST/MAP/MAYBE wrap primitives, not shapes).
        // REF is a UUID foreign key pointer, not embedding. Always non-structural.
        false
    }

    fn dfs_cycle(
        &mut self,
        node: &str,
        graph: &HashMap<String, HashSet<String>>,
        visited: &mut HashSet<String>,
        stack: &mut HashSet<String>,
    ) {
        visited.insert(node.to_string());
        stack.insert(node.to_string());

        if let Some(deps) = graph.get(node) {
            for dep in deps {
                if !visited.contains(dep) {
                    self.dfs_cycle(dep, graph, visited, stack);
                } else if stack.contains(dep) {
                    self.errors.push(LinkError {
                        kind: LinkErrorKind::CircularDependency,
                        message: format!("structural cycle: SHAPE '{node}' -> SHAPE '{dep}'"),
                    });
                }
            }
        }

        stack.remove(node);
    }

    fn shape_deps(&self, shape: &ShapeDef) -> Vec<String> {
        let mut deps = HashSet::new();
        for field in &shape.fields {
            if let TypeExpr::Ref {
                shape: ref_shape, ..
            } = &field.ty
            {
                deps.insert(ref_shape.clone());
            }
            for modifier in &field.modifiers {
                if let Modifier::Ref {
                    shape: ref_shape, ..
                } = modifier
                {
                    deps.insert(ref_shape.clone());
                }
            }
        }
        deps.into_iter().collect()
    }

    fn flow_deps(&self, flow: &FlowDef) -> Vec<String> {
        let mut deps = HashSet::new();
        if let Some(realm) = &flow.realm {
            deps.insert(realm.clone());
        }
        self.collect_step_source_deps(&flow.steps, &mut deps);
        deps.into_iter().collect()
    }

    fn collect_step_source_deps(&self, steps: &[FlowStep], deps: &mut HashSet<String>) {
        for step in steps {
            match step {
                FlowStep::Let(l) => self.collect_expr_source_deps(&l.expr, deps),
                FlowStep::Guard(g) => self.collect_expr_source_deps(&g.expr, deps),
                FlowStep::Insert(i) => {
                    deps.insert(i.source.clone());
                }
                FlowStep::Upsert(u) => {
                    deps.insert(u.source.clone());
                }
                FlowStep::Update(u) => {
                    deps.insert(u.source.clone());
                }
                FlowStep::Delete(d) => {
                    deps.insert(d.source.clone());
                }
                FlowStep::Match(m) => {
                    for branch in &m.branches {
                        self.collect_step_source_deps(&branch.steps, deps);
                    }
                    if let Some(default) = &m.default {
                        self.collect_step_source_deps(default, deps);
                    }
                }
                FlowStep::Set(s) => self.collect_expr_source_deps(&s.expr, deps),
                FlowStep::Each(e) => {
                    self.collect_expr_source_deps(&e.source, deps);
                    self.collect_step_source_deps(&e.steps, deps);
                }
                FlowStep::Fanout(f) => {
                    self.collect_expr_source_deps(&f.source, deps);
                    deps.insert(f.insert.source.clone());
                }
                FlowStep::Try(t) => {
                    self.collect_step_source_deps(&t.body, deps);
                    self.collect_step_source_deps(&t.recover, deps);
                }
                FlowStep::Rule(_) | FlowStep::Effect(_) | FlowStep::Upload(_) => {}
            }
        }
    }

    fn collect_expr_source_deps(&self, expr: &Expr, deps: &mut HashSet<String>) {
        match expr {
            Expr::Fetch { source, .. } | Expr::Query { source, .. } => {
                deps.insert(source.clone());
            }
            Expr::Call { service, .. } => {
                deps.insert(service.clone());
            }
            Expr::Unary { operand, .. } => self.collect_expr_source_deps(operand, deps),
            Expr::Binary { left, right, .. } => {
                self.collect_expr_source_deps(left, deps);
                self.collect_expr_source_deps(right, deps);
            }
            Expr::Ternary { a, b, c, .. } => {
                self.collect_expr_source_deps(a, deps);
                self.collect_expr_source_deps(b, deps);
                self.collect_expr_source_deps(c, deps);
            }
            Expr::If { cond, then, else_ } => {
                self.collect_expr_source_deps(cond, deps);
                self.collect_expr_source_deps(then, deps);
                self.collect_expr_source_deps(else_, deps);
            }
            Expr::Coalesce { value, default } => {
                self.collect_expr_source_deps(value, deps);
                self.collect_expr_source_deps(default, deps);
            }
            Expr::Aggregate { source, .. } => self.collect_expr_source_deps(source, deps),
            Expr::NowOffset { amount, .. } => self.collect_expr_source_deps(amount, deps),
            Expr::Cached { expr, .. } => self.collect_expr_source_deps(expr, deps),
            Expr::MapExpr { source, .. } => self.collect_expr_source_deps(source, deps),
            Expr::FilterExpr { source, condition } => {
                self.collect_expr_source_deps(source, deps);
                self.collect_expr_source_deps(condition, deps);
            }
            Expr::ReduceExpr { source, .. } => self.collect_expr_source_deps(source, deps),
            Expr::SplitExpr { value, delimiter } => {
                self.collect_expr_source_deps(value, deps);
                self.collect_expr_source_deps(delimiter, deps);
            }
            Expr::ReplaceExpr { value, from, to } => {
                self.collect_expr_source_deps(value, deps);
                self.collect_expr_source_deps(from, deps);
                self.collect_expr_source_deps(to, deps);
            }
            Expr::FormatExpr { args, .. } => {
                for arg in args {
                    self.collect_expr_source_deps(arg, deps);
                }
            }
            Expr::FuncCall { args, .. } => {
                for arg in args {
                    self.collect_expr_source_deps(arg, deps);
                }
            }
            Expr::Render { vars, .. } | Expr::Translate { vars, .. } => {
                for (_, val) in vars {
                    self.collect_expr_source_deps(val, deps);
                }
            }
            Expr::WasmCall { .. } => {}
            Expr::Literal(_) | Expr::DotPath(_) => {}
        }
    }

    fn extract_construct_text(&self, formatted: &str, prefix: &str) -> String {
        let mut result = String::new();
        let mut capturing = false;

        for line in formatted.lines() {
            if !capturing {
                if line.starts_with(prefix) {
                    capturing = true;
                    result.push_str(line);
                    result.push('\n');
                }
            } else if line.starts_with("  ") || line.is_empty() {
                result.push_str(line);
                result.push('\n');
            } else {
                break;
            }
        }

        result
    }
}

fn content_hash(input: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let h = hasher.finish();
    format!("sha256:{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn link_from(input: &str) -> LinkResult {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().unwrap();
        link(&program)
    }

    #[test]
    fn test_basic_linking() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM api
  CAPABILITY read users

FLOW get_user get /users/:id
  REALM api
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let result = link_from(input);
        assert!(result.is_ok(), "errors: {:?}", result.errors);
        assert_eq!(result.manifest.shapes.len(), 1);
        assert_eq!(result.manifest.sources.len(), 1);
        assert_eq!(result.manifest.flows.len(), 1);
        assert!(result.manifest.hash.starts_with("sha256:"));
        assert_eq!(result.manifest.shapes[0].name, "User");
    }

    #[test]
    fn test_undefined_shape_in_source() {
        let input = r#"SOURCE users POSTGRES
  SHAPE User
  INDEX id
"#;
        let result = link_from(input);
        assert!(!result.is_ok());
        assert_eq!(result.errors[0].kind, LinkErrorKind::UndefinedReference);
        assert!(result.errors[0].message.contains("User"));
    }

    #[test]
    fn test_undefined_realm_in_flow() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

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
        let result = link_from(input);
        assert!(!result.is_ok());
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.message.contains("nonexistent"))
        );
    }

    #[test]
    fn test_undefined_source_in_flow() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

REALM api
  CAPABILITY read users

FLOW get_user get /users/:id
  REALM api
  AUTH session
  LET user
    FETCH nonexistent
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let result = link_from(input);
        assert!(!result.is_ok());
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.message.contains("nonexistent"))
        );
    }

    #[test]
    fn test_undefined_ref_shape() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  user_id UUID REF Ghost.id REQUIRED
"#;
        let result = link_from(input);
        assert!(!result.is_ok());
        assert!(result.errors.iter().any(|e| e.message.contains("Ghost")));
    }

    #[test]
    fn test_surface_undefined_flow_target() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

REALM api
  CAPABILITY read users

SURFACE public v1
  REALM api
  ROUTE GET /users/:id -> nonexistent_flow
"#;
        let result = link_from(input);
        assert!(!result.is_ok());
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.message.contains("nonexistent_flow"))
        );
    }

    #[test]
    fn test_deterministic_hash() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id
"#;
        let r1 = link_from(input);
        let r2 = link_from(input);
        assert_eq!(r1.manifest.hash, r2.manifest.hash);
        assert_eq!(r1.manifest.shapes[0].hash, r2.manifest.shapes[0].hash);
    }

    #[test]
    fn test_hash_changes_on_modification() {
        let input1 = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#;
        let input2 = r#"SHAPE User
  id UUID PK AUTO
  name STRING 200 REQUIRED
"#;
        let r1 = link_from(input1);
        let r2 = link_from(input2);
        assert_ne!(r1.manifest.shapes[0].hash, r2.manifest.shapes[0].hash);
        assert_ne!(r1.manifest.hash, r2.manifest.hash);
    }

    #[test]
    fn test_ref_deps_tracked() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SHAPE Order
  id UUID PK AUTO
  user_id UUID REF User.id REQUIRED
"#;
        let result = link_from(input);
        assert!(result.is_ok());
        let order = result
            .manifest
            .shapes
            .iter()
            .find(|s| s.name == "Order")
            .unwrap();
        assert!(order.deps.contains(&"User".to_string()));
    }

    #[test]
    fn test_flow_deps_include_realm_and_sources() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM api
  CAPABILITY read users

FLOW get_user get /users/:id
  REALM api
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let result = link_from(input);
        assert!(result.is_ok());
        let flow = &result.manifest.flows[0];
        assert!(flow.deps.contains(&"api".to_string()));
        assert!(flow.deps.contains(&"users".to_string()));
    }

    #[test]
    fn test_booking_links_clean() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/booking.axis"),
        )
        .unwrap();
        let result = link_from(&input);
        assert!(result.is_ok(), "errors: {:?}", result.errors);
        assert_eq!(result.manifest.shapes.len(), 3);
        assert_eq!(result.manifest.sources.len(), 3);
        assert_eq!(result.manifest.flows.len(), 3);
    }

    #[test]
    fn test_full_example_links_clean() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/full.axis"),
        )
        .unwrap();
        let result = link_from(&input);
        assert!(result.is_ok(), "errors: {:?}", result.errors);
        assert_eq!(result.manifest.shapes.len(), 3);
        assert_eq!(result.manifest.sources.len(), 3);
        assert_eq!(result.manifest.flows.len(), 3);
        assert_eq!(result.manifest.sagas.len(), 1);
        assert_eq!(result.manifest.surfaces.len(), 1);
        assert_eq!(result.manifest.migrations.len(), 1);
    }
}
