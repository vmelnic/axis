use std::fmt::Write;

use crate::ast::*;

pub fn format_program(program: &Program) -> String {
    let mut out = String::new();
    let mut first = true;

    for construct in &program.constructs {
        if !first {
            out.push('\n');
        }
        first = false;

        match construct {
            Construct::Shape(s) => format_shape(&mut out, s),
            Construct::Source(s) => format_source(&mut out, s),
            Construct::Realm(r) => format_realm(&mut out, r),
            Construct::Policy(p) => format_policy(&mut out, p),
            Construct::Service(s) => format_service(&mut out, s),
            Construct::Flow(f) => format_flow(&mut out, f),
            Construct::Saga(s) => format_saga(&mut out, s),
            Construct::Surface(s) => format_surface(&mut out, s),
            Construct::Migrate(m) => format_migrate(&mut out, m),
            Construct::Stream(s) => format_stream(&mut out, s),
            Construct::Func(f) => format_func(&mut out, f),
            Construct::Storage(s) => format_storage(&mut out, s),
        }
    }

    out
}

fn format_shape(out: &mut String, shape: &ShapeDef) {
    writeln!(out, "SHAPE {}", shape.name).unwrap();
    for field in &shape.fields {
        write!(out, "  {} {}", field.name, format_type(&field.ty)).unwrap();
        for m in &field.modifiers {
            write!(out, " {}", format_modifier(m)).unwrap();
        }
        writeln!(out).unwrap();
    }
}

fn format_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Uuid => "UUID".into(),
        TypeExpr::Bool => "BOOL".into(),
        TypeExpr::Date => "DATE".into(),
        TypeExpr::Timestamp => "TIMESTAMP".into(),
        TypeExpr::Text => "TEXT".into(),
        TypeExpr::String(None) => "STRING".into(),
        TypeExpr::String(Some(n)) => format!("STRING {}", n),
        TypeExpr::Int { min, max } => {
            let mut s = "INT".to_string();
            if let Some(v) = min {
                write!(s, " MIN {}", v).unwrap();
            }
            if let Some(v) = max {
                write!(s, " MAX {}", v).unwrap();
            }
            s
        }
        TypeExpr::Decimal {
            precision: Some(p),
            scale: Some(s),
        } => {
            format!("DECIMAL PRECISION {} SCALE {}", p, s)
        }
        TypeExpr::Decimal { .. } => "DECIMAL".into(),
        TypeExpr::Enum(variants) => format!("ENUM {}", variants.join(" ")),
        TypeExpr::Ref { shape, field } => format!("UUID REF {}.{}", shape, field),
        TypeExpr::List(inner) => format!("LIST {}", format_type(inner)),
        TypeExpr::Map(k, v) => format!("MAP {} {}", format_type(k), format_type(v)),
        TypeExpr::Json => "JSON".into(),
        TypeExpr::Maybe(inner) => format!("MAYBE {}", format_type(inner)),
        TypeExpr::Blob => "BLOB".into(),
    }
}

fn format_modifier(m: &Modifier) -> String {
    match m {
        Modifier::Pk => "PK".into(),
        Modifier::Auto => "AUTO".into(),
        Modifier::Required => "REQUIRED".into(),
        Modifier::Unique => "UNIQUE".into(),
        Modifier::Default(val) => format!("DEFAULT {}", format_literal(val)),
        Modifier::Precision(n) => format!("PRECISION {}", n),
        Modifier::Scale(n) => format!("SCALE {}", n),
        Modifier::Min(n) => format!("MIN {}", n),
        Modifier::Max(n) => format!("MAX {}", n),
        Modifier::Ref { shape, field } => format!("REF {}.{}", shape, field),
    }
}

fn format_literal(val: &LiteralValue) -> String {
    match val {
        LiteralValue::Int(n) => n.to_string(),
        LiteralValue::Decimal(d) => d.clone(),
        LiteralValue::String(s) => format!("\"{}\"", s),
        LiteralValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        LiteralValue::Ident(s) => s.clone(),
        LiteralValue::Now => "NOW".into(),
        LiteralValue::None => "NONE".into(),
    }
}

fn format_source(out: &mut String, source: &SourceDef) {
    let stype = match source.source_type {
        SourceType::Postgres => "POSTGRES",
        SourceType::Mysql => "MYSQL",
        SourceType::Sqlite => "SQLITE",
        SourceType::Redis => "REDIS",
        SourceType::Elasticsearch => "ELASTICSEARCH",
        SourceType::Dynamodb => "DYNAMODB",
    };
    writeln!(out, "SOURCE {} {}", source.name, stype).unwrap();
    writeln!(out, "  SHAPE {}", source.shape).unwrap();
    for index in &source.indexes {
        let fields: Vec<String> = index
            .fields
            .iter()
            .map(|f| match &f.suffix {
                None => f.name.clone(),
                Some(IndexSuffix::Asc) => format!("{} ASC", f.name),
                Some(IndexSuffix::Desc) => format!("{} DESC", f.name),
                Some(IndexSuffix::Unique) => format!("{} UNIQUE", f.name),
                Some(IndexSuffix::Geo) => format!("{} GEO", f.name),
                Some(IndexSuffix::Text) => format!("{} TEXT", f.name),
                Some(IndexSuffix::Keyword) => format!("{} KEYWORD", f.name),
            })
            .collect();
        writeln!(out, "  INDEX {}", fields.join(" ")).unwrap();
    }
    if let Some(ttl) = source.ttl {
        writeln!(out, "  TTL {}", ttl).unwrap();
    }
}

