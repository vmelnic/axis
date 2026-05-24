use crate::ast::*;
use crate::error::{AxisError, AxisResult};
use crate::token::{Span, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse_program(&mut self) -> AxisResult<Program> {
        let mut constructs = Vec::new();

        self.skip_newlines();

        while !self.is_at_end() {
            let construct = self.parse_construct()?;
            constructs.push(construct);
            self.skip_newlines();
        }

        Ok(Program { constructs })
    }

    fn parse_construct(&mut self) -> AxisResult<Construct> {
        match self.peek_kind() {
            TokenKind::Shape => self.parse_shape().map(Construct::Shape),
            TokenKind::Source => self.parse_source().map(Construct::Source),
            TokenKind::Realm => self.parse_realm().map(Construct::Realm),
            TokenKind::Policy => self.parse_policy().map(Construct::Policy),
            TokenKind::Service => self.parse_service().map(Construct::Service),
            TokenKind::Flow => self.parse_flow().map(Construct::Flow),
            TokenKind::Saga => self.parse_saga().map(Construct::Saga),
            TokenKind::Surface => self.parse_surface().map(Construct::Surface),
            TokenKind::Migrate => self.parse_migrate().map(Construct::Migrate),
            TokenKind::Stream => self.parse_stream().map(Construct::Stream),
            TokenKind::Func => self.parse_func().map(Construct::Func),
            TokenKind::Storage => self.parse_storage().map(Construct::Storage),
            _ => Err(self.error(format!(
                "expected top-level construct, got {}",
                self.peek_kind()
            ))),
        }
    }

    // --- SHAPE ---

    fn parse_shape(&mut self) -> AxisResult<ShapeDef> {
        let span = self.expect(TokenKind::Shape)?;
        let name = self.expect_shape_name()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut fields = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            fields.push(self.parse_field_def()?);
        }
        self.expect_dedent()?;

        Ok(ShapeDef { name, fields, span })
    }

    fn parse_field_def(&mut self) -> AxisResult<FieldDef> {
        let span = self.current_span();
        let name = self.expect_ident()?;
        let ty = self.parse_type_expr()?;
        let modifiers = self.parse_modifiers()?;
        self.expect_newline()?;
        Ok(FieldDef {
            name,
            ty,
            modifiers,
            span,
        })
    }

    fn parse_type_expr(&mut self) -> AxisResult<TypeExpr> {
        match self.peek_kind() {
            TokenKind::Uuid => {
                self.advance();
                Ok(TypeExpr::Uuid)
            }
            TokenKind::Bool => {
                self.advance();
                Ok(TypeExpr::Bool)
            }
            TokenKind::Date => {
                self.advance();
                Ok(TypeExpr::Date)
            }
            TokenKind::Timestamp => {
                self.advance();
                Ok(TypeExpr::Timestamp)
            }
            TokenKind::Text => {
                self.advance();
                Ok(TypeExpr::Text)
            }
            TokenKind::String_ => {
                self.advance();
                let max_len = if matches!(self.peek_kind(), TokenKind::IntLit(_)) {
                    Some(self.expect_int()?)
                } else {
                    None
                };
                Ok(TypeExpr::String(max_len))
            }
            TokenKind::Int => {
                self.advance();
                let min = if matches!(self.peek_kind(), TokenKind::Min) {
                    self.advance();
                    Some(self.expect_int()?)
                } else {
                    None
                };
                let max = if matches!(self.peek_kind(), TokenKind::Max) {
                    self.advance();
                    Some(self.expect_int()?)
                } else {
                    None
                };
                Ok(TypeExpr::Int { min, max })
            }
            TokenKind::Decimal => {
                self.advance();
                let (precision, scale) = if matches!(self.peek_kind(), TokenKind::Precision) {
                    self.advance();
                    let p = self.expect_int()?;
                    self.expect(TokenKind::Scale)?;
                    let s = self.expect_int()?;
                    (Some(p), Some(s))
                } else {
                    (None, None)
                };
                Ok(TypeExpr::Decimal { precision, scale })
            }
            TokenKind::Enum => {
                self.advance();
                let mut variants = Vec::new();
                while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                    variants.push(self.expect_ident()?);
                }
                if variants.is_empty() {
                    return Err(self.error("ENUM requires at least one variant"));
                }
                Ok(TypeExpr::Enum(variants))
            }
            TokenKind::Ref => {
                self.advance();
                let shape = self.expect_shape_name()?;
                self.expect(TokenKind::Dot)?;
                let field = self.expect_ident()?;
                Ok(TypeExpr::Ref { shape, field })
            }
            TokenKind::List => {
                self.advance();
                let inner = self.parse_type_expr()?;
                Ok(TypeExpr::List(Box::new(inner)))
            }
            TokenKind::Map => {
                self.advance();
                let key = self.parse_type_expr()?;
                let val = self.parse_type_expr()?;
                Ok(TypeExpr::Map(Box::new(key), Box::new(val)))
            }
            TokenKind::Json => {
                self.advance();
                Ok(TypeExpr::Json)
            }
            TokenKind::Maybe => {
                self.advance();
                let inner = self.parse_type_expr()?;
                Ok(TypeExpr::Maybe(Box::new(inner)))
            }
            TokenKind::Blob => {
                self.advance();
                Ok(TypeExpr::Blob)
            }
            _ => Err(self.error(format!("expected type, got {}", self.peek_kind()))),
        }
    }

    fn parse_modifiers(&mut self) -> AxisResult<Vec<Modifier>> {
        let mut mods = Vec::new();
        loop {
            match self.peek_kind() {
                TokenKind::Pk => {
                    self.advance();
                    mods.push(Modifier::Pk);
                }
                TokenKind::Auto => {
                    self.advance();
                    mods.push(Modifier::Auto);
                }
                TokenKind::Required => {
                    self.advance();
                    mods.push(Modifier::Required);
                }
                TokenKind::Unique => {
                    self.advance();
                    mods.push(Modifier::Unique);
                }
                TokenKind::Ref => {
                    self.advance();
                    let shape = self.expect_shape_name()?;
                    self.expect(TokenKind::Dot)?;
                    let field = self.expect_ident()?;
                    mods.push(Modifier::Ref { shape, field });
                }
                TokenKind::Default => {
                    self.advance();
                    let val = self.parse_literal_value()?;
                    mods.push(Modifier::Default(val));
                }
                TokenKind::Min => {
                    self.advance();
                    let n = self.expect_int()?;
                    mods.push(Modifier::Min(n));
                }
                TokenKind::Max => {
                    self.advance();
                    let n = self.expect_int()?;
                    mods.push(Modifier::Max(n));
                }
                _ => break,
            }
        }
        Ok(mods)
    }

    fn parse_literal_value(&mut self) -> AxisResult<LiteralValue> {
        match self.peek_kind() {
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(LiteralValue::Int(n))
            }
            TokenKind::DecimalLit(ref s) => {
                let s = s.clone();
                self.advance();
                Ok(LiteralValue::Decimal(s))
            }
            TokenKind::StringLit(ref s) => {
                let s = s.clone();
                self.advance();
                Ok(LiteralValue::String(s))
            }
            TokenKind::True_ => {
                self.advance();
                Ok(LiteralValue::Bool(true))
            }
            TokenKind::False_ => {
                self.advance();
                Ok(LiteralValue::Bool(false))
            }
            TokenKind::None_ => {
                self.advance();
                Ok(LiteralValue::None)
            }
            TokenKind::Now => {
                self.advance();
                Ok(LiteralValue::Now)
            }
            TokenKind::Ident(ref s) => {
                let s = s.clone();
                self.advance();
                Ok(LiteralValue::Ident(s))
            }
            _ => Err(self.error(format!("expected literal value, got {}", self.peek_kind()))),
        }
    }

    // --- SOURCE ---

    fn parse_source(&mut self) -> AxisResult<SourceDef> {
        let span = self.expect(TokenKind::Source)?;
        let name = self.expect_ident()?;
        let source_type = self.parse_source_type()?;
        self.expect_newline()?;
        self.expect_indent()?;

        self.expect(TokenKind::Shape)?;
        let shape = self.expect_shape_name()?;
        self.expect_newline()?;

        let mut indexes = Vec::new();
        let mut ttl = None;

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Index => {
                    indexes.push(self.parse_index_def()?);
                }
                TokenKind::Ttl => {
                    self.advance();
                    ttl = Some(self.expect_int()?);
                    self.expect_newline()?;
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        Ok(SourceDef {
            name,
            source_type,
            shape,
            indexes,
            ttl,
            span,
        })
    }

    fn parse_source_type(&mut self) -> AxisResult<SourceType> {
        match self.peek_kind() {
            TokenKind::Postgres => {
                self.advance();
                Ok(SourceType::Postgres)
            }
            TokenKind::Mysql => {
                self.advance();
                Ok(SourceType::Mysql)
            }
            TokenKind::Sqlite => {
                self.advance();
                Ok(SourceType::Sqlite)
            }
            TokenKind::Redis => {
                self.advance();
                Ok(SourceType::Redis)
            }
            TokenKind::Elasticsearch => {
                self.advance();
                Ok(SourceType::Elasticsearch)
            }
            TokenKind::Dynamodb => {
                self.advance();
                Ok(SourceType::Dynamodb)
            }
            _ => Err(self.error(format!("expected source type, got {}", self.peek_kind()))),
        }
    }

    fn parse_index_def(&mut self) -> AxisResult<IndexDef> {
        let span = self.expect(TokenKind::Index)?;
        let mut fields = Vec::new();

        while matches!(self.peek_kind(), TokenKind::Ident(_)) {
            let name = self.expect_ident()?;
            let suffix = match self.peek_kind() {
                TokenKind::Asc => {
                    self.advance();
                    Some(IndexSuffix::Asc)
                }
                TokenKind::Desc => {
                    self.advance();
                    Some(IndexSuffix::Desc)
                }
                TokenKind::Unique => {
                    self.advance();
                    Some(IndexSuffix::Unique)
                }
                TokenKind::Ident(ref s) if s == "geo" => {
                    self.advance();
                    Some(IndexSuffix::Geo)
                }
                TokenKind::Text => {
                    self.advance();
                    Some(IndexSuffix::Text)
                }
                TokenKind::Ident(ref s) if s == "keyword" => {
                    self.advance();
                    Some(IndexSuffix::Keyword)
                }
                _ => None,
            };
            fields.push(IndexField { name, suffix });
        }

        self.expect_newline()?;
        Ok(IndexDef { fields, span })
    }

    // --- REALM ---

    fn parse_realm(&mut self) -> AxisResult<RealmDef> {
        let span = self.expect(TokenKind::Realm)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut tenant = None;
        let mut capabilities = Vec::new();

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Tenant => {
                    self.advance();
                    tenant = Some(self.expect_ident()?);
                    self.expect_newline()?;
                }
                TokenKind::Capability => {
                    self.advance();
                    let kind_str = self.expect_ident()?;
                    let kind = match kind_str.as_str() {
                        "read" => CapabilityKind::Read,
                        "write" => CapabilityKind::Write,
                        "call" => CapabilityKind::Call,
                        "effect" => CapabilityKind::Effect,
                        "admin" => CapabilityKind::Admin,
                        _ => return Err(self.error(format!("unknown capability kind: {kind_str}"))),
                    };
                    let target = self.expect_ident()?;
                    capabilities.push(CapabilityDef { kind, target });
                    self.expect_newline()?;
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        Ok(RealmDef {
            name,
            tenant,
            capabilities,
            span,
        })
    }

    // --- FLOW ---

    fn parse_flow(&mut self) -> AxisResult<FlowDef> {
        let span = self.expect(TokenKind::Flow)?;
        let name = self.expect_ident()?;
        let method = self.parse_http_method()?;
        let path = self.expect_path()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut flow = FlowDef {
            name,
            method,
            path,
            realm: None,
            auth: None,
            limits: Vec::new(),
            cache: Vec::new(),
            scope: None,
            timeout: None,
            body: None,
            params: Vec::new(),
            headers: Vec::new(),
            steps: Vec::new(),
            return_stmt: ReturnStmt {
                code: 200,
                body: None,
                headers: Vec::new(),
                span,
            },
            span,
        };

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Realm => {
                    self.advance();
                    flow.realm = Some(self.expect_ident()?);
                    self.expect_newline()?;
                }
                TokenKind::Auth => {
                    flow.auth = Some(self.parse_auth_decl()?);
                }
                TokenKind::Limit => {
                    flow.limits.push(self.parse_limit_decl()?);
                }
                TokenKind::Cache => {
                    flow.cache.push(self.parse_cache_decl()?);
                }
                TokenKind::Scope => {
                    flow.scope = Some(self.parse_scope_decl()?);
                }
                TokenKind::Timeout => {
                    self.advance();
                    flow.timeout = Some(self.parse_duration()?);
                    self.expect_newline()?;
                }
                TokenKind::Body => {
                    flow.body = Some(self.parse_body_decl()?);
                }
                TokenKind::Param => {
                    flow.params.push(self.parse_param_decl()?);
                }
                TokenKind::Header => {
                    flow.headers.push(self.parse_header_decl()?);
                }
                TokenKind::Rule => {
                    flow.steps.push(FlowStep::Rule(self.parse_rule_step()?));
                }
                TokenKind::Guard => {
                    flow.steps.push(FlowStep::Guard(self.parse_guard_step()?));
                }
                TokenKind::Let => {
                    flow.steps.push(FlowStep::Let(self.parse_let_step()?));
                }
                TokenKind::Insert => {
                    flow.steps.push(FlowStep::Insert(self.parse_insert_step()?));
                }
                TokenKind::Update => {
                    flow.steps.push(FlowStep::Update(self.parse_update_step()?));
                }
                TokenKind::Delete => {
                    flow.steps.push(FlowStep::Delete(self.parse_delete_step()?));
                }
                TokenKind::Effect => {
                    flow.steps.push(FlowStep::Effect(self.parse_effect_step()?));
                }
                TokenKind::Match => {
                    flow.steps.push(FlowStep::Match(self.parse_match_step()?));
                }
                TokenKind::Set => {
                    flow.steps.push(FlowStep::Set(self.parse_set_step()?));
                }
                TokenKind::Each => {
                    flow.steps.push(FlowStep::Each(self.parse_each_step()?));
                }
                TokenKind::Try => {
                    flow.steps.push(FlowStep::Try(self.parse_try_step()?));
                }
                TokenKind::Upload => {
                    flow.steps.push(FlowStep::Upload(self.parse_upload_step()?));
                }
                TokenKind::Return => {
                    flow.return_stmt = self.parse_return_stmt()?;
                }
                TokenKind::Newline => {
                    self.advance();
                }
                _ => {
                    return Err(self.error(format!(
                        "unexpected token in FLOW: {}",
                        self.peek_kind()
                    )));
                }
            }
        }
        self.expect_dedent()?;

        Ok(flow)
    }

    fn parse_http_method(&mut self) -> AxisResult<HttpMethod> {
        let ident = self.expect_ident()?;
        match ident.as_str() {
            "get" => Ok(HttpMethod::Get),
            "post" => Ok(HttpMethod::Post),
            "put" => Ok(HttpMethod::Put),
            "patch" => Ok(HttpMethod::Patch),
            "delete" => Ok(HttpMethod::Delete),
            "webhook" => Ok(HttpMethod::Webhook),
            _ => Err(self.error(format!("expected HTTP method, got {ident}"))),
        }
    }

    fn parse_auth_decl(&mut self) -> AxisResult<AuthDecl> {
        self.expect(TokenKind::Auth)?;
        if matches!(self.peek_kind(), TokenKind::None_) {
            self.advance();
            self.expect_newline()?;
            return Ok(AuthDecl::None);
        }
        if matches!(self.peek_kind(), TokenKind::Webhook) {
            self.advance();
            self.expect(TokenKind::Signature)?;
            let secret = self.expect_ident()?;
            self.expect(TokenKind::Hmac)?;
            let algorithm = self.expect_ident()?;
            self.expect_newline()?;
            return Ok(AuthDecl::WebhookSignature { secret, algorithm });
        }
        let kind = self.expect_ident()?;
        let auth = match kind.as_str() {
            "none" => AuthDecl::None,
            "session" => AuthDecl::Session,
            "bearer" => AuthDecl::Bearer,
            "api_key" => AuthDecl::ApiKey,
            "role" => {
                if matches!(self.peek_kind(), TokenKind::In) {
                    self.advance();
                    let mut roles = Vec::new();
                    while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                        roles.push(self.expect_ident()?);
                    }
                    AuthDecl::RoleIn(roles)
                } else {
                    let role = self.expect_ident()?;
                    AuthDecl::Role(role)
                }
            }
            _ => return Err(self.error(format!("unknown auth type: {kind}"))),
        };
        self.expect_newline()?;
        Ok(auth)
    }

    fn parse_limit_decl(&mut self) -> AxisResult<LimitDecl> {
        self.expect(TokenKind::Limit)?;
        let count = self.expect_int()?;
        let unit_str = self.expect_ident()?;
        let unit = match unit_str.as_str() {
            "per_second" => RateUnit::PerSecond,
            "per_minute" => RateUnit::PerMinute,
            "per_hour" => RateUnit::PerHour,
            "per_day" => RateUnit::PerDay,
            _ => return Err(self.error(format!("unknown rate unit: {unit_str}"))),
        };
        let scope_str = self.expect_ident()?;
        let scope = match scope_str.as_str() {
            "per_user" => RateScope::PerUser,
            "per_ip" => RateScope::PerIp,
            "per_key" => RateScope::PerKey,
            "global" => RateScope::Global,
            _ => return Err(self.error(format!("unknown rate scope: {scope_str}"))),
        };
        self.expect_newline()?;
        Ok(LimitDecl { count, unit, scope })
    }

    fn parse_cache_decl(&mut self) -> AxisResult<CacheDecl> {
        self.expect(TokenKind::Cache)?;
        let ttl = self.expect_int()?;
        let mut vary = Vec::new();
        if matches!(self.peek_kind(), TokenKind::Vary) {
            self.advance();
            while !matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) {
                vary.push(self.parse_dot_path()?);
            }
        }
        self.expect_newline()?;
        Ok(CacheDecl { ttl, vary })
    }

    fn parse_scope_decl(&mut self) -> AxisResult<ScopeDecl> {
        self.expect(TokenKind::Scope)?;
        self.expect(TokenKind::Tenant)?;
        if matches!(self.peek_kind(), TokenKind::Any) {
            self.advance();
            self.expect_newline()?;
            Ok(ScopeDecl::TenantAny)
        } else {
            let path = self.parse_dot_path()?;
            self.expect_newline()?;
            Ok(ScopeDecl::Tenant(path))
        }
    }

    fn parse_body_decl(&mut self) -> AxisResult<BodyDecl> {
        self.expect(TokenKind::Body)?;
        let kind = if matches!(self.peek_kind(), TokenKind::Multipart) {
            self.advance();
            BodyKind::Multipart
        } else {
            BodyKind::Json
        };
        let name = self.expect_shape_name()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut fields = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            fields.push(self.parse_field_def()?);
        }
        self.expect_dedent()?;

        Ok(BodyDecl { name, kind, fields })
    }

    fn parse_param_decl(&mut self) -> AxisResult<ParamDecl> {
        self.expect(TokenKind::Param)?;
        let name = self.expect_ident()?;
        let ty = self.parse_type_expr()?;
        let modifiers = self.parse_modifiers()?;
        self.expect_newline()?;
        Ok(ParamDecl {
            name,
            ty,
            modifiers,
        })
    }

    fn parse_header_decl(&mut self) -> AxisResult<HeaderDecl> {
        self.expect(TokenKind::Header)?;
        let name = self.expect_ident()?;
        let ty = self.parse_type_expr()?;
        let modifiers = self.parse_modifiers()?;
        self.expect_newline()?;
        Ok(HeaderDecl {
            name,
            ty,
            modifiers,
        })
    }

    fn parse_rule_step(&mut self) -> AxisResult<RuleStep> {
        let span = self.expect(TokenKind::Rule)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut requires = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            self.expect(TokenKind::Require)?;
            let path = self.parse_dot_path()?;
            let op = self.parse_compare_op()?;
            let value = self.parse_inline_expr()?;
            self.expect_newline()?;
            requires.push(RequireLine { path, op, value });
        }
        self.expect_dedent()?;

        Ok(RuleStep {
            name,
            requires,
            span,
        })
    }

    fn parse_guard_step(&mut self) -> AxisResult<GuardStep> {
        let span = self.expect(TokenKind::Guard)?;
        let name = self.expect_ident()?;
        let code = self.expect_int()?;
        let message = if matches!(self.peek_kind(), TokenKind::StringLit(_)) {
            Some(self.expect_string()?)
        } else {
            None
        };
        self.expect_newline()?;
        self.expect_indent()?;
        let expr = self.parse_block_expr()?;
        self.expect_dedent()?;

        Ok(GuardStep {
            name,
            code,
            message,
            expr,
            span,
        })
    }

    fn parse_let_step(&mut self) -> AxisResult<LetStep> {
        let span = self.expect(TokenKind::Let)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;
        let expr = self.parse_block_expr()?;
        self.expect_dedent()?;

        Ok(LetStep { name, expr, span })
    }

    fn parse_insert_step(&mut self) -> AxisResult<InsertStep> {
        let span = self.expect(TokenKind::Insert)?;
        let source = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut fields = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            let field_name = self.expect_ident()?;
            let value = self.parse_inline_expr()?;
            self.expect_newline()?;
            fields.push((field_name, value));
        }
        self.expect_dedent()?;

        let binding = if matches!(self.peek_kind(), TokenKind::As) {
            self.advance();
            let name = self.expect_ident()?;
            self.expect_newline()?;
            Some(name)
        } else {
            None
        };

        Ok(InsertStep {
            source,
            fields,
            binding,
            span,
        })
    }

    fn parse_update_step(&mut self) -> AxisResult<UpdateStep> {
        let span = self.expect(TokenKind::Update)?;
        let source = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut wheres = Vec::new();
        let mut sets = Vec::new();

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Where => {
                    self.advance();
                    let field = self.expect_ident()?;
                    let op = self.parse_compare_op()?;
                    let value = self.parse_inline_expr()?;
                    self.expect_newline()?;
                    wheres.push(WhereClause { field, op, value });
                }
                TokenKind::Set => {
                    self.advance();
                    let field = self.expect_ident()?;
                    let value = self.parse_inline_expr()?;
                    self.expect_newline()?;
                    sets.push(SetClause { field, value });
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        let binding = if matches!(self.peek_kind(), TokenKind::As) {
            self.advance();
            let name = self.expect_ident()?;
            self.expect_newline()?;
            Some(UpdateBinding::As(name))
        } else if matches!(self.peek_kind(), TokenKind::Count) {
            self.advance();
            let name = self.expect_ident()?;
            self.expect_newline()?;
            Some(UpdateBinding::Count(name))
        } else {
            None
        };

        let mut or_code = 0;
        let mut or_message = None;
        if matches!(self.peek_kind(), TokenKind::Or) {
            self.advance();
            or_code = self.expect_int()?;
            if matches!(self.peek_kind(), TokenKind::StringLit(_)) {
                or_message = Some(self.expect_string()?);
            }
            self.expect_newline()?;
        }

        Ok(UpdateStep {
            source,
            wheres,
            sets,
            binding,
            or_code,
            or_message,
            span,
        })
    }

    fn parse_delete_step(&mut self) -> AxisResult<DeleteStep> {
        let span = self.expect(TokenKind::Delete)?;
        let source = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut wheres = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            self.expect(TokenKind::Where)?;
            let field = self.expect_ident()?;
            let op = self.parse_compare_op()?;
            let value = self.parse_inline_expr()?;
            self.expect_newline()?;
            wheres.push(WhereClause { field, op, value });
        }
        self.expect_dedent()?;

        let mut or_code = 0;
        let mut or_message = None;
        if matches!(self.peek_kind(), TokenKind::Or) {
            self.advance();
            or_code = self.expect_int()?;
            if matches!(self.peek_kind(), TokenKind::StringLit(_)) {
                or_message = Some(self.expect_string()?);
            }
            self.expect_newline()?;
        }

        Ok(DeleteStep {
            source,
            wheres,
            or_code,
            or_message,
            span,
        })
    }

    fn parse_effect_step(&mut self) -> AxisResult<EffectStep> {
        let span = self.expect(TokenKind::Effect)?;
        let kind = if matches!(self.peek_kind(), TokenKind::Async_) {
            self.advance();
            EffectKind::Async
        } else {
            let kind_str = self.expect_ident()?;
            match kind_str.as_str() {
                "email" => EffectKind::Email,
                "push_notification" => EffectKind::PushNotification,
                "webhook" => EffectKind::Webhook,
                _ => return Err(self.error(format!("unknown effect kind: {kind_str}"))),
            }
        };
        self.expect_newline()?;
        self.expect_indent()?;

        let mut fields = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Template => {
                    self.advance();
                    let name = self.expect_ident()?;
                    fields.push(EffectField::Template(name));
                    self.expect_newline()?;
                }
                TokenKind::To => {
                    self.advance();
                    let expr = self.parse_inline_expr()?;
                    fields.push(EffectField::To(expr));
                    self.expect_newline()?;
                }
                TokenKind::Data => {
                    self.advance();
                    let mut data = Vec::new();
                    while !matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) {
                        data.push(self.parse_inline_expr()?);
                    }
                    fields.push(EffectField::Data(data));
                    self.expect_newline()?;
                }
                TokenKind::Ident(ref s) if s == "url" => {
                    self.advance();
                    let expr = self.parse_inline_expr()?;
                    fields.push(EffectField::Url(expr));
                    self.expect_newline()?;
                }
                TokenKind::Ident(ref s) if s == "event" => {
                    self.advance();
                    let name = self.expect_ident()?;
                    fields.push(EffectField::Event(name));
                    self.expect_newline()?;
                }
                TokenKind::Task => {
                    self.advance();
                    let name = self.expect_ident()?;
                    fields.push(EffectField::Task(name));
                    self.expect_newline()?;
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        Ok(EffectStep { kind, fields, span })
    }

    fn parse_match_step(&mut self) -> AxisResult<MatchStep> {
        let span = self.expect(TokenKind::Match)?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut branches = Vec::new();
        let mut default = None;

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::When => {
                    self.advance();
                    let condition = self.parse_block_expr()?;
                    self.expect_indent()?;
                    let mut steps = Vec::new();
                    while !self.check_dedent() && !self.is_at_end() {
                        steps.push(self.parse_flow_step()?);
                    }
                    self.expect_dedent()?;
                    branches.push(WhenBranch { condition, steps });
                }
                TokenKind::Default => {
                    self.advance();
                    self.expect_newline()?;
                    self.expect_indent()?;
                    let mut steps = Vec::new();
                    while !self.check_dedent() && !self.is_at_end() {
                        steps.push(self.parse_flow_step()?);
                    }
                    self.expect_dedent()?;
                    default = Some(steps);
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        Ok(MatchStep { branches, default, span })
    }

    fn parse_return_stmt(&mut self) -> AxisResult<ReturnStmt> {
        let span = self.expect(TokenKind::Return)?;
        let code = self.expect_int()?;

        if matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) {
            if matches!(self.peek_kind(), TokenKind::Newline) {
                self.advance();
            }
            if matches!(self.peek_kind(), TokenKind::Indent) {
                self.advance();
                if matches!(self.peek_kind(), TokenKind::Items) {
                    return self.parse_paginated_return(code, span);
                }
                return self.parse_inline_return(code, span);
            }
            return Ok(ReturnStmt {
                code,
                body: None,
                headers: Vec::new(),
                span,
            });
        }

        if matches!(self.peek_kind(), TokenKind::Ident(_)) {
            let name = self.expect_ident()?;
            self.expect_newline()?;
            let mut headers = Vec::new();
            if matches!(self.peek_kind(), TokenKind::Indent) {
                self.advance();
                while !self.check_dedent() && !self.is_at_end() {
                    if matches!(self.peek_kind(), TokenKind::Header) {
                        self.advance();
                        let header_name = self.expect_name()?;
                        let value = self.parse_block_expr()?;
                        headers.push((header_name, value));
                    } else {
                        break;
                    }
                }
                self.expect_dedent()?;
            }
            return Ok(ReturnStmt {
                code,
                body: Some(ReturnBody::Binding(name)),
                headers,
                span,
            });
        }

        Err(self.error(format!("expected binding name or newline after RETURN {code}")))
    }

    fn parse_paginated_return(&mut self, code: i64, span: Span) -> AxisResult<ReturnStmt> {
        self.expect(TokenKind::Items)?;
        let items = self.parse_inline_expr()?;
        self.expect_newline()?;
        self.expect(TokenKind::Total)?;
        let total = self.parse_inline_expr()?;
        self.expect_newline()?;
        self.expect(TokenKind::Cursor)?;
        let cursor = self.parse_inline_expr()?;
        self.expect_newline()?;
        self.expect(TokenKind::HasMore)?;
        let has_more = self.parse_inline_expr()?;
        self.expect_newline()?;
        self.expect_dedent()?;
        Ok(ReturnStmt {
            code,
            body: Some(ReturnBody::Paginated {
                items: Box::new(items),
                total: Box::new(total),
                cursor: Box::new(cursor),
                has_more: Box::new(has_more),
            }),
            headers: Vec::new(),
            span,
        })
    }

    fn parse_inline_return(&mut self, code: i64, span: Span) -> AxisResult<ReturnStmt> {
        let mut fields = Vec::new();
        let mut headers = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            if matches!(self.peek_kind(), TokenKind::Header) {
                self.advance();
                let header_name = self.expect_name()?;
                let value = self.parse_block_expr()?;
                headers.push((header_name, value));
            } else if matches!(self.peek_kind(), TokenKind::Ident(_)) {
                let name = self.expect_ident()?;
                if matches!(self.peek_kind(), TokenKind::Newline) {
                    self.advance();
                    let nested = if matches!(self.peek_kind(), TokenKind::Indent) {
                        self.advance();
                        let mut sub = Vec::new();
                        while !self.check_dedent() && !self.is_at_end() {
                            let sub_name = self.expect_ident()?;
                            let sub_val = self.parse_inline_expr()?;
                            self.expect_newline()?;
                            sub.push(ReturnField {
                                name: sub_name,
                                value: ReturnValue::Expr(sub_val),
                            });
                        }
                        self.expect_dedent()?;
                        Some(sub)
                    } else {
                        None
                    };
                    if let Some(nested) = nested {
                        fields.push(ReturnField {
                            name,
                            value: ReturnValue::Nested(nested),
                        });
                    } else {
                        let seg = name.clone();
                        fields.push(ReturnField {
                            name,
                            value: ReturnValue::Expr(Expr::DotPath(DotPath { segments: vec![seg] })),
                        });
                    }
                } else {
                    let value = self.parse_inline_expr()?;
                    self.expect_newline()?;
                    fields.push(ReturnField {
                        name,
                        value: ReturnValue::Expr(value),
                    });
                }
            } else {
                break;
            }
        }
        self.expect_dedent()?;
        Ok(ReturnStmt {
            code,
            body: Some(ReturnBody::Inline(fields)),
            headers,
            span,
        })
    }

    // --- Expressions ---

    fn parse_inline_expr(&mut self) -> AxisResult<Expr> {
        match self.peek_kind() {
            TokenKind::IntLit(_) | TokenKind::DecimalLit(_) | TokenKind::StringLit(_)
            | TokenKind::True_ | TokenKind::False_ | TokenKind::None_ => {
                let lit = self.parse_literal_value()?;
                Ok(Expr::Literal(lit))
            }
            TokenKind::Now => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::Now))
            }
            TokenKind::NowPlus | TokenKind::NowMinus => {
                let direction = match self.peek_kind() {
                    TokenKind::NowPlus => OffsetDirection::Plus,
                    TokenKind::NowMinus => OffsetDirection::Minus,
                    _ => unreachable!(),
                };
                self.advance();
                let amount = self.parse_inline_expr()?;
                let unit = self.parse_time_unit()?;
                Ok(Expr::NowOffset {
                    direction,
                    amount: Box::new(amount),
                    unit,
                })
            }
            TokenKind::Ident(_) => {
                let path = self.parse_dot_path()?;
                Ok(Expr::DotPath(path))
            }
            _ => Err(self.error(format!("expected expression, got {}", self.peek_kind()))),
        }
    }

    fn parse_block_expr(&mut self) -> AxisResult<Expr> {
        match self.peek_kind() {
            TokenKind::Cache => {
                self.advance();
                let ttl = self.expect_int()?;
                self.expect_newline()?;
                self.expect_indent()?;
                let inner = self.parse_block_expr()?;
                self.expect_dedent()?;
                Ok(Expr::Cached { ttl, expr: Box::new(inner) })
            }
            TokenKind::Fetch => self.parse_fetch_expr(),
            TokenKind::Query => self.parse_query_expr(),
            TokenKind::Call => self.parse_call_expr(),
            TokenKind::Add | TokenKind::Sub | TokenKind::Mul | TokenKind::Div | TokenKind::Mod => {
                self.parse_binary_arith_expr()
            }
            TokenKind::And | TokenKind::Or => self.parse_binary_bool_expr(),
            TokenKind::Eq | TokenKind::Neq | TokenKind::Gt | TokenKind::Gte
            | TokenKind::Lt | TokenKind::Lte => self.parse_binary_compare_expr(),
            TokenKind::Not | TokenKind::Empty | TokenKind::Exists => self.parse_unary_expr(),
            TokenKind::Count | TokenKind::Sum | TokenKind::Avg | TokenKind::Min | TokenKind::Max
            | TokenKind::First | TokenKind::Last => {
                self.parse_aggregate_expr()
            }
            TokenKind::If => self.parse_if_expr(),
            TokenKind::DaysBetween | TokenKind::HoursBetween | TokenKind::MinutesBetween
            | TokenKind::FormatDate => {
                self.parse_time_between_expr()
            }
            TokenKind::NowMinus | TokenKind::NowPlus => self.parse_now_offset_expr(),
            TokenKind::Concat | TokenKind::Lower | TokenKind::Upper | TokenKind::Trim
            | TokenKind::Length | TokenKind::Substring
            | TokenKind::StartsWith | TokenKind::EndsWith | TokenKind::Contains => {
                self.parse_string_expr()
            }
            TokenKind::ToInt | TokenKind::ToDecimal | TokenKind::ToString_ => {
                self.parse_conversion_expr()
            }
            TokenKind::Coalesce => self.parse_coalesce_expr(),
            TokenKind::Round => self.parse_round_expr(),
            TokenKind::Ceil | TokenKind::Floor | TokenKind::Abs => {
                self.parse_math_unary_expr()
            }
            TokenKind::Select => self.parse_map_expr(),
            TokenKind::Filter => self.parse_filter_expr(),
            TokenKind::Reduce => self.parse_reduce_expr(),
            TokenKind::Split => self.parse_split_expr(),
            TokenKind::Replace => self.parse_replace_expr(),
            TokenKind::Format => self.parse_format_expr(),
            TokenKind::Render => self.parse_render_expr(),
            TokenKind::Translate => self.parse_translate_expr(),
            TokenKind::Func => self.parse_func_call_expr(),
            TokenKind::Literal => {
                self.advance();
                let val = self.parse_literal_value()?;
                self.expect_newline()?;
                Ok(Expr::Literal(val))
            }
            _ => {
                let expr = self.parse_inline_expr()?;
                self.expect_newline()?;
                Ok(expr)
            }
        }
    }

    fn parse_fetch_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Fetch)?;
        let source = self.expect_ident()?;
        self.expect_newline()?;

        let mut filters = Vec::new();
        let mut with = Vec::new();
        if matches!(self.peek_kind(), TokenKind::Indent) {
            self.expect_indent()?;
            while !self.check_dedent() && !self.is_at_end() {
                match self.peek_kind() {
                    TokenKind::Filter => {
                        filters.push(self.parse_filter_clause()?);
                    }
                    TokenKind::With => {
                        self.advance();
                        with.push(self.expect_ident()?);
                        self.expect_newline()?;
                    }
                    _ => break,
                }
            }
            self.expect_dedent()?;
        }

        self.expect(TokenKind::Or)?;
        let or_code = self.expect_int()?;
        let or_message = if matches!(self.peek_kind(), TokenKind::StringLit(_)) {
            Some(self.expect_string()?)
        } else {
            None
        };
        let or_shape = if matches!(self.peek_kind(), TokenKind::ShapeName(_)) {
            Some(self.parse_error_shape()?)
        } else {
            self.expect_newline()?;
            None
        };

        Ok(Expr::Fetch {
            source,
            filters,
            with,
            or_code,
            or_message,
            or_shape,
        })
    }

    fn parse_query_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Query)?;
        let source = self.expect_ident()?;
        self.expect_newline()?;

        let mut filters = Vec::new();
        let mut sorts = Vec::new();
        let mut cursor = None;
        let mut page_size = None;

        if matches!(self.peek_kind(), TokenKind::Indent) {
            self.expect_indent()?;
            while !self.check_dedent() && !self.is_at_end() {
                match self.peek_kind() {
                    TokenKind::Filter => {
                        filters.push(self.parse_filter_clause()?);
                    }
                    TokenKind::Sort => {
                        self.advance();
                        let field = self.expect_ident()?;
                        let direction = if matches!(self.peek_kind(), TokenKind::Desc) {
                            self.advance();
                            SortDirection::Desc
                        } else {
                            if matches!(self.peek_kind(), TokenKind::Asc) {
                                self.advance();
                            }
                            SortDirection::Asc
                        };
                        sorts.push(SortClause { field, direction });
                        self.expect_newline()?;
                    }
                    TokenKind::Cursor => {
                        self.advance();
                        let expr = self.parse_inline_expr()?;
                        cursor = Some(Box::new(expr));
                        self.expect_newline()?;
                    }
                    TokenKind::PageSize => {
                        self.advance();
                        let expr = self.parse_inline_expr()?;
                        page_size = Some(Box::new(expr));
                        self.expect_newline()?;
                    }
                    _ => break,
                }
            }
            self.expect_dedent()?;
        }

        Ok(Expr::Query {
            source,
            filters,
            sorts,
            cursor,
            page_size,
            cache_ttl: None,
        })
    }

    fn parse_call_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Call)?;

        if matches!(self.peek_kind(), TokenKind::Ident(ref s) if s == "wasm") {
            self.advance();
            let algo = self.expect_ident()?;
            self.expect(TokenKind::Colon)?;
            let digest = self.expect_ident()?;
            let hash = format!("{algo}:{digest}");
            let mut inputs = Vec::new();
            while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                inputs.push(self.expect_ident()?);
            }
            self.expect_newline()?;
            return Ok(Expr::WasmCall { hash, inputs });
        }

        let svc_path = self.parse_dot_path()?;
        let (service, method) = if svc_path.segments.len() == 2 {
            (svc_path.segments[0].clone(), svc_path.segments[1].clone())
        } else {
            return Err(self.error("CALL requires service.method"));
        };

        let mut args = Vec::new();

        if matches!(self.peek_kind(), TokenKind::Ident(_)) {
            while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                let name = self.expect_ident()?;
                let value = self.parse_inline_expr()?;
                args.push((name, value));
            }
            self.expect_newline()?;
        } else {
            self.expect_newline()?;
            if matches!(self.peek_kind(), TokenKind::Indent) {
                self.expect_indent()?;
                while !self.check_dedent() && !self.is_at_end() {
                    let name = self.expect_ident()?;
                    let value = self.parse_inline_expr()?;
                    args.push((name, value));
                    self.expect_newline()?;
                }
                self.expect_dedent()?;
            }
        }

        let mut or_code = 0;
        let mut or_message = None;
        if matches!(self.peek_kind(), TokenKind::Or) {
            self.advance();
            or_code = self.expect_int()?;
            if matches!(self.peek_kind(), TokenKind::StringLit(_)) {
                or_message = Some(self.expect_string()?);
            }
            self.expect_newline()?;
        }

        Ok(Expr::Call {
            service,
            method,
            args,
            or_code,
            or_message,
        })
    }

    fn parse_filter_clause(&mut self) -> AxisResult<FilterClause> {
        self.expect(TokenKind::Filter)?;
        let field = self.expect_ident()?;
        let op = self.parse_filter_op()?;
        let value = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(FilterClause { field, op, value })
    }

    fn parse_filter_op(&mut self) -> AxisResult<FilterOp> {
        let op = match self.peek_kind() {
            TokenKind::Eq => FilterOp::Eq,
            TokenKind::Neq => FilterOp::Neq,
            TokenKind::Gt => FilterOp::Gt,
            TokenKind::Gte => FilterOp::Gte,
            TokenKind::Lt => FilterOp::Lt,
            TokenKind::Lte => FilterOp::Lte,
            TokenKind::In => FilterOp::In,
            TokenKind::Between => FilterOp::Between,
            TokenKind::Like => FilterOp::Like,
            TokenKind::StartsWith => FilterOp::StartsWith,
            TokenKind::Contains => FilterOp::Contains,
            _ => return Err(self.error(format!("expected filter operator, got {}", self.peek_kind()))),
        };
        self.advance();
        Ok(op)
    }

    fn parse_compare_op(&mut self) -> AxisResult<CompareOp> {
        let op = match self.peek_kind() {
            TokenKind::Eq => CompareOp::Eq,
            TokenKind::Neq => CompareOp::Neq,
            TokenKind::Gt => CompareOp::Gt,
            TokenKind::Gte => CompareOp::Gte,
            TokenKind::Lt => CompareOp::Lt,
            TokenKind::Lte => CompareOp::Lte,
            TokenKind::In => CompareOp::In,
            _ => return Err(self.error(format!("expected comparison operator, got {}", self.peek_kind()))),
        };
        self.advance();
        Ok(op)
    }

    fn parse_binary_arith_expr(&mut self) -> AxisResult<Expr> {
        let op = match self.peek_kind() {
            TokenKind::Add => BinaryOp::Add,
            TokenKind::Sub => BinaryOp::Sub,
            TokenKind::Mul => BinaryOp::Mul,
            TokenKind::Div => BinaryOp::Div,
            TokenKind::Mod => BinaryOp::Mod,
            _ => unreachable!(),
        };
        self.advance();
        let left = self.parse_inline_expr()?;
        let right = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_binary_bool_expr(&mut self) -> AxisResult<Expr> {
        let op = match self.peek_kind() {
            TokenKind::And => BinaryOp::And,
            TokenKind::Or => BinaryOp::Or,
            _ => unreachable!(),
        };
        self.advance();
        self.expect_newline()?;

        if matches!(self.peek_kind(), TokenKind::Indent) {
            self.expect_indent()?;
            let mut operands = Vec::new();
            while !self.check_dedent() && !self.is_at_end() {
                let expr = self.parse_block_expr()?;
                operands.push(expr);
            }
            self.expect_dedent()?;

            let mut result = operands.remove(0);
            for operand in operands {
                result = Expr::Binary {
                    op: op.clone(),
                    left: Box::new(result),
                    right: Box::new(operand),
                };
            }
            Ok(result)
        } else {
            let left = self.parse_inline_expr()?;
            let right = self.parse_inline_expr()?;
            self.expect_newline()?;
            Ok(Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
    }

    fn parse_binary_compare_expr(&mut self) -> AxisResult<Expr> {
        let op = match self.peek_kind() {
            TokenKind::Eq => BinaryOp::Eq,
            TokenKind::Neq => BinaryOp::Neq,
            TokenKind::Gt => BinaryOp::Gt,
            TokenKind::Gte => BinaryOp::Gte,
            TokenKind::Lt => BinaryOp::Lt,
            TokenKind::Lte => BinaryOp::Lte,
            _ => unreachable!(),
        };
        self.advance();
        let left = self.parse_inline_expr()?;
        let right = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_unary_expr(&mut self) -> AxisResult<Expr> {
        let op = match self.peek_kind() {
            TokenKind::Not => UnaryOp::Not,
            TokenKind::Empty => UnaryOp::Empty,
            TokenKind::Exists => UnaryOp::Exists,
            _ => unreachable!(),
        };
        self.advance();

        if matches!(self.peek_kind(), TokenKind::Newline) {
            self.expect_newline()?;
            if matches!(self.peek_kind(), TokenKind::Indent) {
                self.expect_indent()?;
                let operand = self.parse_block_expr()?;
                self.expect_dedent()?;
                return Ok(Expr::Unary {
                    op,
                    operand: Box::new(operand),
                });
            }
        }

        let operand = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::Unary {
            op,
            operand: Box::new(operand),
        })
    }

    fn parse_aggregate_expr(&mut self) -> AxisResult<Expr> {
        let op = match self.peek_kind() {
            TokenKind::Count => AggregateOp::Count,
            TokenKind::Sum => AggregateOp::Sum,
            TokenKind::Avg => AggregateOp::Avg,
            TokenKind::Min => AggregateOp::Min,
            TokenKind::Max => AggregateOp::Max,
            TokenKind::First => AggregateOp::First,
            TokenKind::Last => AggregateOp::Last,
            _ => unreachable!(),
        };
        self.advance();
        let source = self.parse_inline_expr()?;
        let field = if matches!(self.peek_kind(), TokenKind::Ident(_))
            && !matches!(self.peek_kind(), TokenKind::Newline)
        {
            Some(self.expect_ident()?)
        } else {
            None
        };
        self.expect_newline()?;
        Ok(Expr::Aggregate {
            op,
            source: Box::new(source),
            field,
        })
    }

    fn parse_if_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::If)?;
        let cond = self.parse_inline_expr()?;
        self.expect_newline()?;

        if matches!(self.peek_kind(), TokenKind::Indent) {
            self.expect_indent()?;
            self.expect(TokenKind::Then)?;
            let then = self.parse_inline_expr()?;
            self.expect_newline()?;
            self.expect(TokenKind::Else)?;
            let else_ = self.parse_inline_expr()?;
            self.expect_newline()?;
            self.expect_dedent()?;
            return Ok(Expr::If {
                cond: Box::new(cond),
                then: Box::new(then),
                else_: Box::new(else_),
            });
        }

        self.expect(TokenKind::Then)?;
        let then = self.parse_inline_expr()?;
        self.expect(TokenKind::Else)?;
        let else_ = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::If {
            cond: Box::new(cond),
            then: Box::new(then),
            else_: Box::new(else_),
        })
    }

    fn parse_time_between_expr(&mut self) -> AxisResult<Expr> {
        let op = match self.peek_kind() {
            TokenKind::DaysBetween => BinaryOp::DaysBetween,
            TokenKind::HoursBetween => BinaryOp::HoursBetween,
            TokenKind::MinutesBetween => BinaryOp::MinutesBetween,
            TokenKind::FormatDate => BinaryOp::FormatDate,
            _ => unreachable!(),
        };
        self.advance();
        let left = self.parse_inline_expr()?;
        let right = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_now_offset_expr(&mut self) -> AxisResult<Expr> {
        let direction = match self.peek_kind() {
            TokenKind::NowPlus => OffsetDirection::Plus,
            TokenKind::NowMinus => OffsetDirection::Minus,
            _ => unreachable!(),
        };
        self.advance();
        let amount = self.parse_inline_expr()?;
        let unit = self.parse_time_unit()?;
        self.expect_newline()?;
        Ok(Expr::NowOffset {
            direction,
            amount: Box::new(amount),
            unit,
        })
    }

    fn parse_time_unit(&mut self) -> AxisResult<TimeUnit> {
        let unit = match self.peek_kind() {
            TokenKind::Seconds => TimeUnit::Seconds,
            TokenKind::Minutes => TimeUnit::Minutes,
            TokenKind::Hours => TimeUnit::Hours,
            TokenKind::Days => TimeUnit::Days,
            TokenKind::Weeks => TimeUnit::Weeks,
            TokenKind::Months => TimeUnit::Months,
            TokenKind::Years => TimeUnit::Years,
            _ => return Err(self.error(format!("expected time unit, got {}", self.peek_kind()))),
        };
        self.advance();
        Ok(unit)
    }

    fn parse_string_expr(&mut self) -> AxisResult<Expr> {
        match self.peek_kind() {
            TokenKind::Concat => {
                self.advance();
                let left = self.parse_inline_expr()?;
                let right = self.parse_inline_expr()?;
                self.expect_newline()?;
                Ok(Expr::Binary {
                    op: BinaryOp::Concat,
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
            TokenKind::Lower => {
                self.advance();
                let operand = self.parse_inline_expr()?;
                self.expect_newline()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Lower,
                    operand: Box::new(operand),
                })
            }
            TokenKind::Upper => {
                self.advance();
                let operand = self.parse_inline_expr()?;
                self.expect_newline()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Upper,
                    operand: Box::new(operand),
                })
            }
            TokenKind::Trim => {
                self.advance();
                let operand = self.parse_inline_expr()?;
                self.expect_newline()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Trim,
                    operand: Box::new(operand),
                })
            }
            TokenKind::Length => {
                self.advance();
                let operand = self.parse_inline_expr()?;
                self.expect_newline()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Length,
                    operand: Box::new(operand),
                })
            }
            TokenKind::Substring => {
                self.advance();
                let a = self.parse_inline_expr()?;
                let b = self.parse_inline_expr()?;
                let c = self.parse_inline_expr()?;
                self.expect_newline()?;
                Ok(Expr::Ternary {
                    op: TernaryOp::Substring,
                    a: Box::new(a),
                    b: Box::new(b),
                    c: Box::new(c),
                })
            }
            TokenKind::StartsWith | TokenKind::EndsWith | TokenKind::Contains => {
                let op = match self.peek_kind() {
                    TokenKind::StartsWith => BinaryOp::StartsWith,
                    TokenKind::EndsWith => BinaryOp::EndsWith,
                    TokenKind::Contains => BinaryOp::Contains,
                    _ => unreachable!(),
                };
                self.advance();
                let left = self.parse_inline_expr()?;
                let right = self.parse_inline_expr()?;
                self.expect_newline()?;
                Ok(Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
            _ => unreachable!(),
        }
    }

    fn parse_conversion_expr(&mut self) -> AxisResult<Expr> {
        let op = match self.peek_kind() {
            TokenKind::ToInt => UnaryOp::ToInt,
            TokenKind::ToDecimal => UnaryOp::ToDecimal,
            TokenKind::ToString_ => UnaryOp::ToString,
            _ => unreachable!(),
        };
        self.advance();
        let operand = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::Unary {
            op,
            operand: Box::new(operand),
        })
    }

    fn parse_coalesce_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Coalesce)?;
        let value = self.parse_inline_expr()?;
        let default = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::Coalesce {
            value: Box::new(value),
            default: Box::new(default),
        })
    }

    fn parse_round_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Round)?;
        self.expect_newline()?;
        if matches!(self.peek_kind(), TokenKind::Indent) {
            self.expect_indent()?;
            let value = self.parse_block_expr()?;
            let scale = self.parse_inline_expr()?;
            self.expect_newline()?;
            self.expect_dedent()?;
            return Ok(Expr::Binary {
                op: BinaryOp::Round,
                left: Box::new(value),
                right: Box::new(scale),
            });
        }
        let value = self.parse_inline_expr()?;
        let scale = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::Binary {
            op: BinaryOp::Round,
            left: Box::new(value),
            right: Box::new(scale),
        })
    }

    fn parse_math_unary_expr(&mut self) -> AxisResult<Expr> {
        let op = match self.peek_kind() {
            TokenKind::Ceil => UnaryOp::Ceil,
            TokenKind::Floor => UnaryOp::Floor,
            TokenKind::Abs => UnaryOp::Abs,
            _ => unreachable!(),
        };
        self.advance();
        let operand = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::Unary {
            op,
            operand: Box::new(operand),
        })
    }

    // --- POLICY ---

    fn parse_policy(&mut self) -> AxisResult<PolicyDef> {
        let span = self.expect(TokenKind::Policy)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut filters = Vec::new();
        let mut requires = Vec::new();

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::AppliesTo => {
                    self.advance();
                    self.expect(TokenKind::Flow)?;
                    if matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) {
                        self.expect_newline()?;
                    } else {
                        self.expect(TokenKind::Where)?;
                        while !matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) {
                            let filter = self.parse_policy_filter()?;
                            filters.push(filter);
                        }
                        self.expect_newline()?;
                    }
                }
                TokenKind::Require => {
                    self.advance();
                    let clause = self.parse_require_clause()?;
                    requires.push(clause);
                    self.expect_newline()?;
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        Ok(PolicyDef {
            name,
            applies_to: AppliesTo { filters },
            requires,
            span,
        })
    }

    fn parse_policy_filter(&mut self) -> AxisResult<PolicyFilter> {
        match self.peek_kind() {
            TokenKind::Method => {
                self.advance();
                self.expect(TokenKind::In)?;
                let mut methods = Vec::new();
                while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                    methods.push(self.expect_ident()?);
                }
                Ok(PolicyFilter::MethodIn(methods))
            }
            TokenKind::Reads => {
                self.advance();
                let source = self.expect_ident()?;
                Ok(PolicyFilter::Reads(source))
            }
            TokenKind::Writes => {
                self.advance();
                let source = self.expect_ident()?;
                Ok(PolicyFilter::Writes(source))
            }
            TokenKind::Ident(ref s) if s == "path" => {
                self.advance();
                self.expect(TokenKind::StartsWith)?;
                let path = self.expect_string()?;
                Ok(PolicyFilter::PathStartsWith(path))
            }
            _ => Err(self.error(format!("expected policy filter (METHOD, READS, WRITES, PATH), got {}", self.peek_kind()))),
        }
    }

    fn parse_require_clause(&mut self) -> AxisResult<RequireClause> {
        match self.peek_kind() {
            TokenKind::Auth => {
                self.advance();
                if matches!(self.peek_kind(), TokenKind::Ident(ref s) if s == "role") {
                    self.advance();
                    let role = self.expect_ident()?;
                    Ok(RequireClause::Auth(Some(role)))
                } else {
                    Ok(RequireClause::Auth(None))
                }
            }
            TokenKind::Limit => {
                self.advance();
                Ok(RequireClause::Limit)
            }
            TokenKind::Scope => {
                self.advance();
                Ok(RequireClause::Scope)
            }
            TokenKind::Rule => {
                self.advance();
                let name = self.expect_ident()?;
                Ok(RequireClause::Rule(name))
            }
            TokenKind::Guard => {
                self.advance();
                let name = self.expect_ident()?;
                Ok(RequireClause::Guard(name))
            }
            _ => Err(self.error(format!("expected REQUIRE target, got {}", self.peek_kind()))),
        }
    }

    // --- SERVICE ---

    fn parse_service(&mut self) -> AxisResult<ServiceDef> {
        let span = self.expect(TokenKind::Service)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        self.expect(TokenKind::Endpoint)?;
        let endpoint = self.expect_ident()?;
        self.expect_newline()?;

        self.expect(TokenKind::Auth)?;
        let auth_type = self.expect_ident()?;
        self.expect(TokenKind::Vault)?;
        let vault_key = self.expect_ident()?;
        self.expect_newline()?;

        let mut methods = Vec::new();
        while matches!(self.peek_kind(), TokenKind::Method) {
            methods.push(self.parse_service_method()?);
        }
        self.expect_dedent()?;

        Ok(ServiceDef {
            name,
            endpoint,
            auth_type,
            vault_key,
            methods,
            span,
        })
    }

    fn parse_service_method(&mut self) -> AxisResult<ServiceMethod> {
        self.expect(TokenKind::Method)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        let mut timeout = None;
        let mut retry = None;
        let mut cache_ttl = None;

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Input => {
                    self.advance();
                    while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                        let field_name = self.expect_ident()?;
                        let ty = self.parse_type_expr()?;
                        inputs.push((field_name, ty));
                    }
                    self.expect_newline()?;
                }
                TokenKind::Output => {
                    self.advance();
                    while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                        let field_name = self.expect_ident()?;
                        let ty = self.parse_type_expr()?;
                        outputs.push((field_name, ty));
                    }
                    self.expect_newline()?;
                }
                TokenKind::Timeout => {
                    self.advance();
                    timeout = Some(self.parse_duration()?);
                    self.expect_newline()?;
                }
                TokenKind::Retry => {
                    self.advance();
                    let count = self.expect_int()?;
                    self.expect(TokenKind::Backoff)?;
                    let strat_name = self.expect_ident()?;
                    let strategy = match strat_name.as_str() {
                        "exponential" => RetryStrategy::Exponential,
                        "linear" => RetryStrategy::Linear,
                        "none" => RetryStrategy::None,
                        _ => return Err(self.error(format!("unknown retry strategy: {strat_name}"))),
                    };
                    retry = Some(RetryConfig { count, strategy });
                    self.expect_newline()?;
                }
                TokenKind::Cache => {
                    self.advance();
                    cache_ttl = Some(self.expect_int()?);
                    self.expect_newline()?;
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        Ok(ServiceMethod {
            name,
            inputs,
            outputs,
            timeout,
            retry,
            cache_ttl,
        })
    }

    // --- SAGA ---

    fn parse_saga(&mut self) -> AxisResult<SagaDef> {
        let span = self.expect(TokenKind::Saga)?;
        let name = self.expect_ident()?;
        let method = self.parse_http_method()?;
        let path = self.expect_path()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut realm = None;
        let mut auth = None;
        let mut body = None;
        let mut steps = Vec::new();

        if matches!(self.peek_kind(), TokenKind::Realm) {
            self.advance();
            realm = Some(self.expect_ident()?);
            self.expect_newline()?;
        }
        if matches!(self.peek_kind(), TokenKind::Auth) {
            auth = Some(self.parse_auth_decl()?);
        }
        if matches!(self.peek_kind(), TokenKind::Body) {
            body = Some(self.parse_body_decl()?);
        }

        while matches!(self.peek_kind(), TokenKind::Step) {
            steps.push(self.parse_saga_step()?);
        }

        self.expect(TokenKind::OnFailure)?;
        self.expect(TokenKind::RunCompensations)?;
        self.expect_newline()?;

        self.expect(TokenKind::OnSuccess)?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut effects = Vec::new();
        while matches!(self.peek_kind(), TokenKind::Effect) {
            effects.push(self.parse_effect_step()?);
        }

        let return_stmt = self.parse_return_stmt()?;
        self.expect_dedent()?;

        self.expect_dedent()?;

        Ok(SagaDef {
            name,
            method,
            path,
            realm,
            auth,
            body,
            steps,
            on_failure: OnFailure { run_compensations: true },
            on_success: OnSuccess { effects, return_stmt },
            span,
        })
    }

    fn parse_saga_step(&mut self) -> AxisResult<SagaStep> {
        let span = self.expect(TokenKind::Step)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut flow_steps = Vec::new();
        let mut verify = None;
        let mut yields = Vec::new();
        let mut compensate = Compensate::None;

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Verify => {
                    self.advance();
                    self.expect_newline()?;
                    self.expect_indent()?;
                    verify = Some(self.parse_block_expr()?);
                    self.expect_dedent()?;
                }
                TokenKind::Yield_ => {
                    self.advance();
                    while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                        yields.push(self.expect_ident()?);
                    }
                    self.expect_newline()?;
                }
                TokenKind::Compensate => {
                    self.advance();
                    if matches!(self.peek_kind(), TokenKind::None_) {
                        self.advance();
                        self.expect_newline()?;
                        compensate = Compensate::None;
                    } else {
                        self.expect_newline()?;
                        self.expect_indent()?;
                        let mut comp_steps = Vec::new();
                        while !self.check_dedent() && !self.is_at_end() {
                            comp_steps.push(self.parse_flow_step()?);
                        }
                        self.expect_dedent()?;
                        compensate = Compensate::Steps(comp_steps);
                    }
                    break;
                }
                _ => {
                    flow_steps.push(self.parse_flow_step()?);
                }
            }
        }
        self.expect_dedent()?;

        Ok(SagaStep {
            name,
            flow_steps,
            verify,
            yields,
            compensate,
            span,
        })
    }

    fn parse_flow_step(&mut self) -> AxisResult<FlowStep> {
        match self.peek_kind() {
            TokenKind::Rule => Ok(FlowStep::Rule(self.parse_rule_step()?)),
            TokenKind::Guard => Ok(FlowStep::Guard(self.parse_guard_step()?)),
            TokenKind::Let => Ok(FlowStep::Let(self.parse_let_step()?)),
            TokenKind::Insert => Ok(FlowStep::Insert(self.parse_insert_step()?)),
            TokenKind::Update => Ok(FlowStep::Update(self.parse_update_step()?)),
            TokenKind::Delete => Ok(FlowStep::Delete(self.parse_delete_step()?)),
            TokenKind::Effect => Ok(FlowStep::Effect(self.parse_effect_step()?)),
            TokenKind::Match => Ok(FlowStep::Match(self.parse_match_step()?)),
            TokenKind::Set => Ok(FlowStep::Set(self.parse_set_step()?)),
            TokenKind::Each => Ok(FlowStep::Each(self.parse_each_step()?)),
            TokenKind::Try => Ok(FlowStep::Try(self.parse_try_step()?)),
            TokenKind::Upload => Ok(FlowStep::Upload(self.parse_upload_step()?)),
            TokenKind::Call => {
                // bare CALL (not LET ... CALL) for saga compensations
                let span = self.current_span();
                let expr = self.parse_call_expr()?;
                Ok(FlowStep::Let(LetStep {
                    name: "_".into(),
                    expr,
                    span,
                }))
            }
            _ => Err(self.error(format!("expected flow step, got {}", self.peek_kind()))),
        }
    }

    // --- STORAGE ---

    fn parse_storage(&mut self) -> AxisResult<StorageDef> {
        let span = self.expect(TokenKind::Storage)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut backend = None;
        let mut bucket = None;
        let mut prefix = None;
        let mut access = None;
        let mut max_size = None;
        let mut types = Vec::new();

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Backend => {
                    self.advance();
                    let b = self.expect_ident()?;
                    backend = Some(match b.as_str() {
                        "local" => StorageBackend::Local,
                        "s3" => StorageBackend::S3,
                        _ => return Err(self.error(format!("unknown storage backend: {b}, expected local or s3"))),
                    });
                    self.expect_newline()?;
                }
                TokenKind::Bucket => {
                    self.advance();
                    bucket = Some(self.expect_ident_or_path()?);
                    self.expect_newline()?;
                }
                TokenKind::Prefix => {
                    self.advance();
                    prefix = Some(self.expect_ident_or_path()?);
                    self.expect_newline()?;
                }
                TokenKind::Access => {
                    self.advance();
                    let a = self.expect_ident()?;
                    access = Some(match a.as_str() {
                        "public" => StorageAccess::Public,
                        "private" => StorageAccess::Private,
                        _ => return Err(self.error(format!("unknown access level: {a}, expected public or private"))),
                    });
                    self.expect_newline()?;
                }
                TokenKind::MaxSize => {
                    self.advance();
                    max_size = Some(self.expect_int()?);
                    self.expect_newline()?;
                }
                TokenKind::Types => {
                    self.advance();
                    while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                        types.push(self.expect_ident()?);
                    }
                    self.expect_newline()?;
                }
                TokenKind::Newline => {
                    self.advance();
                }
                _ => {
                    return Err(self.error(format!(
                        "unexpected token in STORAGE: {}",
                        self.peek_kind()
                    )));
                }
            }
        }
        self.expect_dedent()?;

        let backend = backend.ok_or_else(|| self.error("STORAGE requires BACKEND"))?;
        let bucket = bucket.ok_or_else(|| self.error("STORAGE requires BUCKET"))?;
        let access = access.unwrap_or(StorageAccess::Private);

        Ok(StorageDef {
            name,
            backend,
            bucket,
            prefix,
            access,
            max_size,
            types,
            span,
        })
    }

    fn parse_upload_step(&mut self) -> AxisResult<UploadStep> {
        let span = self.expect(TokenKind::Upload)?;
        let file_expr = self.parse_inline_expr()?;
        self.expect(TokenKind::Arrow)?;
        let storage = self.expect_ident()?;
        self.expect(TokenKind::As)?;
        let binding = self.expect_ident()?;
        self.expect_newline()?;
        Ok(UploadStep {
            file_expr,
            storage,
            binding,
            span,
        })
    }

    // --- SURFACE ---

    fn parse_surface(&mut self) -> AxisResult<SurfaceDef> {
        let span = self.expect(TokenKind::Surface)?;
        let name = self.expect_ident()?;
        let version = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut realm = None;
        let mut base_path = None;
        let mut routes = Vec::new();
        let mut exposes = Vec::new();
        let mut deprecate = None;

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Realm => {
                    self.advance();
                    realm = Some(self.expect_ident()?);
                    self.expect_newline()?;
                }
                TokenKind::BasePath => {
                    self.advance();
                    base_path = Some(self.expect_path()?);
                    self.expect_newline()?;
                }
                TokenKind::Route => {
                    self.advance();
                    let method = self.parse_http_method()?;
                    let path = self.expect_path()?;
                    self.expect(TokenKind::Arrow)?;
                    let target = self.expect_ident()?;
                    self.expect_newline()?;
                    routes.push(RouteDef { method, path, target });
                }
                TokenKind::Expose => {
                    self.advance();
                    let shape = self.expect_shape_name()?;
                    self.expect(TokenKind::As)?;
                    let alias = self.expect_shape_name()?;
                    self.expect_newline()?;
                    self.expect_indent()?;

                    let mut fields = Vec::new();
                    while !self.check_dedent() && !self.is_at_end() {
                        match self.peek_kind() {
                            TokenKind::Field => {
                                self.advance();
                                let fname = self.expect_ident()?;
                                let ty = self.parse_type_expr()?;
                                self.expect_newline()?;
                                fields.push(ExposeField::Field { name: fname, ty });
                            }
                            TokenKind::Hide => {
                                self.advance();
                                let fname = self.expect_ident()?;
                                self.expect_newline()?;
                                fields.push(ExposeField::Hide(fname));
                            }
                            TokenKind::Rename => {
                                self.advance();
                                let from = self.expect_ident()?;
                                self.expect(TokenKind::As)?;
                                let to = self.expect_ident()?;
                                self.expect_newline()?;
                                fields.push(ExposeField::Rename { from, to });
                            }
                            _ => break,
                        }
                    }
                    self.expect_dedent()?;
                    exposes.push(ExposeDef {
                        shape,
                        alias: Some(alias),
                        fields,
                    });
                }
                TokenKind::Deprecate => {
                    self.advance();
                    let ver = self.expect_ident()?;
                    self.expect(TokenKind::Sunset)?;
                    let sunset = self.expect_string()?;
                    self.expect_newline()?;
                    deprecate = Some(DeprecateDef { version: ver, sunset });
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        Ok(SurfaceDef {
            name,
            version,
            realm,
            base_path,
            routes,
            exposes,
            deprecate,
            span,
        })
    }

    // --- MIGRATE ---

    fn parse_migrate(&mut self) -> AxisResult<MigrateDef> {
        let span = self.expect(TokenKind::Migrate)?;
        let shape = self.expect_shape_name()?;
        let from_version = self.expect_ident()?;
        self.expect(TokenKind::To)?;
        let to_version = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut ops = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Copy => {
                    self.advance();
                    let mut fields = Vec::new();
                    while matches!(self.peek_kind(), TokenKind::Ident(_)) {
                        fields.push(self.expect_ident()?);
                    }
                    self.expect_newline()?;
                    ops.push(MigrateOp::Copy(fields));
                }
                TokenKind::Compute => {
                    self.advance();
                    let field = self.expect_ident()?;
                    self.expect_newline()?;
                    self.expect_indent()?;
                    let expr = self.parse_block_expr()?;
                    self.expect_dedent()?;
                    ops.push(MigrateOp::Compute { field, expr });
                }
                TokenKind::Drop => {
                    self.advance();
                    let field = self.expect_ident()?;
                    self.expect_newline()?;
                    ops.push(MigrateOp::Drop(field));
                }
                TokenKind::Add => {
                    self.advance();
                    let field_name = self.expect_ident()?;
                    let ty = self.parse_type_expr()?;
                    let modifiers = self.parse_modifiers()?;
                    self.expect_newline()?;
                    ops.push(MigrateOp::Add(FieldDef {
                        name: field_name,
                        ty,
                        modifiers,
                        span,
                    }));
                }
                TokenKind::Rename => {
                    self.advance();
                    let from = self.expect_ident()?;
                    self.expect(TokenKind::To)?;
                    let to = self.expect_ident()?;
                    self.expect_newline()?;
                    ops.push(MigrateOp::Rename { from, to });
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        Ok(MigrateDef {
            shape,
            from_version,
            to_version,
            ops,
            span,
        })
    }

    fn parse_stream(&mut self) -> AxisResult<StreamDef> {
        let span = self.expect(TokenKind::Stream)?;
        let name = self.expect_ident()?;
        let transport_str = self.expect_ident()?;
        let transport = match transport_str.as_str() {
            "ws" => StreamTransport::WebSocket,
            "sse" => StreamTransport::Sse,
            _ => return Err(self.error(format!("expected 'ws' or 'sse', got '{transport_str}'"))),
        };
        let path = match self.peek_kind() {
            TokenKind::Path(s) => { self.advance(); s }
            TokenKind::StringLit(s) => { self.advance(); s }
            other => return Err(self.error(format!("expected path, got {other}"))),
        };
        self.expect_newline()?;
        self.expect_indent()?;

        let mut realm = None;
        let mut auth = None;
        let mut events = Vec::new();
        let mut receivers = Vec::new();

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Realm => {
                    self.advance();
                    realm = Some(self.expect_ident()?);
                    self.expect_newline()?;
                }
                TokenKind::Auth => {
                    auth = Some(self.parse_auth_decl()?);
                }
                TokenKind::Event => {
                    let evt_span = self.current_span();
                    self.advance();
                    let evt_name = self.expect_ident()?;
                    self.expect_newline()?;
                    let mut fields = Vec::new();
                    if matches!(self.peek_kind(), TokenKind::Indent) {
                        self.expect_indent()?;
                        while !self.check_dedent() && !self.is_at_end() {
                            let field_span = self.current_span();
                            let field_name = self.expect_ident()?;
                            let ty = self.parse_type_expr()?;
                            let modifiers = self.parse_modifiers()?;
                            self.expect_newline()?;
                            fields.push(FieldDef { name: field_name, ty, modifiers, span: field_span });
                        }
                        self.expect_dedent()?;
                    }
                    events.push(StreamEvent { name: evt_name, fields, span: evt_span });
                }
                TokenKind::Receive => {
                    self.advance();
                    self.expect_newline()?;
                    self.expect_indent()?;
                    while !self.check_dedent() && !self.is_at_end() {
                        match self.peek_kind() {
                            TokenKind::On => {
                                let recv_span = self.current_span();
                                self.advance();
                                let event = self.expect_ident()?;
                                self.expect_newline()?;
                                self.expect_indent()?;
                                let mut steps = Vec::new();
                                while !self.check_dedent() && !self.is_at_end() {
                                    steps.push(self.parse_flow_step()?);
                                }
                                self.expect_dedent()?;
                                receivers.push(StreamReceiver { event, steps, span: recv_span });
                            }
                            _ => break,
                        }
                    }
                    self.expect_dedent()?;
                }
                _ => break,
            }
        }
        self.expect_dedent()?;

        Ok(StreamDef { name, transport, path, realm, auth, events, receivers, span })
    }

    fn parse_dot_path(&mut self) -> AxisResult<DotPath> {
        let mut segments = vec![self.expect_ident()?];
        while matches!(self.peek_kind(), TokenKind::Dot) {
            self.advance();
            segments.push(self.expect_ident()?);
        }
        Ok(DotPath { segments })
    }

    // --- Helpers ---

    fn peek_kind(&self) -> TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| t.kind.clone())
            .unwrap_or(TokenKind::Eof)
    }

    fn current_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span)
            .unwrap_or(Span {
                offset: 0,
                len: 0,
                line: 0,
                col: 0,
            })
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn expect(&mut self, expected: TokenKind) -> AxisResult<Span> {
        let actual = self.peek_kind();
        if std::mem::discriminant(&actual) == std::mem::discriminant(&expected) {
            let span = self.current_span();
            self.advance();
            Ok(span)
        } else {
            Err(self.error(format!("expected {expected}, got {actual}")))
        }
    }

    fn expect_ident(&mut self) -> AxisResult<String> {
        match self.peek_kind() {
            TokenKind::Ident(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(self.error(format!("expected identifier, got {other}"))),
        }
    }

    fn expect_ident_or_path(&mut self) -> AxisResult<String> {
        match self.peek_kind() {
            TokenKind::Ident(s) | TokenKind::Path(s) | TokenKind::StringLit(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(self.error(format!("expected identifier, path, or string, got {other}"))),
        }
    }

    fn expect_name(&mut self) -> AxisResult<String> {
        match self.peek_kind() {
            TokenKind::Ident(s) | TokenKind::ShapeName(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(self.error(format!("expected name, got {other}"))),
        }
    }

    fn expect_shape_name(&mut self) -> AxisResult<String> {
        match self.peek_kind() {
            TokenKind::ShapeName(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(self.error(format!("expected shape name (PascalCase), got {other}"))),
        }
    }

    fn expect_int(&mut self) -> AxisResult<i64> {
        match self.peek_kind() {
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(n)
            }
            other => Err(self.error(format!("expected integer, got {other}"))),
        }
    }

    fn expect_string(&mut self) -> AxisResult<String> {
        match self.peek_kind() {
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(self.error(format!("expected string, got {other}"))),
        }
    }

    fn expect_path(&mut self) -> AxisResult<String> {
        match self.peek_kind() {
            TokenKind::Path(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(self.error(format!("expected path, got {other}"))),
        }
    }

    fn parse_duration(&mut self) -> AxisResult<Duration> {
        let value = self.expect_int()?;
        let unit = if matches!(self.peek_kind(), TokenKind::Ident(_)) {
            let u = self.expect_ident()?;
            match u.as_str() {
                "ms" => DurationUnit::Milliseconds,
                "s" => DurationUnit::Seconds,
                "m" => DurationUnit::Minutes,
                "h" => DurationUnit::Hours,
                _ => return Err(self.error(format!("unknown duration unit: {u}, expected ms/s/m/h"))),
            }
        } else {
            DurationUnit::Seconds
        };
        Ok(Duration { value, unit })
    }

    fn expect_newline(&mut self) -> AxisResult<()> {
        match self.peek_kind() {
            TokenKind::Newline => {
                self.advance();
                self.skip_newlines();
                Ok(())
            }
            TokenKind::Eof => Ok(()),
            other => Err(self.error(format!("expected newline, got {other}"))),
        }
    }

    fn expect_indent(&mut self) -> AxisResult<()> {
        match self.peek_kind() {
            TokenKind::Indent => {
                self.advance();
                Ok(())
            }
            other => Err(self.error(format!("expected indent, got {other}"))),
        }
    }

    fn expect_dedent(&mut self) -> AxisResult<()> {
        match self.peek_kind() {
            TokenKind::Dedent => {
                self.advance();
                Ok(())
            }
            other => Err(self.error(format!("expected dedent, got {other}"))),
        }
    }

    fn check_dedent(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Dedent)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek_kind(), TokenKind::Newline) {
            self.advance();
        }
    }

    fn error(&self, message: impl Into<String>) -> AxisError {
        let span = self.current_span();
        AxisError::parse(span.line, span.col, message)
    }

    fn parse_error_shape(&mut self) -> AxisResult<ErrorShape> {
        let shape = self.expect_shape_name()?;
        self.expect_newline()?;
        self.expect_indent()?;
        let mut fields = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            let name = self.expect_ident()?;
            let value = self.parse_inline_expr()?;
            self.expect_newline()?;
            fields.push((name, value));
        }
        self.expect_dedent()?;
        Ok(ErrorShape { shape, fields })
    }

    // --- SET / EACH / TRY ---

    fn parse_set_step(&mut self) -> AxisResult<SetStep> {
        let span = self.expect(TokenKind::Set)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;
        let expr = self.parse_block_expr()?;
        self.expect_dedent()?;
        Ok(SetStep { name, expr, span })
    }

    fn parse_each_step(&mut self) -> AxisResult<EachStep> {
        let span = self.expect(TokenKind::Each)?;
        let binding = self.expect_ident()?;
        self.expect(TokenKind::In)?;
        let source = self.parse_inline_expr()?;
        let parallel = if matches!(self.peek_kind(), TokenKind::Parallel) {
            self.advance();
            Some(self.expect_int()?)
        } else {
            None
        };
        self.expect_newline()?;
        self.expect_indent()?;
        let mut steps = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            steps.push(self.parse_flow_step()?);
        }
        self.expect_dedent()?;
        Ok(EachStep { binding, source, parallel, steps, span })
    }

    fn parse_try_step(&mut self) -> AxisResult<TryStep> {
        let span = self.expect(TokenKind::Try)?;
        self.expect_newline()?;
        self.expect_indent()?;
        let mut body = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            body.push(self.parse_flow_step()?);
        }
        self.expect_dedent()?;
        self.expect(TokenKind::Recover)?;
        self.expect_newline()?;
        self.expect_indent()?;
        let mut recover = Vec::new();
        while !self.check_dedent() && !self.is_at_end() {
            recover.push(self.parse_flow_step()?);
        }
        self.expect_dedent()?;
        Ok(TryStep { body, recover, span })
    }

    // --- FUNC ---

    fn parse_func(&mut self) -> AxisResult<FuncDef> {
        let span = self.expect(TokenKind::Func)?;
        let name = self.expect_ident()?;
        self.expect_newline()?;
        self.expect_indent()?;

        let mut inputs = Vec::new();
        let mut output = TypeExpr::String(None);
        let mut steps = Vec::new();
        let mut return_expr = None;

        while !self.check_dedent() && !self.is_at_end() {
            match self.peek_kind() {
                TokenKind::Input => {
                    self.advance();
                    let pname = self.expect_ident()?;
                    let ty = self.parse_type_expr()?;
                    self.expect_newline()?;
                    inputs.push(FuncParam { name: pname, ty });
                }
                TokenKind::Output => {
                    self.advance();
                    output = self.parse_type_expr()?;
                    self.expect_newline()?;
                }
                TokenKind::Return => {
                    self.advance();
                    return_expr = Some(self.parse_inline_expr()?);
                    self.expect_newline()?;
                }
                TokenKind::Let => {
                    steps.push(FlowStep::Let(self.parse_let_step()?));
                }
                TokenKind::Set => {
                    steps.push(FlowStep::Set(self.parse_set_step()?));
                }
                TokenKind::Each => {
                    steps.push(FlowStep::Each(self.parse_each_step()?));
                }
                TokenKind::Try => {
                    steps.push(FlowStep::Try(self.parse_try_step()?));
                }
                TokenKind::Match => {
                    steps.push(FlowStep::Match(self.parse_match_step()?));
                }
                TokenKind::Insert => {
                    steps.push(FlowStep::Insert(self.parse_insert_step()?));
                }
                TokenKind::Update => {
                    steps.push(FlowStep::Update(self.parse_update_step()?));
                }
                TokenKind::Delete => {
                    steps.push(FlowStep::Delete(self.parse_delete_step()?));
                }
                TokenKind::Effect => {
                    steps.push(FlowStep::Effect(self.parse_effect_step()?));
                }
                TokenKind::Guard => {
                    steps.push(FlowStep::Guard(self.parse_guard_step()?));
                }
                _ => {
                    return Err(self.error(format!(
                        "unexpected token in FUNC: {}",
                        self.peek_kind()
                    )));
                }
            }
        }
        self.expect_dedent()?;

        let return_expr = return_expr.ok_or_else(|| {
            self.error("FUNC requires a RETURN expression")
        })?;

        Ok(FuncDef { name, inputs, output, steps, return_expr, span })
    }

    // --- Collection expression parsers ---

    fn parse_map_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Select)?;
        let source = self.parse_inline_expr()?;
        let mut fields = Vec::new();
        while matches!(self.peek_kind(), TokenKind::Ident(_)) {
            fields.push(self.expect_ident()?);
        }
        self.expect_newline()?;
        Ok(Expr::MapExpr {
            source: Box::new(source),
            fields,
        })
    }

    fn parse_filter_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Filter)?;
        let source = self.parse_inline_expr()?;
        let condition = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::FilterExpr {
            source: Box::new(source),
            condition: Box::new(condition),
        })
    }

    fn parse_reduce_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Reduce)?;
        let source = self.parse_inline_expr()?;
        let op = match self.peek_kind() {
            TokenKind::Sum => AggregateOp::Sum,
            TokenKind::Count => AggregateOp::Count,
            TokenKind::Avg => AggregateOp::Avg,
            TokenKind::Min => AggregateOp::Min,
            TokenKind::Max => AggregateOp::Max,
            _ => return Err(self.error(format!("expected aggregate op after REDUCE source, got {}", self.peek_kind()))),
        };
        self.advance();
        let field = self.expect_ident()?;
        self.expect_newline()?;
        Ok(Expr::ReduceExpr {
            op,
            source: Box::new(source),
            field,
        })
    }

    fn parse_split_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Split)?;
        let value = self.parse_inline_expr()?;
        let delimiter = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::SplitExpr {
            value: Box::new(value),
            delimiter: Box::new(delimiter),
        })
    }

    fn parse_replace_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Replace)?;
        let value = self.parse_inline_expr()?;
        let from = self.parse_inline_expr()?;
        let to = self.parse_inline_expr()?;
        self.expect_newline()?;
        Ok(Expr::ReplaceExpr {
            value: Box::new(value),
            from: Box::new(from),
            to: Box::new(to),
        })
    }

    fn parse_format_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Format)?;
        let template = self.expect_string()?;
        let mut args = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof | TokenKind::Dedent) {
            args.push(self.parse_inline_expr()?);
        }
        self.expect_newline()?;
        Ok(Expr::FormatExpr { template, args })
    }

    fn parse_func_call_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Func)?;
        let name = self.expect_ident()?;
        let mut args = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof | TokenKind::Dedent) {
            args.push(self.parse_inline_expr()?);
        }
        self.expect_newline()?;
        Ok(Expr::FuncCall { name, args })
    }

    fn parse_render_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Render)?;
        let template = self.expect_string()?;
        let mut vars = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof | TokenKind::Dedent) {
            let key = self.expect_ident()?;
            let val = self.parse_inline_expr()?;
            vars.push((key, val));
        }
        self.expect_newline()?;
        Ok(Expr::Render { template, vars })
    }

    fn parse_translate_expr(&mut self) -> AxisResult<Expr> {
        self.expect(TokenKind::Translate)?;
        let key = self.expect_string()?;
        let mut vars = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof | TokenKind::Dedent) {
            let name = self.expect_ident()?;
            let val = self.parse_inline_expr()?;
            vars.push((name, val));
        }
        self.expect_newline()?;
        Ok(Expr::Translate { key, vars })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(input: &str) -> AxisResult<Program> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        let mut parser = Parser::new(tokens);
        parser.parse_program()
    }

    #[test]
    fn test_parse_shape() {
        let input = r#"SHAPE Booking
  id UUID PK AUTO
  user_id UUID REF User.id REQUIRED
  status ENUM pending confirmed cancelled REQUIRED
  total DECIMAL PRECISION 10 SCALE 2 REQUIRED
  note MAYBE TEXT
"#;
        let program = parse(input).unwrap();
        assert_eq!(program.constructs.len(), 1);
        match &program.constructs[0] {
            Construct::Shape(s) => {
                assert_eq!(s.name, "Booking");
                assert_eq!(s.fields.len(), 5);
                assert_eq!(s.fields[0].name, "id");
                assert!(matches!(s.fields[0].ty, TypeExpr::Uuid));
                assert_eq!(s.fields[2].name, "status");
                assert!(matches!(&s.fields[2].ty, TypeExpr::Enum(v) if v.len() == 3));
                assert!(matches!(&s.fields[4].ty, TypeExpr::Maybe(_)));
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn test_parse_source() {
        let input = r#"SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX user_id created_at DESC
  INDEX listing_id check_in check_out
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Source(s) => {
                assert_eq!(s.name, "bookings");
                assert_eq!(s.source_type, SourceType::Postgres);
                assert_eq!(s.shape, "Booking");
                assert_eq!(s.indexes.len(), 2);
                assert_eq!(s.indexes[0].fields.len(), 2);
                assert_eq!(s.indexes[0].fields[0].name, "user_id");
                assert!(matches!(s.indexes[0].fields[1].suffix, Some(IndexSuffix::Desc)));
            }
            _ => panic!("expected Source"),
        }
    }

    #[test]
    fn test_parse_realm() {
        let input = r#"REALM booking_api
  TENANT user_id
  CAPABILITY read bookings
  CAPABILITY write bookings
  CAPABILITY call payments
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Realm(r) => {
                assert_eq!(r.name, "booking_api");
                assert_eq!(r.tenant, Some("user_id".to_string()));
                assert_eq!(r.capabilities.len(), 3);
                assert_eq!(r.capabilities[0].kind, CapabilityKind::Read);
                assert_eq!(r.capabilities[2].kind, CapabilityKind::Call);
            }
            _ => panic!("expected Realm"),
        }
    }

    #[test]
    fn test_parse_simple_flow() {
        let input = r#"FLOW get_booking get /bookings/:id
  AUTH session
  SCOPE TENANT auth.user_id
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  GUARD ownership 403 "not your booking"
    EQ booking.user_id auth.user_id
  RETURN 200 booking
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.name, "get_booking");
                assert_eq!(f.method, HttpMethod::Get);
                assert_eq!(f.path, "/bookings/:id");
                assert!(matches!(f.auth, Some(AuthDecl::Session)));
                assert!(matches!(&f.scope, Some(ScopeDecl::Tenant(_))));
                assert_eq!(f.steps.len(), 2); // LET + GUARD
                assert_eq!(f.return_stmt.code, 200);
                assert!(matches!(&f.return_stmt.body, Some(ReturnBody::Binding(s)) if s == "booking"));
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_flow_with_insert() {
        let input = r#"FLOW create_user post /users
  AUTH none
  BODY UserCreate
    name STRING 100 REQUIRED
    email STRING 255 REQUIRED
  INSERT users
    name body.name
    email body.email
  AS user
  RETURN 201 user
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert!(f.body.is_some());
                assert_eq!(f.body.as_ref().unwrap().fields.len(), 2);
                assert!(matches!(&f.steps[0], FlowStep::Insert(i) if i.source == "users"));
                assert!(matches!(&f.steps[0], FlowStep::Insert(i) if i.binding == Some("user".to_string())));
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_flow_with_let_arithmetic() {
        let input = r#"FLOW calc get /calc
  AUTH none
  LET nights
    DAYS_BETWEEN body.check_in body.check_out
  LET total
    MUL listing.price nights
  RETURN 200 total
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.steps.len(), 2);
                match &f.steps[0] {
                    FlowStep::Let(l) => {
                        assert_eq!(l.name, "nights");
                        assert!(matches!(&l.expr, Expr::Binary { op: BinaryOp::DaysBetween, .. }));
                    }
                    _ => panic!("expected Let"),
                }
                match &f.steps[1] {
                    FlowStep::Let(l) => {
                        assert_eq!(l.name, "total");
                        assert!(matches!(&l.expr, Expr::Binary { op: BinaryOp::Mul, .. }));
                    }
                    _ => panic!("expected Let"),
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_multiple_constructs() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM user_api
  CAPABILITY read users

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let program = parse(input).unwrap();
        assert_eq!(program.constructs.len(), 4);
        assert!(matches!(&program.constructs[0], Construct::Shape(_)));
        assert!(matches!(&program.constructs[1], Construct::Source(_)));
        assert!(matches!(&program.constructs[2], Construct::Realm(_)));
        assert!(matches!(&program.constructs[3], Construct::Flow(_)));
    }

    #[test]
    fn test_parse_policy() {
        let input = r#"POLICY require_rate_limit_on_writes
  APPLIES_TO FLOW WHERE METHOD IN post put patch delete
  REQUIRE LIMIT
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Policy(p) => {
                assert_eq!(p.name, "require_rate_limit_on_writes");
                assert_eq!(p.applies_to.filters.len(), 1);
                assert!(matches!(&p.applies_to.filters[0], PolicyFilter::MethodIn(m) if m.len() == 4));
                assert_eq!(p.requires.len(), 1);
                assert!(matches!(&p.requires[0], RequireClause::Limit));
            }
            _ => panic!("expected Policy"),
        }
    }

    #[test]
    fn test_parse_policy_writes_filter() {
        let input = r#"POLICY require_fraud_check
  APPLIES_TO FLOW WHERE WRITES bookings
  REQUIRE RULE fraud_score_ok
  REQUIRE AUTH
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Policy(p) => {
                assert_eq!(p.applies_to.filters.len(), 1);
                assert!(matches!(&p.applies_to.filters[0], PolicyFilter::Writes(s) if s == "bookings"));
                assert_eq!(p.requires.len(), 2);
                assert!(matches!(&p.requires[0], RequireClause::Rule(r) if r == "fraud_score_ok"));
                assert!(matches!(&p.requires[1], RequireClause::Auth(None)));
            }
            _ => panic!("expected Policy"),
        }
    }

    #[test]
    fn test_parse_auth_webhook_signature() {
        let input = r#"FLOW webhook_handler post /webhooks/stripe
  AUTH WEBHOOK SIGNATURE stripe_secret HMAC sha256
  RETURN 200
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                match &f.auth {
                    Some(AuthDecl::WebhookSignature { secret, algorithm }) => {
                        assert_eq!(secret, "stripe_secret");
                        assert_eq!(algorithm, "sha256");
                    }
                    other => panic!("expected WebhookSignature, got {other:?}"),
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_policy_path_starts_with() {
        let input = r#"POLICY admin_only
  APPLIES_TO FLOW WHERE PATH STARTS_WITH "/admin"
  REQUIRE AUTH ROLE admin
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Policy(p) => {
                assert_eq!(p.applies_to.filters.len(), 1);
                assert!(matches!(&p.applies_to.filters[0], PolicyFilter::PathStartsWith(s) if s == "/admin"));
                assert_eq!(p.requires.len(), 1);
            }
            _ => panic!("expected Policy"),
        }
    }

    #[test]
    fn test_parse_wasm_call() {
        let input = r#"FLOW hash_content post /hash
  BODY HashRequest
    data STRING 1000 REQUIRED
  LET result
    CALL wasm sha256:abc123def content
  RETURN 200 result
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.steps.len(), 1);
                if let FlowStep::Let(l) = &f.steps[0] {
                    assert_eq!(l.name, "result");
                    if let Expr::WasmCall { hash, inputs } = &l.expr {
                        assert_eq!(hash, "sha256:abc123def");
                        assert_eq!(inputs, &["content"]);
                    } else {
                        panic!("expected WasmCall expr");
                    }
                } else {
                    panic!("expected Let step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_stream_websocket() {
        let input = r#"STREAM notifications ws "/ws/notifications"
  REALM core
  AUTH bearer
  EVENT new_message
    id UUID
    body STRING 1000
  EVENT user_joined
    user_id UUID
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Stream(s) => {
                assert_eq!(s.name, "notifications");
                assert!(matches!(s.transport, StreamTransport::WebSocket));
                assert_eq!(s.path, "/ws/notifications");
                assert_eq!(s.realm.as_deref(), Some("core"));
                assert!(matches!(s.auth, Some(AuthDecl::Bearer)));
                assert_eq!(s.events.len(), 2);
                assert_eq!(s.events[0].name, "new_message");
                assert_eq!(s.events[0].fields.len(), 2);
                assert_eq!(s.events[1].name, "user_joined");
                assert_eq!(s.events[1].fields.len(), 1);
            }
            _ => panic!("expected Stream"),
        }
    }

    #[test]
    fn test_parse_stream_sse() {
        let input = r#"STREAM updates sse "/events/updates"
  EVENT price_change
    symbol STRING 10
    price DECIMAL
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Stream(s) => {
                assert_eq!(s.name, "updates");
                assert!(matches!(s.transport, StreamTransport::Sse));
                assert_eq!(s.path, "/events/updates");
                assert!(s.realm.is_none());
                assert!(s.auth.is_none());
                assert_eq!(s.events.len(), 1);
            }
            _ => panic!("expected Stream"),
        }
    }

    #[test]
    fn test_parse_service() {
        let input = r#"SERVICE payments
  ENDPOINT stripe
  AUTH bearer VAULT stripe_api_key
  METHOD hold
    INPUT amount DECIMAL currency STRING 10
    OUTPUT hold_id STRING 100 status STRING 20
    TIMEOUT 30 s
    RETRY 3 BACKOFF exponential
  METHOD refund
    INPUT transaction_id STRING 100 amount MAYBE DECIMAL
    OUTPUT refund_id STRING 100
    TIMEOUT 30 s
    RETRY 2 BACKOFF exponential
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Service(s) => {
                assert_eq!(s.name, "payments");
                assert_eq!(s.endpoint, "stripe");
                assert_eq!(s.auth_type, "bearer");
                assert_eq!(s.vault_key, "stripe_api_key");
                assert_eq!(s.methods.len(), 2);
                assert_eq!(s.methods[0].name, "hold");
                assert_eq!(s.methods[0].inputs.len(), 2);
                assert_eq!(s.methods[0].outputs.len(), 2);
                assert!(s.methods[0].timeout.is_some());
                assert_eq!(s.methods[0].timeout.as_ref().unwrap().value, 30);
                assert_eq!(s.methods[0].timeout.as_ref().unwrap().unit, DurationUnit::Seconds);
                assert!(s.methods[0].retry.is_some());
                assert_eq!(s.methods[0].retry.as_ref().unwrap().count, 3);
                assert!(matches!(s.methods[0].retry.as_ref().unwrap().strategy, RetryStrategy::Exponential));
                assert_eq!(s.methods[1].name, "refund");
            }
            _ => panic!("expected Service"),
        }
    }

    #[test]
    fn test_parse_service_with_cache() {
        let input = r#"SERVICE geocoding
  ENDPOINT google_maps
  AUTH query_param VAULT google_maps_key
  METHOD reverse
    INPUT lat DECIMAL lng DECIMAL
    OUTPUT address STRING 255 city STRING 100 country STRING 100
    TIMEOUT 5 s
    RETRY 2 BACKOFF linear
    CACHE 86400
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Service(s) => {
                assert_eq!(s.methods[0].cache_ttl, Some(86400));
            }
            _ => panic!("expected Service"),
        }
    }

    #[test]
    fn test_parse_surface() {
        let input = r#"SURFACE public v1
  REALM booking_api
  BASE_PATH /api/v1
  ROUTE GET /bookings -> list_bookings
  ROUTE POST /bookings -> create_booking
  ROUTE GET /bookings/:id -> get_booking
  EXPOSE Booking AS BookingResponse
    FIELD id UUID
    FIELD status STRING 20
    HIDE total_price
    HIDE user_id
    RENAME note AS special_requests
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Surface(s) => {
                assert_eq!(s.name, "public");
                assert_eq!(s.version, "v1");
                assert_eq!(s.realm, Some("booking_api".into()));
                assert_eq!(s.base_path, Some("/api/v1".into()));
                assert_eq!(s.routes.len(), 3);
                assert_eq!(s.routes[0].method, HttpMethod::Get);
                assert_eq!(s.routes[0].path, "/bookings");
                assert_eq!(s.routes[0].target, "list_bookings");
                assert_eq!(s.routes[1].method, HttpMethod::Post);
                assert_eq!(s.exposes.len(), 1);
                assert_eq!(s.exposes[0].shape, "Booking");
                assert_eq!(s.exposes[0].alias, Some("BookingResponse".into()));
                assert_eq!(s.exposes[0].fields.len(), 5);
                assert!(matches!(&s.exposes[0].fields[0], ExposeField::Field { name, .. } if name == "id"));
                assert!(matches!(&s.exposes[0].fields[2], ExposeField::Hide(f) if f == "total_price"));
                assert!(matches!(&s.exposes[0].fields[4], ExposeField::Rename { from, to } if from == "note" && to == "special_requests"));
            }
            _ => panic!("expected Surface"),
        }
    }

    #[test]
    fn test_parse_surface_with_deprecate() {
        let input = r#"SURFACE public v2
  REALM booking_api
  BASE_PATH /api/v2
  ROUTE GET /bookings -> list_bookings_v2
  DEPRECATE v1 SUNSET "2027-06-01"
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Surface(s) => {
                assert!(s.deprecate.is_some());
                let d = s.deprecate.as_ref().unwrap();
                assert_eq!(d.version, "v1");
                assert_eq!(d.sunset, "2027-06-01");
            }
            _ => panic!("expected Surface"),
        }
    }

    #[test]
    fn test_parse_migrate() {
        let input = r#"MIGRATE Booking v1 TO v2
  COPY id user_id listing_id check_in check_out status created_at
  DROP nights
  DROP price_per_night
  ADD cancellation_policy ENUM flexible moderate strict DEFAULT flexible
  ADD updated_at TIMESTAMP DEFAULT NOW
  RENAME note TO special_requests
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Migrate(m) => {
                assert_eq!(m.shape, "Booking");
                assert_eq!(m.from_version, "v1");
                assert_eq!(m.to_version, "v2");
                assert_eq!(m.ops.len(), 6);
                assert!(matches!(&m.ops[0], MigrateOp::Copy(fields) if fields.len() == 7));
                assert!(matches!(&m.ops[1], MigrateOp::Drop(f) if f == "nights"));
                assert!(matches!(&m.ops[2], MigrateOp::Drop(f) if f == "price_per_night"));
                assert!(matches!(&m.ops[3], MigrateOp::Add(f) if f.name == "cancellation_policy"));
                assert!(matches!(&m.ops[4], MigrateOp::Add(f) if f.name == "updated_at"));
                assert!(matches!(&m.ops[5], MigrateOp::Rename { from, to } if from == "note" && to == "special_requests"));
            }
            _ => panic!("expected Migrate"),
        }
    }

    #[test]
    fn test_parse_migrate_with_compute() {
        let input = r#"MIGRATE Booking v1 TO v2
  COPY id user_id
  COMPUTE total_price
    MUL v1.nights v1.price_per_night
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Migrate(m) => {
                assert_eq!(m.ops.len(), 2);
                assert!(matches!(&m.ops[0], MigrateOp::Copy(f) if f.len() == 2));
                assert!(matches!(&m.ops[1], MigrateOp::Compute { field, .. } if field == "total_price"));
            }
            _ => panic!("expected Migrate"),
        }
    }

    #[test]
    fn test_parse_saga() {
        let input = r#"SAGA process_order POST /orders
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
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Saga(s) => {
                assert_eq!(s.name, "process_order");
                assert_eq!(s.method, HttpMethod::Post);
                assert_eq!(s.path, "/orders");
                assert!(matches!(s.auth, Some(AuthDecl::Session)));
                assert_eq!(s.steps.len(), 2);
                assert_eq!(s.steps[0].name, "verify_stock");
                assert_eq!(s.steps[0].flow_steps.len(), 1);
                assert!(s.steps[0].verify.is_some());
                assert_eq!(s.steps[0].yields, vec!["item"]);
                assert!(matches!(s.steps[0].compensate, Compensate::None));
                assert_eq!(s.steps[1].name, "create_order");
                assert_eq!(s.steps[1].yields, vec!["order"]);
                assert!(matches!(&s.steps[1].compensate, Compensate::Steps(steps) if steps.len() == 1));
                assert!(s.on_failure.run_compensations);
                assert_eq!(s.on_success.effects.len(), 1);
                assert_eq!(s.on_success.return_stmt.code, 201);
            }
            _ => panic!("expected Saga"),
        }
    }

    #[test]
    fn test_parse_flow_with_cache() {
        let input = "\
FLOW get_listing get /listings/:id
  CACHE 60 VARY path.id
  CACHE 300 VARY path.id auth.user_id
  RETURN 200 listing
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.cache.len(), 2);
                assert_eq!(f.cache[0].ttl, 60);
                assert_eq!(f.cache[0].vary.len(), 1);
                assert_eq!(f.cache[0].vary[0].as_str(), "path.id");
                assert_eq!(f.cache[1].ttl, 300);
                assert_eq!(f.cache[1].vary.len(), 2);
                assert_eq!(f.cache[1].vary[0].as_str(), "path.id");
                assert_eq!(f.cache[1].vary[1].as_str(), "auth.user_id");
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_return_headers() {
        let input = r#"FLOW create_booking post /bookings
  RETURN 201 booking
    HEADER Location CONCAT "/bookings/" booking.id
    HEADER X-Idempotency-Key header.idempotency_key
"#;
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.return_stmt.code, 201);
                assert!(matches!(&f.return_stmt.body, Some(ReturnBody::Binding(n)) if n == "booking"));
                assert_eq!(f.return_stmt.headers.len(), 2);
                assert_eq!(f.return_stmt.headers[0].0, "Location");
                assert_eq!(f.return_stmt.headers[1].0, "X-Idempotency-Key");
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_index_suffixes() {
        let input = "\
SHAPE Location
  lat DECIMAL
  lng DECIMAL
  title STRING
  tags LIST STRING

SOURCE locations ELASTICSEARCH
  SHAPE Location
  INDEX lat GEO
  INDEX title TEXT
  INDEX tags KEYWORD
";
        let program = parse(input).unwrap();
        match &program.constructs[1] {
            Construct::Source(s) => {
                assert_eq!(s.indexes.len(), 3);
                assert!(matches!(s.indexes[0].fields[0].suffix, Some(IndexSuffix::Geo)));
                assert!(matches!(s.indexes[1].fields[0].suffix, Some(IndexSuffix::Text)));
                assert!(matches!(s.indexes[2].fields[0].suffix, Some(IndexSuffix::Keyword)));
            }
            _ => panic!("expected Source"),
        }
    }

    #[test]
    fn test_parse_return_inline() {
        let input = "\
FLOW get_booking get /bookings/:id
  RETURN 200
    id booking.id
    status booking.status
    check_in booking.check_in
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.return_stmt.code, 200);
                match &f.return_stmt.body {
                    Some(ReturnBody::Inline(fields)) => {
                        assert_eq!(fields.len(), 3);
                        assert_eq!(fields[0].name, "id");
                        assert_eq!(fields[1].name, "status");
                        assert_eq!(fields[2].name, "check_in");
                    }
                    other => panic!("expected Inline, got {:?}", other),
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_return_paginated() {
        let input = "\
FLOW list_bookings get /bookings
  RETURN 200
    ITEMS results.items
    TOTAL results.total
    CURSOR results.next_cursor
    HAS_MORE results.has_more
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.return_stmt.code, 200);
                assert!(matches!(&f.return_stmt.body, Some(ReturnBody::Paginated { .. })));
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_inline_cache() {
        let input = "\
FLOW list_popular get /popular
  LET popular
    CACHE 300
      QUERY listings
        FILTER status EQ active
        SORT booking_count DESC
        PAGE_SIZE 10
  RETURN 200 popular
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.steps.len(), 1);
                if let FlowStep::Let(l) = &f.steps[0] {
                    assert_eq!(l.name, "popular");
                    match &l.expr {
                        Expr::Cached { ttl, expr } => {
                            assert_eq!(*ttl, 300);
                            assert!(matches!(expr.as_ref(), Expr::Query { source, .. } if source == "listings"));
                        }
                        other => panic!("expected Cached, got {:?}", other),
                    }
                } else {
                    panic!("expected Let step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_match() {
        let input = "\
FLOW cancel_booking post /bookings/:id/cancel
  LET hours_until
    HOURS_BETWEEN NOW booking.check_in
  MATCH
    WHEN GTE hours_until 48
      LET refund
        MUL booking.total 1
    WHEN GTE hours_until 24
      LET refund
        MUL booking.total 0
    DEFAULT
      LET refund
        MUL booking.total 0
  RETURN 200 refund
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.steps.len(), 2);
                if let FlowStep::Match(m) = &f.steps[1] {
                    assert_eq!(m.branches.len(), 2);
                    assert!(m.default.is_some());
                    assert_eq!(m.default.as_ref().unwrap().len(), 1);
                } else {
                    panic!("expected Match step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_return_nested() {
        let input = "\
FLOW get_booking get /bookings/:id
  RETURN 201
    id booking.id
    status booking.status
    listing
      id listing.id
      title listing.title
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.return_stmt.code, 201);
                match &f.return_stmt.body {
                    Some(ReturnBody::Inline(fields)) => {
                        assert_eq!(fields.len(), 3);
                        assert_eq!(fields[0].name, "id");
                        assert_eq!(fields[1].name, "status");
                        assert_eq!(fields[2].name, "listing");
                        match &fields[2].value {
                            ReturnValue::Nested(sub) => {
                                assert_eq!(sub.len(), 2);
                                assert_eq!(sub[0].name, "id");
                                assert_eq!(sub[1].name, "title");
                            }
                            other => panic!("expected Nested, got {:?}", other),
                        }
                    }
                    other => panic!("expected Inline, got {:?}", other),
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_set_step() {
        let input = "\
FLOW update_counter post /counter
  LET counter
    FETCH counters
      FILTER id EQ path.id
    OR 404
  SET counter
    ADD counter.value 1
  RETURN 200 counter
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.steps.len(), 2);
                if let FlowStep::Set(s) = &f.steps[1] {
                    assert_eq!(s.name, "counter");
                    assert!(matches!(s.expr, Expr::Binary { .. }));
                } else {
                    panic!("expected Set step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_each_step() {
        let input = "\
FLOW notify_all post /notify
  LET users
    QUERY active_users
  EACH user IN users
    EFFECT email
      TEMPLATE welcome
      TO user.email
  RETURN 200
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.steps.len(), 2);
                if let FlowStep::Each(e) = &f.steps[1] {
                    assert_eq!(e.binding, "user");
                    assert_eq!(e.steps.len(), 1);
                    assert!(matches!(e.steps[0], FlowStep::Effect(_)));
                } else {
                    panic!("expected Each step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_try_recover() {
        let input = "\
FLOW safe_transfer post /transfer
  TRY
    LET result
      CALL payment.charge
        amount body.amount
  RECOVER
    EFFECT webhook
      TEMPLATE transfer_failed
      TO body.callback
  RETURN 200 result
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.steps.len(), 1);
                if let FlowStep::Try(t) = &f.steps[0] {
                    assert_eq!(t.body.len(), 1);
                    assert!(matches!(t.body[0], FlowStep::Let(_)));
                    assert_eq!(t.recover.len(), 1);
                    assert!(matches!(t.recover[0], FlowStep::Effect(_)));
                } else {
                    panic!("expected Try step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_func() {
        let input = "\
FUNC compute_tax
  INPUT amount DECIMAL
  INPUT rate DECIMAL
  OUTPUT DECIMAL
  LET tax
    MUL amount rate
  RETURN tax
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Func(f) => {
                assert_eq!(f.name, "compute_tax");
                assert_eq!(f.inputs.len(), 2);
                assert_eq!(f.inputs[0].name, "amount");
                assert_eq!(f.inputs[1].name, "rate");
                assert!(matches!(f.output, TypeExpr::Decimal { .. }));
                assert_eq!(f.steps.len(), 1);
            }
            _ => panic!("expected Func"),
        }
    }

    #[test]
    fn test_parse_map_expr() {
        let input = "\
FLOW get_names get /names
  LET names
    SELECT users id name
  RETURN 200 names
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                if let FlowStep::Let(l) = &f.steps[0] {
                    if let Expr::MapExpr { fields, .. } = &l.expr {
                        assert_eq!(fields, &["id", "name"]);
                    } else {
                        panic!("expected MapExpr");
                    }
                } else {
                    panic!("expected Let step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_reduce_expr() {
        let input = "\
FLOW get_total get /total
  LET total
    REDUCE orders SUM amount
  RETURN 200 total
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                if let FlowStep::Let(l) = &f.steps[0] {
                    if let Expr::ReduceExpr { op, field, .. } = &l.expr {
                        assert_eq!(*op, AggregateOp::Sum);
                        assert_eq!(field, "amount");
                    } else {
                        panic!("expected ReduceExpr");
                    }
                } else {
                    panic!("expected Let step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_split_replace_format() {
        let input = "\
FLOW process post /process
  LET parts
    SPLIT body.input \",\"
  LET cleaned
    REPLACE body.text \"old\" \"new\"
  LET msg
    FORMAT \"Hello {0}, welcome to {1}\" body.name body.site
  RETURN 200 msg
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                assert_eq!(f.steps.len(), 3);
                assert!(matches!(&f.steps[0], FlowStep::Let(l) if matches!(l.expr, Expr::SplitExpr { .. })));
                assert!(matches!(&f.steps[1], FlowStep::Let(l) if matches!(l.expr, Expr::ReplaceExpr { .. })));
                if let FlowStep::Let(l) = &f.steps[2] {
                    if let Expr::FormatExpr { template, args } = &l.expr {
                        assert_eq!(template, "Hello {0}, welcome to {1}");
                        assert_eq!(args.len(), 2);
                    } else {
                        panic!("expected FormatExpr");
                    }
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_func_call_expr() {
        let input = "\
FLOW calc get /calc
  LET result
    FUNC compute_tax body.amount body.rate
  RETURN 200 result
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                if let FlowStep::Let(l) = &f.steps[0] {
                    if let Expr::FuncCall { name, args } = &l.expr {
                        assert_eq!(name, "compute_tax");
                        assert_eq!(args.len(), 2);
                    } else {
                        panic!("expected FuncCall");
                    }
                } else {
                    panic!("expected Let step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_if_else_expr() {
        let input = "\
FLOW check get /check/:id
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  LET status
    IF user.active
      THEN \"active\"
      ELSE \"inactive\"
  RETURN 200 status
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                if let FlowStep::Let(l) = &f.steps[1] {
                    assert_eq!(l.name, "status");
                    assert!(matches!(l.expr, Expr::If { .. }));
                } else {
                    panic!("expected Let step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_each_parallel() {
        let input = "\
FLOW batch post /batch
  LET items
    QUERY items
  EACH item IN items PARALLEL 10
    EFFECT email
      TEMPLATE notify
      TO item.email
  RETURN 200
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                if let FlowStep::Each(e) = &f.steps[1] {
                    assert_eq!(e.binding, "item");
                    assert_eq!(e.parallel, Some(10));
                    assert_eq!(e.steps.len(), 1);
                } else {
                    panic!("expected Each step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_fetch_with() {
        let input = "\
FLOW get_user get /users/:id
  LET user
    FETCH users
      FILTER id EQ path.id
      WITH orders
      WITH profile
    OR 404
  RETURN 200 user
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                if let FlowStep::Let(l) = &f.steps[0] {
                    if let Expr::Fetch { with, .. } = &l.expr {
                        assert_eq!(with, &["orders", "profile"]);
                    } else {
                        panic!("expected Fetch expr");
                    }
                } else {
                    panic!("expected Let step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_body_multipart() {
        let input = "\
FLOW upload post /upload
  BODY MULTIPART UploadInput
    file BLOB
    label STRING 100
  RETURN 200
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Flow(f) => {
                let body = f.body.as_ref().unwrap();
                assert_eq!(body.kind, BodyKind::Multipart);
                assert_eq!(body.fields.len(), 2);
                assert!(matches!(body.fields[0].ty, TypeExpr::Blob));
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_stream_receive() {
        let input = "\
STREAM chat ws /ws/chat
  EVENT message
    text STRING 500
  RECEIVE
    ON message
      INSERT messages
        text body.text
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Stream(s) => {
                assert_eq!(s.events.len(), 1);
                assert_eq!(s.receivers.len(), 1);
                assert_eq!(s.receivers[0].event, "message");
                assert_eq!(s.receivers[0].steps.len(), 1);
            }
            _ => panic!("expected Stream"),
        }
    }

    #[test]
    fn test_parse_error_shape() {
        let input = "\
SHAPE ApiError
  code INT
  message STRING 500

FLOW get_user get /users/:id
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404 ApiError
      code 1001
      message \"User not found\"
  RETURN 200 user
";
        let program = parse(input).unwrap();
        match &program.constructs[1] {
            Construct::Flow(f) => {
                if let FlowStep::Let(l) = &f.steps[0] {
                    if let Expr::Fetch { or_shape, or_code, .. } = &l.expr {
                        assert_eq!(*or_code, 404);
                        let shape = or_shape.as_ref().unwrap();
                        assert_eq!(shape.shape, "ApiError");
                        assert_eq!(shape.fields.len(), 2);
                        assert_eq!(shape.fields[0].0, "code");
                        assert_eq!(shape.fields[1].0, "message");
                    } else {
                        panic!("expected Fetch expr");
                    }
                } else {
                    panic!("expected Let step");
                }
            }
            _ => panic!("expected Flow"),
        }
    }

    #[test]
    fn test_parse_func_with_mutations() {
        let input = "\
FUNC process_order
  INPUT order_id UUID
  OUTPUT BOOL
  LET order
    FETCH orders
      FILTER id EQ order_id
    OR 404
  UPDATE orders
    WHERE id EQ order_id
    SET status \"processed\"
  EFFECT email
    TEMPLATE order_processed
    TO order.email
  RETURN order.id
";
        let program = parse(input).unwrap();
        match &program.constructs[0] {
            Construct::Func(f) => {
                assert_eq!(f.name, "process_order");
                assert_eq!(f.steps.len(), 3);
                assert!(matches!(f.steps[0], FlowStep::Let(_)));
                assert!(matches!(f.steps[1], FlowStep::Update(_)));
                assert!(matches!(f.steps[2], FlowStep::Effect(_)));
            }
            _ => panic!("expected Func"),
        }
    }
}
