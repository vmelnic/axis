use crate::token::TokenKind;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    Exactly(Vec<TokenKind>),
    AnyIdent,
    AnyInt,
    AnyDecimal,
    AnyString,
    AnyOf(Vec<Constraint>),
}

impl Constraint {
    pub fn allows(&self, token: &TokenKind) -> bool {
        match self {
            Constraint::Exactly(kinds) => kinds.contains(token),
            Constraint::AnyIdent => matches!(token, TokenKind::Ident(_)),
            Constraint::AnyInt => matches!(token, TokenKind::IntLit(_)),
            Constraint::AnyDecimal => matches!(token, TokenKind::DecimalLit(_)),
            Constraint::AnyString => matches!(token, TokenKind::StringLit(_)),
            Constraint::AnyOf(constraints) => constraints.iter().any(|c| c.allows(token)),
        }
    }

    pub fn token_names(&self) -> Vec<String> {
        match self {
            Constraint::Exactly(kinds) => kinds.iter().map(|k| format!("{}", k)).collect(),
            Constraint::AnyIdent => vec!["<identifier>".into()],
            Constraint::AnyInt => vec!["<integer>".into()],
            Constraint::AnyDecimal => vec!["<decimal>".into()],
            Constraint::AnyString => vec!["<string>".into()],
            Constraint::AnyOf(cs) => cs.iter().flat_map(|c| c.token_names()).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseState {
    TopLevel,
    ShapeBody,
    ShapeFieldType,
    ShapeFieldModifiers,
    SourceHeader,
    SourceBody,
    SourceIndexFields,
    RealmBody,
    RealmCapability,
    PolicyBody,
    PolicyFilter,
    RequireClause,
    ServiceBody,
    ServiceMethodBody,
    FlowHeader,
    FlowBody,
    FlowAuthKind,
    FlowBodyField,
    FlowParamType,
    FlowStepStart,
    FilterClause,
    FilterOp,
    InlineExpr,
    BlockExpr,
    InsertField,
    UpsertBody,
    FanoutBody,
    ReturnStmt,
    SagaHeader,
    SagaBody,
    SagaStepBody,
    SurfaceBody,
    ExposeBody,
    DeprecateClause,
    MigrateBody,
    UpdateBody,
    DeleteBody,
    WhereClause,
    SetClause,
    MatchBody,
    WhenBranch,
    EffectBody,
    StreamBody,
}

pub fn valid_tokens(state: &ParseState) -> Constraint {
    match state {
        ParseState::TopLevel => Constraint::Exactly(vec![
            TokenKind::Shape,
            TokenKind::Source,
            TokenKind::Realm,
            TokenKind::Policy,
            TokenKind::Service,
            TokenKind::Flow,
            TokenKind::Saga,
            TokenKind::Surface,
            TokenKind::Migrate,
            TokenKind::Stream,
            TokenKind::Eof,
        ]),

        ParseState::ShapeBody => Constraint::AnyOf(vec![
            Constraint::AnyIdent,
            Constraint::Exactly(vec![TokenKind::Dedent]),
        ]),

        ParseState::ShapeFieldType => Constraint::Exactly(vec![
            TokenKind::Uuid,
            TokenKind::Bool,
            TokenKind::Date,
            TokenKind::Timestamp,
            TokenKind::Text,
            TokenKind::String_,
            TokenKind::Int,
            TokenKind::Decimal,
            TokenKind::Enum,
            TokenKind::Json,
            TokenKind::List,
            TokenKind::Map,
            TokenKind::Maybe,
        ]),

        ParseState::ShapeFieldModifiers => Constraint::Exactly(vec![
            TokenKind::Pk,
            TokenKind::Auto,
            TokenKind::Required,
            TokenKind::Unique,
            TokenKind::Default,
            TokenKind::Precision,
            TokenKind::Scale,
            TokenKind::Min,
            TokenKind::Max,
            TokenKind::Ref,
            TokenKind::Newline,
        ]),

        ParseState::SourceHeader => Constraint::Exactly(vec![
            TokenKind::Postgres,
            TokenKind::Mysql,
            TokenKind::Redis,
            TokenKind::Elasticsearch,
            TokenKind::Dynamodb,
        ]),

        ParseState::SourceBody => Constraint::Exactly(vec![
            TokenKind::Shape,
            TokenKind::Index,
            TokenKind::Ttl,
            TokenKind::Dedent,
        ]),

        ParseState::SourceIndexFields => Constraint::AnyOf(vec![
            Constraint::AnyIdent,
            Constraint::Exactly(vec![
                TokenKind::Asc,
                TokenKind::Desc,
                TokenKind::Unique,
                TokenKind::Text,
                TokenKind::Newline,
            ]),
        ]),

        ParseState::RealmBody => Constraint::Exactly(vec![
            TokenKind::Tenant,
            TokenKind::Capability,
            TokenKind::Dedent,
        ]),

        ParseState::RealmCapability => Constraint::AnyIdent,

        ParseState::PolicyBody => Constraint::Exactly(vec![
            TokenKind::AppliesTo,
            TokenKind::Require,
            TokenKind::Dedent,
        ]),

        ParseState::PolicyFilter => Constraint::AnyOf(vec![
            Constraint::Exactly(vec![
                TokenKind::Method,
                TokenKind::Reads,
                TokenKind::Writes,
                TokenKind::Not,
                TokenKind::Any,
            ]),
            Constraint::AnyIdent,
        ]),

        ParseState::RequireClause => Constraint::Exactly(vec![
            TokenKind::Auth,
            TokenKind::Limit,
            TokenKind::Scope,
            TokenKind::Rule,
            TokenKind::Guard,
            TokenKind::Idempotency,
            TokenKind::Fanout,
        ]),

        ParseState::ServiceBody => Constraint::Exactly(vec![
            TokenKind::Endpoint,
            TokenKind::Auth,
            TokenKind::Method,
            TokenKind::Dedent,
        ]),

        ParseState::ServiceMethodBody => Constraint::Exactly(vec![
            TokenKind::Pure,
            TokenKind::Idempotency,
            TokenKind::Input,
            TokenKind::Output,
            TokenKind::Timeout,
            TokenKind::Retry,
            TokenKind::Cache,
            TokenKind::Dedent,
        ]),

        ParseState::FlowHeader => Constraint::AnyOf(vec![Constraint::AnyIdent]),

        ParseState::FlowBody => Constraint::Exactly(vec![
            TokenKind::Realm,
            TokenKind::Auth,
            TokenKind::Scope,
            TokenKind::Limit,
            TokenKind::Cache,
            TokenKind::Timeout,
            TokenKind::Body,
            TokenKind::Param,
            TokenKind::Header,
            TokenKind::Idempotency,
            TokenKind::Rule,
            TokenKind::Guard,
            TokenKind::Let,
            TokenKind::Insert,
            TokenKind::Upsert,
            TokenKind::Update,
            TokenKind::Delete,
            TokenKind::Fanout,
            TokenKind::Effect,
            TokenKind::Match,
            TokenKind::Set,
            TokenKind::Each,
            TokenKind::Try,
            TokenKind::Upload,
            TokenKind::Return,
        ]),

        ParseState::FlowAuthKind => Constraint::AnyOf(vec![
            Constraint::Exactly(vec![TokenKind::None_]),
            Constraint::AnyIdent,
        ]),

        ParseState::FlowBodyField => Constraint::AnyOf(vec![
            Constraint::AnyIdent,
            Constraint::Exactly(vec![TokenKind::Dedent]),
        ]),

        ParseState::FlowParamType => Constraint::Exactly(vec![
            TokenKind::Uuid,
            TokenKind::Bool,
            TokenKind::String_,
            TokenKind::Int,
            TokenKind::Decimal,
            TokenKind::Enum,
            TokenKind::Maybe,
        ]),

        ParseState::FlowStepStart => Constraint::Exactly(vec![
            TokenKind::Rule,
            TokenKind::Guard,
            TokenKind::Let,
            TokenKind::Insert,
            TokenKind::Upsert,
            TokenKind::Update,
            TokenKind::Delete,
            TokenKind::Fanout,
            TokenKind::Effect,
            TokenKind::Match,
            TokenKind::Set,
            TokenKind::Each,
            TokenKind::Try,
            TokenKind::Upload,
            TokenKind::Return,
            TokenKind::Dedent,
        ]),

        ParseState::FilterClause => Constraint::Exactly(vec![
            TokenKind::Filter,
            TokenKind::Sort,
            TokenKind::Cursor,
            TokenKind::PageSize,
            TokenKind::Cache,
            TokenKind::Dedent,
        ]),

        ParseState::FilterOp => Constraint::Exactly(vec![
            TokenKind::Eq,
            TokenKind::Neq,
            TokenKind::Gt,
            TokenKind::Gte,
            TokenKind::Lt,
            TokenKind::Lte,
            TokenKind::In,
            TokenKind::Between,
            TokenKind::Like,
            TokenKind::StartsWith,
            TokenKind::Contains,
        ]),

        ParseState::InlineExpr => Constraint::AnyOf(vec![
            Constraint::AnyIdent,
            Constraint::AnyInt,
            Constraint::AnyDecimal,
            Constraint::AnyString,
            Constraint::Exactly(vec![
                TokenKind::True_,
                TokenKind::False_,
                TokenKind::None_,
                TokenKind::Now,
            ]),
        ]),

        ParseState::BlockExpr => Constraint::AnyOf(vec![
            Constraint::Exactly(vec![
                TokenKind::Cache,
                TokenKind::Fetch,
                TokenKind::Query,
                TokenKind::Call,
                TokenKind::Add,
                TokenKind::Sub,
                TokenKind::Mul,
                TokenKind::Div,
                TokenKind::Mod,
                TokenKind::And,
                TokenKind::Or,
                TokenKind::Eq,
                TokenKind::Neq,
                TokenKind::Gt,
                TokenKind::Gte,
                TokenKind::Lt,
                TokenKind::Lte,
                TokenKind::Not,
                TokenKind::Empty,
                TokenKind::Exists,
                TokenKind::Count,
                TokenKind::Sum,
                TokenKind::Avg,
                TokenKind::Min,
                TokenKind::Max,
                TokenKind::First,
                TokenKind::Last,
                TokenKind::If,
                TokenKind::DaysBetween,
                TokenKind::HoursBetween,
                TokenKind::MinutesBetween,
                TokenKind::NowMinus,
                TokenKind::NowPlus,
                TokenKind::Concat,
                TokenKind::Lower,
                TokenKind::Upper,
                TokenKind::Trim,
                TokenKind::Length,
                TokenKind::Substring,
                TokenKind::StartsWith,
                TokenKind::EndsWith,
                TokenKind::Contains,
                TokenKind::FormatDate,
                TokenKind::Ceil,
                TokenKind::Floor,
                TokenKind::Abs,
                TokenKind::ToInt,
                TokenKind::ToDecimal,
                TokenKind::ToString_,
                TokenKind::Coalesce,
                TokenKind::Round,
            ]),
            Constraint::AnyIdent,
            Constraint::AnyInt,
            Constraint::AnyDecimal,
            Constraint::AnyString,
        ]),

        ParseState::InsertField => Constraint::AnyOf(vec![
            Constraint::AnyIdent,
            Constraint::Exactly(vec![TokenKind::As, TokenKind::Dedent]),
        ]),

        ParseState::UpsertBody => Constraint::AnyOf(vec![Constraint::Exactly(vec![
            TokenKind::Key,
            TokenKind::Set,
            TokenKind::As,
            TokenKind::Dedent,
        ])]),

        ParseState::FanoutBody => Constraint::Exactly(vec![TokenKind::Insert, TokenKind::Dedent]),

        ParseState::ReturnStmt => Constraint::AnyOf(vec![Constraint::AnyInt]),

        ParseState::SagaHeader => Constraint::AnyOf(vec![Constraint::AnyIdent]),

        ParseState::SagaBody => Constraint::Exactly(vec![
            TokenKind::Realm,
            TokenKind::Auth,
            TokenKind::Body,
            TokenKind::Step,
            TokenKind::OnFailure,
            TokenKind::OnSuccess,
            TokenKind::Dedent,
        ]),

        ParseState::SagaStepBody => Constraint::Exactly(vec![
            TokenKind::Let,
            TokenKind::Guard,
            TokenKind::Rule,
            TokenKind::Insert,
            TokenKind::Update,
            TokenKind::Delete,
            TokenKind::Call,
            TokenKind::Effect,
            TokenKind::Verify,
            TokenKind::Yield_,
            TokenKind::Compensate,
            TokenKind::Dedent,
        ]),

        ParseState::SurfaceBody => Constraint::Exactly(vec![
            TokenKind::Realm,
            TokenKind::BasePath,
            TokenKind::Route,
            TokenKind::Expose,
            TokenKind::Deprecate,
            TokenKind::Dedent,
        ]),

        ParseState::ExposeBody => Constraint::Exactly(vec![
            TokenKind::Field,
            TokenKind::Hide,
            TokenKind::Rename,
            TokenKind::Dedent,
        ]),

        ParseState::DeprecateClause => Constraint::AnyOf(vec![Constraint::AnyIdent]),

        ParseState::MigrateBody => Constraint::Exactly(vec![
            TokenKind::Copy,
            TokenKind::Compute,
            TokenKind::Drop,
            TokenKind::Add,
            TokenKind::Rename,
            TokenKind::Dedent,
        ]),

        ParseState::UpdateBody => {
            Constraint::Exactly(vec![TokenKind::Where, TokenKind::Set, TokenKind::Dedent])
        }

        ParseState::DeleteBody => Constraint::Exactly(vec![TokenKind::Where, TokenKind::Dedent]),

        ParseState::WhereClause => Constraint::AnyOf(vec![Constraint::AnyIdent]),

        ParseState::SetClause => Constraint::AnyOf(vec![Constraint::AnyIdent]),

        ParseState::MatchBody => {
            Constraint::Exactly(vec![TokenKind::When, TokenKind::Default, TokenKind::Dedent])
        }

        ParseState::WhenBranch => Constraint::Exactly(vec![
            TokenKind::Let,
            TokenKind::Guard,
            TokenKind::Insert,
            TokenKind::Update,
            TokenKind::Delete,
            TokenKind::Effect,
            TokenKind::Call,
            TokenKind::Dedent,
        ]),

        ParseState::EffectBody => Constraint::AnyOf(vec![
            Constraint::Exactly(vec![
                TokenKind::Template,
                TokenKind::To,
                TokenKind::Data,
                TokenKind::Task,
                TokenKind::Dedent,
            ]),
            Constraint::AnyIdent,
        ]),

        ParseState::StreamBody => Constraint::Exactly(vec![
            TokenKind::Realm,
            TokenKind::Auth,
            TokenKind::Event,
            TokenKind::Dedent,
        ]),
    }
}

pub fn next_states(state: &ParseState, token: &TokenKind) -> Vec<ParseState> {
    match state {
        ParseState::TopLevel => match token {
            TokenKind::Shape => vec![ParseState::ShapeBody],
            TokenKind::Source => vec![ParseState::SourceHeader],
            TokenKind::Realm => vec![ParseState::RealmBody],
            TokenKind::Policy => vec![ParseState::PolicyBody],
            TokenKind::Service => vec![ParseState::ServiceBody],
            TokenKind::Flow => vec![ParseState::FlowHeader],
            TokenKind::Saga => vec![ParseState::SagaHeader],
            TokenKind::Surface => vec![ParseState::SurfaceBody],
            TokenKind::Migrate => vec![ParseState::MigrateBody],
            TokenKind::Stream => vec![ParseState::StreamBody],
            _ => vec![ParseState::TopLevel],
        },
        ParseState::ShapeBody => match token {
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::ShapeFieldType],
        },
        ParseState::ShapeFieldType => vec![ParseState::ShapeFieldModifiers],
        ParseState::ShapeFieldModifiers => match token {
            TokenKind::Newline => vec![ParseState::ShapeBody],
            _ => vec![ParseState::ShapeFieldModifiers],
        },
        ParseState::SourceHeader => vec![ParseState::SourceBody],
        ParseState::SourceBody => match token {
            TokenKind::Index => vec![ParseState::SourceIndexFields],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::SourceBody],
        },
        ParseState::SourceIndexFields => match token {
            TokenKind::Newline => vec![ParseState::SourceBody],
            _ => vec![ParseState::SourceIndexFields],
        },
        ParseState::RealmBody => match token {
            TokenKind::Capability => vec![ParseState::RealmCapability],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::RealmBody],
        },
        ParseState::RealmCapability => vec![ParseState::RealmBody],
        ParseState::PolicyBody => match token {
            TokenKind::AppliesTo => vec![ParseState::PolicyFilter],
            TokenKind::Require => vec![ParseState::RequireClause],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::PolicyBody],
        },
        ParseState::PolicyFilter => vec![ParseState::PolicyBody],
        ParseState::RequireClause => vec![ParseState::PolicyBody],
        ParseState::ServiceBody => match token {
            TokenKind::Method => vec![ParseState::ServiceMethodBody],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::ServiceBody],
        },
        ParseState::ServiceMethodBody => match token {
            TokenKind::Dedent => vec![ParseState::ServiceBody],
            _ => vec![ParseState::ServiceMethodBody],
        },
        ParseState::FlowHeader => vec![ParseState::FlowBody],
        ParseState::FlowBody => match token {
            TokenKind::Auth => vec![ParseState::FlowAuthKind],
            TokenKind::Body => vec![ParseState::FlowBodyField],
            TokenKind::Param | TokenKind::Header => vec![ParseState::FlowParamType],
            TokenKind::Rule | TokenKind::Guard => vec![ParseState::BlockExpr],
            TokenKind::Let => vec![ParseState::BlockExpr],
            TokenKind::Insert => vec![ParseState::InsertField],
            TokenKind::Upsert => vec![ParseState::UpsertBody],
            TokenKind::Update => vec![ParseState::UpdateBody],
            TokenKind::Delete => vec![ParseState::DeleteBody],
            TokenKind::Match => vec![ParseState::MatchBody],
            TokenKind::Effect => vec![ParseState::EffectBody],
            TokenKind::Fanout => vec![ParseState::FanoutBody],
            TokenKind::Return => vec![ParseState::ReturnStmt],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::FlowBody],
        },
        ParseState::FlowAuthKind => vec![ParseState::FlowBody],
        ParseState::FlowBodyField => match token {
            TokenKind::Dedent => vec![ParseState::FlowBody],
            _ => vec![ParseState::ShapeFieldType],
        },
        ParseState::FlowParamType => vec![ParseState::ShapeFieldModifiers],
        ParseState::FlowStepStart => match token {
            TokenKind::Return => vec![ParseState::ReturnStmt],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::FlowBody],
        },
        ParseState::FilterClause => match token {
            TokenKind::Filter => vec![ParseState::FilterOp],
            TokenKind::Dedent => vec![ParseState::FlowBody],
            _ => vec![ParseState::FilterClause],
        },
        ParseState::FilterOp => vec![ParseState::InlineExpr],
        ParseState::InlineExpr => vec![ParseState::FlowBody],
        ParseState::BlockExpr => vec![ParseState::FlowBody],
        ParseState::InsertField => match token {
            TokenKind::Dedent | TokenKind::As => vec![ParseState::FlowBody],
            _ => vec![ParseState::InlineExpr],
        },
        ParseState::UpsertBody => match token {
            TokenKind::Dedent | TokenKind::As => vec![ParseState::FlowBody],
            _ => vec![ParseState::InlineExpr],
        },
        ParseState::FanoutBody => match token {
            TokenKind::Insert => vec![ParseState::InsertField],
            TokenKind::Dedent => vec![ParseState::FlowBody],
            _ => vec![ParseState::FanoutBody],
        },
        ParseState::ReturnStmt => vec![ParseState::TopLevel],
        ParseState::SagaHeader => vec![ParseState::SagaBody],
        ParseState::SagaBody => match token {
            TokenKind::Step => vec![ParseState::SagaStepBody],
            TokenKind::OnFailure => vec![ParseState::SagaBody],
            TokenKind::OnSuccess => vec![ParseState::SagaBody],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::SagaBody],
        },
        ParseState::SagaStepBody => match token {
            TokenKind::Compensate => vec![ParseState::SagaStepBody],
            TokenKind::Dedent => vec![ParseState::SagaBody],
            _ => vec![ParseState::SagaStepBody],
        },
        ParseState::SurfaceBody => match token {
            TokenKind::Expose => vec![ParseState::ExposeBody],
            TokenKind::Deprecate => vec![ParseState::DeprecateClause],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::SurfaceBody],
        },
        ParseState::ExposeBody => match token {
            TokenKind::Dedent => vec![ParseState::SurfaceBody],
            _ => vec![ParseState::ExposeBody],
        },
        ParseState::DeprecateClause => vec![ParseState::SurfaceBody],
        ParseState::UpdateBody => match token {
            TokenKind::Where => vec![ParseState::WhereClause],
            TokenKind::Set => vec![ParseState::SetClause],
            TokenKind::Dedent => vec![ParseState::FlowBody],
            _ => vec![ParseState::UpdateBody],
        },
        ParseState::DeleteBody => match token {
            TokenKind::Where => vec![ParseState::WhereClause],
            TokenKind::Dedent => vec![ParseState::FlowBody],
            _ => vec![ParseState::DeleteBody],
        },
        ParseState::WhereClause => vec![ParseState::BlockExpr],
        ParseState::SetClause => vec![ParseState::BlockExpr],
        ParseState::MatchBody => match token {
            TokenKind::When => vec![ParseState::WhenBranch],
            TokenKind::Default => vec![ParseState::WhenBranch],
            TokenKind::Dedent => vec![ParseState::FlowBody],
            _ => vec![ParseState::MatchBody],
        },
        ParseState::WhenBranch => match token {
            TokenKind::Dedent => vec![ParseState::MatchBody],
            TokenKind::Insert => vec![ParseState::InsertField],
            TokenKind::Update => vec![ParseState::UpdateBody],
            TokenKind::Delete => vec![ParseState::DeleteBody],
            TokenKind::Effect => vec![ParseState::EffectBody],
            _ => vec![ParseState::WhenBranch],
        },
        ParseState::EffectBody => match token {
            TokenKind::Dedent => vec![ParseState::FlowBody],
            _ => vec![ParseState::EffectBody],
        },
        ParseState::MigrateBody => match token {
            TokenKind::Compute => vec![ParseState::BlockExpr],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::MigrateBody],
        },
        ParseState::StreamBody => match token {
            TokenKind::Event => vec![ParseState::StreamBody],
            TokenKind::Dedent => vec![ParseState::TopLevel],
            _ => vec![ParseState::StreamBody],
        },
    }
}