fn format_realm(out: &mut String, realm: &RealmDef) {
    writeln!(out, "REALM {}", realm.name).unwrap();
    if let Some(ref tenant) = realm.tenant {
        writeln!(out, "  TENANT {}", tenant).unwrap();
    }
    for cap in &realm.capabilities {
        let kind = match cap.kind {
            CapabilityKind::Read => "read",
            CapabilityKind::Write => "write",
            CapabilityKind::Call => "call",
            CapabilityKind::Effect => "effect",
            CapabilityKind::Admin => "admin",
        };
        writeln!(out, "  CAPABILITY {} {}", kind, cap.target).unwrap();
    }
}

fn format_policy(out: &mut String, policy: &PolicyDef) {
    writeln!(out, "POLICY {}", policy.name).unwrap();
    if policy.applies_to.filters.is_empty() {
        writeln!(out, "  APPLIES_TO FLOW").unwrap();
    } else {
        let mode = match policy.applies_to.mode {
            PolicyMatchMode::All => "ALL",
            PolicyMatchMode::Any => "ANY",
        };
        writeln!(out, "  APPLIES_TO FLOW {}", mode).unwrap();
        for filter in &policy.applies_to.filters {
            writeln!(out, "    {}", format_policy_filter(filter)).unwrap();
        }
    }
    for req in &policy.requires {
        match req {
            RequireClause::Auth(None) => writeln!(out, "  REQUIRE AUTH").unwrap(),
            RequireClause::Auth(Some(kind)) => writeln!(out, "  REQUIRE AUTH {}", kind).unwrap(),
            RequireClause::Limit => writeln!(out, "  REQUIRE LIMIT").unwrap(),
            RequireClause::Scope => writeln!(out, "  REQUIRE SCOPE").unwrap(),
            RequireClause::Rule(name) => writeln!(out, "  REQUIRE RULE {}", name).unwrap(),
            RequireClause::Guard(name) => writeln!(out, "  REQUIRE GUARD {}", name).unwrap(),
            RequireClause::Idempotency => writeln!(out, "  REQUIRE IDEMPOTENCY").unwrap(),
            RequireClause::Fanout => writeln!(out, "  REQUIRE FANOUT").unwrap(),
        }
    }
}

fn format_policy_filter(filter: &PolicyFilter) -> String {
    match filter {
        PolicyFilter::MethodIn(methods) => format!("METHOD IN {}", methods.join(" ")),
        PolicyFilter::Reads(source) => format!("READS {source}"),
        PolicyFilter::Writes(source) => format!("WRITES {source}"),
        PolicyFilter::PathStartsWith(path) => format!("PATH STARTS_WITH \"{path}\""),
        PolicyFilter::Not(inner) => format!("NOT {}", format_policy_filter(inner)),
    }
}

fn format_service(out: &mut String, service: &ServiceDef) {
    writeln!(out, "SERVICE {}", service.name).unwrap();
    writeln!(out, "  ENDPOINT {}", service.endpoint).unwrap();
    writeln!(
        out,
        "  AUTH {} VAULT {}",
        service.auth_type, service.vault_key
    )
    .unwrap();
    for method in &service.methods {
        writeln!(out, "  METHOD {}", method.name).unwrap();
        if method.pure {
            writeln!(out, "    PURE").unwrap();
        }
        if let Some(input) = &method.idempotency_input {
            writeln!(out, "    IDEMPOTENCY {input}").unwrap();
        }
        if !method.inputs.is_empty() {
            let pairs: Vec<String> = method
                .inputs
                .iter()
                .map(|(name, ty)| format!("{} {}", name, format_type(ty)))
                .collect();
            writeln!(out, "    INPUT {}", pairs.join(" ")).unwrap();
        }
        if !method.outputs.is_empty() {
            let pairs: Vec<String> = method
                .outputs
                .iter()
                .map(|(name, ty)| format!("{} {}", name, format_type(ty)))
                .collect();
            writeln!(out, "    OUTPUT {}", pairs.join(" ")).unwrap();
        }
        if let Some(timeout) = &method.timeout {
            writeln!(out, "    TIMEOUT {}", timeout).unwrap();
        }
        if let Some(retry) = &method.retry {
            let strat = match retry.strategy {
                RetryStrategy::Exponential => "exponential",
                RetryStrategy::Linear => "linear",
                RetryStrategy::None => "none",
            };
            writeln!(out, "    RETRY {} BACKOFF {}", retry.count, strat).unwrap();
        }
        if let Some(ttl) = method.cache_ttl {
            writeln!(out, "    CACHE {}", ttl).unwrap();
        }
    }
}

