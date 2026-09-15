use std::fmt::Write;

use serde::Serialize;

use crate::ast::*;

pub struct CodegenResult {
    pub sql: String,
    pub routes: Vec<RouteInfo>,
    pub migrations: Vec<String>,
    pub surfaces: Vec<SurfaceSpec>,
    pub sagas: Vec<SagaPlan>,
    pub streams: Vec<StreamSpec>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamSpec {
    pub name: String,
    pub transport: String,
    pub path: String,
    pub auth: Option<String>,
    pub events: Vec<StreamEventSpec>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamEventSpec {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SagaPlan {
    pub name: String,
    pub method: String,
    pub path: String,
    pub auth: Option<String>,
    pub steps: Vec<SagaStepPlan>,
    pub on_success_effects: Vec<Instruction>,
    pub return_code: i64,
    pub return_binding: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SagaStepPlan {
    pub name: String,
    pub instructions: Vec<Instruction>,
    pub verify: Option<ConditionIr>,
    pub yields: Vec<String>,
    pub compensate: Vec<Instruction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SurfaceSpec {
    pub name: String,
    pub version: String,
    pub base_path: String,
    pub routes: Vec<SurfaceRoute>,
    pub schemas: Vec<SchemaSpec>,
    pub deprecation: Option<DeprecationSpec>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SurfaceRoute {
    pub method: String,
    pub path: String,
    pub operation_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaSpec {
    pub name: String,
    pub source_shape: String,
    pub fields: Vec<SchemaField>,
    pub hidden: Vec<String>,
    pub renames: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaField {
    pub name: String,
    pub ty: String,
    pub format: Option<String>,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeprecationSpec {
    pub replaces: String,
    pub sunset: String,
}

#[derive(Debug, Serialize)]
pub struct RouteInfo {
    pub name: String,
    pub method: String,
    pub path: String,
    pub auth: Option<String>,
    pub instructions: Vec<Instruction>,
    pub return_code: i64,
    pub return_binding: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Instruction {
    CheckRule {
        name: String,
        checks: Vec<RuleCheck>,
    },
    Guard {
        name: String,
        error_code: i64,
        error_message: Option<String>,
        condition: ConditionIr,
    },
    FetchOne {
        binding: String,
        source: String,
        filters: Vec<FilterIr>,
        or_code: i64,
        or_message: Option<String>,
    },
    QueryMany {
        binding: String,
        source: String,
        filters: Vec<FilterIr>,
        sorts: Vec<SortIr>,
        page_size: Option<ExprIr>,
    },
    Compute {
        binding: String,
        expr: ExprIr,
    },
    Insert {
        source: String,
        fields: Vec<(String, ExprIr)>,
        binding: Option<String>,
    },
    Upsert {
        source: String,
        keys: Vec<(String, ExprIr)>,
        sets: Vec<(String, ExprIr)>,
        binding: Option<String>,
        auto_updated_at: bool,
    },
    Fanout {
        item: String,
        collection: ExprIr,
        source: String,
        fields: Vec<(String, ExprIr)>,
    },
    Update {
        source: String,
        wheres: Vec<FilterIr>,
        sets: Vec<(String, ExprIr)>,
        binding: Option<String>,
        or_code: i64,
    },
    Delete {
        source: String,
        wheres: Vec<FilterIr>,
        or_code: i64,
    },
    EmitEffect {
        kind: String,
        fields: Vec<(String, ExprIr)>,
    },
    Match {
        branches: Vec<MatchBranchIr>,
        default: Option<Vec<Instruction>>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchBranchIr {
    pub condition: ConditionIr,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleCheck {
    pub path: String,
    pub op: String,
    pub value: ExprIr,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConditionIr {
    Compare {
        op: String,
        left: ExprIr,
        right: ExprIr,
    },
    Unary {
        op: String,
        operand: Box<ConditionIr>,
    },
    Expr(ExprIr),
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterIr {
    pub field: String,
    pub op: String,
    pub value: ExprIr,
}

#[derive(Debug, Clone, Serialize)]
pub struct SortIr {
    pub field: String,
    pub direction: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExprIr {
    Literal {
        value: String,
    },
    Path {
        value: String,
    },
    BinaryOp {
        op: String,
        left: Box<ExprIr>,
        right: Box<ExprIr>,
    },
    UnaryOp {
        op: String,
        operand: Box<ExprIr>,
    },
    FuncCall {
        name: String,
        args: Vec<ExprIr>,
    },
}

pub fn generate(program: &Program) -> CodegenResult {
    let mut sql = String::new();
    let mut routes = Vec::new();
    let mut migrations = Vec::new();
    let mut surfaces = Vec::new();
    let mut sagas = Vec::new();
    let mut streams = Vec::new();

    let shapes = collect_shapes(program);
    let sources = collect_sources(program);
    let shape_to_source: std::collections::HashMap<String, String> = sources
        .iter()
        .map(|s| (s.shape.clone(), s.name.clone()))
        .collect();
    let auto_updated_sources: std::collections::HashSet<String> = sources
        .iter()
        .filter(|source| {
            shapes.get(&source.shape).is_some_and(|shape| {
                shape.fields.iter().any(|field| {
                    field.name == "updated_at"
                        && field
                            .modifiers
                            .iter()
                            .any(|modifier| matches!(modifier, Modifier::Auto))
                })
            })
        })
        .map(|source| source.name.clone())
        .collect();

    let primary_dialect = sources
        .first()
        .map(|s| source_dialect(s))
        .unwrap_or(SqlDialect::Postgres);

    let sorted_sources = toposort_sources(&sources, &shapes);
    for source in &sorted_sources {
        if let Some(shape) = shapes.get(&source.shape) {
            generate_table(&mut sql, source, shape, &shape_to_source);
        }
    }

    for construct in &program.constructs {
        match construct {
            Construct::Flow(flow) => {
                routes.push(generate_route(flow, &auto_updated_sources));
            }
            Construct::Saga(saga) => {
                generate_saga_journal(&mut sql, saga, primary_dialect);
                sagas.push(generate_saga_plan(saga, &auto_updated_sources));
            }
            Construct::Migrate(m) => {
                migrations.push(generate_migration(m, &sources));
            }
            Construct::Surface(surface) => {
                surfaces.push(generate_surface(surface, &shapes));
            }
            Construct::Stream(stream) => {
                streams.push(generate_stream(stream));
            }
            _ => {}
        }
    }

    let has_effects = program.constructs.iter().any(|c| {
        if let Construct::Flow(f) = c {
            flow_has_effects(&f.steps)
        } else {
            false
        }
    });
    if has_effects {
        generate_outbox_table(&mut sql, primary_dialect);
    }
    if program
        .constructs
        .iter()
        .any(|construct| matches!(construct, Construct::Flow(flow) if flow.idempotency.is_some()))
    {
        generate_idempotency_table(&mut sql, primary_dialect);
    }

    CodegenResult {
        sql,
        routes,
        migrations,
        surfaces,
        sagas,
        streams,
    }
}

fn flow_has_effects(steps: &[FlowStep]) -> bool {
    steps.iter().any(|s| match s {
        FlowStep::Effect(_) => true,
        FlowStep::Match(m) => {
            m.branches.iter().any(|b| flow_has_effects(&b.steps))
                || m.default.as_ref().is_some_and(|d| flow_has_effects(d))
        }
        FlowStep::Each(e) => flow_has_effects(&e.steps),
        FlowStep::Try(t) => flow_has_effects(&t.body) || flow_has_effects(&t.recover),
        _ => false,
    })
}

fn toposort_sources<'a>(
    sources: &[&'a SourceDef],
    shapes: &std::collections::HashMap<String, &ShapeDef>,
) -> Vec<&'a SourceDef> {
    use std::collections::{HashMap, HashSet, VecDeque};

    let shape_to_idx: HashMap<&str, usize> = sources
        .iter()
        .enumerate()
        .map(|(i, s)| (s.shape.as_str(), i))
        .collect();

    let mut deps: Vec<HashSet<usize>> = vec![HashSet::new(); sources.len()];
    for (i, source) in sources.iter().enumerate() {
        if let Some(shape) = shapes.get(&source.shape) {
            for field in &shape.fields {
                let ref_shapes: Vec<&str> = std::iter::empty()
                    .chain(match &field.ty {
                        TypeExpr::Ref { shape, .. } => Some(shape.as_str()),
                        _ => None,
                    })
                    .chain(field.modifiers.iter().filter_map(|m| match m {
                        Modifier::Ref { shape, .. } => Some(shape.as_str()),
                        _ => None,
                    }))
                    .collect();
                for ref_shape in ref_shapes {
                    if let Some(&j) = shape_to_idx.get(ref_shape) {
                        if j != i {
                            deps[i].insert(j);
                        }
                    }
                }
            }
        }
    }

    let mut in_degree: Vec<usize> = vec![0; sources.len()];
    for d in &deps {
        for &dep in d {
            in_degree[dep] += 0;
        }
    }
    for i in 0..sources.len() {
        in_degree[i] = deps[i].len();
    }

    let mut queue: VecDeque<usize> = (0..sources.len()).filter(|&i| in_degree[i] == 0).collect();
    let mut order = Vec::with_capacity(sources.len());

    while let Some(idx) = queue.pop_front() {
        order.push(idx);
        for i in 0..sources.len() {
            if deps[i].remove(&idx) {
                in_degree[i] -= 1;
                if in_degree[i] == 0 {
                    queue.push_back(i);
                }
            }
        }
    }

    // append any remaining (cycles) in original order
    for i in 0..sources.len() {
        if !order.contains(&i) {
            order.push(i);
        }
    }

    order.into_iter().map(|i| sources[i]).collect()
}

fn collect_shapes(program: &Program) -> std::collections::HashMap<String, &ShapeDef> {
    let mut map = std::collections::HashMap::new();
    for c in &program.constructs {
        if let Construct::Shape(s) = c {
            map.insert(s.name.clone(), s);
        }
    }
    map
}

fn collect_sources(program: &Program) -> Vec<&SourceDef> {
    program
        .constructs
        .iter()
        .filter_map(|c| {
            if let Construct::Source(s) = c {
                Some(s)
            } else {
                None
            }
        })
        .collect()
}

fn source_dialect(source: &SourceDef) -> SqlDialect {
    match source.source_type {
        SourceType::Mysql => SqlDialect::Mysql,
        SourceType::Sqlite => SqlDialect::Sqlite,
        _ => SqlDialect::Postgres,
    }
}

#[derive(Clone, Copy)]
enum SqlDialect {
    Postgres,
    Mysql,
    Sqlite,
}

fn generate_table(
    sql: &mut String,
    source: &SourceDef,
    shape: &ShapeDef,
    shape_to_source: &std::collections::HashMap<String, String>,
) {
    let dialect = source_dialect(source);
    writeln!(sql, "CREATE TABLE IF NOT EXISTS {} (", source.name).unwrap();

    let field_count = shape.fields.len();
    for (i, field) in shape.fields.iter().enumerate() {
        let col_type = sql_type(&field.ty, dialect).replace("{col}", &field.name);
        write!(sql, "  {} {}", field.name, col_type).unwrap();

        let is_pk = field.modifiers.iter().any(|m| matches!(m, Modifier::Pk));
        let is_required = field
            .modifiers
            .iter()
            .any(|m| matches!(m, Modifier::Required));
        let is_unique = field
            .modifiers
            .iter()
            .any(|m| matches!(m, Modifier::Unique));
        let is_auto = field.modifiers.iter().any(|m| matches!(m, Modifier::Auto));

        if is_pk {
            write!(sql, " PRIMARY KEY").unwrap();
        }
        if is_required && !is_pk {
            write!(sql, " NOT NULL").unwrap();
        }
        if is_unique {
            write!(sql, " UNIQUE").unwrap();
        }

        if is_auto {
            match (&field.ty, dialect) {
                (TypeExpr::Uuid, SqlDialect::Postgres) => {
                    write!(sql, " DEFAULT gen_random_uuid()").unwrap()
                }
                (TypeExpr::Timestamp, SqlDialect::Postgres) => {
                    write!(sql, " DEFAULT now()").unwrap()
                }
                (TypeExpr::Timestamp, SqlDialect::Mysql) => write!(sql, " DEFAULT NOW()").unwrap(),
                (TypeExpr::Timestamp, SqlDialect::Sqlite) => {
                    write!(sql, " DEFAULT (datetime('now'))").unwrap()
                }
                _ => {}
            }
        }

        for m in &field.modifiers {
            if let Modifier::Default(val) = m {
                write!(sql, " DEFAULT {}", sql_literal(val, dialect)).unwrap();
            }
        }

        if let TypeExpr::Int { min, max } = &field.ty {
            if let Some(v) = min {
                write!(sql, " CHECK ({} >= {})", field.name, v).unwrap();
            }
            if let Some(v) = max {
                write!(sql, " CHECK ({} <= {})", field.name, v).unwrap();
            }
        }

        if let TypeExpr::Ref {
            shape: ref_shape,
            field: ref_field,
        } = &field.ty
        {
            let table = shape_to_source
                .get(ref_shape)
                .map(|s| s.as_str())
                .unwrap_or_else(|| ref_shape.as_str());
            write!(sql, " REFERENCES {}({})", table.to_lowercase(), ref_field).unwrap();
        }
        for m in &field.modifiers {
            if let Modifier::Ref {
                shape,
                field: ref_field,
            } = m
            {
                let table = shape_to_source
                    .get(shape)
                    .map(|s| s.as_str())
                    .unwrap_or_else(|| shape.as_str());
                write!(sql, " REFERENCES {}({})", table.to_lowercase(), ref_field).unwrap();
            }
        }

        if i < field_count - 1 {
            writeln!(sql, ",").unwrap();
        } else {
            writeln!(sql).unwrap();
        }
    }
    writeln!(sql, ");\n").unwrap();

    for index in &source.indexes {
        let is_unique = index
            .fields
            .iter()
            .any(|f| matches!(&f.suffix, Some(IndexSuffix::Unique)));
        let is_geo = index
            .fields
            .iter()
            .any(|f| matches!(&f.suffix, Some(IndexSuffix::Geo)));
        let is_text = index
            .fields
            .iter()
            .any(|f| matches!(&f.suffix, Some(IndexSuffix::Text)));
        let field_names: Vec<&str> = index.fields.iter().map(|f| f.name.as_str()).collect();
        let idx_name = format!("idx_{}_{}", source.name, field_names.join("_"));

        if is_geo && matches!(dialect, SqlDialect::Postgres) {
            let col = &index.fields[0].name;
            writeln!(
                sql,
                "CREATE INDEX IF NOT EXISTS {} ON {} USING gist ({});",
                idx_name, source.name, col
            )
            .unwrap();
        } else if is_text && matches!(dialect, SqlDialect::Postgres) {
            let col = &index.fields[0].name;
            writeln!(
                sql,
                "CREATE INDEX IF NOT EXISTS {} ON {} USING gin (to_tsvector('english', {}));",
                idx_name, source.name, col
            )
            .unwrap();
        } else if is_geo || is_text {
            // GIN/GiST not available on MySQL/SQLite — fall back to regular index
            let idx_fields: Vec<&str> = index.fields.iter().map(|f| f.name.as_str()).collect();
            writeln!(
                sql,
                "CREATE INDEX IF NOT EXISTS {} ON {} ({});",
                idx_name,
                source.name,
                idx_fields.join(", ")
            )
            .unwrap();
        } else {
            let idx_fields: Vec<String> = index
                .fields
                .iter()
                .map(|f| match &f.suffix {
                    Some(IndexSuffix::Desc) => format!("{} DESC", f.name),
                    Some(IndexSuffix::Asc) => format!("{} ASC", f.name),
                    _ => f.name.clone(),
                })
                .collect();

            if is_unique {
                writeln!(
                    sql,
                    "CREATE UNIQUE INDEX IF NOT EXISTS {} ON {} ({});",
                    idx_name,
                    source.name,
                    idx_fields.join(", ")
                )
                .unwrap();
            } else {
                writeln!(
                    sql,
                    "CREATE INDEX IF NOT EXISTS {} ON {} ({});",
                    idx_name,
                    source.name,
                    idx_fields.join(", ")
                )
                .unwrap();
            }
        }
    }

    if !source.indexes.is_empty() {
        writeln!(sql).unwrap();
    }
}

fn sql_type(ty: &TypeExpr, dialect: SqlDialect) -> String {
    match (ty, dialect) {
        (TypeExpr::Uuid, SqlDialect::Postgres) => "UUID".into(),
        (TypeExpr::Uuid, SqlDialect::Mysql) => "CHAR(36)".into(),
        (TypeExpr::Uuid, SqlDialect::Sqlite) => "TEXT".into(),

        (TypeExpr::Bool, SqlDialect::Postgres) => "BOOLEAN".into(),
        (TypeExpr::Bool, SqlDialect::Mysql) => "TINYINT(1)".into(),
        (TypeExpr::Bool, SqlDialect::Sqlite) => "INTEGER".into(),

        (TypeExpr::Date, SqlDialect::Sqlite) => "TEXT".into(),
        (TypeExpr::Date, _) => "DATE".into(),

        (TypeExpr::Timestamp, SqlDialect::Postgres) => "TIMESTAMPTZ".into(),
        (TypeExpr::Timestamp, SqlDialect::Mysql) => "DATETIME".into(),
        (TypeExpr::Timestamp, SqlDialect::Sqlite) => "TEXT".into(),

        (TypeExpr::Text, _) => "TEXT".into(),

        (TypeExpr::String(Some(_)), SqlDialect::Sqlite) => "TEXT".into(),
        (TypeExpr::String(Some(n)), _) => format!("VARCHAR({})", n),
        (TypeExpr::String(None), _) => "TEXT".into(),

        (TypeExpr::Int { .. }, _) => "INTEGER".into(),

        (TypeExpr::Decimal { .. }, SqlDialect::Sqlite) => "REAL".into(),
        (
            TypeExpr::Decimal {
                precision: Some(p),
                scale: Some(s),
            },
            SqlDialect::Mysql,
        ) => format!("DECIMAL({},{})", p, s),
        (
            TypeExpr::Decimal {
                precision: Some(p),
                scale: Some(s),
            },
            _,
        ) => format!("NUMERIC({},{})", p, s),
        (TypeExpr::Decimal { .. }, SqlDialect::Mysql) => "DECIMAL".into(),
        (TypeExpr::Decimal { .. }, _) => "NUMERIC".into(),

        (TypeExpr::Enum(variants), _) => {
            format!(
                "VARCHAR(50) CHECK ({{col}} IN ({}))",
                variants
                    .iter()
                    .map(|v| format!("'{}'", v))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }

        (TypeExpr::Json, SqlDialect::Postgres) => "JSONB".into(),
        (TypeExpr::Json, SqlDialect::Mysql) => "JSON".into(),
        (TypeExpr::Json, SqlDialect::Sqlite) => "TEXT".into(),

        (TypeExpr::List(inner), SqlDialect::Postgres) => format!("{}[]", sql_type(inner, dialect)),
        (TypeExpr::List(_), SqlDialect::Mysql) => "JSON".into(),
        (TypeExpr::List(_), SqlDialect::Sqlite) => "TEXT".into(),

        (TypeExpr::Map(_, _), SqlDialect::Postgres) => "JSONB".into(),
        (TypeExpr::Map(_, _), SqlDialect::Mysql) => "JSON".into(),
        (TypeExpr::Map(_, _), SqlDialect::Sqlite) => "TEXT".into(),

        (TypeExpr::Ref { .. }, SqlDialect::Postgres) => "UUID".into(),
        (TypeExpr::Ref { .. }, SqlDialect::Mysql) => "CHAR(36)".into(),
        (TypeExpr::Ref { .. }, SqlDialect::Sqlite) => "TEXT".into(),

        (TypeExpr::Blob, SqlDialect::Postgres) => "BYTEA".into(),
        (TypeExpr::Blob, SqlDialect::Mysql) => "LONGBLOB".into(),
        (TypeExpr::Blob, SqlDialect::Sqlite) => "BLOB".into(),

        (TypeExpr::Maybe(inner), _) => sql_type(inner, dialect),
    }
}

fn sql_literal(val: &LiteralValue, dialect: SqlDialect) -> String {
    match (val, dialect) {
        (LiteralValue::Int(n), _) => n.to_string(),
        (LiteralValue::Decimal(d), _) => d.clone(),
        (LiteralValue::String(s), _) => format!("'{}'", s.replace('\'', "''")),
        (LiteralValue::Bool(b), SqlDialect::Sqlite) => if *b { "1" } else { "0" }.into(),
        (LiteralValue::Bool(b), _) => if *b { "TRUE" } else { "FALSE" }.into(),
        (LiteralValue::Ident(s), _) => format!("'{}'", s),
        (LiteralValue::Now, SqlDialect::Sqlite) => "datetime('now')".into(),
        (LiteralValue::Now, _) => "now()".into(),
        (LiteralValue::None, _) => "NULL".into(),
    }
}

fn generate_route(
    flow: &FlowDef,
    auto_updated_sources: &std::collections::HashSet<String>,
) -> RouteInfo {
    let method = match flow.method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Webhook => "POST",
    }
    .into();

    let auth = flow.auth.as_ref().map(|a| match a {
        AuthDecl::None => "none".into(),
        AuthDecl::Session => "session".into(),
        AuthDecl::Bearer => "bearer".into(),
        AuthDecl::ApiKey => "api_key".into(),
        AuthDecl::Role(r) => format!("role:{}", r),
        AuthDecl::RoleIn(roles) => format!("role_in:{}", roles.join(",")),
        AuthDecl::WebhookSignature { .. } => "webhook_signature".into(),
    });

    let instructions = lower_steps(&flow.steps, auto_updated_sources);

    let return_binding = flow.return_stmt.body.as_ref().and_then(|b| match b {
        ReturnBody::Binding(name) => Some(name.clone()),
        _ => None,
    });

    RouteInfo {
        name: flow.name.clone(),
        method,
        path: flow.path.clone(),
        auth,
        instructions,
        return_code: flow.return_stmt.code,
        return_binding,
    }
}

fn lower_steps(
    steps: &[FlowStep],
    auto_updated_sources: &std::collections::HashSet<String>,
) -> Vec<Instruction> {
    let mut instructions = Vec::new();
    for step in steps {
        match step {
            FlowStep::Rule(rule) => {
                let checks = rule
                    .requires
                    .iter()
                    .map(|r| RuleCheck {
                        path: r.path.as_str(),
                        op: compare_op_str(&r.op),
                        value: lower_expr(&r.value),
                    })
                    .collect();
                instructions.push(Instruction::CheckRule {
                    name: rule.name.clone(),
                    checks,
                });
            }
            FlowStep::Guard(guard) => {
                instructions.push(Instruction::Guard {
                    name: guard.name.clone(),
                    error_code: guard.code,
                    error_message: guard.message.clone(),
                    condition: lower_condition(&guard.expr),
                });
            }
            FlowStep::Let(let_step) => match &let_step.expr {
                Expr::Fetch {
                    source,
                    filters,
                    or_code,
                    or_message,
                    ..
                } => {
                    instructions.push(Instruction::FetchOne {
                        binding: let_step.name.clone(),
                        source: source.clone(),
                        filters: lower_filters(filters),
                        or_code: *or_code,
                        or_message: or_message.clone(),
                    });
                }
                Expr::Query {
                    source,
                    filters,
                    sorts,
                    page_size,
                    ..
                } => {
                    instructions.push(Instruction::QueryMany {
                        binding: let_step.name.clone(),
                        source: source.clone(),
                        filters: lower_filters(filters),
                        sorts: lower_sorts(sorts),
                        page_size: page_size.as_ref().map(|p| lower_expr(p)),
                    });
                }
                _ => {
                    instructions.push(Instruction::Compute {
                        binding: let_step.name.clone(),
                        expr: lower_expr(&let_step.expr),
                    });
                }
            },
            FlowStep::Insert(insert) => {
                let fields = insert
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), lower_expr(v)))
                    .collect();
                instructions.push(Instruction::Insert {
                    source: insert.source.clone(),
                    fields,
                    binding: insert.binding.clone(),
                });
            }
            FlowStep::Upsert(upsert) => {
                instructions.push(Instruction::Upsert {
                    source: upsert.source.clone(),
                    keys: upsert
                        .keys
                        .iter()
                        .map(|(k, v)| (k.clone(), lower_expr(v)))
                        .collect(),
                    sets: upsert
                        .sets
                        .iter()
                        .map(|s| (s.field.clone(), lower_expr(&s.value)))
                        .collect(),
                    binding: upsert.binding.clone(),
                    auto_updated_at: auto_updated_sources.contains(&upsert.source)
                        && !upsert.sets.iter().any(|set| set.field == "updated_at"),
                });
            }
            FlowStep::Update(update) => {
                let wheres = update
                    .wheres
                    .iter()
                    .map(|w| FilterIr {
                        field: w.field.clone(),
                        op: compare_op_str(&w.op),
                        value: lower_expr(&w.value),
                    })
                    .collect();
                let sets = update
                    .sets
                    .iter()
                    .map(|s| (s.field.clone(), lower_expr(&s.value)))
                    .collect();
                instructions.push(Instruction::Update {
                    source: update.source.clone(),
                    wheres,
                    sets,
                    binding: update.binding.as_ref().map(|b| match b {
                        UpdateBinding::As(name) | UpdateBinding::Count(name) => name.clone(),
                    }),
                    or_code: update.or_code,
                });
            }
            FlowStep::Delete(delete) => {
                let wheres = delete
                    .wheres
                    .iter()
                    .map(|w| FilterIr {
                        field: w.field.clone(),
                        op: compare_op_str(&w.op),
                        value: lower_expr(&w.value),
                    })
                    .collect();
                instructions.push(Instruction::Delete {
                    source: delete.source.clone(),
                    wheres,
                    or_code: delete.or_code,
                });
            }
            FlowStep::Fanout(fanout) => {
                instructions.push(Instruction::Fanout {
                    item: fanout.binding.clone(),
                    collection: lower_expr(&fanout.source),
                    source: fanout.insert.source.clone(),
                    fields: fanout
                        .insert
                        .fields
                        .iter()
                        .map(|(k, v)| (k.clone(), lower_expr(v)))
                        .collect(),
                });
            }
            FlowStep::Effect(effect) => {
                let kind = match effect.kind {
                    EffectKind::Email => "email",
                    EffectKind::PushNotification => "push_notification",
                    EffectKind::Async => "async",
                    EffectKind::Webhook => "webhook",
                }
                .into();
                let fields = effect
                    .fields
                    .iter()
                    .filter_map(|f| match f {
                        EffectField::Template(t) => {
                            Some(("template".into(), ExprIr::Literal { value: t.clone() }))
                        }
                        EffectField::To(e) => Some(("to".into(), lower_expr(e))),
                        EffectField::Url(e) => Some(("url".into(), lower_expr(e))),
                        EffectField::Event(e) => {
                            Some(("event".into(), ExprIr::Literal { value: e.clone() }))
                        }
                        EffectField::Task(t) => {
                            Some(("task".into(), ExprIr::Literal { value: t.clone() }))
                        }
                        EffectField::Data(_) => None,
                    })
                    .collect();
                instructions.push(Instruction::EmitEffect { kind, fields });
            }
            FlowStep::Match(m) => {
                let branches = m
                    .branches
                    .iter()
                    .map(|b| MatchBranchIr {
                        condition: lower_condition(&b.condition),
                        instructions: lower_steps(&b.steps, auto_updated_sources),
                    })
                    .collect();
                let default = m
                    .default
                    .as_ref()
                    .map(|d| lower_steps(d, auto_updated_sources));
                instructions.push(Instruction::Match { branches, default });
            }
            FlowStep::Set(s) => {
                instructions.push(Instruction::Compute {
                    binding: s.name.clone(),
                    expr: lower_expr(&s.expr),
                });
            }
            FlowStep::Each(_) => {
                // each handled at runtime
            }
            FlowStep::Try(_) => {
                // try/recover handled at runtime
            }
            FlowStep::Upload(_) => {
                // upload handled at runtime
            }
        }
    }
    instructions
}

pub(crate) fn lower_expr(expr: &Expr) -> ExprIr {
    match expr {
        Expr::Literal(lit) => ExprIr::Literal {
            value: sql_literal(lit, SqlDialect::Postgres),
        },
        Expr::DotPath(path) => ExprIr::Path {
            value: path.as_str(),
        },
        Expr::Binary { op, left, right } => ExprIr::BinaryOp {
            op: binary_op_str(op),
            left: Box::new(lower_expr(left)),
            right: Box::new(lower_expr(right)),
        },
        Expr::Unary { op, operand } => ExprIr::UnaryOp {
            op: unary_op_str(op),
            operand: Box::new(lower_expr(operand)),
        },
        Expr::If {
            cond, then, else_, ..
        } => ExprIr::FuncCall {
            name: "IF".into(),
            args: vec![lower_expr(cond), lower_expr(then), lower_expr(else_)],
        },
        Expr::Aggregate { op, source, field } => {
            let name = match op {
                AggregateOp::Count => "COUNT",
                AggregateOp::Sum => "SUM",
                AggregateOp::Avg => "AVG",
                AggregateOp::Min => "MIN",
                AggregateOp::Max => "MAX",
                AggregateOp::First => "FIRST",
                AggregateOp::Last => "LAST",
            };
            let mut args = vec![lower_expr(source)];
            if let Some(f) = field {
                args.push(ExprIr::Path { value: f.clone() });
            }
            ExprIr::FuncCall {
                name: name.into(),
                args,
            }
        }
        Expr::NowOffset {
            direction,
            amount,
            unit,
        } => {
            let dir = match direction {
                OffsetDirection::Plus => "NOW_PLUS",
                OffsetDirection::Minus => "NOW_MINUS",
            };
            let u = match unit {
                TimeUnit::Seconds => "seconds",
                TimeUnit::Minutes => "minutes",
                TimeUnit::Hours => "hours",
                TimeUnit::Days => "days",
                TimeUnit::Weeks => "weeks",
                TimeUnit::Months => "months",
                TimeUnit::Years => "years",
            };
            ExprIr::FuncCall {
                name: dir.into(),
                args: vec![lower_expr(amount), ExprIr::Literal { value: u.into() }],
            }
        }
        Expr::Coalesce { value, default } => ExprIr::FuncCall {
            name: "COALESCE".into(),
            args: vec![lower_expr(value), lower_expr(default)],
        },
        Expr::Ternary { op, a, b, c } => {
            let name = match op {
                TernaryOp::Between => "BETWEEN",
                TernaryOp::Substring => "SUBSTRING",
            };
            ExprIr::FuncCall {
                name: name.into(),
                args: vec![lower_expr(a), lower_expr(b), lower_expr(c)],
            }
        }
        Expr::Fetch {
            source,
            filters,
            or_code,
            ..
        } => ExprIr::FuncCall {
            name: format!("FETCH_{}", source),
            args: std::iter::once(ExprIr::Literal {
                value: or_code.to_string(),
            })
            .chain(filters.iter().map(|f| lower_expr(&f.value)))
            .collect(),
        },
        Expr::Query { source, .. } => ExprIr::FuncCall {
            name: format!("QUERY_{}", source),
            args: vec![],
        },
        Expr::Call {
            service,
            method,
            args,
            ..
        } => ExprIr::FuncCall {
            name: format!("{}.{}", service, method),
            args: args.iter().map(|(_, v)| lower_expr(v)).collect(),
        },
        Expr::Cached { expr, .. } => lower_expr(expr),
        Expr::WasmCall { hash, inputs } => ExprIr::FuncCall {
            name: format!("wasm:{}", hash),
            args: inputs
                .iter()
                .map(|i| ExprIr::Path { value: i.clone() })
                .collect(),
        },
        Expr::MapExpr { source, .. } => ExprIr::FuncCall {
            name: "MAP".into(),
            args: vec![lower_expr(source)],
        },
        Expr::FilterExpr { source, condition } => ExprIr::FuncCall {
            name: "FILTER".into(),
            args: vec![lower_expr(source), lower_expr(condition)],
        },
        Expr::ReduceExpr { op, source, .. } => ExprIr::FuncCall {
            name: format!("REDUCE_{:?}", op).to_uppercase(),
            args: vec![lower_expr(source)],
        },
        Expr::SplitExpr { value, delimiter } => ExprIr::FuncCall {
            name: "SPLIT".into(),
            args: vec![lower_expr(value), lower_expr(delimiter)],
        },
        Expr::ReplaceExpr { value, from, to } => ExprIr::FuncCall {
            name: "REPLACE".into(),
            args: vec![lower_expr(value), lower_expr(from), lower_expr(to)],
        },
        Expr::FormatExpr { template, args } => ExprIr::FuncCall {
            name: format!("FORMAT:{}", template),
            args: args.iter().map(lower_expr).collect(),
        },
        Expr::Render { template, vars } => ExprIr::FuncCall {
            name: format!("RENDER:{}", template),
            args: vars.iter().map(|(_, v)| lower_expr(v)).collect(),
        },
        Expr::Translate { key, vars } => ExprIr::FuncCall {
            name: format!("T:{}", key),
            args: vars.iter().map(|(_, v)| lower_expr(v)).collect(),
        },
        Expr::FuncCall { name, args } => ExprIr::FuncCall {
            name: name.clone(),
            args: args.iter().map(lower_expr).collect(),
        },
    }
}

fn lower_condition(expr: &Expr) -> ConditionIr {
    match expr {
        Expr::Binary { op, left, right } => ConditionIr::Compare {
            op: binary_op_str(op),
            left: lower_expr(left),
            right: lower_expr(right),
        },
        Expr::Unary { op, operand } => ConditionIr::Unary {
            op: unary_op_str(op),
            operand: Box::new(lower_condition(operand)),
        },
        _ => ConditionIr::Expr(lower_expr(expr)),
    }
}

fn lower_filters(filters: &[FilterClause]) -> Vec<FilterIr> {
    filters
        .iter()
        .map(|f| FilterIr {
            field: f.field.clone(),
            op: filter_op_str(&f.op),
            value: lower_expr(&f.value),
        })
        .collect()
}

fn lower_sorts(sorts: &[SortClause]) -> Vec<SortIr> {
    sorts
        .iter()
        .map(|s| SortIr {
            field: s.field.clone(),
            direction: match s.direction {
                SortDirection::Asc => "ASC".into(),
                SortDirection::Desc => "DESC".into(),
            },
        })
        .collect()
}

fn filter_op_str(op: &FilterOp) -> String {
    match op {
        FilterOp::Eq => "=",
        FilterOp::Neq => "!=",
        FilterOp::Gt => ">",
        FilterOp::Gte => ">=",
        FilterOp::Lt => "<",
        FilterOp::Lte => "<=",
        FilterOp::In => "IN",
        FilterOp::Between => "BETWEEN",
        FilterOp::Like => "LIKE",
        FilterOp::StartsWith => "STARTS_WITH",
        FilterOp::Contains => "CONTAINS",
    }
    .into()
}

fn compare_op_str(op: &CompareOp) -> String {
    match op {
        CompareOp::Eq => "=",
        CompareOp::Neq => "!=",
        CompareOp::Gt => ">",
        CompareOp::Gte => ">=",
        CompareOp::Lt => "<",
        CompareOp::Lte => "<=",
        CompareOp::In => "IN",
    }
    .into()
}

fn binary_op_str(op: &BinaryOp) -> String {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::And => "AND",
        BinaryOp::Or => "OR",
        BinaryOp::Eq => "=",
        BinaryOp::Neq => "!=",
        BinaryOp::Gt => ">",
        BinaryOp::Gte => ">=",
        BinaryOp::Lt => "<",
        BinaryOp::Lte => "<=",
        BinaryOp::DaysBetween => "DAYS_BETWEEN",
        BinaryOp::HoursBetween => "HOURS_BETWEEN",
        BinaryOp::MinutesBetween => "MINUTES_BETWEEN",
        BinaryOp::Concat => "||",
        BinaryOp::StartsWith => "STARTS_WITH",
        BinaryOp::EndsWith => "ENDS_WITH",
        BinaryOp::Contains => "CONTAINS",
        BinaryOp::Round => "ROUND",
        BinaryOp::Coalesce => "COALESCE",
        BinaryOp::FormatDate => "FORMAT_DATE",
    }
    .into()
}

fn unary_op_str(op: &UnaryOp) -> String {
    match op {
        UnaryOp::Not => "NOT",
        UnaryOp::Empty => "EMPTY",
        UnaryOp::Exists => "EXISTS",
        UnaryOp::Lower => "LOWER",
        UnaryOp::Upper => "UPPER",
        UnaryOp::Trim => "TRIM",
        UnaryOp::Length => "LENGTH",
        UnaryOp::ToInt => "TO_INT",
        UnaryOp::ToDecimal => "TO_DECIMAL",
        UnaryOp::ToString => "TO_STRING",
        UnaryOp::Abs => "ABS",
        UnaryOp::Ceil => "CEIL",
        UnaryOp::Floor => "FLOOR",
        UnaryOp::First => "FIRST",
        UnaryOp::Last => "LAST",
        UnaryOp::Count => "COUNT",
    }
    .into()
}

impl std::fmt::Display for RouteInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {} {}", self.method, self.path, self.name)?;
        if let Some(ref auth) = self.auth {
            writeln!(f, "  auth: {}", auth)?;
        }
        for inst in &self.instructions {
            match inst {
                Instruction::CheckRule { name, checks } => {
                    writeln!(f, "  RULE {}", name)?;
                    for c in checks {
                        writeln!(f, "    {} {} {:?}", c.path, c.op, c.value)?;
                    }
                }
                Instruction::Guard {
                    name,
                    error_code,
                    error_message,
                    ..
                } => {
                    write!(f, "  GUARD {} {}", name, error_code)?;
                    if let Some(msg) = error_message {
                        write!(f, " \"{}\"", msg)?;
                    }
                    writeln!(f)?;
                }
                Instruction::FetchOne {
                    binding,
                    source,
                    or_code,
                    ..
                } => {
                    writeln!(f, "  FETCH {} -> {} (or {})", source, binding, or_code)?;
                }
                Instruction::QueryMany {
                    binding, source, ..
                } => {
                    writeln!(f, "  QUERY {} -> {}", source, binding)?;
                }
                Instruction::Compute { binding, expr } => {
                    writeln!(f, "  COMPUTE {} = {:?}", binding, expr)?;
                }
                Instruction::Insert {
                    source, binding, ..
                } => {
                    write!(f, "  INSERT {}", source)?;
                    if let Some(b) = binding {
                        write!(f, " -> {}", b)?;
                    }
                    writeln!(f)?;
                }
                Instruction::Upsert {
                    source, binding, ..
                } => {
                    write!(f, "  UPSERT {}", source)?;
                    if let Some(b) = binding {
                        write!(f, " -> {}", b)?;
                    }
                    writeln!(f)?;
                }
                Instruction::Fanout { source, item, .. } => {
                    writeln!(f, "  FANOUT {} -> {}", item, source)?;
                }
                Instruction::Update {
                    source, binding, ..
                } => {
                    write!(f, "  UPDATE {}", source)?;
                    if let Some(b) = binding {
                        write!(f, " -> {}", b)?;
                    }
                    writeln!(f)?;
                }
                Instruction::Delete { source, .. } => {
                    writeln!(f, "  DELETE {}", source)?;
                }
                Instruction::EmitEffect { kind, .. } => {
                    writeln!(f, "  EFFECT {}", kind)?;
                }
                Instruction::Match { branches, default } => {
                    writeln!(f, "  MATCH")?;
                    for (i, branch) in branches.iter().enumerate() {
                        writeln!(f, "    WHEN {:?}", branch.condition)?;
                        for inst in &branch.instructions {
                            write!(f, "      ")?;
                            match inst {
                                Instruction::FetchOne {
                                    binding, source, ..
                                } => writeln!(f, "FETCH {} -> {}", source, binding)?,
                                Instruction::Compute { binding, .. } => {
                                    writeln!(f, "COMPUTE {}", binding)?
                                }
                                Instruction::Update { source, .. } => {
                                    writeln!(f, "UPDATE {}", source)?
                                }
                                Instruction::Insert { source, .. } => {
                                    writeln!(f, "INSERT {}", source)?
                                }
                                Instruction::Delete { source, .. } => {
                                    writeln!(f, "DELETE {}", source)?
                                }
                                _ => writeln!(f, "{:?}", std::mem::discriminant(inst))?,
                            }
                        }
                        if i < branches.len() - 1 || default.is_some() {
                            // separator handled by indent
                        }
                    }
                    if let Some(def) = default {
                        writeln!(f, "    DEFAULT")?;
                        for inst in def {
                            write!(f, "      ")?;
                            match inst {
                                Instruction::Update { source, .. } => {
                                    writeln!(f, "UPDATE {}", source)?
                                }
                                Instruction::Insert { source, .. } => {
                                    writeln!(f, "INSERT {}", source)?
                                }
                                Instruction::Delete { source, .. } => {
                                    writeln!(f, "DELETE {}", source)?
                                }
                                _ => writeln!(f, "{:?}", std::mem::discriminant(inst))?,
                            }
                        }
                    }
                }
            }
        }
        writeln!(f, "  RETURN {}", self.return_code)?;
        Ok(())
    }
}

fn generate_saga_journal(sql: &mut String, saga: &SagaDef, dialect: SqlDialect) {
    let table = format!("saga_{}_journal", saga.name);
    let uuid_t = sql_type(&TypeExpr::Uuid, dialect);
    let ts_t = sql_type(&TypeExpr::Timestamp, dialect);
    let json_t = sql_type(&TypeExpr::Json, dialect);
    let uuid_default = match dialect {
        SqlDialect::Postgres => " DEFAULT gen_random_uuid()",
        _ => "",
    };
    let now_default = match dialect {
        SqlDialect::Postgres => " DEFAULT now()",
        SqlDialect::Mysql => " DEFAULT NOW()",
        SqlDialect::Sqlite => " DEFAULT (datetime('now'))",
    };

    writeln!(sql, "CREATE TABLE IF NOT EXISTS {} (", table).unwrap();
    writeln!(sql, "  id {} PRIMARY KEY{},", uuid_t, uuid_default).unwrap();
    writeln!(sql, "  saga_id {} NOT NULL,", uuid_t).unwrap();
    writeln!(sql, "  step_index INTEGER NOT NULL,").unwrap();
    writeln!(sql, "  step_name VARCHAR(100) NOT NULL,").unwrap();
    writeln!(sql, "  status VARCHAR(20) NOT NULL DEFAULT 'pending',").unwrap();
    writeln!(sql, "  idempotency_key {} NOT NULL UNIQUE,", uuid_t).unwrap();
    writeln!(sql, "  input {},", json_t).unwrap();
    writeln!(sql, "  output {},", json_t).unwrap();
    writeln!(sql, "  error TEXT,").unwrap();
    writeln!(sql, "  created_at {}{},", ts_t, now_default).unwrap();
    writeln!(sql, "  completed_at {}", ts_t).unwrap();
    writeln!(sql, ");").unwrap();
    writeln!(sql).unwrap();
    writeln!(
        sql,
        "CREATE INDEX IF NOT EXISTS idx_{}_saga_id ON {} (saga_id);",
        table, table
    )
    .unwrap();
    writeln!(sql).unwrap();
}

fn generate_outbox_table(sql: &mut String, dialect: SqlDialect) {
    let uuid_t = sql_type(&TypeExpr::Uuid, dialect);
    let ts_t = sql_type(&TypeExpr::Timestamp, dialect);
    let uuid_default = match dialect {
        SqlDialect::Postgres => " DEFAULT gen_random_uuid()",
        _ => "",
    };
    let now_default = match dialect {
        SqlDialect::Postgres => " DEFAULT now()",
        SqlDialect::Mysql => " DEFAULT NOW()",
        SqlDialect::Sqlite => " DEFAULT (datetime('now'))",
    };

    writeln!(sql).unwrap();
    writeln!(sql, "CREATE TABLE IF NOT EXISTS _axis_outbox (").unwrap();
    writeln!(sql, "  id {} PRIMARY KEY{},", uuid_t, uuid_default).unwrap();
    writeln!(sql, "  kind VARCHAR(50) NOT NULL,").unwrap();
    writeln!(sql, "  template VARCHAR(200),").unwrap();
    writeln!(sql, "  recipient TEXT,").unwrap();
    writeln!(sql, "  url TEXT,").unwrap();
    writeln!(sql, "  event VARCHAR(200),").unwrap();
    writeln!(sql, "  task VARCHAR(200),").unwrap();
    writeln!(sql, "  payload TEXT,").unwrap();
    writeln!(sql, "  status VARCHAR(20) NOT NULL DEFAULT 'pending',").unwrap();
    writeln!(sql, "  created_at {}{},", ts_t, now_default).unwrap();
    if matches!(dialect, SqlDialect::Mysql) {
        writeln!(sql, "  processed_at {},", ts_t).unwrap();
        writeln!(sql, "  INDEX idx_axis_outbox_status (status),").unwrap();
    } else {
        writeln!(sql, "  processed_at {},", ts_t).unwrap();
    }
    writeln!(
        sql,
        "  CHECK (status IN ('pending', 'processing', 'completed', 'failed'))"
    )
    .unwrap();
    writeln!(sql, ");").unwrap();
    writeln!(sql).unwrap();
    if !matches!(dialect, SqlDialect::Mysql) {
        writeln!(
            sql,
            "CREATE INDEX IF NOT EXISTS idx_axis_outbox_status ON _axis_outbox (status);"
        )
        .unwrap();
    }
    writeln!(sql).unwrap();
}

fn generate_idempotency_table(sql: &mut String, dialect: SqlDialect) {
    writeln!(sql).unwrap();
    writeln!(sql, "CREATE TABLE IF NOT EXISTS _axis_idempotency (").unwrap();
    writeln!(sql, "  flow_name VARCHAR(200) NOT NULL,").unwrap();
    writeln!(sql, "  scope_key VARCHAR(512) NOT NULL,").unwrap();
    writeln!(sql, "  idempotency_key VARCHAR(255) NOT NULL,").unwrap();
    writeln!(sql, "  request_hash CHAR(64) NOT NULL,").unwrap();
    writeln!(
        sql,
        "  state VARCHAR(16) NOT NULL CHECK (state IN ('processing', 'completed')),"
    )
    .unwrap();
    writeln!(sql, "  response_status BIGINT,").unwrap();
    writeln!(sql, "  response_body TEXT,").unwrap();
    writeln!(sql, "  response_headers TEXT,").unwrap();
    writeln!(sql, "  expires_at BIGINT NOT NULL,").unwrap();
    writeln!(sql, "  created_at BIGINT NOT NULL,").unwrap();
    writeln!(sql, "  updated_at BIGINT NOT NULL,").unwrap();
    if matches!(dialect, SqlDialect::Mysql) {
        writeln!(sql, "  INDEX idx_axis_idempotency_expires_at (expires_at),").unwrap();
    }
    writeln!(sql, "  PRIMARY KEY (flow_name, scope_key, idempotency_key)").unwrap();
    writeln!(sql, ");").unwrap();
    if !matches!(dialect, SqlDialect::Mysql) {
        writeln!(sql, "CREATE INDEX IF NOT EXISTS idx_axis_idempotency_expires_at ON _axis_idempotency (expires_at);").unwrap();
    }
    writeln!(sql).unwrap();
}

fn generate_saga_plan(
    saga: &SagaDef,
    auto_updated_sources: &std::collections::HashSet<String>,
) -> SagaPlan {
    let method = match saga.method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Webhook => "POST",
    }
    .into();

    let auth = saga.auth.as_ref().map(|a| match a {
        AuthDecl::None => "none".into(),
        AuthDecl::Session => "session".into(),
        AuthDecl::Bearer => "bearer".into(),
        AuthDecl::ApiKey => "api_key".into(),
        AuthDecl::Role(r) => format!("role:{}", r),
        AuthDecl::RoleIn(roles) => format!("role_in:{}", roles.join(",")),
        AuthDecl::WebhookSignature { .. } => "webhook_signature".into(),
    });

    let steps = saga
        .steps
        .iter()
        .map(|step| {
            let instructions = lower_steps(&step.flow_steps, auto_updated_sources);
            let verify = step.verify.as_ref().map(lower_condition);
            let compensate = match &step.compensate {
                Compensate::None => vec![],
                Compensate::Steps(s) => lower_steps(s, auto_updated_sources),
            };
            SagaStepPlan {
                name: step.name.clone(),
                instructions,
                verify,
                yields: step.yields.clone(),
                compensate,
            }
        })
        .collect();

    let on_success_effects = saga
        .on_success
        .effects
        .iter()
        .map(|effect| {
            let kind = match effect.kind {
                EffectKind::Email => "email",
                EffectKind::PushNotification => "push_notification",
                EffectKind::Async => "async",
                EffectKind::Webhook => "webhook",
            }
            .into();
            let fields = effect
                .fields
                .iter()
                .filter_map(|f| match f {
                    EffectField::Template(t) => {
                        Some(("template".into(), ExprIr::Literal { value: t.clone() }))
                    }
                    EffectField::To(e) => Some(("to".into(), lower_expr(e))),
                    EffectField::Url(e) => Some(("url".into(), lower_expr(e))),
                    EffectField::Event(e) => {
                        Some(("event".into(), ExprIr::Literal { value: e.clone() }))
                    }
                    EffectField::Task(t) => {
                        Some(("task".into(), ExprIr::Literal { value: t.clone() }))
                    }
                    EffectField::Data(_) => None,
                })
                .collect();
            Instruction::EmitEffect { kind, fields }
        })
        .collect();

    let return_binding = saga
        .on_success
        .return_stmt
        .body
        .as_ref()
        .and_then(|b| match b {
            ReturnBody::Binding(name) => Some(name.clone()),
            _ => None,
        });

    SagaPlan {
        name: saga.name.clone(),
        method,
        path: saga.path.clone(),
        auth,
        steps,
        on_success_effects,
        return_code: saga.on_success.return_stmt.code,
        return_binding,
    }
}

impl std::fmt::Display for SagaPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SAGA {} {} {}", self.method, self.path, self.name)?;
        if let Some(ref auth) = self.auth {
            writeln!(f, "  auth: {}", auth)?;
        }
        for (i, step) in self.steps.iter().enumerate() {
            writeln!(f, "  STEP {} [{}]", step.name, i)?;
            for inst in &step.instructions {
                writeln!(f, "    {:?}", std::mem::discriminant(inst))?;
            }
            if let Some(ref v) = step.verify {
                writeln!(f, "    VERIFY {:?}", v)?;
            }
            if !step.yields.is_empty() {
                writeln!(f, "    YIELD {}", step.yields.join(" "))?;
            }
            if !step.compensate.is_empty() {
                writeln!(f, "    COMPENSATE ({} steps)", step.compensate.len())?;
            } else {
                writeln!(f, "    COMPENSATE NONE")?;
            }
        }
        for eff in &self.on_success_effects {
            if let Instruction::EmitEffect { kind, .. } = eff {
                writeln!(f, "  ON_SUCCESS EFFECT {}", kind)?;
            }
        }
        writeln!(f, "  RETURN {}", self.return_code)?;
        Ok(())
    }
}

fn generate_migration(m: &MigrateDef, sources: &[&SourceDef]) -> String {
    let source = sources.iter().find(|s| s.shape == m.shape);
    let table = source.map(|s| s.name.as_str()).unwrap_or(&m.shape);
    let dialect = source
        .map(|s| source_dialect(s))
        .unwrap_or(SqlDialect::Postgres);

    let mut ddl = String::new();
    writeln!(
        ddl,
        "-- MIGRATE {} {} -> {}",
        m.shape, m.from_version, m.to_version
    )
    .unwrap();
    writeln!(ddl, "BEGIN;").unwrap();

    for op in &m.ops {
        match op {
            MigrateOp::Copy(_) => {}
            MigrateOp::Drop(field) => {
                writeln!(ddl, "ALTER TABLE {} DROP COLUMN {};", table, field).unwrap();
            }
            MigrateOp::Add(field) => {
                let ty = sql_type(&field.ty, dialect).replace("{col}", &field.name);
                let nullable = !field
                    .modifiers
                    .iter()
                    .any(|m| matches!(m, Modifier::Required));
                let default = field.modifiers.iter().find_map(|m| {
                    if let Modifier::Default(v) = m {
                        Some(sql_literal(v, dialect))
                    } else {
                        None
                    }
                });
                write!(
                    ddl,
                    "ALTER TABLE {} ADD COLUMN {} {}",
                    table, field.name, ty
                )
                .unwrap();
                if !nullable {
                    write!(ddl, " NOT NULL").unwrap();
                }
                if let Some(d) = default {
                    write!(ddl, " DEFAULT {}", d).unwrap();
                }
                writeln!(ddl, ";").unwrap();
            }
            MigrateOp::Rename { from, to } => {
                writeln!(
                    ddl,
                    "ALTER TABLE {} RENAME COLUMN {} TO {};",
                    table, from, to
                )
                .unwrap();
            }
            MigrateOp::Compute { field, .. } => {
                writeln!(
                    ddl,
                    "-- COMPUTE {} requires backfill (handled by runtime)",
                    field
                )
                .unwrap();
            }
        }
    }

    writeln!(ddl, "COMMIT;").unwrap();
    ddl
}

fn generate_surface(
    surface: &SurfaceDef,
    _shapes: &std::collections::HashMap<String, &ShapeDef>,
) -> SurfaceSpec {
    let base = surface.base_path.as_deref().unwrap_or("");

    let routes: Vec<SurfaceRoute> = surface
        .routes
        .iter()
        .map(|r| {
            let method = match r.method {
                HttpMethod::Get => "GET",
                HttpMethod::Post => "POST",
                HttpMethod::Put => "PUT",
                HttpMethod::Patch => "PATCH",
                HttpMethod::Delete => "DELETE",
                HttpMethod::Webhook => "POST",
            };
            SurfaceRoute {
                method: method.into(),
                path: format!("{}{}", base, r.path),
                operation_id: r.target.clone(),
            }
        })
        .collect();

    let schemas: Vec<SchemaSpec> = surface
        .exposes
        .iter()
        .map(|expose| {
            let mut fields = Vec::new();
            let mut hidden = Vec::new();
            let mut renames = Vec::new();

            for ef in &expose.fields {
                match ef {
                    ExposeField::Field { name, ty } => {
                        let (oapi_type, oapi_format) = openapi_type(ty);
                        let nullable = matches!(ty, TypeExpr::Maybe(_));
                        fields.push(SchemaField {
                            name: name.clone(),
                            ty: oapi_type,
                            format: oapi_format,
                            nullable,
                        });
                    }
                    ExposeField::Hide(name) => {
                        hidden.push(name.clone());
                    }
                    ExposeField::Rename { from, to } => {
                        renames.push((from.clone(), to.clone()));
                    }
                }
            }

            SchemaSpec {
                name: expose.alias.as_ref().unwrap_or(&expose.shape).clone(),
                source_shape: expose.shape.clone(),
                fields,
                hidden,
                renames,
            }
        })
        .collect();

    let deprecation = surface.deprecate.as_ref().map(|d| DeprecationSpec {
        replaces: d.version.clone(),
        sunset: d.sunset.clone(),
    });

    SurfaceSpec {
        name: surface.name.clone(),
        version: surface.version.clone(),
        base_path: base.into(),
        routes,
        schemas,
        deprecation,
    }
}

fn openapi_type(ty: &TypeExpr) -> (String, Option<String>) {
    match ty {
        TypeExpr::Uuid => ("string".into(), Some("uuid".into())),
        TypeExpr::Bool => ("boolean".into(), None),
        TypeExpr::Date => ("string".into(), Some("date".into())),
        TypeExpr::Timestamp => ("string".into(), Some("date-time".into())),
        TypeExpr::Text | TypeExpr::String(_) => ("string".into(), None),
        TypeExpr::Int { .. } => ("integer".into(), None),
        TypeExpr::Decimal { .. } => ("number".into(), None),
        TypeExpr::Enum(_) => ("string".into(), None),
        TypeExpr::Json => ("object".into(), None),
        TypeExpr::List(inner) => {
            let (inner_type, _) = openapi_type(inner);
            (format!("array<{}>", inner_type), None)
        }
        TypeExpr::Map(_, _) => ("object".into(), None),
        TypeExpr::Ref { .. } => ("string".into(), Some("uuid".into())),
        TypeExpr::Blob => ("string".into(), Some("binary".into())),
        TypeExpr::Maybe(inner) => openapi_type(inner),
    }
}

impl std::fmt::Display for SurfaceSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{{")?;
        writeln!(f, "  \"openapi\": \"3.1.0\",")?;
        writeln!(f, "  \"info\": {{")?;
        writeln!(f, "    \"title\": \"{}\",", self.name)?;
        writeln!(f, "    \"version\": \"{}\"", self.version)?;
        writeln!(f, "  }},")?;

        // paths
        writeln!(f, "  \"paths\": {{")?;
        for (i, route) in self.routes.iter().enumerate() {
            let oapi_path = route.path.replace(":", "{").replace("{id", "{id}");
            let oapi_path = convert_path_params(&oapi_path);
            writeln!(f, "    \"{}\": {{", oapi_path)?;
            writeln!(f, "      \"{}\": {{", route.method.to_lowercase())?;
            writeln!(f, "        \"operationId\": \"{}\"", route.operation_id)?;
            writeln!(f, "      }}")?;
            write!(f, "    }}")?;
            if i < self.routes.len() - 1 {
                writeln!(f, ",")?;
            } else {
                writeln!(f)?;
            }
        }
        writeln!(f, "  }},")?;

        // schemas
        writeln!(f, "  \"components\": {{")?;
        writeln!(f, "    \"schemas\": {{")?;
        for (i, schema) in self.schemas.iter().enumerate() {
            writeln!(f, "      \"{}\": {{", schema.name)?;
            writeln!(f, "        \"type\": \"object\",")?;
            writeln!(f, "        \"properties\": {{")?;
            let visible: Vec<&SchemaField> = schema
                .fields
                .iter()
                .filter(|sf| !schema.hidden.contains(&sf.name))
                .collect();
            for (j, field) in visible.iter().enumerate() {
                let display_name = schema
                    .renames
                    .iter()
                    .find(|(from, _)| from == &field.name)
                    .map(|(_, to)| to.as_str())
                    .unwrap_or(&field.name);
                write!(
                    f,
                    "          \"{}\": {{ \"type\": \"{}\"",
                    display_name, field.ty
                )?;
                if let Some(ref fmt_str) = field.format {
                    write!(f, ", \"format\": \"{}\"", fmt_str)?;
                }
                if field.nullable {
                    write!(f, ", \"nullable\": true")?;
                }
                write!(f, " }}")?;
                if j < visible.len() - 1 {
                    writeln!(f, ",")?;
                } else {
                    writeln!(f)?;
                }
            }
            writeln!(f, "        }}")?;
            write!(f, "      }}")?;
            if i < self.schemas.len() - 1 {
                writeln!(f, ",")?;
            } else {
                writeln!(f)?;
            }
        }
        writeln!(f, "    }}")?;
        writeln!(f, "  }}")?;

        // deprecation as extension
        if let Some(ref dep) = self.deprecation {
            // reopen to add x-deprecation
            writeln!(f, "  ,\"x-deprecation\": {{")?;
            writeln!(f, "    \"replaces\": \"{}\",", dep.replaces)?;
            writeln!(f, "    \"sunset\": \"{}\"", dep.sunset)?;
            writeln!(f, "  }}")?;
        }

        writeln!(f, "}}")?;
        Ok(())
    }
}

fn generate_stream(stream: &StreamDef) -> StreamSpec {
    let transport = match stream.transport {
        StreamTransport::WebSocket => "websocket",
        StreamTransport::Sse => "sse",
    }
    .into();

    let auth = stream.auth.as_ref().map(|a| match a {
        AuthDecl::None => "none".into(),
        AuthDecl::Session => "session".into(),
        AuthDecl::Bearer => "bearer".into(),
        AuthDecl::ApiKey => "api_key".into(),
        AuthDecl::Role(r) => format!("role:{}", r),
        AuthDecl::RoleIn(roles) => format!("role_in:{}", roles.join(",")),
        AuthDecl::WebhookSignature { .. } => "webhook_signature".into(),
    });

    let events = stream
        .events
        .iter()
        .map(|evt| {
            let fields = evt
                .fields
                .iter()
                .map(|f| (f.name.clone(), format_type_short(&f.ty)))
                .collect();
            StreamEventSpec {
                name: evt.name.clone(),
                fields,
            }
        })
        .collect();

    StreamSpec {
        name: stream.name.clone(),
        transport,
        path: stream.path.clone(),
        auth,
        events,
    }
}

fn format_type_short(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Uuid => "uuid".into(),
        TypeExpr::Bool => "boolean".into(),
        TypeExpr::Date => "date".into(),
        TypeExpr::Timestamp => "timestamp".into(),
        TypeExpr::Text | TypeExpr::String(_) => "string".into(),
        TypeExpr::Int { .. } => "integer".into(),
        TypeExpr::Decimal { .. } => "number".into(),
        TypeExpr::Enum(_) => "string".into(),
        TypeExpr::Json => "object".into(),
        TypeExpr::List(_) => "array".into(),
        TypeExpr::Map(_, _) => "object".into(),
        TypeExpr::Ref { .. } => "uuid".into(),
        TypeExpr::Blob => "binary".into(),
        TypeExpr::Maybe(inner) => format!("{}?", format_type_short(inner)),
    }
}

fn convert_path_params(path: &str) -> String {
    let mut result = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ':' {
            result.push('{');
            while let Some(&nc) = chars.peek() {
                if nc == '/' || nc == '{' {
                    break;
                }
                result.push(nc);
                chars.next();
            }
            result.push('}');
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn codegen(input: &str) -> CodegenResult {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().unwrap();
        generate(&program)
    }

    #[test]
    fn test_sql_simple_table() {
        let result = codegen(
            r#"SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX email UNIQUE
  INDEX id
"#,
        );
        assert!(result.sql.contains("CREATE TABLE IF NOT EXISTS users"));
        assert!(
            result
                .sql
                .contains("id UUID PRIMARY KEY DEFAULT gen_random_uuid()")
        );
        assert!(result.sql.contains("email VARCHAR(255) NOT NULL UNIQUE"));
        assert!(result.sql.contains("name VARCHAR(100) NOT NULL"));
        assert!(
            result
                .sql
                .contains("CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email")
        );
        assert!(
            result
                .sql
                .contains("CREATE INDEX IF NOT EXISTS idx_users_id")
        );
    }

    #[test]
    fn test_sql_with_constraints() {
        let result = codegen(
            r#"SHAPE Booking
  id UUID PK AUTO
  guest_count INT MIN 1 MAX 16 DEFAULT 1
  price DECIMAL PRECISION 10 SCALE 2 REQUIRED
  status ENUM pending confirmed cancelled REQUIRED
  created_at TIMESTAMP AUTO

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX status
"#,
        );
        assert!(result.sql.contains("guest_count INTEGER"));
        assert!(result.sql.contains("CHECK (guest_count >= 1)"));
        assert!(result.sql.contains("CHECK (guest_count <= 16)"));
        assert!(result.sql.contains("DEFAULT 1"));
        assert!(result.sql.contains("price NUMERIC(10,2) NOT NULL"));
        assert!(result.sql.contains("DEFAULT now()"));
    }

    #[test]
    fn test_route_generation() {
        let result = codegen(
            r#"SHAPE User
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
"#,
        );
        assert_eq!(result.routes.len(), 1);
        let route = &result.routes[0];
        assert_eq!(route.name, "get_user");
        assert_eq!(route.method, "GET");
        assert_eq!(route.path, "/users/:id");
        assert_eq!(route.auth.as_deref(), Some("session"));
        assert_eq!(route.return_code, 200);
        assert_eq!(route.return_binding.as_deref(), Some("user"));
        assert_eq!(route.instructions.len(), 1);
        assert!(
            matches!(&route.instructions[0], Instruction::FetchOne { source, .. } if source == "users")
        );
    }

    #[test]
    fn test_route_with_insert() {
        let result = codegen(
            r#"SHAPE User
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
    name body.name
  AS user
  RETURN 201 user
"#,
        );
        let route = &result.routes[0];
        assert_eq!(route.method, "POST");
        assert!(
            matches!(&route.instructions[0], Instruction::Insert { source, binding, .. }
            if source == "users" && binding.as_deref() == Some("user"))
        );
    }

    #[test]
    fn test_booking_full() {
        let booking_input = std::fs::read_to_string("examples/booking.axis").unwrap();
        let result = codegen(&booking_input);
        assert!(result.sql.contains("CREATE TABLE IF NOT EXISTS users"));
        assert!(result.sql.contains("CREATE TABLE IF NOT EXISTS listings"));
        assert!(result.sql.contains("CREATE TABLE IF NOT EXISTS bookings"));
        assert_eq!(result.routes.len(), 3);
        assert_eq!(result.routes[0].name, "get_booking");
        assert_eq!(result.routes[1].name, "list_bookings");
        assert_eq!(result.routes[2].name, "create_booking");
    }

    #[test]
    fn test_migrate_ddl() {
        let result = codegen(
            r#"SHAPE Order
  id UUID PK AUTO
  status STRING 20 REQUIRED

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

MIGRATE Order v1 TO v2
  COPY id status
  ADD tracking_number MAYBE STRING 100
  ADD shipped_at MAYBE TIMESTAMP
  DROP old_field
  RENAME status TO order_status
"#,
        );
        assert_eq!(result.migrations.len(), 1);
        let ddl = &result.migrations[0];
        assert!(ddl.contains("ALTER TABLE orders ADD COLUMN tracking_number"));
        assert!(ddl.contains("ALTER TABLE orders ADD COLUMN shipped_at"));
        assert!(ddl.contains("ALTER TABLE orders DROP COLUMN old_field"));
        assert!(ddl.contains("ALTER TABLE orders RENAME COLUMN status TO order_status"));
        assert!(ddl.contains("BEGIN;"));
        assert!(ddl.contains("COMMIT;"));
    }

    #[test]
    fn test_surface_basic() {
        let result = codegen(
            r#"SHAPE Booking
  id UUID PK AUTO
  status STRING 20 REQUIRED
  check_in DATE REQUIRED
  check_out DATE REQUIRED
  total_price DECIMAL PRECISION 10 SCALE 2 REQUIRED
  user_id UUID REQUIRED

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX id

FLOW get_booking get /bookings/:id
  AUTH session
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  RETURN 200 booking

FLOW list_bookings get /bookings
  AUTH session
  LET results
    QUERY bookings
      FILTER user_id EQ auth.user_id
  RETURN 200 results

SURFACE public v1
  BASE_PATH /api/v1
  ROUTE GET /bookings -> list_bookings
  ROUTE GET /bookings/:id -> get_booking
  EXPOSE Booking AS BookingResponse
    FIELD id UUID
    FIELD status STRING
    FIELD check_in DATE
    FIELD check_out DATE
    HIDE total_price
    HIDE user_id
"#,
        );
        assert_eq!(result.surfaces.len(), 1);
        let s = &result.surfaces[0];
        assert_eq!(s.name, "public");
        assert_eq!(s.version, "v1");
        assert_eq!(s.base_path, "/api/v1");
        assert_eq!(s.routes.len(), 2);
        assert_eq!(s.routes[0].method, "GET");
        assert_eq!(s.routes[0].path, "/api/v1/bookings");
        assert_eq!(s.routes[0].operation_id, "list_bookings");
        assert_eq!(s.routes[1].path, "/api/v1/bookings/:id");
        assert_eq!(s.schemas.len(), 1);
        assert_eq!(s.schemas[0].name, "BookingResponse");
        assert_eq!(s.schemas[0].source_shape, "Booking");
        assert_eq!(s.schemas[0].fields.len(), 4);
        assert_eq!(s.schemas[0].hidden, vec!["total_price", "user_id"]);
        assert!(s.deprecation.is_none());
    }

    #[test]
    fn test_surface_openapi_output() {
        let result = codegen(
            r#"SHAPE User
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

SURFACE api v2
  BASE_PATH /v2
  ROUTE GET /users/:id -> get_user
  EXPOSE User AS UserResponse
    FIELD id UUID
    FIELD name STRING
"#,
        );
        let s = &result.surfaces[0];
        let output = format!("{s}");
        assert!(output.contains("\"openapi\": \"3.1.0\""));
        assert!(output.contains("\"title\": \"api\""));
        assert!(output.contains("\"version\": \"v2\""));
        assert!(output.contains("/v2/users/{id}"));
        assert!(output.contains("\"operationId\": \"get_user\""));
        assert!(output.contains("\"UserResponse\""));
        assert!(output.contains("\"id\": { \"type\": \"string\", \"format\": \"uuid\" }"));
        assert!(output.contains("\"name\": { \"type\": \"string\" }"));
    }

    #[test]
    fn test_surface_with_deprecation() {
        let result = codegen(
            r#"SHAPE Item
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE items POSTGRES
  SHAPE Item
  INDEX id

FLOW list_items get /items
  AUTH session
  LET results
    QUERY items
      FILTER id EQ auth.user_id
  RETURN 200 results

SURFACE shop v2
  BASE_PATH /api/v2
  ROUTE GET /items -> list_items
  EXPOSE Item AS ItemResponse
    FIELD id UUID
    FIELD name STRING
  DEPRECATE v1 SUNSET "2027-06-01"
"#,
        );
        let s = &result.surfaces[0];
        assert!(s.deprecation.is_some());
        let dep = s.deprecation.as_ref().unwrap();
        assert_eq!(dep.replaces, "v1");
        assert_eq!(dep.sunset, "2027-06-01");
        let output = format!("{s}");
        assert!(output.contains("\"x-deprecation\""));
        assert!(output.contains("\"replaces\": \"v1\""));
        assert!(output.contains("\"sunset\": \"2027-06-01\""));
    }

    #[test]
    fn test_surface_rename_fields() {
        let result = codegen(
            r#"SHAPE Booking
  id UUID PK AUTO
  note STRING 500

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX id

FLOW get_booking get /bookings/:id
  AUTH session
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  RETURN 200 booking

SURFACE public v2
  BASE_PATH /api/v2
  ROUTE GET /bookings/:id -> get_booking
  EXPOSE Booking AS BookingResponse
    FIELD id UUID
    FIELD note STRING
    RENAME note AS special_requests
"#,
        );
        let s = &result.surfaces[0];
        assert_eq!(
            s.schemas[0].renames,
            vec![("note".to_string(), "special_requests".to_string())]
        );
        let output = format!("{s}");
        assert!(output.contains("\"special_requests\""));
        assert!(!output.contains("\"note\""));
    }

    #[test]
    fn test_saga_codegen() {
        let result = codegen(
            r#"SHAPE Item
  id UUID PK AUTO
  quantity INT REQUIRED

SOURCE inventory POSTGRES
  SHAPE Item
  INDEX id

SHAPE Order
  id UUID PK AUTO
  user_id UUID REQUIRED
  item_id UUID REQUIRED REF Item.id
  quantity INT REQUIRED
  status ENUM pending confirmed cancelled REQUIRED

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id
  INDEX user_id

REALM main
  TENANT user_id
  CAPABILITY read write

SAGA process_order POST /orders
  AUTH session

  STEP verify_stock
    LET item
      FETCH inventory
        FILTER id EQ body.item_id
      OR 404
    VERIFY
      GT item.quantity 0
    YIELD item
    COMPENSATE NONE

  STEP create_order
    INSERT orders
      user_id auth.user_id
      item_id body.item_id
      quantity body.quantity
      status pending
    AS order
    YIELD order
    COMPENSATE
      DELETE orders
        WHERE id EQ order.id

  ON_FAILURE RUN_COMPENSATIONS
  ON_SUCCESS
    EFFECT email
      TEMPLATE order_confirmed
      TO auth.user_id
      DATA order
    RETURN 201 order
"#,
        );
        assert_eq!(result.sagas.len(), 1);
        let saga = &result.sagas[0];
        assert_eq!(saga.name, "process_order");
        assert_eq!(saga.method, "POST");
        assert_eq!(saga.path, "/orders");
        assert_eq!(saga.auth, Some("session".into()));
        assert_eq!(saga.steps.len(), 2);
        assert_eq!(saga.steps[0].name, "verify_stock");
        assert!(saga.steps[0].verify.is_some());
        assert_eq!(saga.steps[0].yields, vec!["item"]);
        assert!(saga.steps[0].compensate.is_empty());
        assert_eq!(saga.steps[1].name, "create_order");
        assert_eq!(saga.steps[1].yields, vec!["order"]);
        assert!(!saga.steps[1].compensate.is_empty());
        assert_eq!(saga.on_success_effects.len(), 1);
        assert_eq!(saga.return_code, 201);
        assert_eq!(saga.return_binding, Some("order".into()));

        let output = format!("{saga}");
        assert!(output.contains("SAGA POST /orders process_order"));
        assert!(output.contains("STEP verify_stock"));
        assert!(output.contains("STEP create_order"));
        assert!(output.contains("YIELD item"));
        assert!(output.contains("YIELD order"));
        assert!(output.contains("COMPENSATE NONE"));
        assert!(output.contains("COMPENSATE (1 steps)"));
        assert!(output.contains("ON_SUCCESS EFFECT email"));
        assert!(output.contains("RETURN 201"));
    }

    #[test]
    fn test_match_codegen() {
        let result = codegen(
            r#"SHAPE User
  id UUID PK AUTO
  role ENUM admin user REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM main
  TENANT user_id
  CAPABILITY read users
  CAPABILITY write users
  CAPABILITY admin users

FLOW get_user GET /users/:id
  REALM main
  AUTH session
  PARAM id UUID
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  MATCH
    WHEN EQ user.role "admin"
      LET extra
        FETCH users
          FILTER id EQ path.id
        OR 404
    DEFAULT
      LET extra
        FETCH users
          FILTER id EQ path.id
        OR 404
  RETURN 200 user
"#,
        );
        assert_eq!(result.routes.len(), 1);
        let route = &result.routes[0];
        let has_match = route
            .instructions
            .iter()
            .any(|i| matches!(i, Instruction::Match { .. }));
        assert!(has_match, "route should contain a MATCH instruction");
        if let Some(Instruction::Match { branches, default }) = route
            .instructions
            .iter()
            .find(|i| matches!(i, Instruction::Match { .. }))
        {
            assert_eq!(branches.len(), 1);
            assert!(!branches[0].instructions.is_empty());
            assert!(default.is_some());
        }
    }

    #[test]
    fn test_stream_codegen() {
        let result = codegen(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

STREAM notifications ws /ws/notifications
  AUTH session
  EVENT user_online
    user_id UUID
    timestamp TIMESTAMP
  EVENT message
    from UUID
    body STRING 500
"#,
        );
        assert_eq!(result.streams.len(), 1);
        let s = &result.streams[0];
        assert_eq!(s.name, "notifications");
        assert_eq!(s.transport, "websocket");
        assert_eq!(s.path, "/ws/notifications");
        assert_eq!(s.auth, Some("session".into()));
        assert_eq!(s.events.len(), 2);
        assert_eq!(s.events[0].name, "user_online");
        assert_eq!(s.events[0].fields.len(), 2);
        assert_eq!(s.events[1].name, "message");
    }

    #[test]
    fn test_messenger_primitives_lower_to_atomic_instructions() {
        let result = codegen(include_str!("../../examples/messenger-primitives.axis"));
        let route = result
            .routes
            .iter()
            .find(|route| route.name == "deliver_message")
            .unwrap();
        assert!(
            route
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::Upsert { .. }))
        );
        assert!(
            route
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::Fanout { .. }))
        );
        assert!(
            result
                .sql
                .contains("CREATE TABLE IF NOT EXISTS _axis_idempotency")
        );
        assert!(
            result
                .sql
                .contains("PRIMARY KEY (flow_name, scope_key, idempotency_key)")
        );
    }
}