const ALL_STATES: &[ParseState] = &[
    ParseState::TopLevel,
    ParseState::ShapeBody,
    ParseState::ShapeFieldType,
    ParseState::ShapeFieldModifiers,
    ParseState::SourceHeader,
    ParseState::SourceBody,
    ParseState::SourceIndexFields,
    ParseState::RealmBody,
    ParseState::RealmCapability,
    ParseState::PolicyBody,
    ParseState::PolicyFilter,
    ParseState::RequireClause,
    ParseState::ServiceBody,
    ParseState::ServiceMethodBody,
    ParseState::FlowHeader,
    ParseState::FlowBody,
    ParseState::FlowAuthKind,
    ParseState::FlowBodyField,
    ParseState::FlowParamType,
    ParseState::FlowStepStart,
    ParseState::FilterClause,
    ParseState::FilterOp,
    ParseState::InlineExpr,
    ParseState::BlockExpr,
    ParseState::InsertField,
    ParseState::ReturnStmt,
    ParseState::SagaHeader,
    ParseState::SagaBody,
    ParseState::SagaStepBody,
    ParseState::SurfaceBody,
    ParseState::ExposeBody,
    ParseState::DeprecateClause,
    ParseState::MigrateBody,
    ParseState::UpdateBody,
    ParseState::DeleteBody,
    ParseState::WhereClause,
    ParseState::SetClause,
    ParseState::MatchBody,
    ParseState::WhenBranch,
    ParseState::EffectBody,
    ParseState::StreamBody,
];