fn format_flow(out: &mut String, flow: &FlowDef) {
    let method = match flow.method {
        HttpMethod::Get => "get",
        HttpMethod::Post => "post",
        HttpMethod::Put => "put",
        HttpMethod::Patch => "patch",
        HttpMethod::Delete => "delete",
        HttpMethod::Webhook => "webhook",
    };
    writeln!(out, "FLOW {} {} {}", flow.name, method, flow.path).unwrap();

    if let Some(ref realm) = flow.realm {
        writeln!(out, "  REALM {}", realm).unwrap();
    }

    if let Some(ref auth) = flow.auth {
        match auth {
            AuthDecl::None => writeln!(out, "  AUTH none").unwrap(),
            AuthDecl::Session => writeln!(out, "  AUTH session").unwrap(),
            AuthDecl::Bearer => writeln!(out, "  AUTH bearer").unwrap(),
            AuthDecl::ApiKey => writeln!(out, "  AUTH api_key").unwrap(),
            AuthDecl::Role(r) => writeln!(out, "  AUTH role {}", r).unwrap(),
            AuthDecl::RoleIn(roles) => writeln!(out, "  AUTH role_in {}", roles.join(" ")).unwrap(),
            AuthDecl::WebhookSignature { secret, algorithm } => {
                writeln!(out, "  AUTH webhook_signature {} {}", secret, algorithm).unwrap();
            }
        }
    }

    if let Some(ref scope) = flow.scope {
        match scope {
            ScopeDecl::Tenant(path) => writeln!(out, "  SCOPE TENANT {}", path.as_str()).unwrap(),
            ScopeDecl::TenantAny => writeln!(out, "  SCOPE TENANT_ANY").unwrap(),
        }
    }

    for limit in &flow.limits {
        let unit = match limit.unit {
            RateUnit::PerSecond => "per_second",
            RateUnit::PerMinute => "per_minute",
            RateUnit::PerHour => "per_hour",
            RateUnit::PerDay => "per_day",
        };
        let scope = match limit.scope {
            RateScope::PerUser => "per_user",
            RateScope::PerIp => "per_ip",
            RateScope::PerKey => "per_key",
            RateScope::Global => "global",
        };
        writeln!(out, "  LIMIT {} {} {}", limit.count, unit, scope).unwrap();
    }

    for cache in &flow.cache {
        write!(out, "  CACHE {}", cache.ttl).unwrap();
        if !cache.vary.is_empty() {
            write!(out, " VARY").unwrap();
            for v in &cache.vary {
                write!(out, " {}", v.as_str()).unwrap();
            }
        }
        writeln!(out).unwrap();
    }

    if let Some(ref body) = flow.body {
        if body.kind == BodyKind::Multipart {
            writeln!(out, "  BODY MULTIPART {}", body.name).unwrap();
        } else {
            writeln!(out, "  BODY {}", body.name).unwrap();
        }
        for field in &body.fields {
            write!(out, "    {} {}", field.name, format_type(&field.ty)).unwrap();
            for m in &field.modifiers {
                write!(out, " {}", format_modifier(m)).unwrap();
            }
            writeln!(out).unwrap();
        }
    }

    for param in &flow.params {
        write!(out, "  PARAM {} {}", param.name, format_type(&param.ty)).unwrap();
        for m in &param.modifiers {
            write!(out, " {}", format_modifier(m)).unwrap();
        }
        writeln!(out).unwrap();
    }

    for header in &flow.headers {
        write!(out, "  HEADER {} {}", header.name, format_type(&header.ty)).unwrap();
        for m in &header.modifiers {
            write!(out, " {}", format_modifier(m)).unwrap();
        }
        writeln!(out).unwrap();
    }

    if let Some(idempotency) = &flow.idempotency {
        writeln!(
            out,
            "  IDEMPOTENCY {} SCOPE {} TTL {}",
            idempotency.key.as_str(),
            idempotency.scope.as_str(),
            idempotency.ttl
        )
        .unwrap();
    }

    for step in &flow.steps {
        format_step(out, step, 2);
    }

    format_return(out, &flow.return_stmt);
}

