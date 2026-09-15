use crate::token::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub constructs: Vec<Construct>,
}

#[derive(Debug, Clone)]
pub enum Construct {
    Shape(ShapeDef),
    Source(SourceDef),
    Realm(RealmDef),
    Policy(PolicyDef),
    Service(ServiceDef),
    Flow(FlowDef),
    Saga(SagaDef),
    Surface(SurfaceDef),
    Migrate(MigrateDef),
    Stream(StreamDef),
    Func(FuncDef),
    Storage(StorageDef),
}

// --- Shapes ---

#[derive(Debug, Clone)]
pub struct ShapeDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub ty: TypeExpr,
    pub modifiers: Vec<Modifier>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeExpr {
    Uuid,
    Bool,
    Date,
    Timestamp,
    Text,
    String(Option<i64>),
    Int {
        min: Option<i64>,
        max: Option<i64>,
    },
    Decimal {
        precision: Option<i64>,
        scale: Option<i64>,
    },
    Enum(Vec<String>),
    Ref {
        shape: String,
        field: String,
    },
    List(Box<TypeExpr>),
    Map(Box<TypeExpr>, Box<TypeExpr>),
    Json,
    Maybe(Box<TypeExpr>),
    Blob,
}

#[derive(Debug, Clone)]
pub enum Modifier {
    Pk,
    Auto,
    Required,
    Unique,
    Default(LiteralValue),
    Precision(i64),
    Scale(i64),
    Min(i64),
    Max(i64),
    Ref { shape: String, field: String },
}

#[derive(Debug, Clone)]
pub enum LiteralValue {
    Int(i64),
    Decimal(String),
    String(String),
    Bool(bool),
    Ident(String),
    Now,
    None,
}

// --- Sources ---