fn keyword_tokens() -> Vec<TokenKind> {
    vec![
        TokenKind::Shape,
        TokenKind::Source,
        TokenKind::Realm,
        TokenKind::Flow,
        TokenKind::Saga,
        TokenKind::Surface,
        TokenKind::Migrate,
        TokenKind::Policy,
        TokenKind::Service,
        TokenKind::Postgres,
        TokenKind::Mysql,
        TokenKind::Redis,
        TokenKind::Elasticsearch,
        TokenKind::Dynamodb,
        TokenKind::Auth,
        TokenKind::Body,
        TokenKind::Param,
        TokenKind::Header,
        TokenKind::Rule,
        TokenKind::Guard,
        TokenKind::Let,
        TokenKind::Fetch,
        TokenKind::Query,
        TokenKind::Insert,
        TokenKind::Update,
        TokenKind::Delete,
        TokenKind::Call,
        TokenKind::Effect,
        TokenKind::Match,
        TokenKind::When,
        TokenKind::Default,
        TokenKind::Return,
        TokenKind::Limit,
        TokenKind::Cache,
        TokenKind::Scope,
        TokenKind::Require,
        TokenKind::Filter,
        TokenKind::Sort,
        TokenKind::Asc,
        TokenKind::Desc,
        TokenKind::Cursor,
        TokenKind::PageSize,
        TokenKind::Or,
        TokenKind::And,
        TokenKind::Not,
        TokenKind::If,
        TokenKind::Eq,
        TokenKind::Neq,
        TokenKind::Gt,
        TokenKind::Gte,
        TokenKind::Lt,
        TokenKind::Lte,
        TokenKind::In,
        TokenKind::Between,
        TokenKind::Like,
        TokenKind::Empty,
        TokenKind::Exists,
        TokenKind::Add,
        TokenKind::Sub,
        TokenKind::Mul,
        TokenKind::Div,
        TokenKind::Mod,
        TokenKind::Round,
        TokenKind::Ceil,
        TokenKind::Floor,
        TokenKind::Abs,
        TokenKind::Count,
        TokenKind::Sum,
        TokenKind::Avg,
        TokenKind::Min,
        TokenKind::Max,
        TokenKind::First,
        TokenKind::Last,
        TokenKind::Concat,
        TokenKind::Lower,
        TokenKind::Upper,
        TokenKind::Trim,
        TokenKind::Substring,
        TokenKind::Length,
        TokenKind::StartsWith,
        TokenKind::EndsWith,
        TokenKind::Contains,
        TokenKind::DaysBetween,
        TokenKind::HoursBetween,
        TokenKind::MinutesBetween,
        TokenKind::Now,
        TokenKind::NowPlus,
        TokenKind::NowMinus,
        TokenKind::FormatDate,
        TokenKind::Coalesce,
        TokenKind::ToInt,
        TokenKind::ToDecimal,
        TokenKind::ToString_,
        TokenKind::Uuid,
        TokenKind::String_,
        TokenKind::Text,
        TokenKind::Int,
        TokenKind::Decimal,
        TokenKind::Bool,
        TokenKind::Date,
        TokenKind::Timestamp,
        TokenKind::Enum,
        TokenKind::Ref,
        TokenKind::List,
        TokenKind::Map,
        TokenKind::Json,
        TokenKind::Maybe,
        TokenKind::Pk,
        TokenKind::Auto,
        TokenKind::Required,
        TokenKind::Unique,
        TokenKind::Precision,
        TokenKind::Scale,
        TokenKind::Index,
        TokenKind::Tenant,
        TokenKind::Capability,
        TokenKind::Field,
        TokenKind::Hide,
        TokenKind::Expose,
        TokenKind::Deprecate,
        TokenKind::Route,
        TokenKind::Step,
        TokenKind::Verify,
        TokenKind::Compensate,
        TokenKind::Yield_,
        TokenKind::OnSuccess,
        TokenKind::OnFailure,
        TokenKind::Template,
        TokenKind::Data,
        TokenKind::Task,
        TokenKind::AppliesTo,
        TokenKind::Writes,
        TokenKind::Reads,
        TokenKind::Method,
        TokenKind::Where,
        TokenKind::Set,
        TokenKind::Copy,
        TokenKind::Compute,
        TokenKind::Drop,
        TokenKind::None_,
        TokenKind::True_,
        TokenKind::False_,
        TokenKind::As,
        TokenKind::To,
        TokenKind::Rename,
        TokenKind::Sunset,
        TokenKind::BasePath,
        TokenKind::Endpoint,
        TokenKind::Vault,
        TokenKind::Input,
        TokenKind::Output,
        TokenKind::Timeout,
        TokenKind::Retry,
        TokenKind::Backoff,
        TokenKind::Ttl,
        TokenKind::Stream,
        TokenKind::Event,
        TokenKind::Dedent,
        TokenKind::Newline,
        TokenKind::Eof,
    ]
}

