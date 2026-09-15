use std::collections::{HashMap, HashSet};

use crate::ast::*;

#[derive(Debug)]
pub struct PlanResult {
    pub flow_plans: Vec<FlowPlan>,
    pub saga_plans: Vec<SagaPlan>,
    pub warnings: Vec<PlanWarning>,
}

#[derive(Debug)]
pub struct PlanWarning {
    pub kind: PlanWarningKind,
    pub message: String,
    pub flow: String,
}

#[derive(Debug, PartialEq)]
pub enum PlanWarningKind {
    NPlusOne,
    RedundantQuery,
}

#[derive(Debug)]
pub struct FlowPlan {
    pub name: String,
    pub execution_groups: Vec<ExecutionGroup>,
    pub transaction: Option<TransactionPlan>,
    pub query_plans: Vec<QueryPlan>,
}

#[derive(Debug)]
pub struct ExecutionGroup {
    pub steps: Vec<String>,
    pub parallel: bool,
}

#[derive(Debug)]
pub struct TransactionPlan {
    pub mutations: Vec<String>,
    pub outbox_effects: Vec<String>,
}

#[derive(Debug)]
pub struct SagaPlan {
    pub name: String,
    pub step_transactions: Vec<StepTransaction>,
}

#[derive(Debug)]
pub struct StepTransaction {
    pub step_name: String,
    pub has_compensate: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryPlan {
    pub binding: String,
    pub source: String,
    pub backend: BackendStrategy,
    pub selected_index: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendStrategy {
    Sql,
    RedisGet,
    ElasticsearchDsl,
    DynamoDbQuery,
}

pub fn plan(program: &Program) -> PlanResult {
    let mut planner = Planner::new();
    planner.plan(program)
}

struct SourceInfo {
    source_type: SourceType,
    indexes: Vec<Vec<String>>,
}

struct Planner {
    source_shapes: HashMap<String, String>,
    source_info: HashMap<String, SourceInfo>,
    warnings: Vec<PlanWarning>,
}

impl Planner {
    fn new() -> Self {
        Self {
            source_shapes: HashMap::new(),
            source_info: HashMap::new(),
            warnings: Vec::new(),
        }
    }

    fn plan(&mut self, program: &Program) -> PlanResult {
        for construct in &program.constructs {
            if let Construct::Source(s) = construct {
                self.source_shapes.insert(s.name.clone(), s.shape.clone());
                self.source_info.insert(
                    s.name.clone(),
                    SourceInfo {
                        source_type: s.source_type,
                        indexes: s
                            .indexes
                            .iter()
                            .map(|idx| idx.fields.iter().map(|f| f.name.clone()).collect())
                            .collect(),
                    },
                );
            }
        }

        let mut flow_plans = Vec::new();
        let mut saga_plans = Vec::new();

        for construct in &program.constructs {
            match construct {
                Construct::Flow(flow) => {
                    flow_plans.push(self.plan_flow(flow));
                }
                Construct::Saga(saga) => {
                    saga_plans.push(self.plan_saga(saga));
                }
                _ => {}
            }
        }

        PlanResult {
            flow_plans,
            saga_plans,
            warnings: std::mem::take(&mut self.warnings),
        }
    }

    fn plan_flow(&mut self, flow: &FlowDef) -> FlowPlan {
        let dep_graph = self.build_dependency_graph(&flow.steps);
        self.detect_n_plus_one(flow, &flow.steps);
        self.detect_redundant_queries(flow, &flow.steps);
        let execution_groups = self.schedule_parallel(&dep_graph, &flow.steps);
        let transaction = self.plan_transaction(flow, &flow.steps);
        let query_plans = self.plan_queries(&flow.steps);

        FlowPlan {
            name: flow.name.clone(),
            execution_groups,
            transaction,
            query_plans,
        }
    }

    fn plan_saga(&mut self, saga: &SagaDef) -> SagaPlan {
        let step_transactions = saga
            .steps
            .iter()
            .map(|step| StepTransaction {
                step_name: step.name.clone(),
                has_compensate: !matches!(step.compensate, Compensate::None),
            })
            .collect();

        SagaPlan {
            name: saga.name.clone(),
            step_transactions,
        }
    }

    fn build_dependency_graph(&self, steps: &[FlowStep]) -> HashMap<String, HashSet<String>> {
        let mut graph: HashMap<String, HashSet<String>> = HashMap::new();
        let mut defined_bindings: Vec<String> = Vec::new();

        for step in steps {
            match step {
                FlowStep::Let(let_step) => {
                    let refs = self.collect_expr_refs(&let_step.expr);
                    let mut deps = HashSet::new();
                    for r in &refs {
                        if defined_bindings.contains(r) {
                            deps.insert(r.clone());
                        }
                    }
                    graph.insert(let_step.name.clone(), deps);
                    defined_bindings.push(let_step.name.clone());
                }
                FlowStep::Guard(guard) => {
                    let name = format!("__guard_{}", guard.name);
                    let refs = self.collect_expr_refs(&guard.expr);
                    let mut deps = HashSet::new();
                    for r in &refs {
                        if defined_bindings.contains(r) {
                            deps.insert(r.clone());
                        }
                    }
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(name);
                }
                FlowStep::Rule(rule) => {
                    let name = format!("__rule_{}", rule.name);
                    let mut deps = HashSet::new();
                    for req in &rule.requires {
                        let root = req.path.segments.first().cloned().unwrap_or_default();
                        if defined_bindings.contains(&root) {
                            deps.insert(root);
                        }
                        for r in &self.collect_expr_refs(&req.value) {
                            if defined_bindings.contains(r) {
                                deps.insert(r.clone());
                            }
                        }
                    }
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(name);
                }
                FlowStep::Insert(insert) => {
                    let name = format!("__insert_{}", insert.source);
                    let mut deps = HashSet::new();
                    for (_, expr) in &insert.fields {
                        for r in &self.collect_expr_refs(expr) {
                            if defined_bindings.contains(r) {
                                deps.insert(r.clone());
                            }
                        }
                    }
                    // inserts depend on all preceding guards/rules (they're validations)
                    for b in &defined_bindings {
                        if b.starts_with("__guard_") || b.starts_with("__rule_") {
                            deps.insert(b.clone());
                        }
                    }
                    graph.insert(name.clone(), deps);
                    if let Some(binding) = &insert.binding {
                        defined_bindings.push(binding.clone());
                    }
                    defined_bindings.push(name);
                }
                FlowStep::Upsert(upsert) => {
                    let name = format!("__upsert_{}", upsert.source);
                    let mut deps = HashSet::new();
                    for (_, expr) in &upsert.keys {
                        for r in &self.collect_expr_refs(expr) {
                            if defined_bindings.contains(r) {
                                deps.insert(r.clone());
                            }
                        }
                    }
                    for set in &upsert.sets {
                        for r in &self.collect_expr_refs(&set.value) {
                            if defined_bindings.contains(r) {
                                deps.insert(r.clone());
                            }
                        }
                    }
                    for b in &defined_bindings {
                        if b.starts_with("__guard_") || b.starts_with("__rule_") {
                            deps.insert(b.clone());
                        }
                    }
                    graph.insert(name.clone(), deps);
                    if let Some(binding) = &upsert.binding {
                        defined_bindings.push(binding.clone());
                    }
                    defined_bindings.push(name);
                }
                FlowStep::Update(update) => {
                    let name = format!("__update_{}", update.source);
                    let mut deps = HashSet::new();
                    for wh in &update.wheres {
                        for r in &self.collect_expr_refs(&wh.value) {
                            if defined_bindings.contains(r) {
                                deps.insert(r.clone());
                            }
                        }
                    }
                    for sc in &update.sets {
                        for r in &self.collect_expr_refs(&sc.value) {
                            if defined_bindings.contains(r) {
                                deps.insert(r.clone());
                            }
                        }
                    }
                    for b in &defined_bindings {
                        if b.starts_with("__guard_") || b.starts_with("__rule_") {
                            deps.insert(b.clone());
                        }
                    }
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(name);
                }
                FlowStep::Delete(delete) => {
                    let name = format!("__delete_{}", delete.source);
                    let mut deps = HashSet::new();
                    for wh in &delete.wheres {
                        for r in &self.collect_expr_refs(&wh.value) {
                            if defined_bindings.contains(r) {
                                deps.insert(r.clone());
                            }
                        }
                    }
                    for b in &defined_bindings {
                        if b.starts_with("__guard_") || b.starts_with("__rule_") {
                            deps.insert(b.clone());
                        }
                    }
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(name);
                }
                FlowStep::Effect(effect) => {
                    let kind_str = match effect.kind {
                        EffectKind::Email => "email",
                        EffectKind::PushNotification => "push",
                        EffectKind::Async => "async",
                        EffectKind::Webhook => "webhook",
                    };
                    let name = format!("__effect_{kind_str}");
                    let mut deps = HashSet::new();
                    for field in &effect.fields {
                        match field {
                            EffectField::To(expr) | EffectField::Url(expr) => {
                                for r in &self.collect_expr_refs(expr) {
                                    if defined_bindings.contains(r) {
                                        deps.insert(r.clone());
                                    }
                                }
                            }
                            EffectField::Data(exprs) => {
                                for expr in exprs {
                                    for r in &self.collect_expr_refs(expr) {
                                        if defined_bindings.contains(r) {
                                            deps.insert(r.clone());
                                        }
                                    }
                                }
                            }
                            EffectField::Template(_)
                            | EffectField::Event(_)
                            | EffectField::Task(_) => {}
                        }
                    }
                    // effects depend on all mutations
                    for b in &defined_bindings {
                        if b.starts_with("__insert_")
                            || b.starts_with("__update_")
                            || b.starts_with("__delete_")
                        {
                            deps.insert(b.clone());
                        }
                    }
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(name);
                }
                FlowStep::Match(_) => {
                    let name = "__match".to_string();
                    let mut deps = HashSet::new();
                    for b in &defined_bindings {
                        deps.insert(b.clone());
                    }
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(name);
                }
                FlowStep::Set(set_step) => {
                    let refs = self.collect_expr_refs(&set_step.expr);
                    let mut deps = HashSet::new();
                    for r in &refs {
                        if defined_bindings.contains(r) {
                            deps.insert(r.clone());
                        }
                    }
                    graph.insert(set_step.name.clone(), deps);
                    defined_bindings.push(set_step.name.clone());
                }
                FlowStep::Each(each_step) => {
                    let name = format!("__each_{}", each_step.binding);
                    let refs = self.collect_expr_refs(&each_step.source);
                    let mut deps = HashSet::new();
                    for r in &refs {
                        if defined_bindings.contains(r) {
                            deps.insert(r.clone());
                        }
                    }
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(name);
                }
                FlowStep::Fanout(fanout) => {
                    let name = format!("__fanout_{}", fanout.insert.source);
                    let mut deps = self.collect_expr_refs(&fanout.source);
                    for (_, expr) in &fanout.insert.fields {
                        deps.extend(self.collect_expr_refs(expr));
                    }
                    deps.retain(|r| defined_bindings.contains(r));
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(name);
                }
                FlowStep::Try(_) => {
                    let name = "__try".to_string();
                    let mut deps = HashSet::new();
                    for b in &defined_bindings {
                        deps.insert(b.clone());
                    }
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(name);
                }
                FlowStep::Upload(upload) => {
                    let name = format!("__upload_{}", upload.storage);
                    let refs = self.collect_expr_refs(&upload.file_expr);
                    let mut deps = HashSet::new();
                    for r in &refs {
                        if defined_bindings.contains(r) {
                            deps.insert(r.clone());
                        }
                    }
                    for b in &defined_bindings {
                        if b.starts_with("__guard_") || b.starts_with("__rule_") {
                            deps.insert(b.clone());
                        }
                    }
                    graph.insert(name.clone(), deps);
                    defined_bindings.push(upload.binding.clone());
                    defined_bindings.push(name);
                }
            }
        }

        graph
    }

    fn collect_expr_refs(&self, expr: &Expr) -> HashSet<String> {
        let mut refs = HashSet::new();
        self.walk_expr_refs(expr, &mut refs);
        refs
    }

    fn walk_expr_refs(&self, expr: &Expr, refs: &mut HashSet<String>) {
        match expr {
            Expr::DotPath(path) => {
                if let Some(root) = path.segments.first() {
                    if !matches!(root.as_str(), "auth" | "body" | "path" | "query" | "header") {
                        refs.insert(root.clone());
                    }
                }
            }
            Expr::Literal(_) => {}
            Expr::Unary { operand, .. } => self.walk_expr_refs(operand, refs),
            Expr::Binary { left, right, .. } => {
                self.walk_expr_refs(left, refs);
                self.walk_expr_refs(right, refs);
            }
            Expr::Ternary { a, b, c, .. } => {
                self.walk_expr_refs(a, refs);
                self.walk_expr_refs(b, refs);
                self.walk_expr_refs(c, refs);
            }
            Expr::If {
                cond, then, else_, ..
            } => {
                self.walk_expr_refs(cond, refs);
                self.walk_expr_refs(then, refs);
                self.walk_expr_refs(else_, refs);
            }
            Expr::Fetch { filters, .. } => {
                for f in filters {
                    self.walk_expr_refs(&f.value, refs);
                }
            }
            Expr::Query {
                filters,
                cursor,
                page_size,
                ..
            } => {
                for f in filters {
                    self.walk_expr_refs(&f.value, refs);
                }
                if let Some(c) = cursor {
                    self.walk_expr_refs(c, refs);
                }
                if let Some(ps) = page_size {
                    self.walk_expr_refs(ps, refs);
                }
            }
            Expr::Call { args, .. } => {
                for (_, arg) in args {
                    self.walk_expr_refs(arg, refs);
                }
            }
            Expr::Aggregate { source, .. } => {
                self.walk_expr_refs(source, refs);
            }
            Expr::NowOffset { amount, .. } => {
                self.walk_expr_refs(amount, refs);
            }
            Expr::Coalesce { value, default, .. } => {
                self.walk_expr_refs(value, refs);
                self.walk_expr_refs(default, refs);
            }
            Expr::Cached { expr, .. } => {
                self.walk_expr_refs(expr, refs);
            }
            Expr::MapExpr { source, .. } => self.walk_expr_refs(source, refs),
            Expr::FilterExpr { source, condition } => {
                self.walk_expr_refs(source, refs);
                self.walk_expr_refs(condition, refs);
            }
            Expr::ReduceExpr { source, .. } => self.walk_expr_refs(source, refs),
            Expr::SplitExpr { value, delimiter } => {
                self.walk_expr_refs(value, refs);
                self.walk_expr_refs(delimiter, refs);
            }
            Expr::ReplaceExpr { value, from, to } => {
                self.walk_expr_refs(value, refs);
                self.walk_expr_refs(from, refs);
                self.walk_expr_refs(to, refs);
            }
            Expr::FormatExpr { args, .. } => {
                for arg in args {
                    self.walk_expr_refs(arg, refs);
                }
            }
            Expr::FuncCall { args, .. } => {
                for arg in args {
                    self.walk_expr_refs(arg, refs);
                }
            }
            Expr::Render { vars, .. } | Expr::Translate { vars, .. } => {
                for (_, val) in vars {
                    self.walk_expr_refs(val, refs);
                }
            }
            Expr::WasmCall { .. } => {}
        }
    }

    fn detect_n_plus_one(&mut self, flow: &FlowDef, steps: &[FlowStep]) {
        let mut query_bindings: HashMap<String, String> = HashMap::new();

        for step in steps {
            if let FlowStep::Let(let_step) = step {
                if let Expr::Query { source, .. } = &let_step.expr {
                    query_bindings.insert(let_step.name.clone(), source.clone());
                }
            }
        }

        if query_bindings.is_empty() {
            return;
        }

        for step in steps {
            if let FlowStep::Let(let_step) = step {
                if let Expr::Fetch {
                    source, filters, ..
                } = &let_step.expr
                {
                    for filter in filters {
                        let refs = self.collect_expr_refs(&filter.value);
                        for r in &refs {
                            if let Some(query_source) = query_bindings.get(r) {
                                self.warnings.push(PlanWarning {
                                    kind: PlanWarningKind::NPlusOne,
                                    message: format!(
                                        "FETCH {source} references query binding '{r}' (QUERY {query_source}) — \
                                         potential N+1; batch with IN query"
                                    ),
                                    flow: flow.name.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    fn detect_redundant_queries(&mut self, flow: &FlowDef, steps: &[FlowStep]) {
        let mut seen: Vec<(String, Vec<(String, String)>)> = Vec::new();

        for step in steps {
            if let FlowStep::Let(let_step) = step {
                if let Expr::Query {
                    source, filters, ..
                } = &let_step.expr
                {
                    let filter_sig: Vec<(String, String)> = filters
                        .iter()
                        .map(|f| (f.field.clone(), format!("{:?}", f.op)))
                        .collect();

                    for (prev_source, prev_sig) in &seen {
                        if prev_source == source && prev_sig == &filter_sig {
                            self.warnings.push(PlanWarning {
                                kind: PlanWarningKind::RedundantQuery,
                                message: format!(
                                    "QUERY {source} with identical filters appears multiple times; deduplicate"
                                ),
                                flow: flow.name.clone(),
                            });
                        }
                    }
                    seen.push((source.clone(), filter_sig));
                }
            }
        }
    }

    fn schedule_parallel(
        &self,
        dep_graph: &HashMap<String, HashSet<String>>,
        steps: &[FlowStep],
    ) -> Vec<ExecutionGroup> {
        let ordered = self.topo_sort(dep_graph, steps);
        let mut groups: Vec<ExecutionGroup> = Vec::new();
        let mut scheduled: HashSet<String> = HashSet::new();

        while scheduled.len() < ordered.len() {
            let mut ready: Vec<String> = Vec::new();
            for name in &ordered {
                if scheduled.contains(name) {
                    continue;
                }
                let deps = dep_graph.get(name).cloned().unwrap_or_default();
                if deps.iter().all(|d| scheduled.contains(d)) {
                    ready.push(name.clone());
                }
            }

            if ready.is_empty() {
                break;
            }

            let parallel = ready.len() > 1;
            for r in &ready {
                scheduled.insert(r.clone());
            }
            groups.push(ExecutionGroup {
                steps: ready,
                parallel,
            });
        }

        groups
    }

    fn topo_sort(
        &self,
        dep_graph: &HashMap<String, HashSet<String>>,
        steps: &[FlowStep],
    ) -> Vec<String> {
        let mut result = Vec::new();
        for step in steps {
            let name = self.step_name(step);
            if dep_graph.contains_key(&name) {
                result.push(name);
            }
        }
        result
    }

    fn step_name(&self, step: &FlowStep) -> String {
        match step {
            FlowStep::Let(l) => l.name.clone(),
            FlowStep::Guard(g) => format!("__guard_{}", g.name),
            FlowStep::Rule(r) => format!("__rule_{}", r.name),
            FlowStep::Insert(i) => format!("__insert_{}", i.source),
            FlowStep::Upsert(u) => format!("__upsert_{}", u.source),
            FlowStep::Update(u) => format!("__update_{}", u.source),
            FlowStep::Delete(d) => format!("__delete_{}", d.source),
            FlowStep::Effect(e) => {
                let kind = match e.kind {
                    EffectKind::Email => "email",
                    EffectKind::PushNotification => "push",
                    EffectKind::Async => "async",
                    EffectKind::Webhook => "webhook",
                };
                format!("__effect_{kind}")
            }
            FlowStep::Match(_) => "__match".to_string(),
            FlowStep::Set(s) => s.name.clone(),
            FlowStep::Each(e) => format!("__each_{}", e.binding),
            FlowStep::Fanout(f) => format!("__fanout_{}", f.insert.source),
            FlowStep::Try(_) => "__try".to_string(),
            FlowStep::Upload(u) => format!("__upload_{}", u.storage),
        }
    }

    fn plan_queries(&self, steps: &[FlowStep]) -> Vec<QueryPlan> {
        let mut plans = Vec::new();
        for step in steps {
            if let FlowStep::Let(let_step) = step {
                match &let_step.expr {
                    Expr::Fetch {
                        source, filters, ..
                    } => {
                        let filter_fields: Vec<&str> =
                            filters.iter().map(|f| f.field.as_str()).collect();
                        plans.push(QueryPlan {
                            binding: let_step.name.clone(),
                            source: source.clone(),
                            backend: self.backend_for(source),
                            selected_index: self.select_index(source, &filter_fields),
                        });
                    }
                    Expr::Query {
                        source,
                        filters,
                        sorts,
                        ..
                    } => {
                        let mut fields: Vec<&str> =
                            filters.iter().map(|f| f.field.as_str()).collect();
                        for sort in sorts {
                            if !fields.contains(&sort.field.as_str()) {
                                fields.push(&sort.field);
                            }
                        }
                        plans.push(QueryPlan {
                            binding: let_step.name.clone(),
                            source: source.clone(),
                            backend: self.backend_for(source),
                            selected_index: self.select_index(source, &fields),
                        });
                    }
                    _ => {}
                }
            }
        }
        plans
    }

    fn backend_for(&self, source: &str) -> BackendStrategy {
        match self.source_info.get(source).map(|s| &s.source_type) {
            Some(SourceType::Redis) => BackendStrategy::RedisGet,
            Some(SourceType::Elasticsearch) => BackendStrategy::ElasticsearchDsl,
            Some(SourceType::Dynamodb) => BackendStrategy::DynamoDbQuery,
            _ => BackendStrategy::Sql,
        }
    }

    fn select_index(&self, source: &str, query_fields: &[&str]) -> Option<String> {
        let info = self.source_info.get(source)?;
        let mut best: Option<(usize, &Vec<String>)> = None;

        for index in &info.indexes {
            let prefix_match = index
                .iter()
                .take_while(|f| query_fields.contains(&f.as_str()))
                .count();
            if prefix_match > 0 && (best.is_none() || prefix_match > best.unwrap().0) {
                best = Some((prefix_match, index));
            }
        }

        best.map(|(_, idx)| idx.join("_"))
    }

    fn plan_transaction(&self, _flow: &FlowDef, steps: &[FlowStep]) -> Option<TransactionPlan> {
        let mut mutations = Vec::new();
        let mut outbox_effects = Vec::new();

        for step in steps {
            match step {
                FlowStep::Insert(i) => mutations.push(format!("INSERT {}", i.source)),
                FlowStep::Upsert(u) => mutations.push(format!("UPSERT {}", u.source)),
                FlowStep::Update(u) => mutations.push(format!("UPDATE {}", u.source)),
                FlowStep::Delete(d) => mutations.push(format!("DELETE {}", d.source)),
                FlowStep::Fanout(f) => mutations.push(format!("FANOUT {}", f.insert.source)),
                FlowStep::Effect(e) => {
                    let kind = match e.kind {
                        EffectKind::Email => "email",
                        EffectKind::PushNotification => "push_notification",
                        EffectKind::Async => "async",
                        EffectKind::Webhook => "webhook",
                    };
                    outbox_effects.push(kind.to_string());
                }
                _ => {}
            }
        }

        if mutations.is_empty() && outbox_effects.is_empty() {
            return None;
        }

        Some(TransactionPlan {
            mutations,
            outbox_effects,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn plan_from(input: &str) -> PlanResult {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().unwrap();
        plan(&program)
    }

    #[test]
    fn test_parallel_independent_fetches() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SHAPE Item
  id UUID PK AUTO
  name STRING 200 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

SOURCE items POSTGRES
  SHAPE Item
  INDEX id

REALM api
  CAPABILITY read users
  CAPABILITY read items

FLOW get_both get /both/:user_id/:item_id
  REALM api
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.user_id
    OR 404
  LET item
    FETCH items
      FILTER id EQ path.item_id
    OR 404
  RETURN 200 user
"#;
        let result = plan_from(input);
        assert_eq!(result.flow_plans.len(), 1);
        let plan = &result.flow_plans[0];
        assert_eq!(plan.name, "get_both");

        // user and item have no mutual dependency — should be in same parallel group
        let parallel_group = plan.execution_groups.iter().find(|g| g.parallel);
        assert!(parallel_group.is_some(), "expected a parallel group");
        let pg = parallel_group.unwrap();
        assert!(pg.steps.contains(&"user".to_string()));
        assert!(pg.steps.contains(&"item".to_string()));
    }

    #[test]
    fn test_sequential_dependent_bindings() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SHAPE Order
  id UUID PK AUTO
  user_id UUID REF User.id REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

SOURCE orders POSTGRES
  SHAPE Order
  INDEX user_id

REALM api
  CAPABILITY read users
  CAPABILITY read orders

FLOW get_user_orders get /users/:id/orders
  REALM api
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  LET orders
    QUERY orders
      FILTER user_id EQ user.id
  RETURN 200 orders
"#;
        let result = plan_from(input);
        let plan = &result.flow_plans[0];

        // orders depends on user → must be in separate groups
        assert!(plan.execution_groups.len() >= 2);
        let first = &plan.execution_groups[0];
        let second = &plan.execution_groups[1];
        assert!(first.steps.contains(&"user".to_string()));
        assert!(second.steps.contains(&"orders".to_string()));
        assert!(!first.parallel);
        assert!(!second.parallel);
    }

    #[test]
    fn test_n_plus_one_detection() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SHAPE Order
  id UUID PK AUTO
  user_id UUID REF User.id REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id
  INDEX user_id

REALM api
  CAPABILITY read users
  CAPABILITY read orders

FLOW bad_fetch get /orders
  REALM api
  AUTH session
  SCOPE TENANT auth.user_id
  LET all_orders
    QUERY orders
      FILTER user_id EQ auth.user_id
  LET order_user
    FETCH users
      FILTER id EQ all_orders.user_id
    OR 404
  RETURN 200 order_user
"#;
        let result = plan_from(input);
        assert!(!result.warnings.is_empty());
        assert_eq!(result.warnings[0].kind, PlanWarningKind::NPlusOne);
        assert!(result.warnings[0].message.contains("all_orders"));
    }

    #[test]
    fn test_redundant_query_detection() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  user_id UUID REQUIRED
  status ENUM pending confirmed REQUIRED

SOURCE orders POSTGRES
  SHAPE Order
  INDEX user_id

REALM api
  CAPABILITY read orders

FLOW double_query get /orders
  REALM api
  AUTH session
  SCOPE TENANT auth.user_id
  LET orders_a
    QUERY orders
      FILTER user_id EQ auth.user_id
  LET orders_b
    QUERY orders
      FILTER user_id EQ auth.user_id
  RETURN 200 orders_a
"#;
        let result = plan_from(input);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.kind == PlanWarningKind::RedundantQuery)
        );
    }

    #[test]
    fn test_transaction_plan_mutations() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE
  name STRING 100 REQUIRED

SHAPE Order
  id UUID PK AUTO
  user_id UUID REF User.id REQUIRED
  status ENUM pending confirmed REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id
  INDEX email UNIQUE

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id
  INDEX user_id

REALM api
  CAPABILITY read users
  CAPABILITY read orders
  CAPABILITY write orders
  CAPABILITY effect email

FLOW create_order post /orders
  REALM api
  AUTH session
  SCOPE TENANT auth.user_id
  LIMIT 10 PER_MINUTE PER_USER
  BODY OrderCreate
    user_id UUID REQUIRED
  LET user
    FETCH users
      FILTER id EQ body.user_id
    OR 404
  INSERT orders
    user_id body.user_id
    status pending
  AS order
  EFFECT email
    TEMPLATE order_created
    TO user.id
    DATA order
  RETURN 201 order
"#;
        let result = plan_from(input);
        let plan = &result.flow_plans[0];
        assert!(plan.transaction.is_some());
        let txn = plan.transaction.as_ref().unwrap();
        assert_eq!(txn.mutations.len(), 1);
        assert_eq!(txn.mutations[0], "INSERT orders");
        assert_eq!(txn.outbox_effects.len(), 1);
        assert_eq!(txn.outbox_effects[0], "email");
    }

    #[test]
    fn test_saga_step_transactions() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  status ENUM pending confirmed REQUIRED
  total DECIMAL PRECISION 10 SCALE 2 REQUIRED

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

REALM api
  CAPABILITY read orders
  CAPABILITY write orders

SERVICE payments
  ENDPOINT stripe
  AUTH bearer VAULT stripe_key
  METHOD charge
    INPUT amount DECIMAL currency STRING 10
    OUTPUT transaction_id STRING 100 status STRING 20
    TIMEOUT 30 s
    RETRY 3 BACKOFF exponential
  METHOD refund
    INPUT transaction_id STRING 100
    OUTPUT refund_id STRING 100
    TIMEOUT 30 s
    RETRY 2 BACKOFF exponential

SAGA process_order POST /orders/process
  REALM api
  AUTH session
  BODY OrderProcess
    order_id UUID REQUIRED

  STEP verify_order
    LET order
      FETCH orders
        FILTER id EQ body.order_id
      OR 404
    VERIFY
      EQ order.status pending
    YIELD order
    COMPENSATE NONE

  STEP charge_payment
    LET payment
      CALL payments.charge
        amount order.total
        currency usd
      OR 500
    YIELD payment
    COMPENSATE
      LET refund
        CALL payments.refund
          transaction_id payment.transaction_id
        OR 500

  STEP confirm_order
    UPDATE orders
      WHERE id EQ order.id
      SET status confirmed
    YIELD confirmed
    COMPENSATE
      UPDATE orders
        WHERE id EQ order.id
        SET status pending

  ON_FAILURE RUN_COMPENSATIONS
  ON_SUCCESS
    EFFECT email
      TEMPLATE order_confirmed
      TO order.user_id
      DATA order
    RETURN 200 order
"#;
        let result = plan_from(input);
        assert_eq!(result.saga_plans.len(), 1);
        let sp = &result.saga_plans[0];
        assert_eq!(sp.name, "process_order");
        assert_eq!(sp.step_transactions.len(), 3);
        assert!(!sp.step_transactions[0].has_compensate); // COMPENSATE NONE
        assert!(sp.step_transactions[1].has_compensate);
        assert!(sp.step_transactions[2].has_compensate);
    }

    #[test]
    fn test_read_only_flow_no_transaction() {
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
        let result = plan_from(input);
        let plan = &result.flow_plans[0];
        assert!(plan.transaction.is_none());
    }

    #[test]
    fn test_three_independent_fetches() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SHAPE Item
  id UUID PK AUTO
  name STRING 200 REQUIRED

SHAPE Category
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

SOURCE items POSTGRES
  SHAPE Item
  INDEX id

SOURCE categories POSTGRES
  SHAPE Category
  INDEX id

REALM api
  CAPABILITY read users
  CAPABILITY read items
  CAPABILITY read categories

FLOW get_all get /all/:uid/:iid/:cid
  REALM api
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.uid
    OR 404
  LET item
    FETCH items
      FILTER id EQ path.iid
    OR 404
  LET category
    FETCH categories
      FILTER id EQ path.cid
    OR 404
  RETURN 200 user
"#;
        let result = plan_from(input);
        let plan = &result.flow_plans[0];
        let parallel_group = plan.execution_groups.iter().find(|g| g.parallel);
        assert!(parallel_group.is_some());
        let pg = parallel_group.unwrap();
        assert_eq!(pg.steps.len(), 3);
    }

    #[test]
    fn test_guard_blocks_dependent_insert() {
        let input = r#"SHAPE Item
  id UUID PK AUTO
  name STRING 200 REQUIRED
  stock INT MIN 0 REQUIRED

SOURCE items POSTGRES
  SHAPE Item
  INDEX id

REALM api
  CAPABILITY read items
  CAPABILITY write items

FLOW restock post /items/:id/restock
  REALM api
  AUTH session
  LIMIT 10 PER_MINUTE PER_USER
  BODY Restock
    quantity INT MIN 1 REQUIRED
  LET item
    FETCH items
      FILTER id EQ path.id
    OR 404
  GUARD positive_stock 400 "stock must be positive"
    GTE item.stock 0
  INSERT items
    name item.name
    stock body.quantity
  AS restocked
  RETURN 201 restocked
"#;
        let result = plan_from(input);
        let plan = &result.flow_plans[0];

        // The insert should come after the guard
        let insert_group_idx = plan
            .execution_groups
            .iter()
            .position(|g| g.steps.iter().any(|s| s.starts_with("__insert_")))
            .unwrap();
        let guard_group_idx = plan
            .execution_groups
            .iter()
            .position(|g| g.steps.iter().any(|s| s.starts_with("__guard_")))
            .unwrap();
        assert!(guard_group_idx < insert_group_idx);
    }

    #[test]
    fn test_booking_flow_plan() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/booking.axis"),
        )
        .unwrap();
        let result = plan_from(&input);
        assert!(result.warnings.is_empty());
        assert_eq!(result.flow_plans.len(), 3);

        let create = result
            .flow_plans
            .iter()
            .find(|p| p.name == "create_booking")
            .unwrap();
        assert!(create.transaction.is_some());
        let txn = create.transaction.as_ref().unwrap();
        assert_eq!(txn.mutations.len(), 1);
        assert_eq!(txn.outbox_effects.len(), 1);
    }

    #[test]
    fn test_query_plan_index_selection() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE
  name STRING 100 REQUIRED
  status ENUM active suspended REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX email UNIQUE
  INDEX status name

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH users
      FILTER email EQ path.id
    OR 404
  RETURN 200 user
"#;
        let result = plan_from(input);
        assert_eq!(result.flow_plans.len(), 1);
        let qp = &result.flow_plans[0].query_plans;
        assert_eq!(qp.len(), 1);
        assert_eq!(qp[0].backend, BackendStrategy::Sql);
        assert_eq!(qp[0].selected_index, Some("email".into()));
    }

    #[test]
    fn test_query_plan_backend_strategy() {
        let input = r#"SHAPE Session
  id UUID PK AUTO
  data STRING 1000 REQUIRED

SOURCE sessions REDIS
  SHAPE Session
  TTL 3600

FLOW get_session get /sessions/:id
  AUTH session
  LET s
    FETCH sessions
      FILTER id EQ path.id
    OR 404
  RETURN 200 s
"#;
        let result = plan_from(input);
        let qp = &result.flow_plans[0].query_plans;
        assert_eq!(qp.len(), 1);
        assert_eq!(qp[0].backend, BackendStrategy::RedisGet);
    }

    #[test]
    fn test_query_plan_best_prefix_index() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  user_id UUID REQUIRED
  status ENUM pending confirmed REQUIRED
  created_at TIMESTAMP AUTO

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id
  INDEX user_id status
  INDEX user_id created_at

FLOW list get /orders
  AUTH session
  LET orders
    QUERY orders
      FILTER user_id EQ auth.user_id
      FILTER status EQ query.status
  RETURN 200 orders
"#;
        let result = plan_from(input);
        let qp = &result.flow_plans[0].query_plans;
        assert_eq!(qp.len(), 1);
        assert_eq!(qp[0].selected_index, Some("user_id_status".into()));
    }
}