#[derive(Debug, Clone)]
pub struct SourceDef {
    pub name: String,
    pub source_type: SourceType,
    pub shape: String,
    pub indexes: Vec<IndexDef>,
    pub ttl: Option<i64>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SourceType {
    Postgres,
    Mysql,
    Sqlite,
    Redis,
    Elasticsearch,
    Dynamodb,
}

#[derive(Debug, Clone)]
pub struct IndexDef {
    pub fields: Vec<IndexField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct IndexField {
    pub name: String,
    pub suffix: Option<IndexSuffix>,
}

#[derive(Debug, Clone)]
pub enum IndexSuffix {
    Asc,
    Desc,
    Geo,
    Text,
    Keyword,
    Unique,
}

// --- Realms ---

#[derive(Debug, Clone)]
pub struct RealmDef {
    pub name: String,
    pub tenant: Option<String>,
    pub capabilities: Vec<CapabilityDef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CapabilityDef {
    pub kind: CapabilityKind,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CapabilityKind {
    Read,
    Write,
    Call,
    Effect,
    Admin,
}

// --- Policies ---

#[derive(Debug, Clone)]
pub struct PolicyDef {
    pub name: String,
    pub applies_to: AppliesTo,
    pub requires: Vec<RequireClause>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AppliesTo {
    pub filters: Vec<PolicyFilter>,
    pub mode: PolicyMatchMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PolicyMatchMode {
    All,
    Any,
}

#[derive(Debug, Clone)]
pub enum PolicyFilter {
    MethodIn(Vec<String>),
    Reads(String),
    Writes(String),
    PathStartsWith(String),
    Not(Box<PolicyFilter>),
}

#[derive(Debug, Clone)]
pub enum RequireClause {
    Auth(Option<String>),
    Limit,
    Scope,
    Rule(String),
    Guard(String),
    Idempotency,
    Fanout,
}

// --- Services ---

#[derive(Debug, Clone)]
pub struct ServiceDef {
    pub name: String,
    pub endpoint: String,
    pub auth_type: String,
    pub vault_key: String,
    pub methods: Vec<ServiceMethod>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ServiceMethod {
    pub name: String,
    pub pure: bool,
    /// Input field used by the provider to deduplicate externally observable
    /// effects. Axis verifies that idempotent flows pass their exact flow key.
    pub idempotency_input: Option<String>,
    pub inputs: Vec<(String, TypeExpr)>,
    pub outputs: Vec<(String, TypeExpr)>,
    pub timeout: Option<Duration>,
    pub retry: Option<RetryConfig>,
    pub cache_ttl: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Duration {
    pub value: i64,
    pub unit: DurationUnit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DurationUnit {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
}

impl std::fmt::Display for Duration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let suffix = match self.unit {
            DurationUnit::Milliseconds => "ms",
            DurationUnit::Seconds => "s",
            DurationUnit::Minutes => "m",
            DurationUnit::Hours => "h",
        };
        write!(f, "{}{}", self.value, suffix)
    }
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub count: i64,
    pub strategy: RetryStrategy,
}

#[derive(Debug, Clone)]
pub enum RetryStrategy {
    Exponential,
    Linear,
    None,
}

// --- Flows ---

#[derive(Debug, Clone)]
pub struct FlowDef {
    pub name: String,
    pub method: HttpMethod,
    pub path: String,
    pub realm: Option<String>,
    pub auth: Option<AuthDecl>,
    pub limits: Vec<LimitDecl>,
    pub cache: Vec<CacheDecl>,
    pub scope: Option<ScopeDecl>,
    pub timeout: Option<Duration>,
    pub body: Option<BodyDecl>,
    pub params: Vec<ParamDecl>,
    pub headers: Vec<HeaderDecl>,
    pub idempotency: Option<IdempotencyDecl>,
    pub steps: Vec<FlowStep>,
    pub return_stmt: ReturnStmt,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CacheDecl {
    pub ttl: i64,
    pub vary: Vec<DotPath>,
}

#[derive(Debug, Clone)]
pub struct IdempotencyDecl {
    pub key: DotPath,
    pub scope: DotPath,
    pub ttl: i64,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Webhook,
}

#[derive(Debug, Clone)]
pub enum AuthDecl {
    None,
    Session,
    Bearer,
    ApiKey,
    Role(String),
    RoleIn(Vec<String>),
    WebhookSignature { secret: String, algorithm: String },
}

#[derive(Debug, Clone)]
pub struct LimitDecl {
    pub count: i64,
    pub unit: RateUnit,
    pub scope: RateScope,
}

#[derive(Debug, Clone)]
pub enum RateUnit {
    PerSecond,
    PerMinute,
    PerHour,
    PerDay,
}

#[derive(Debug, Clone)]
pub enum RateScope {
    PerUser,
    PerIp,
    PerKey,
    Global,
}

#[derive(Debug, Clone)]
pub enum ScopeDecl {
    Tenant(DotPath),
    TenantAny,
}

#[derive(Debug, Clone)]
pub struct BodyDecl {
    pub name: String,
    pub kind: BodyKind,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BodyKind {
    Json,
    Multipart,
}

#[derive(Debug, Clone)]
pub struct ParamDecl {
    pub name: String,
    pub ty: TypeExpr,
    pub modifiers: Vec<Modifier>,
}

#[derive(Debug, Clone)]
pub struct HeaderDecl {
    pub name: String,
    pub ty: TypeExpr,
    pub modifiers: Vec<Modifier>,
}

#[derive(Debug, Clone)]
pub enum FlowStep {
    Rule(RuleStep),
    Guard(GuardStep),
    Let(LetStep),
    Set(SetStep),
    Insert(InsertStep),
    Upsert(UpsertStep),
    Update(UpdateStep),
    Delete(DeleteStep),
    Fanout(FanoutStep),
    Effect(EffectStep),
    Match(MatchStep),
    Each(EachStep),
    Try(TryStep),
    Upload(UploadStep),
}

#[derive(Debug, Clone)]
pub struct RuleStep {
    pub name: String,
    pub requires: Vec<RequireLine>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RequireLine {
    pub path: DotPath,
    pub op: CompareOp,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct GuardStep {
    pub name: String,
    pub code: i64,
    pub message: Option<String>,
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct LetStep {
    pub name: String,
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct InsertStep {
    pub source: String,
    pub fields: Vec<(String, Expr)>,
    pub binding: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct UpsertStep {
    pub source: String,
    pub keys: Vec<(String, Expr)>,
    pub sets: Vec<SetClause>,
    pub binding: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FanoutStep {
    pub binding: String,
    pub source: Expr,
    pub insert: InsertStep,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct UpdateStep {
    pub source: String,
    pub wheres: Vec<WhereClause>,
    pub sets: Vec<SetClause>,
    pub binding: Option<UpdateBinding>,
    pub or_code: i64,
    pub or_message: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum UpdateBinding {
    As(String),
    Count(String),
}

#[derive(Debug, Clone)]
pub struct DeleteStep {
    pub source: String,
    pub wheres: Vec<WhereClause>,
    pub or_code: i64,
    pub or_message: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct WhereClause {
    pub field: String,
    pub op: CompareOp,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct SetClause {
    pub field: String,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct EffectStep {
    pub kind: EffectKind,
    pub fields: Vec<EffectField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum EffectKind {
    Email,
    PushNotification,
    Async,
    Webhook,
}

#[derive(Debug, Clone)]
pub enum EffectField {
    Template(String),
    To(Expr),
    Data(Vec<Expr>),
    Url(Expr),
    Event(String),
    Task(String),
}

#[derive(Debug, Clone)]
pub struct MatchStep {
    pub branches: Vec<WhenBranch>,
    pub default: Option<Vec<FlowStep>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct WhenBranch {
    pub condition: Expr,
    pub steps: Vec<FlowStep>,
}

#[derive(Debug, Clone)]
pub struct SetStep {
    pub name: String,
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EachStep {
    pub binding: String,
    pub source: Expr,
    pub parallel: Option<i64>,
    pub steps: Vec<FlowStep>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TryStep {
    pub body: Vec<FlowStep>,
    pub recover: Vec<FlowStep>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub code: i64,
    pub body: Option<ReturnBody>,
    pub headers: Vec<(String, Expr)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ReturnBody {
    Binding(String),
    Inline(Vec<ReturnField>),
    Paginated {
        items: Box<Expr>,
        total: Box<Expr>,
        cursor: Box<Expr>,
        has_more: Box<Expr>,
    },
}

#[derive(Debug, Clone)]
pub struct ReturnField {
    pub name: String,
    pub value: ReturnValue,
}

#[derive(Debug, Clone)]
pub enum ReturnValue {
    Expr(Expr),
    Nested(Vec<ReturnField>),
}

// --- Expressions ---

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(LiteralValue),
    DotPath(DotPath),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Ternary {
        op: TernaryOp,
        a: Box<Expr>,
        b: Box<Expr>,
        c: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },
    Fetch {
        source: String,
        filters: Vec<FilterClause>,
        with: Vec<String>,
        or_code: i64,
        or_message: Option<String>,
        or_shape: Option<ErrorShape>,
    },
    Query {
        source: String,
        filters: Vec<FilterClause>,
        sorts: Vec<SortClause>,
        cursor: Option<Box<Expr>>,
        page_size: Option<Box<Expr>>,
        cache_ttl: Option<i64>,
    },
    Call {
        service: String,
        method: String,
        args: Vec<(String, Expr)>,
        or_code: i64,
        or_message: Option<String>,
    },
    WasmCall {
        hash: String,
        inputs: Vec<String>,
    },
    Aggregate {
        op: AggregateOp,
        source: Box<Expr>,
        field: Option<String>,
    },
    NowOffset {
        direction: OffsetDirection,
        amount: Box<Expr>,
        unit: TimeUnit,
    },
    Coalesce {
        value: Box<Expr>,
        default: Box<Expr>,
    },
    Cached {
        ttl: i64,
        expr: Box<Expr>,
    },
    MapExpr {
        source: Box<Expr>,
        fields: Vec<String>,
    },
    FilterExpr {
        source: Box<Expr>,
        condition: Box<Expr>,
    },
    ReduceExpr {
        op: AggregateOp,
        source: Box<Expr>,
        field: String,
    },
    SplitExpr {
        value: Box<Expr>,
        delimiter: Box<Expr>,
    },
    ReplaceExpr {
        value: Box<Expr>,
        from: Box<Expr>,
        to: Box<Expr>,
    },
    FormatExpr {
        template: String,
        args: Vec<Expr>,
    },
    FuncCall {
        name: String,
        args: Vec<Expr>,
    },
    Render {
        template: String,
        vars: Vec<(String, Expr)>,
    },
    Translate {
        key: String,
        vars: Vec<(String, Expr)>,
    },
}

#[derive(Debug, Clone)]
pub struct DotPath {
    pub segments: Vec<String>,
}

impl DotPath {
    pub fn as_str(&self) -> String {
        self.segments.join(".")
    }
}

#[derive(Debug, Clone)]
pub struct FilterClause {
    pub field: String,
    pub op: FilterOp,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct SortClause {
    pub field: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub enum FilterOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    In,
    Between,
    Like,
    StartsWith,
    Contains,
}

#[derive(Debug, Clone)]
pub enum CompareOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    In,
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Not,
    Empty,
    Exists,
    Lower,
    Upper,
    Trim,
    Abs,
    Ceil,
    Floor,
    Length,
    First,
    Last,
    ToInt,
    ToDecimal,
    ToString,
    Count,
}

#[derive(Debug, Clone)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Concat,
    StartsWith,
    EndsWith,
    Contains,
    DaysBetween,
    HoursBetween,
    MinutesBetween,
    Round,
    Coalesce,
    FormatDate,
}

#[derive(Debug, Clone)]
pub enum TernaryOp {
    Substring,
    Between,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AggregateOp {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    First,
    Last,
}

#[derive(Debug, Clone)]
pub enum OffsetDirection {
    Plus,
    Minus,
}

#[derive(Debug, Clone)]
pub enum TimeUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
    Years,
}

// --- Sagas ---

#[derive(Debug, Clone)]
pub struct SagaDef {
    pub name: String,
    pub method: HttpMethod,
    pub path: String,
    pub realm: Option<String>,
    pub auth: Option<AuthDecl>,
    pub body: Option<BodyDecl>,
    pub steps: Vec<SagaStep>,
    pub on_failure: OnFailure,
    pub on_success: OnSuccess,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SagaStep {
    pub name: String,
    pub flow_steps: Vec<FlowStep>,
    pub verify: Option<Expr>,
    pub yields: Vec<String>,
    pub compensate: Compensate,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Compensate {
    None,
    Steps(Vec<FlowStep>),
}

#[derive(Debug, Clone)]
pub struct OnFailure {
    pub run_compensations: bool,
}

#[derive(Debug, Clone)]
pub struct OnSuccess {
    pub effects: Vec<EffectStep>,
    pub return_stmt: ReturnStmt,
}

// --- Surfaces ---

#[derive(Debug, Clone)]
pub struct SurfaceDef {
    pub name: String,
    pub version: String,
    pub realm: Option<String>,
    pub base_path: Option<String>,
    pub routes: Vec<RouteDef>,
    pub exposes: Vec<ExposeDef>,
    pub deprecate: Option<DeprecateDef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RouteDef {
    pub method: HttpMethod,
    pub path: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct ExposeDef {
    pub shape: String,
    pub alias: Option<String>,
    pub fields: Vec<ExposeField>,
}

#[derive(Debug, Clone)]
pub enum ExposeField {
    Field { name: String, ty: TypeExpr },
    Hide(String),
    Rename { from: String, to: String },
}

#[derive(Debug, Clone)]
pub struct DeprecateDef {
    pub version: String,
    pub sunset: String,
}

// --- Migrations ---

#[derive(Debug, Clone)]
pub struct MigrateDef {
    pub shape: String,
    pub from_version: String,
    pub to_version: String,
    pub ops: Vec<MigrateOp>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum MigrateOp {
    Copy(Vec<String>),
    Compute { field: String, expr: Expr },
    Drop(String),
    Add(FieldDef),
    Rename { from: String, to: String },
}

// --- Streams ---

#[derive(Debug, Clone)]
pub struct StreamDef {
    pub name: String,
    pub transport: StreamTransport,
    pub path: String,
    pub realm: Option<String>,
    pub auth: Option<AuthDecl>,
    pub events: Vec<StreamEvent>,
    pub receivers: Vec<StreamReceiver>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StreamTransport {
    WebSocket,
    Sse,
}

#[derive(Debug, Clone)]
pub struct StreamEvent {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub span: Span,
}

// --- Functions ---

#[derive(Debug, Clone)]
pub struct FuncDef {
    pub name: String,
    pub inputs: Vec<FuncParam>,
    pub output: TypeExpr,
    pub steps: Vec<FlowStep>,
    pub return_expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FuncParam {
    pub name: String,
    pub ty: TypeExpr,
}

// --- Error shapes ---

#[derive(Debug, Clone)]
pub struct ErrorShape {
    pub shape: String,
    pub fields: Vec<(String, Expr)>,
}

// --- Storage ---

#[derive(Debug, Clone)]
pub struct StorageDef {
    pub name: String,
    pub backend: StorageBackend,
    pub bucket: String,
    pub prefix: Option<String>,
    pub access: StorageAccess,
    pub max_size: Option<i64>,
    pub types: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageBackend {
    Local,
    S3,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageAccess {
    Public,
    Private,
}

#[derive(Debug, Clone)]
pub struct UploadStep {
    pub file_expr: Expr,
    pub storage: String,
    pub binding: String,
    pub span: Span,
}

// --- Stream receivers ---

#[derive(Debug, Clone)]
pub struct StreamReceiver {
    pub event: String,
    pub steps: Vec<FlowStep>,
    pub span: Span,
}