#[derive(Serialize)]
pub struct GrammarExport {
    pub version: u32,
    pub initial_state: ParseState,
    pub states: Vec<StateExport>,
}

#[derive(Serialize)]
pub struct StateExport {
    pub name: ParseState,
    pub valid: ValidSet,
    pub transitions: Vec<TransitionExport>,
}

#[derive(Serialize)]
pub struct ValidSet {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ident: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub int: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub decimal: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub string: bool,
}

#[derive(Serialize)]
pub struct TransitionExport {
    pub on: String,
    pub to: Vec<ParseState>,
}

fn constraint_to_valid_set(c: &Constraint) -> ValidSet {
    let mut set = ValidSet {
        keywords: Vec::new(),
        ident: false,
        int: false,
        decimal: false,
        string: false,
    };
    collect_into_set(c, &mut set);
    set
}

fn collect_into_set(c: &Constraint, set: &mut ValidSet) {
    match c {
        Constraint::Exactly(kinds) => {
            for k in kinds {
                set.keywords.push(format!("{k}"));
            }
        }
        Constraint::AnyIdent => set.ident = true,
        Constraint::AnyInt => set.int = true,
        Constraint::AnyDecimal => set.decimal = true,
        Constraint::AnyString => set.string = true,
        Constraint::AnyOf(cs) => {
            for inner in cs {
                collect_into_set(inner, set);
            }
        }
    }
}