fn format_step(out: &mut String, step: &FlowStep, indent: usize) {
    let pad = " ".repeat(indent);
    match step {
        FlowStep::Rule(rule) => {
            writeln!(out, "{}RULE {}", pad, rule.name).unwrap();
            for req in &rule.requires {
                writeln!(
                    out,
                    "{}  REQUIRE {} {} {}",
                    pad,
                    req.path.as_str(),
                    format_compare_op(&req.op),
                    format_expr(&req.value)
                )
                .unwrap();
            }
        }
        FlowStep::Guard(guard) => {
            write!(out, "{}GUARD {} {}", pad, guard.name, guard.code).unwrap();
            if let Some(msg) = &guard.message {
                write!(out, " \"{}\"", msg).unwrap();
            }
            writeln!(out).unwrap();
            format_guard_expr(out, &guard.expr, indent + 2);
        }
        FlowStep::Let(let_step) => {
            writeln!(out, "{}LET {}", pad, let_step.name).unwrap();
            format_block_expr(out, &let_step.expr, indent + 2);
        }
        FlowStep::Insert(insert) => {
            writeln!(out, "{}INSERT {}", pad, insert.source).unwrap();
            for (field, value) in &insert.fields {
                writeln!(out, "{}  {} {}", pad, field, format_expr(value)).unwrap();
            }
            if let Some(ref binding) = insert.binding {
                writeln!(out, "{}AS {}", pad, binding).unwrap();
            }
        }
        FlowStep::Upsert(upsert) => {
            writeln!(out, "{}UPSERT {}", pad, upsert.source).unwrap();
            for (field, value) in &upsert.keys {
                writeln!(out, "{}  KEY {} {}", pad, field, format_expr(value)).unwrap();
            }
            for set in &upsert.sets {
                writeln!(
                    out,
                    "{}  SET {} {}",
                    pad,
                    set.field,
                    format_expr(&set.value)
                )
                .unwrap();
            }
            if let Some(binding) = &upsert.binding {
                writeln!(out, "{}AS {}", pad, binding).unwrap();
            }
        }
        FlowStep::Update(update) => {
            writeln!(out, "{}UPDATE {}", pad, update.source).unwrap();
            for w in &update.wheres {
                writeln!(
                    out,
                    "{}  WHERE {} {} {}",
                    pad,
                    w.field,
                    format_compare_op(&w.op),
                    format_expr(&w.value)
                )
                .unwrap();
            }
            for s in &update.sets {
                writeln!(out, "{}  SET {} {}", pad, s.field, format_expr(&s.value)).unwrap();
            }
            if update.or_code != 0 || update.or_message.is_some() {
                write!(out, "{}OR {}", pad, update.or_code).unwrap();
                if let Some(msg) = &update.or_message {
                    write!(out, " \"{}\"", msg).unwrap();
                }
                writeln!(out).unwrap();
            }
        }
        FlowStep::Delete(delete) => {
            writeln!(out, "{}DELETE {}", pad, delete.source).unwrap();
            for w in &delete.wheres {
                writeln!(
                    out,
                    "{}  WHERE {} {} {}",
                    pad,
                    w.field,
                    format_compare_op(&w.op),
                    format_expr(&w.value)
                )
                .unwrap();
            }
            if delete.or_code != 0 || delete.or_message.is_some() {
                write!(out, "{}OR {}", pad, delete.or_code).unwrap();
                if let Some(msg) = &delete.or_message {
                    write!(out, " \"{}\"", msg).unwrap();
                }
                writeln!(out).unwrap();
            }
        }
        FlowStep::Effect(effect) => {
            let kind = match effect.kind {
                EffectKind::Email => "email",
                EffectKind::PushNotification => "push_notification",
                EffectKind::Async => "ASYNC",
                EffectKind::Webhook => "webhook",
            };
            writeln!(out, "{}EFFECT {}", pad, kind).unwrap();
            for field in &effect.fields {
                match field {
                    EffectField::Template(t) => writeln!(out, "{}  TEMPLATE {}", pad, t).unwrap(),
                    EffectField::To(e) => writeln!(out, "{}  TO {}", pad, format_expr(e)).unwrap(),
                    EffectField::Data(exprs) => {
                        let parts: Vec<String> = exprs.iter().map(format_expr).collect();
                        writeln!(out, "{}  DATA {}", pad, parts.join(" ")).unwrap();
                    }
                    EffectField::Url(e) => {
                        writeln!(out, "{}  URL {}", pad, format_expr(e)).unwrap()
                    }
                    EffectField::Event(e) => writeln!(out, "{}  EVENT {}", pad, e).unwrap(),
                    EffectField::Task(t) => writeln!(out, "{}  TASK {}", pad, t).unwrap(),
                }
            }
        }
        FlowStep::Match(match_step) => {
            writeln!(out, "{}MATCH", pad).unwrap();
            for branch in &match_step.branches {
                writeln!(out, "{}  WHEN {}", pad, format_expr(&branch.condition)).unwrap();
                for s in &branch.steps {
                    format_step(out, s, indent + 4);
                }
            }
            if let Some(ref default) = match_step.default {
                writeln!(out, "{}  DEFAULT", pad).unwrap();
                for s in default {
                    format_step(out, s, indent + 4);
                }
            }
        }
        FlowStep::Set(set) => {
            writeln!(out, "{}SET {} {}", pad, set.name, format_expr(&set.expr)).unwrap();
        }
        FlowStep::Each(each) => {
            write!(
                out,
                "{}EACH {} IN {}",
                pad,
                each.binding,
                format_expr(&each.source)
            )
            .unwrap();
            if let Some(n) = each.parallel {
                write!(out, " PARALLEL {}", n).unwrap();
            }
            writeln!(out).unwrap();
            for s in &each.steps {
                format_step(out, s, indent + 2);
            }
        }
        FlowStep::Fanout(fanout) => {
            writeln!(
                out,
                "{}FANOUT {} IN {}",
                pad,
                fanout.binding,
                format_expr(&fanout.source)
            )
            .unwrap();
            format_step(out, &FlowStep::Insert(fanout.insert.clone()), indent + 2);
        }
        FlowStep::Try(try_step) => {
            writeln!(out, "{}TRY", pad).unwrap();
            for s in &try_step.body {
                format_step(out, s, indent + 2);
            }
            writeln!(out, "{}RECOVER", pad).unwrap();
            for s in &try_step.recover {
                format_step(out, s, indent + 2);
            }
        }
        FlowStep::Upload(upload) => {
            writeln!(
                out,
                "{}UPLOAD {} -> {} AS {}",
                pad,
                format_expr(&upload.file_expr),
                upload.storage,
                upload.binding
            )
            .unwrap();
        }
    }
}

fn format_guard_expr(out: &mut String, expr: &Expr, indent: usize) {
    let pad = " ".repeat(indent);
    match expr {
        Expr::Unary { op, operand } => {
            if is_block_expr(operand) {
                writeln!(out, "{}{}", pad, format_unary_op(op)).unwrap();
                format_block_expr(out, operand, indent + 2);
            } else {
                writeln!(
                    out,
                    "{}{} {}",
                    pad,
                    format_unary_op(op),
                    format_expr(operand)
                )
                .unwrap();
            }
        }
        Expr::Binary { op, left, right } => {
            writeln!(
                out,
                "{}{} {} {}",
                pad,
                format_binary_op(op),
                format_expr(left),
                format_expr(right)
            )
            .unwrap();
        }
        _ => {
            if is_block_expr(expr) {
                format_block_expr(out, expr, indent);
            } else {
                writeln!(out, "{}{}", pad, format_expr(expr)).unwrap();
            }
        }
    }
}

fn is_block_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Fetch { .. } | Expr::Query { .. } | Expr::Call { .. } | Expr::Cached { .. }
    )
}