pub fn export_grammar() -> GrammarExport {
    let all_tokens = keyword_tokens();
    let states = ALL_STATES
        .iter()
        .map(|state| {
            let constraint = valid_tokens(state);
            let valid = constraint_to_valid_set(&constraint);

            let mut transitions = Vec::new();
            let mut seen_targets = std::collections::HashSet::new();
            for token in &all_tokens {
                if constraint.allows(token) {
                    let targets = next_states(state, token);
                    let key = format!("{token}");
                    if seen_targets.insert((key.clone(), targets.clone())) {
                        transitions.push(TransitionExport {
                            on: key,
                            to: targets,
                        });
                    }
                }
            }
            let wildcard_targets = next_states(state, &TokenKind::Ident("_".into()));
            if constraint.allows(&TokenKind::Ident("_".into())) {
                let key = "<ident>".to_string();
                if seen_targets.insert((key.clone(), wildcard_targets.clone())) {
                    transitions.push(TransitionExport {
                        on: key,
                        to: wildcard_targets,
                    });
                }
            }

            StateExport {
                name: state.clone(),
                valid,
                transitions,
            }
        })
        .collect();

    GrammarExport {
        version: 1,
        initial_state: ParseState::TopLevel,
        states,
    }
}

pub fn token_vocabulary() -> Vec<String> {
    let mut vocab: Vec<String> = keyword_tokens().iter().map(|k| format!("{k}")).collect();
    vocab.extend_from_slice(&[
        "<ident>".into(),
        "<int>".into(),
        "<decimal>".into(),
        "<string>".into(),
        "<path>".into(),
        "INDENT".into(),
    ]);
    vocab
}

pub fn valid_token_ids_for_state(state: &ParseState, vocab: &[String]) -> Vec<usize> {
    let constraint = valid_tokens(state);
    let mut ids = Vec::new();
    for (i, name) in vocab.iter().enumerate() {
        let allowed = match name.as_str() {
            "<ident>" => constraint.allows(&TokenKind::Ident("x".into())),
            "<int>" => constraint.allows(&TokenKind::IntLit(0)),
            "<decimal>" => constraint.allows(&TokenKind::DecimalLit("0.0".into())),
            "<string>" => constraint.allows(&TokenKind::StringLit("".into())),
            "<path>" => constraint.allows(&TokenKind::Path("/".into())),
            "INDENT" => constraint.allows(&TokenKind::Indent),
            _ => {
                let kw_tokens = keyword_tokens();
                kw_tokens
                    .iter()
                    .any(|k| format!("{k}") == *name && constraint.allows(k))
            }
        };
        if allowed {
            ids.push(i);
        }
    }
    ids
}

#[derive(Serialize)]
pub struct LogitMask {
    pub state: ParseState,
    pub allowed_ids: Vec<usize>,
    pub vocab_size: usize,
}