fn format_block_expr(out: &mut String, expr: &Expr, indent: usize) {
    let pad = " ".repeat(indent);
    match expr {
        Expr::Fetch {
            source,
            filters,
            with,
            or_code,
            or_message,
            or_shape,
        } => {
            writeln!(out, "{}FETCH {}", pad, source).unwrap();
            for f in filters {
                writeln!(
                    out,
                    "{}  FILTER {} {} {}",
                    pad,
                    f.field,
                    format_filter_op(&f.op),
                    format_expr(&f.value)
                )
                .unwrap();
            }
            for w in with {
                writeln!(out, "{}  WITH {}", pad, w).unwrap();
            }
            if let Some(shape) = or_shape {
                writeln!(out, "{}OR_SHAPE {}", pad, shape.shape).unwrap();
                for (name, expr) in &shape.fields {
                    writeln!(out, "{}  {} {}", pad, name, format_expr(expr)).unwrap();
                }
            } else {
                write!(out, "{}OR {}", pad, or_code).unwrap();
                if let Some(msg) = or_message {
                    write!(out, " \"{}\"", msg).unwrap();
                }
                writeln!(out).unwrap();
            }
        }
        Expr::Query {
            source,
            filters,
            sorts,
            cursor: _,
            page_size,
            cache_ttl: _,
        } => {
            writeln!(out, "{}QUERY {}", pad, source).unwrap();
            for f in filters {
                writeln!(
                    out,
                    "{}  FILTER {} {} {}",
                    pad,
                    f.field,
                    format_filter_op(&f.op),
                    format_expr(&f.value)
                )
                .unwrap();
            }
            for s in sorts {
                let dir = match s.direction {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                };
                writeln!(out, "{}  SORT {} {}", pad, s.field, dir).unwrap();
            }
            if let Some(ps) = page_size {
                writeln!(out, "{}  PAGE_SIZE {}", pad, format_expr(ps)).unwrap();
            }
        }
        Expr::Call {
            service,
            method,
            args,
            or_code,
            or_message,
        } => {
            writeln!(out, "{}CALL {}.{}", pad, service, method).unwrap();
            if !args.is_empty() {
                for (name, value) in args {
                    writeln!(out, "{}  {} {}", pad, name, format_expr(value)).unwrap();
                }
            }
            if *or_code != 0 || or_message.is_some() {
                write!(out, "{}OR {}", pad, or_code).unwrap();
                if let Some(msg) = or_message {
                    write!(out, " \"{}\"", msg).unwrap();
                }
                writeln!(out).unwrap();
            }
        }
        Expr::WasmCall { hash, inputs } => {
            writeln!(out, "{}CALL wasm {} {}", pad, hash, inputs.join(" ")).unwrap();
        }
        Expr::Cached { ttl, expr: inner } => {
            writeln!(out, "{}CACHE {}", pad, ttl).unwrap();
            format_block_expr(out, inner, indent + 2);
        }
        _ => {
            writeln!(out, "{}{}", pad, format_expr(expr)).unwrap();
        }
    }
}

fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::Literal(val) => format_literal(val),
        Expr::DotPath(path) => path.as_str(),
        Expr::Binary { op, left, right } => {
            format!(
                "{} {} {}",
                format_binary_op(op),
                format_expr(left),
                format_expr(right)
            )
        }
        Expr::Unary { op, operand } => {
            format!("{} {}", format_unary_op(op), format_expr(operand))
        }
        Expr::Ternary { op, a, b, c } => {
            let name = match op {
                TernaryOp::Between => "BETWEEN",
                TernaryOp::Substring => "SUBSTRING",
            };
            format!(
                "{} {} {} {}",
                name,
                format_expr(a),
                format_expr(b),
                format_expr(c)
            )
        }
        Expr::If { cond, then, else_ } => {
            format!(
                "IF {} THEN {} ELSE {}",
                format_expr(cond),
                format_expr(then),
                format_expr(else_)
            )
        }
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
            let mut s = format!("{} {}", name, format_expr(source));
            if let Some(f) = field {
                write!(s, " {}", f).unwrap();
            }
            s
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
                TimeUnit::Seconds => "SECONDS",
                TimeUnit::Minutes => "MINUTES",
                TimeUnit::Hours => "HOURS",
                TimeUnit::Days => "DAYS",
                TimeUnit::Weeks => "WEEKS",
                TimeUnit::Months => "MONTHS",
                TimeUnit::Years => "YEARS",
            };
            format!("{} {} {}", dir, format_expr(amount), u)
        }
        Expr::Coalesce { value, default } => {
            format!("COALESCE {} {}", format_expr(value), format_expr(default))
        }
        Expr::Fetch { .. }
        | Expr::Query { .. }
        | Expr::Call { .. }
        | Expr::Cached { .. }
        | Expr::WasmCall { .. } => "<block>".into(),
        Expr::MapExpr { source, fields } => {
            format!("MAP {} SELECT {}", format_expr(source), fields.join(" "))
        }
        Expr::FilterExpr { source, condition } => {
            format!("FILTER {} {}", format_expr(source), format_expr(condition))
        }
        Expr::ReduceExpr { op, source, field } => {
            let name = match op {
                AggregateOp::Count => "COUNT",
                AggregateOp::Sum => "SUM",
                AggregateOp::Avg => "AVG",
                AggregateOp::Min => "MIN",
                AggregateOp::Max => "MAX",
                AggregateOp::First => "FIRST",
                AggregateOp::Last => "LAST",
            };
            format!("REDUCE {} {} {}", format_expr(source), name, field)
        }
        Expr::SplitExpr { value, delimiter } => {
            format!("SPLIT {} {}", format_expr(value), format_expr(delimiter))
        }
        Expr::ReplaceExpr { value, from, to } => {
            format!(
                "REPLACE {} {} {}",
                format_expr(value),
                format_expr(from),
                format_expr(to)
            )
        }
        Expr::FormatExpr { template, args } => {
            let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
            format!("FORMAT \"{}\" {}", template, arg_strs.join(" "))
        }
        Expr::Render { template, vars } => {
            let var_strs: Vec<String> = vars
                .iter()
                .map(|(k, v)| format!("{} {}", k, format_expr(v)))
                .collect();
            format!("RENDER \"{}\" {}", template, var_strs.join(" "))
        }
        Expr::Translate { key, vars } => {
            let var_strs: Vec<String> = vars
                .iter()
                .map(|(k, v)| format!("{} {}", k, format_expr(v)))
                .collect();
            format!("T \"{}\" {}", key, var_strs.join(" "))
        }
        Expr::FuncCall { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
            format!("CALL {} {}", name, arg_strs.join(" "))
        }
    }
}

fn format_return_at(out: &mut String, ret: &ReturnStmt, indent: usize) {
    let pad = " ".repeat(indent);
    match &ret.body {
        None => writeln!(out, "{}RETURN {}", pad, ret.code).unwrap(),
        Some(ReturnBody::Binding(name)) => {
            writeln!(out, "{}RETURN {} {}", pad, ret.code, name).unwrap();
            if !ret.headers.is_empty() {
                let inner = " ".repeat(indent + 2);
                for (hname, hval) in &ret.headers {
                    writeln!(out, "{}HEADER {} {}", inner, hname, format_expr(hval)).unwrap();
                }
            }
        }
        Some(ReturnBody::Inline(fields)) => {
            writeln!(out, "{}RETURN {}", pad, ret.code).unwrap();
            let inner = " ".repeat(indent + 2);
            for field in fields {
                match &field.value {
                    ReturnValue::Expr(e) => {
                        writeln!(out, "{}{} {}", inner, field.name, format_expr(e)).unwrap()
                    }
                    ReturnValue::Nested(sub) => {
                        writeln!(out, "{}{}", inner, field.name).unwrap();
                        let sub_pad = " ".repeat(indent + 4);
                        for sf in sub {
                            match &sf.value {
                                ReturnValue::Expr(e) => {
                                    writeln!(out, "{}{} {}", sub_pad, sf.name, format_expr(e))
                                        .unwrap()
                                }
                                ReturnValue::Nested(_) => {
                                    writeln!(out, "{}{}", sub_pad, sf.name).unwrap()
                                }
                            }
                        }
                    }
                }
            }
        }
        Some(ReturnBody::Paginated {
            items,
            total,
            cursor,
            has_more,
        }) => {
            writeln!(out, "{}RETURN {}", pad, ret.code).unwrap();
            let inner = " ".repeat(indent + 2);
            writeln!(out, "{}ITEMS {}", inner, format_expr(items)).unwrap();
            writeln!(out, "{}TOTAL {}", inner, format_expr(total)).unwrap();
            writeln!(out, "{}CURSOR {}", inner, format_expr(cursor)).unwrap();
            writeln!(out, "{}HAS_MORE {}", inner, format_expr(has_more)).unwrap();
        }
    }
}

fn format_return(out: &mut String, ret: &ReturnStmt) {
    format_return_at(out, ret, 2);
}

fn format_filter_op(op: &FilterOp) -> &'static str {
    match op {
        FilterOp::Eq => "EQ",
        FilterOp::Neq => "NEQ",
        FilterOp::Gt => "GT",
        FilterOp::Gte => "GTE",
        FilterOp::Lt => "LT",
        FilterOp::Lte => "LTE",
        FilterOp::In => "IN",
        FilterOp::Between => "BETWEEN",
        FilterOp::Like => "LIKE",
        FilterOp::StartsWith => "STARTS_WITH",
        FilterOp::Contains => "CONTAINS",
    }
}

fn format_compare_op(op: &CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "EQ",
        CompareOp::Neq => "NEQ",
        CompareOp::Gt => "GT",
        CompareOp::Gte => "GTE",
        CompareOp::Lt => "LT",
        CompareOp::Lte => "LTE",
        CompareOp::In => "IN",
    }
}

fn format_binary_op(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "ADD",
        BinaryOp::Sub => "SUB",
        BinaryOp::Mul => "MUL",
        BinaryOp::Div => "DIV",
        BinaryOp::Mod => "MOD",
        BinaryOp::And => "AND",
        BinaryOp::Or => "OR",
        BinaryOp::Eq => "EQ",
        BinaryOp::Neq => "NEQ",
        BinaryOp::Gt => "GT",
        BinaryOp::Gte => "GTE",
        BinaryOp::Lt => "LT",
        BinaryOp::Lte => "LTE",
        BinaryOp::Concat => "CONCAT",
        BinaryOp::StartsWith => "STARTS_WITH",
        BinaryOp::EndsWith => "ENDS_WITH",
        BinaryOp::Contains => "CONTAINS",
        BinaryOp::DaysBetween => "DAYS_BETWEEN",
        BinaryOp::HoursBetween => "HOURS_BETWEEN",
        BinaryOp::MinutesBetween => "MINUTES_BETWEEN",
        BinaryOp::Round => "ROUND",
        BinaryOp::Coalesce => "COALESCE",
        BinaryOp::FormatDate => "FORMAT_DATE",
    }
}

fn format_unary_op(op: &UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "NOT",
        UnaryOp::Empty => "EMPTY",
        UnaryOp::Exists => "EXISTS",
        UnaryOp::Lower => "LOWER",
        UnaryOp::Upper => "UPPER",
        UnaryOp::Trim => "TRIM",
        UnaryOp::Abs => "ABS",
        UnaryOp::Ceil => "CEIL",
        UnaryOp::Floor => "FLOOR",
        UnaryOp::Length => "LENGTH",
        UnaryOp::First => "FIRST",
        UnaryOp::Last => "LAST",
        UnaryOp::ToInt => "TO_INT",
        UnaryOp::ToDecimal => "TO_DECIMAL",
        UnaryOp::ToString => "TO_STRING",
        UnaryOp::Count => "COUNT",
    }
}

fn format_http_method(method: &HttpMethod) -> &'static str {
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Webhook => "WEBHOOK",
    }
}