pub fn export_logit_masks() -> Vec<LogitMask> {
    let vocab = token_vocabulary();
    ALL_STATES
        .iter()
        .map(|state| LogitMask {
            state: state.clone(),
            allowed_ids: valid_token_ids_for_state(state, &vocab),
            vocab_size: vocab.len(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_top_level_constraints() {
        let c = valid_tokens(&ParseState::TopLevel);
        assert!(c.allows(&TokenKind::Shape));
        assert!(c.allows(&TokenKind::Source));
        assert!(c.allows(&TokenKind::Flow));
        assert!(c.allows(&TokenKind::Eof));
        assert!(!c.allows(&TokenKind::Let));
        assert!(!c.allows(&TokenKind::Ident("foo".into())));
    }

    #[test]
    fn test_shape_body_constraints() {
        let c = valid_tokens(&ParseState::ShapeBody);
        assert!(c.allows(&TokenKind::Ident("name".into())));
        assert!(c.allows(&TokenKind::Dedent));
        assert!(!c.allows(&TokenKind::Flow));
    }

    #[test]
    fn test_field_type_constraints() {
        let c = valid_tokens(&ParseState::ShapeFieldType);
        assert!(c.allows(&TokenKind::Uuid));
        assert!(c.allows(&TokenKind::Bool));
        assert!(c.allows(&TokenKind::Int));
        assert!(c.allows(&TokenKind::Enum));
        assert!(!c.allows(&TokenKind::Flow));
    }

    #[test]
    fn test_filter_op_constraints() {
        let c = valid_tokens(&ParseState::FilterOp);
        assert!(c.allows(&TokenKind::Eq));
        assert!(c.allows(&TokenKind::Gt));
        assert!(c.allows(&TokenKind::In));
        assert!(!c.allows(&TokenKind::Shape));
    }

    #[test]
    fn test_inline_expr_constraints() {
        let c = valid_tokens(&ParseState::InlineExpr);
        assert!(c.allows(&TokenKind::Ident("user".into())));
        assert!(c.allows(&TokenKind::IntLit(42)));
        assert!(c.allows(&TokenKind::StringLit("hello".into())));
        assert!(c.allows(&TokenKind::True_));
        assert!(!c.allows(&TokenKind::Shape));
    }

    #[test]
    fn test_block_expr_constraints() {
        let c = valid_tokens(&ParseState::BlockExpr);
        assert!(c.allows(&TokenKind::Fetch));
        assert!(c.allows(&TokenKind::Query));
        assert!(c.allows(&TokenKind::Add));
        assert!(c.allows(&TokenKind::Mul));
        assert!(c.allows(&TokenKind::If));
        assert!(c.allows(&TokenKind::DaysBetween));
        assert!(c.allows(&TokenKind::Ident("x".into())));
    }

    #[test]
    fn test_state_transitions() {
        let next = next_states(&ParseState::TopLevel, &TokenKind::Shape);
        assert_eq!(next, vec![ParseState::ShapeBody]);

        let next = next_states(&ParseState::ShapeBody, &TokenKind::Dedent);
        assert_eq!(next, vec![ParseState::TopLevel]);

        let next = next_states(&ParseState::TopLevel, &TokenKind::Flow);
        assert_eq!(next, vec![ParseState::FlowHeader]);
    }

    #[test]
    fn test_flow_body_constraints() {
        let c = valid_tokens(&ParseState::FlowBody);
        assert!(c.allows(&TokenKind::Auth));
        assert!(c.allows(&TokenKind::Let));
        assert!(c.allows(&TokenKind::Guard));
        assert!(c.allows(&TokenKind::Insert));
        assert!(c.allows(&TokenKind::Return));
        assert!(!c.allows(&TokenKind::Shape));
    }

    #[test]
    fn test_realm_body_constraints() {
        let c = valid_tokens(&ParseState::RealmBody);
        assert!(c.allows(&TokenKind::Tenant));
        assert!(c.allows(&TokenKind::Capability));
        assert!(c.allows(&TokenKind::Dedent));
        assert!(!c.allows(&TokenKind::Shape));
    }

    #[test]
    fn test_auth_kind_constraints() {
        let c = valid_tokens(&ParseState::FlowAuthKind);
        assert!(c.allows(&TokenKind::Ident("session".into())));
        assert!(c.allows(&TokenKind::Ident("bearer".into())));
        assert!(c.allows(&TokenKind::None_));
        assert!(!c.allows(&TokenKind::Shape));
    }

    #[test]
    fn test_surface_body_constraints_and_transitions() {
        let c = valid_tokens(&ParseState::SurfaceBody);
        assert!(c.allows(&TokenKind::Expose));
        assert!(c.allows(&TokenKind::Deprecate));
        assert!(c.allows(&TokenKind::Route));
        assert!(c.allows(&TokenKind::Dedent));
        assert!(!c.allows(&TokenKind::Let));

        let next = next_states(&ParseState::SurfaceBody, &TokenKind::Expose);
        assert_eq!(next, vec![ParseState::ExposeBody]);

        let next = next_states(&ParseState::SurfaceBody, &TokenKind::Deprecate);
        assert_eq!(next, vec![ParseState::DeprecateClause]);

        let next = next_states(&ParseState::SurfaceBody, &TokenKind::Dedent);
        assert_eq!(next, vec![ParseState::TopLevel]);
    }

    #[test]
    fn test_expose_body_constraints() {
        let c = valid_tokens(&ParseState::ExposeBody);
        assert!(c.allows(&TokenKind::Field));
        assert!(c.allows(&TokenKind::Hide));
        assert!(c.allows(&TokenKind::Rename));
        assert!(c.allows(&TokenKind::Dedent));
        assert!(!c.allows(&TokenKind::Shape));

        let next = next_states(&ParseState::ExposeBody, &TokenKind::Dedent);
        assert_eq!(next, vec![ParseState::SurfaceBody]);
    }

    #[test]
    fn test_update_delete_body_transitions() {
        let next = next_states(&ParseState::FlowBody, &TokenKind::Update);
        assert_eq!(next, vec![ParseState::UpdateBody]);

        let next = next_states(&ParseState::FlowBody, &TokenKind::Delete);
        assert_eq!(next, vec![ParseState::DeleteBody]);

        let c = valid_tokens(&ParseState::UpdateBody);
        assert!(c.allows(&TokenKind::Where));
        assert!(c.allows(&TokenKind::Set));
        assert!(c.allows(&TokenKind::Dedent));

        let c = valid_tokens(&ParseState::DeleteBody);
        assert!(c.allows(&TokenKind::Where));
        assert!(c.allows(&TokenKind::Dedent));
        assert!(!c.allows(&TokenKind::Set));

        let next = next_states(&ParseState::UpdateBody, &TokenKind::Where);
        assert_eq!(next, vec![ParseState::WhereClause]);

        let next = next_states(&ParseState::UpdateBody, &TokenKind::Set);
        assert_eq!(next, vec![ParseState::SetClause]);

        let next = next_states(&ParseState::UpdateBody, &TokenKind::Dedent);
        assert_eq!(next, vec![ParseState::FlowBody]);
    }

    #[test]
    fn test_match_body_transitions() {
        let next = next_states(&ParseState::FlowBody, &TokenKind::Match);
        assert_eq!(next, vec![ParseState::MatchBody]);

        let c = valid_tokens(&ParseState::MatchBody);
        assert!(c.allows(&TokenKind::When));
        assert!(c.allows(&TokenKind::Default));
        assert!(c.allows(&TokenKind::Dedent));
        assert!(!c.allows(&TokenKind::Let));

        let next = next_states(&ParseState::MatchBody, &TokenKind::When);
        assert_eq!(next, vec![ParseState::WhenBranch]);

        let next = next_states(&ParseState::MatchBody, &TokenKind::Dedent);
        assert_eq!(next, vec![ParseState::FlowBody]);
    }

    #[test]
    fn test_when_branch_constraints() {
        let c = valid_tokens(&ParseState::WhenBranch);
        assert!(c.allows(&TokenKind::Let));
        assert!(c.allows(&TokenKind::Insert));
        assert!(c.allows(&TokenKind::Update));
        assert!(c.allows(&TokenKind::Delete));
        assert!(c.allows(&TokenKind::Effect));
        assert!(c.allows(&TokenKind::Dedent));
        assert!(!c.allows(&TokenKind::Shape));

        let next = next_states(&ParseState::WhenBranch, &TokenKind::Insert);
        assert_eq!(next, vec![ParseState::InsertField]);

        let next = next_states(&ParseState::WhenBranch, &TokenKind::Update);
        assert_eq!(next, vec![ParseState::UpdateBody]);

        let next = next_states(&ParseState::WhenBranch, &TokenKind::Dedent);
        assert_eq!(next, vec![ParseState::MatchBody]);
    }

    #[test]
    fn test_effect_body_constraints() {
        let c = valid_tokens(&ParseState::EffectBody);
        assert!(c.allows(&TokenKind::Template));
        assert!(c.allows(&TokenKind::To));
        assert!(c.allows(&TokenKind::Data));
        assert!(c.allows(&TokenKind::Task));
        assert!(c.allows(&TokenKind::Ident("url".into())));
        assert!(c.allows(&TokenKind::Ident("event".into())));
        assert!(c.allows(&TokenKind::Dedent));

        let next = next_states(&ParseState::EffectBody, &TokenKind::Dedent);
        assert_eq!(next, vec![ParseState::FlowBody]);
    }

    fn skip_line(tokens: &[&crate::token::Token], i: &mut usize) {
        while *i < tokens.len() && !matches!(tokens[*i].kind, TokenKind::Newline | TokenKind::Eof) {
            *i += 1;
        }
        if *i < tokens.len() && tokens[*i].kind == TokenKind::Newline {
            *i += 1;
        }
    }

    fn skip_block(tokens: &[&crate::token::Token], i: &mut usize) {
        skip_line(tokens, i);
        if *i < tokens.len() && tokens[*i].kind == TokenKind::Indent {
            let mut depth = 1;
            *i += 1;
            while *i < tokens.len() && depth > 0 {
                match &tokens[*i].kind {
                    TokenKind::Indent => depth += 1,
                    TokenKind::Dedent => depth -= 1,
                    _ => {}
                }
                *i += 1;
            }
        }
    }

    fn walk_tokens(tokens: &[crate::token::Token]) -> Result<(), String> {
        let mut state = ParseState::TopLevel;
        let skip_structural = |t: &TokenKind| {
            matches!(
                t,
                TokenKind::ShapeName(_)
                    | TokenKind::Path(_)
                    | TokenKind::Arrow
                    | TokenKind::Dot
                    | TokenKind::Colon
            )
        };

        let tokens: Vec<&crate::token::Token> = tokens
            .iter()
            .filter(|t| !skip_structural(&t.kind))
            .collect();

        let mut i = 0;
        while i < tokens.len() {
            let token = tokens[i];

            if token.kind == TokenKind::Eof {
                break;
            }

            // Construct headers: after SHAPE/SOURCE/REALM/etc, skip name tokens
            // until we reach the body state naturally
            if matches!(state, ParseState::TopLevel) {
                match &token.kind {
                    TokenKind::Shape => {
                        state = ParseState::ShapeBody;
                        i += 1;
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                        continue;
                    }
                    TokenKind::Source => {
                        state = ParseState::SourceBody;
                        i += 1;
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                        continue;
                    }
                    TokenKind::Realm => {
                        state = ParseState::RealmBody;
                        i += 1;
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                        continue;
                    }
                    TokenKind::Policy => {
                        state = ParseState::PolicyBody;
                        i += 1;
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                        continue;
                    }
                    TokenKind::Service => {
                        state = ParseState::ServiceBody;
                        i += 1;
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                        continue;
                    }
                    TokenKind::Migrate => {
                        state = ParseState::MigrateBody;
                        i += 1;
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                        continue;
                    }
                    TokenKind::Stream => {
                        state = ParseState::StreamBody;
                        i += 1;
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                        continue;
                    }
                    TokenKind::Flow | TokenKind::Saga => {
                        state = next_states(&state, &token.kind).into_iter().next().unwrap();
                        i += 1;
                        // skip header tokens (name, method, path) until newline
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                        // FlowHeader/SagaHeader → FlowBody/SagaBody
                        state = match state {
                            ParseState::FlowHeader => ParseState::FlowBody,
                            ParseState::SagaHeader => ParseState::SagaBody,
                            _ => state,
                        };
                        continue;
                    }
                    TokenKind::Surface => {
                        state = ParseState::SurfaceBody;
                        i += 1;
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                        continue;
                    }
                    _ => {}
                }
            }

            // Inside bodies: some keywords consume their arguments on the same line
            // and we validate the keyword itself but skip inline args
            match (&state, &token.kind) {
                // REALM/AUTH/SCOPE/LIMIT/CACHE inside flow: keyword + rest of line
                (ParseState::FlowBody, TokenKind::Realm)
                | (ParseState::FlowBody, TokenKind::Scope)
                | (ParseState::FlowBody, TokenKind::Limit)
                | (ParseState::FlowBody, TokenKind::Cache)
                | (ParseState::FlowBody, TokenKind::Timeout)
                | (ParseState::FlowBody, TokenKind::Idempotency) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                (ParseState::FlowBody, TokenKind::Auth) => {
                    state = ParseState::FlowBody;
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // BODY name + nested fields block
                (ParseState::FlowBody, TokenKind::Body) => {
                    i += 1;
                    skip_block(&tokens, &mut i);
                    continue;
                }
                // PARAM/HEADER name type modifiers
                (ParseState::FlowBody, TokenKind::Param)
                | (ParseState::FlowBody, TokenKind::Header) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // INSERT has AS continuation at the parent indent level
                (ParseState::FlowBody, TokenKind::Insert)
                | (ParseState::FlowBody, TokenKind::Upsert) => {
                    i += 1;
                    skip_block(&tokens, &mut i);
                    if i < tokens.len() && tokens[i].kind == TokenKind::As {
                        i += 1;
                        skip_line(&tokens, &mut i);
                    }
                    continue;
                }
                // Flow step keywords: skip keyword + inline args + any nested block
                (ParseState::FlowBody, TokenKind::Rule)
                | (ParseState::FlowBody, TokenKind::Guard)
                | (ParseState::FlowBody, TokenKind::Let)
                | (ParseState::FlowBody, TokenKind::Set)
                | (ParseState::FlowBody, TokenKind::Update)
                | (ParseState::FlowBody, TokenKind::Delete)
                | (ParseState::FlowBody, TokenKind::Fanout)
                | (ParseState::FlowBody, TokenKind::Effect)
                | (ParseState::FlowBody, TokenKind::Match)
                | (ParseState::FlowBody, TokenKind::Each)
                | (ParseState::FlowBody, TokenKind::Try)
                | (ParseState::FlowBody, TokenKind::Upload) => {
                    i += 1;
                    skip_block(&tokens, &mut i);
                    continue;
                }
                // RETURN code binding
                (ParseState::FlowBody, TokenKind::Return) => {
                    state = ParseState::TopLevel;
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // SourceBody internals
                (ParseState::SourceBody, TokenKind::Shape) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                (ParseState::SourceBody, TokenKind::Index) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                (ParseState::SourceBody, TokenKind::Ttl) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // RealmBody internals
                (ParseState::RealmBody, TokenKind::Tenant) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                (ParseState::RealmBody, TokenKind::Capability) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // PolicyBody internals
                (ParseState::PolicyBody, TokenKind::AppliesTo) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    // skip WHERE clause on next line if present
                    while i < tokens.len() && matches!(tokens[i].kind, TokenKind::Where) {
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                        {
                            i += 1;
                        }
                        if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                            i += 1;
                        }
                    }
                    continue;
                }
                (ParseState::PolicyBody, TokenKind::Require) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // ServiceBody internals
                (ParseState::ServiceBody, TokenKind::Endpoint)
                | (ParseState::ServiceBody, TokenKind::Auth) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                (ParseState::ServiceBody, TokenKind::Method) => {
                    state = ParseState::ServiceMethodBody;
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // ServiceMethodBody internals
                (ParseState::ServiceMethodBody, k) if !matches!(k, TokenKind::Dedent) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // FlowBodyField (BODY fields) — field lines
                (ParseState::FlowBodyField, k) if !matches!(k, TokenKind::Dedent) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // SurfaceBody internals
                (ParseState::SurfaceBody, TokenKind::Realm)
                | (ParseState::SurfaceBody, TokenKind::BasePath)
                | (ParseState::SurfaceBody, TokenKind::Route) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                (ParseState::SurfaceBody, TokenKind::Expose) => {
                    i += 1;
                    skip_block(&tokens, &mut i);
                    continue;
                }
                (ParseState::SurfaceBody, TokenKind::Deprecate) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // ExposeBody internals
                (ParseState::ExposeBody, k) if !matches!(k, TokenKind::Dedent) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // MigrateBody internals
                (ParseState::MigrateBody, k) if !matches!(k, TokenKind::Dedent) => {
                    i += 1;
                    while i < tokens.len()
                        && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                    {
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                        i += 1;
                    }
                    continue;
                }
                // SagaBody internals — single-line items
                (ParseState::SagaBody, TokenKind::Realm)
                | (ParseState::SagaBody, TokenKind::Auth) => {
                    i += 1;
                    skip_line(&tokens, &mut i);
                    continue;
                }
                // SagaBody internals — items with nested blocks
                (ParseState::SagaBody, TokenKind::Body)
                | (ParseState::SagaBody, TokenKind::Step)
                | (ParseState::SagaBody, TokenKind::OnFailure)
                | (ParseState::SagaBody, TokenKind::OnSuccess) => {
                    i += 1;
                    skip_block(&tokens, &mut i);
                    continue;
                }
                // SagaStepBody — consume step content lines
                (ParseState::SagaStepBody, k) if !matches!(k, TokenKind::Dedent) => {
                    i += 1;
                    skip_block(&tokens, &mut i);
                    continue;
                }
                // StreamBody internals
                (ParseState::StreamBody, TokenKind::Realm)
                | (ParseState::StreamBody, TokenKind::Auth) => {
                    i += 1;
                    skip_line(&tokens, &mut i);
                    continue;
                }
                (ParseState::StreamBody, TokenKind::Event) => {
                    i += 1;
                    skip_block(&tokens, &mut i);
                    continue;
                }
                _ => {}
            }

            // Handle Dedent transitions
            if token.kind == TokenKind::Dedent {
                let next = next_states(&state, &TokenKind::Dedent);
                if let Some(s) = next.first() {
                    state = s.clone();
                }
                i += 1;
                continue;
            }

            // Handle Indent/Newline — skip structural whitespace
            if matches!(token.kind, TokenKind::Indent | TokenKind::Newline) {
                i += 1;
                continue;
            }

            // ShapeBody field lines
            if state == ParseState::ShapeBody {
                let constraint = valid_tokens(&state);
                if !constraint.allows(&token.kind) {
                    return Err(format!(
                        "token {} ({:?}) at line {} not allowed in state {:?}. Valid: {:?}",
                        i,
                        token.kind,
                        token.span.line,
                        state,
                        constraint.token_names()
                    ));
                }
                // field name → skip rest of line (type + modifiers)
                i += 1;
                while i < tokens.len()
                    && !matches!(tokens[i].kind, TokenKind::Newline | TokenKind::Eof)
                {
                    i += 1;
                }
                if i < tokens.len() && tokens[i].kind == TokenKind::Newline {
                    i += 1;
                }
                continue;
            }

            // Generic: validate and advance
            let constraint = valid_tokens(&state);
            if !constraint.allows(&token.kind) {
                return Err(format!(
                    "token {} ({:?}) at line {} not allowed in state {:?}. Valid: {:?}",
                    i,
                    token.kind,
                    token.span.line,
                    state,
                    constraint.token_names()
                ));
            }
            let next = next_states(&state, &token.kind);
            if let Some(s) = next.first() {
                state = s.clone();
            }
            i += 1;
        }
        Ok(())
    }

    fn lex(input: &str) -> Vec<crate::token::Token> {
        let mut lexer = crate::lexer::Lexer::new(input);
        lexer.tokenize().unwrap()
    }

    #[test]
    fn test_walk_simple_shape() {
        let tokens = lex(r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_simple_flow() {
        let tokens = lex(r#"SHAPE User
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
"#);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_flow_with_insert() {
        let tokens = lex(r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM api
  CAPABILITY read users
  CAPABILITY write users
  CAPABILITY effect email

FLOW create_user post /users
  REALM api
  AUTH session
  LIMIT 10 PER_MINUTE PER_USER
  BODY UserCreate
    name STRING 100 REQUIRED
  INSERT users
    name body.name
  AS user
  EFFECT email
    TEMPLATE welcome
    TO user.id
    DATA user
  RETURN 201 user
"#);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_flow_with_guard() {
        let tokens = lex(r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  role ENUM admin user REQUIRED

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
  GUARD ownership 403 "not yours"
    EQ user.id auth.user_id
  RETURN 200 user
"#);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_policy() {
        let tokens = lex(r#"POLICY require_auth
  APPLIES_TO FLOW
  REQUIRE AUTH

POLICY rate_limit_writes
  APPLIES_TO FLOW WHERE METHOD IN post put delete
  REQUIRE LIMIT
"#);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_service() {
        let tokens = lex(r#"SERVICE payments
  ENDPOINT stripe
  AUTH bearer VAULT stripe_key
  METHOD charge
    INPUT amount DECIMAL currency STRING 10
    OUTPUT transaction_id STRING 100 status STRING 20
    TIMEOUT 30 s
    RETRY 3 BACKOFF exponential
"#);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_surface() {
        let tokens = lex(r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  email STRING 255 REQUIRED

SURFACE public v1
  BASE_PATH /api/v1
  ROUTE GET /users/:id -> get_user
  EXPOSE User AS UserResponse
    FIELD id UUID
    FIELD name STRING
    HIDE email
  DEPRECATE v0 SUNSET "2027-01-01"
"#);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_migrate() {
        let tokens = lex(r#"SHAPE Order
  id UUID PK AUTO
  status STRING 20 REQUIRED

MIGRATE Order v1 TO v2
  COPY id status
  ADD tracking_number MAYBE STRING 100
  DROP old_field
  RENAME status TO order_status
"#);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_saga() {
        let tokens = lex(r#"SHAPE Order
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
"#);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_booking_example() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/booking.axis"),
        )
        .unwrap();
        let tokens = lex(&input);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_walk_full_example() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/full.axis"),
        )
        .unwrap();
        let tokens = lex(&input);
        walk_tokens(&tokens).unwrap();
    }

    #[test]
    fn test_export_grammar_structure() {
        let grammar = export_grammar();
        assert_eq!(grammar.version, 1);
        assert_eq!(grammar.initial_state, ParseState::TopLevel);
        assert_eq!(grammar.states.len(), ALL_STATES.len());
    }

    #[test]
    fn test_export_grammar_serializes() {
        let grammar = export_grammar();
        let json = serde_json::to_string_pretty(&grammar).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["initial_state"], "top_level");
        assert!(parsed["states"].as_array().unwrap().len() > 30);
    }

    #[test]
    fn test_export_grammar_top_level_transitions() {
        let grammar = export_grammar();
        let top = grammar
            .states
            .iter()
            .find(|s| s.name == ParseState::TopLevel)
            .unwrap();
        let shape_t = top.transitions.iter().find(|t| t.on == "SHAPE").unwrap();
        assert_eq!(shape_t.to, vec![ParseState::ShapeBody]);
        let flow_t = top.transitions.iter().find(|t| t.on == "FLOW").unwrap();
        assert_eq!(flow_t.to, vec![ParseState::FlowHeader]);
        assert!(top.valid.keywords.contains(&"SHAPE".to_string()));
        assert!(!top.valid.ident);
    }

    #[test]
    fn test_export_grammar_flow_body_valid_set() {
        let grammar = export_grammar();
        let flow = grammar
            .states
            .iter()
            .find(|s| s.name == ParseState::FlowBody)
            .unwrap();
        assert!(flow.valid.keywords.contains(&"LET".to_string()));
        assert!(flow.valid.keywords.contains(&"GUARD".to_string()));
        assert!(flow.valid.keywords.contains(&"RETURN".to_string()));
        assert!(!flow.valid.ident);
    }

    #[test]
    fn test_export_grammar_shape_body_accepts_ident() {
        let grammar = export_grammar();
        let shape = grammar
            .states
            .iter()
            .find(|s| s.name == ParseState::ShapeBody)
            .unwrap();
        assert!(shape.valid.ident);
        assert!(shape.valid.keywords.contains(&"DEDENT".to_string()));
    }

    #[test]
    fn test_export_grammar_block_expr_accepts_literals() {
        let grammar = export_grammar();
        let expr = grammar
            .states
            .iter()
            .find(|s| s.name == ParseState::BlockExpr)
            .unwrap();
        assert!(expr.valid.ident);
        assert!(expr.valid.int);
        assert!(expr.valid.decimal);
        assert!(expr.valid.string);
        assert!(expr.valid.keywords.contains(&"FETCH".to_string()));
    }

    #[test]
    fn test_export_grammar_every_state_has_valid_tokens() {
        let grammar = export_grammar();
        for state in &grammar.states {
            let has_content = !state.valid.keywords.is_empty()
                || state.valid.ident
                || state.valid.int
                || state.valid.decimal
                || state.valid.string;
            assert!(has_content, "state {:?} has empty valid set", state.name);
        }
    }

    #[test]
    fn test_logit_masks_cover_all_states() {
        let masks = export_logit_masks();
        assert_eq!(masks.len(), ALL_STATES.len());
        for mask in &masks {
            assert!(
                !mask.allowed_ids.is_empty(),
                "state {:?} has no allowed tokens",
                mask.state
            );
        }
    }

    #[test]
    fn test_token_vocabulary_nonempty() {
        let vocab = token_vocabulary();
        assert!(vocab.len() > 100);
        assert!(vocab.contains(&"SHAPE".to_string()));
        assert!(vocab.contains(&"STREAM".to_string()));
        assert!(vocab.contains(&"<ident>".to_string()));
    }

    #[test]
    fn test_stream_body_constraints() {
        let c = valid_tokens(&ParseState::StreamBody);
        assert!(c.allows(&TokenKind::Realm));
        assert!(c.allows(&TokenKind::Auth));
        assert!(c.allows(&TokenKind::Event));
        assert!(c.allows(&TokenKind::Dedent));
        assert!(!c.allows(&TokenKind::Shape));
    }

    #[test]
    fn test_top_level_allows_stream() {
        let c = valid_tokens(&ParseState::TopLevel);
        assert!(c.allows(&TokenKind::Stream));
    }

    #[test]
    fn test_walk_stream() {
        let tokens = lex(r#"STREAM notifications ws "/ws/notify"
  REALM api
  AUTH bearer
  EVENT new_message
    id UUID
    body STRING 1000
"#);
        walk_tokens(&tokens).unwrap();
    }
}