fn format_saga(out: &mut String, saga: &SagaDef) {
    writeln!(
        out,
        "SAGA {} {} {}",
        saga.name,
        format_http_method(&saga.method),
        saga.path
    )
    .unwrap();

    if let Some(ref realm) = saga.realm {
        writeln!(out, "  REALM {}", realm).unwrap();
    }
    if let Some(ref auth) = saga.auth {
        match auth {
            AuthDecl::None => writeln!(out, "  AUTH none").unwrap(),
            AuthDecl::Session => writeln!(out, "  AUTH session").unwrap(),
            AuthDecl::Bearer => writeln!(out, "  AUTH bearer").unwrap(),
            AuthDecl::ApiKey => writeln!(out, "  AUTH api_key").unwrap(),
            AuthDecl::Role(r) => writeln!(out, "  AUTH role {}", r).unwrap(),
            AuthDecl::RoleIn(roles) => writeln!(out, "  AUTH role_in {}", roles.join(" ")).unwrap(),
            AuthDecl::WebhookSignature { secret, algorithm } => {
                writeln!(out, "  AUTH webhook_signature {} {}", secret, algorithm).unwrap();
            }
        }
    }
    if let Some(ref body) = saga.body {
        if body.kind == BodyKind::Multipart {
            writeln!(out, "  BODY MULTIPART {}", body.name).unwrap();
        } else {
            writeln!(out, "  BODY {}", body.name).unwrap();
        }
        for field in &body.fields {
            let mods: Vec<String> = field.modifiers.iter().map(format_modifier).collect();
            let mod_str = if mods.is_empty() {
                String::new()
            } else {
                format!(" {}", mods.join(" "))
            };
            writeln!(
                out,
                "    {} {}{}",
                field.name,
                format_type(&field.ty),
                mod_str
            )
            .unwrap();
        }
    }

    for step in &saga.steps {
        writeln!(out, "  STEP {}", step.name).unwrap();
        for fs in &step.flow_steps {
            format_step(out, fs, 4);
        }
        if let Some(ref verify) = step.verify {
            writeln!(out, "    VERIFY").unwrap();
            format_block_expr(out, verify, 6);
        }
        if !step.yields.is_empty() {
            writeln!(out, "    YIELD {}", step.yields.join(" ")).unwrap();
        }
        match &step.compensate {
            Compensate::None => writeln!(out, "    COMPENSATE NONE").unwrap(),
            Compensate::Steps(steps) => {
                writeln!(out, "    COMPENSATE").unwrap();
                for s in steps {
                    format_step(out, s, 6);
                }
            }
        }
    }

    writeln!(out, "  ON_FAILURE RUN_COMPENSATIONS").unwrap();
    writeln!(out, "  ON_SUCCESS").unwrap();
    for effect in &saga.on_success.effects {
        format_step(out, &FlowStep::Effect(effect.clone()), 4);
    }
    format_return_at(out, &saga.on_success.return_stmt, 4);
}

fn format_surface(out: &mut String, surface: &SurfaceDef) {
    writeln!(out, "SURFACE {} {}", surface.name, surface.version).unwrap();
    if let Some(ref realm) = surface.realm {
        writeln!(out, "  REALM {}", realm).unwrap();
    }
    if let Some(ref base_path) = surface.base_path {
        writeln!(out, "  BASE_PATH {}", base_path).unwrap();
    }
    for route in &surface.routes {
        writeln!(
            out,
            "  ROUTE {} {} -> {}",
            format_http_method(&route.method),
            route.path,
            route.target
        )
        .unwrap();
    }
    for expose in &surface.exposes {
        if let Some(ref alias) = expose.alias {
            writeln!(out, "  EXPOSE {} AS {}", expose.shape, alias).unwrap();
        } else {
            writeln!(out, "  EXPOSE {}", expose.shape).unwrap();
        }
        for field in &expose.fields {
            match field {
                ExposeField::Field { name, ty } => {
                    writeln!(out, "    FIELD {} {}", name, format_type(ty)).unwrap()
                }
                ExposeField::Hide(name) => writeln!(out, "    HIDE {}", name).unwrap(),
                ExposeField::Rename { from, to } => {
                    writeln!(out, "    RENAME {} AS {}", from, to).unwrap()
                }
            }
        }
    }
    if let Some(ref dep) = surface.deprecate {
        writeln!(out, "  DEPRECATE {} SUNSET \"{}\"", dep.version, dep.sunset).unwrap();
    }
}

fn format_migrate(out: &mut String, migrate: &MigrateDef) {
    writeln!(
        out,
        "MIGRATE {} {} TO {}",
        migrate.shape, migrate.from_version, migrate.to_version
    )
    .unwrap();
    for op in &migrate.ops {
        match op {
            MigrateOp::Copy(fields) => writeln!(out, "  COPY {}", fields.join(" ")).unwrap(),
            MigrateOp::Drop(field) => writeln!(out, "  DROP {}", field).unwrap(),
            MigrateOp::Add(field) => {
                let mods: Vec<String> = field.modifiers.iter().map(format_modifier).collect();
                let mod_str = if mods.is_empty() {
                    String::new()
                } else {
                    format!(" {}", mods.join(" "))
                };
                writeln!(
                    out,
                    "  ADD {} {}{}",
                    field.name,
                    format_type(&field.ty),
                    mod_str
                )
                .unwrap();
            }
            MigrateOp::Rename { from, to } => writeln!(out, "  RENAME {} TO {}", from, to).unwrap(),
            MigrateOp::Compute { field, expr } => {
                writeln!(out, "  COMPUTE {}", field).unwrap();
                format_block_expr(out, expr, 4);
            }
        }
    }
}

fn format_stream(out: &mut String, stream: &StreamDef) {
    let transport = match stream.transport {
        StreamTransport::WebSocket => "WEBSOCKET",
        StreamTransport::Sse => "SSE",
    };
    writeln!(
        out,
        "STREAM {} {} \"{}\"",
        stream.name, transport, stream.path
    )
    .unwrap();
    if let Some(ref realm) = stream.realm {
        writeln!(out, "  REALM {}", realm).unwrap();
    }
    if let Some(ref auth) = stream.auth {
        match auth {
            AuthDecl::None => writeln!(out, "  AUTH none").unwrap(),
            AuthDecl::Session => writeln!(out, "  AUTH session").unwrap(),
            AuthDecl::Bearer => writeln!(out, "  AUTH bearer").unwrap(),
            AuthDecl::ApiKey => writeln!(out, "  AUTH api_key").unwrap(),
            AuthDecl::Role(r) => writeln!(out, "  AUTH role {}", r).unwrap(),
            AuthDecl::RoleIn(roles) => writeln!(out, "  AUTH role_in {}", roles.join(" ")).unwrap(),
            AuthDecl::WebhookSignature { secret, algorithm } => {
                writeln!(out, "  AUTH webhook_signature {} {}", secret, algorithm).unwrap();
            }
        }
    }
    for event in &stream.events {
        writeln!(out, "  EVENT {}", event.name).unwrap();
        for field in &event.fields {
            let mods: Vec<String> = field.modifiers.iter().map(format_modifier).collect();
            let mod_str = if mods.is_empty() {
                String::new()
            } else {
                format!(" {}", mods.join(" "))
            };
            writeln!(
                out,
                "    {} {}{}",
                field.name,
                format_type(&field.ty),
                mod_str
            )
            .unwrap();
        }
    }
    if !stream.receivers.is_empty() {
        writeln!(out, "  RECEIVE").unwrap();
        for recv in &stream.receivers {
            writeln!(out, "    ON {}", recv.event).unwrap();
            for s in &recv.steps {
                format_step(out, s, 6);
            }
        }
    }
}

fn format_func(out: &mut String, func: &FuncDef) {
    writeln!(out, "FUNC {}", func.name).unwrap();
    for param in &func.inputs {
        writeln!(out, "  INPUT {} {}", param.name, format_type(&param.ty)).unwrap();
    }
    writeln!(out, "  OUTPUT {}", format_type(&func.output)).unwrap();
    for step in &func.steps {
        format_step(out, step, 2);
    }
    writeln!(out, "  RETURN {}", format_expr(&func.return_expr)).unwrap();
}

fn format_storage(out: &mut String, storage: &StorageDef) {
    writeln!(out, "STORAGE {}", storage.name).unwrap();
    let backend = match storage.backend {
        StorageBackend::Local => "local",
        StorageBackend::S3 => "s3",
    };
    writeln!(out, "  BACKEND {}", backend).unwrap();
    writeln!(out, "  BUCKET {}", storage.bucket).unwrap();
    if let Some(ref prefix) = storage.prefix {
        writeln!(out, "  PREFIX {}", prefix).unwrap();
    }
    let access = match storage.access {
        StorageAccess::Public => "public",
        StorageAccess::Private => "private",
    };
    writeln!(out, "  ACCESS {}", access).unwrap();
    if let Some(max_size) = storage.max_size {
        writeln!(out, "  MAX_SIZE {}", max_size).unwrap();
    }
    if !storage.types.is_empty() {
        writeln!(out, "  TYPES {}", storage.types.join(" ")).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn roundtrip(input: &str) -> String {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().unwrap();
        format_program(&program)
    }

    fn parse_ok(input: &str) -> bool {
        let mut lexer = Lexer::new(input);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(_) => return false,
        };
        let mut parser = Parser::new(tokens);
        parser.parse_program().is_ok()
    }

    #[test]
    fn test_shape_roundtrip() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE
  name STRING 100 REQUIRED
  status ENUM active suspended DEFAULT active
"#;
        let formatted = roundtrip(input);
        assert!(formatted.contains("SHAPE User"));
        assert!(formatted.contains("  id UUID PK AUTO"));
        assert!(formatted.contains("  email STRING 255 REQUIRED UNIQUE"));
        assert!(formatted.contains("  status ENUM active suspended DEFAULT active"));
        assert!(parse_ok(&formatted), "formatted output must re-parse");
    }

    #[test]
    fn test_source_roundtrip() {
        let input = r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id
"#;
        let formatted = roundtrip(input);
        assert!(formatted.contains("SOURCE users POSTGRES"));
        assert!(formatted.contains("  SHAPE User"));
        assert!(formatted.contains("  INDEX id"));
        assert!(parse_ok(&formatted));
    }

    #[test]
    fn test_flow_roundtrip() {
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
"#;
        let formatted = roundtrip(input);
        assert!(formatted.contains("FLOW get_user get /users/:id"));
        assert!(formatted.contains("  AUTH session"));
        assert!(formatted.contains("  LET user"));
        assert!(formatted.contains("    FETCH users"));
        assert!(formatted.contains("      FILTER id EQ path.id"));
        assert!(formatted.contains("    OR 404"));
        assert!(formatted.contains("  RETURN 200 user"));
        assert!(parse_ok(&formatted));
    }

    #[test]
    fn test_booking_roundtrip() {
        let input = std::fs::read_to_string("examples/booking.axis").unwrap();
        let formatted = roundtrip(&input);
        assert!(parse_ok(&formatted), "formatted booking.axis must re-parse");
    }

    #[test]
    fn test_messenger_primitives_roundtrip() {
        let input = std::fs::read_to_string("examples/messenger-primitives.axis").unwrap();
        let formatted = roundtrip(&input);
        assert!(
            formatted.contains("IDEMPOTENCY header.idempotency_key SCOPE body.user_id TTL 86400")
        );
        assert!(formatted.contains("  UPSERT receipts"));
        assert!(formatted.contains("  FANOUT recipient IN body.recipients"));
        assert!(formatted.contains("  APPLIES_TO FLOW ALL"));
        assert!(formatted.contains("    NOT PATH STARTS_WITH \"/internal\""));
        assert!(
            parse_ok(&formatted),
            "formatted primitives example must re-parse"
        );
    }
}
