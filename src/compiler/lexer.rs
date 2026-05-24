use crate::error::{AxisError, AxisResult};
use crate::token::{Span, Token, TokenKind};

pub struct Lexer<'src> {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    indent_stack: Vec<usize>,
    pending_dedents: usize,
    at_line_start: bool,
    emit_eof: bool,
    _phantom: std::marker::PhantomData<&'src str>,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            indent_stack: vec![0],
            pending_dedents: 0,
            at_line_start: true,
            emit_eof: false,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn tokenize(&mut self) -> AxisResult<Vec<Token>> {
        let mut tokens = Vec::new();

        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }

        Ok(tokens)
    }

    fn next_token(&mut self) -> AxisResult<Token> {
        if self.pending_dedents > 0 {
            self.pending_dedents -= 1;
            return Ok(self.make_token(TokenKind::Dedent, 0));
        }

        if self.at_line_start && self.pos < self.chars.len() {
            self.at_line_start = false;
            return self.handle_indentation();
        }

        self.skip_inline_whitespace();

        if self.pos >= self.chars.len() {
            if !self.emit_eof {
                self.emit_eof = true;
                let dedents = self.indent_stack.len() - 1;
                if dedents > 0 {
                    self.pending_dedents = dedents - 1;
                    self.indent_stack.truncate(1);
                    return Ok(self.make_token(TokenKind::Dedent, 0));
                }
            }
            return Ok(self.make_token(TokenKind::Eof, 0));
        }

        let ch = self.chars[self.pos];

        if ch == '\n' {
            return self.lex_newline();
        }

        if ch == '-' && self.peek_at(1) == Some('-') {
            self.skip_comment();
            return self.next_token();
        }

        if ch == '-' && self.peek_at(1) == Some('>') {
            let start = self.pos;
            self.advance();
            self.advance();
            return Ok(Token {
                kind: TokenKind::Arrow,
                span: self.span_from(start, 2),
            });
        }

        if ch == '.' {
            let start = self.pos;
            self.advance();
            return Ok(Token {
                kind: TokenKind::Dot,
                span: self.span_from(start, 1),
            });
        }

        if ch == ':' {
            let start = self.pos;
            self.advance();
            return Ok(Token {
                kind: TokenKind::Colon,
                span: self.span_from(start, 1),
            });
        }

        if ch == '"' {
            return self.lex_string();
        }

        if ch == '/' {
            return self.lex_path();
        }

        if ch.is_ascii_digit() || (ch == '-' && self.peek_at(1).is_some_and(|c| c.is_ascii_digit())) {
            return self.lex_number();
        }

        if ch.is_ascii_uppercase() {
            return self.lex_upper_word();
        }

        if ch.is_ascii_lowercase() || ch == '_' {
            return self.lex_lower_word();
        }

        Err(AxisError::lex(
            self.line,
            self.col,
            format!("unexpected character: '{ch}'"),
        ))
    }

    fn handle_indentation(&mut self) -> AxisResult<Token> {
        let mut spaces = 0;
        while self.pos < self.chars.len() && self.chars[self.pos] == ' ' {
            spaces += 1;
            self.pos += 1;
            self.col += 1;
        }

        if self.pos < self.chars.len() && self.chars[self.pos] == '\t' {
            return Err(AxisError::lex(self.line, self.col, "tabs are not allowed; use 2 spaces"));
        }

        if self.pos < self.chars.len() && self.chars[self.pos] == '\n' {
            return self.lex_newline();
        }

        if self.pos < self.chars.len() && self.chars[self.pos] == '-' && self.peek_at(1) == Some('-') {
            self.skip_comment();
            return self.next_token();
        }

        let current = *self.indent_stack.last().unwrap();

        if spaces > current {
            if spaces != current + 2 {
                return Err(AxisError::lex(
                    self.line,
                    self.col,
                    format!("indent must be exactly 2 spaces (got {}, expected {})", spaces, current + 2),
                ));
            }
            self.indent_stack.push(spaces);
            Ok(self.make_token(TokenKind::Indent, 0))
        } else if spaces < current {
            let mut dedents = 0;
            while *self.indent_stack.last().unwrap() > spaces {
                self.indent_stack.pop();
                dedents += 1;
            }
            if *self.indent_stack.last().unwrap() != spaces {
                return Err(AxisError::lex(
                    self.line,
                    self.col,
                    format!("dedent to {spaces} spaces does not match any outer indent level"),
                ));
            }
            if dedents > 1 {
                self.pending_dedents = dedents - 1;
            }
            Ok(self.make_token(TokenKind::Dedent, 0))
        } else {
            self.next_token()
        }
    }

    fn lex_newline(&mut self) -> AxisResult<Token> {
        let start = self.pos;
        self.advance();
        self.line += 1;
        self.col = 1;
        self.at_line_start = true;

        while self.pos < self.chars.len() && self.chars[self.pos] == '\n' {
            self.advance();
            self.line += 1;
            self.col = 1;
        }

        Ok(Token {
            kind: TokenKind::Newline,
            span: self.span_from(start, 1),
        })
    }

    fn lex_string(&mut self) -> AxisResult<Token> {
        let start = self.pos;
        self.advance(); // skip opening "
        let mut value = String::new();
        while self.pos < self.chars.len() && self.chars[self.pos] != '"' {
            if self.chars[self.pos] == '\n' {
                return Err(AxisError::lex(self.line, self.col, "unterminated string literal"));
            }
            value.push(self.chars[self.pos]);
            self.advance();
        }
        if self.pos >= self.chars.len() {
            return Err(AxisError::lex(self.line, self.col, "unterminated string literal"));
        }
        self.advance(); // skip closing "
        let len = self.pos - start;
        Ok(Token {
            kind: TokenKind::StringLit(value),
            span: self.span_from(start, len),
        })
    }

    fn lex_path(&mut self) -> AxisResult<Token> {
        let start = self.pos;
        let mut path = String::new();
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch.is_ascii_alphanumeric() || ch == '/' || ch == ':' || ch == '_' || ch == '-' || ch == '.' {
                path.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        let len = self.pos - start;
        Ok(Token {
            kind: TokenKind::Path(path),
            span: self.span_from(start, len),
        })
    }

    fn lex_number(&mut self) -> AxisResult<Token> {
        let start = self.pos;
        let mut s = String::new();
        if self.chars[self.pos] == '-' {
            s.push('-');
            self.advance();
        }
        while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
            s.push(self.chars[self.pos]);
            self.advance();
        }
        if self.pos < self.chars.len() && self.chars[self.pos] == '.' && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            s.push('.');
            self.advance();
            while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
                s.push(self.chars[self.pos]);
                self.advance();
            }
            let len = self.pos - start;
            return Ok(Token {
                kind: TokenKind::DecimalLit(s),
                span: self.span_from(start, len),
            });
        }
        let len = self.pos - start;
        let n: i64 = s.parse().map_err(|_| AxisError::lex(self.line, self.col, format!("invalid integer: {s}")))?;
        Ok(Token {
            kind: TokenKind::IntLit(n),
            span: self.span_from(start, len),
        })
    }

    fn lex_upper_word(&mut self) -> AxisResult<Token> {
        let start = self.pos;
        let mut word = String::new();
        while self.pos < self.chars.len() && (self.chars[self.pos].is_ascii_alphanumeric() || self.chars[self.pos] == '_') {
            word.push(self.chars[self.pos]);
            self.advance();
        }
        while self.pos < self.chars.len()
            && self.chars[self.pos] == '-'
            && self.pos + 1 < self.chars.len()
            && self.chars[self.pos + 1].is_ascii_alphabetic()
        {
            word.push(self.chars[self.pos]);
            self.advance();
            while self.pos < self.chars.len() && (self.chars[self.pos].is_ascii_alphanumeric() || self.chars[self.pos] == '_') {
                word.push(self.chars[self.pos]);
                self.advance();
            }
        }
        let len = self.pos - start;
        let span = self.span_from(start, len);
        let kind = match word.as_str() {
            "SHAPE" => TokenKind::Shape,
            "SOURCE" => TokenKind::Source,
            "REALM" => TokenKind::Realm,
            "FLOW" => TokenKind::Flow,
            "SAGA" => TokenKind::Saga,
            "SURFACE" => TokenKind::Surface,
            "MIGRATE" => TokenKind::Migrate,
            "POLICY" => TokenKind::Policy,
            "SERVICE" => TokenKind::Service,
            "STORAGE" => TokenKind::Storage,
            "POSTGRES" => TokenKind::Postgres,
            "MYSQL" => TokenKind::Mysql,
            "SQLITE" => TokenKind::Sqlite,
            "REDIS" => TokenKind::Redis,
            "ELASTICSEARCH" => TokenKind::Elasticsearch,
            "DYNAMODB" => TokenKind::Dynamodb,
            "AUTH" => TokenKind::Auth,
            "BODY" => TokenKind::Body,
            "PARAM" => TokenKind::Param,
            "HEADER" => TokenKind::Header,
            "RULE" => TokenKind::Rule,
            "GUARD" => TokenKind::Guard,
            "LET" => TokenKind::Let,
            "FETCH" => TokenKind::Fetch,
            "QUERY" => TokenKind::Query,
            "INSERT" => TokenKind::Insert,
            "UPDATE" => TokenKind::Update,
            "DELETE" => TokenKind::Delete,
            "CALL" => TokenKind::Call,
            "EFFECT" => TokenKind::Effect,
            "MATCH" => TokenKind::Match,
            "WHEN" => TokenKind::When,
            "DEFAULT" => TokenKind::Default,
            "RETURN" => TokenKind::Return,
            "LIMIT" => TokenKind::Limit,
            "CACHE" => TokenKind::Cache,
            "SCOPE" => TokenKind::Scope,
            "REQUIRE" => TokenKind::Require,
            "FILTER" => TokenKind::Filter,
            "SORT" => TokenKind::Sort,
            "ASC" => TokenKind::Asc,
            "DESC" => TokenKind::Desc,
            "CURSOR" => TokenKind::Cursor,
            "PAGE_SIZE" => TokenKind::PageSize,
            "OR" => TokenKind::Or,
            "AND" => TokenKind::And,
            "NOT" => TokenKind::Not,
            "IF" => TokenKind::If,
            "THEN" => TokenKind::Then,
            "ELSE" => TokenKind::Else,
            "EQ" => TokenKind::Eq,
            "NEQ" => TokenKind::Neq,
            "GT" => TokenKind::Gt,
            "GTE" => TokenKind::Gte,
            "LT" => TokenKind::Lt,
            "LTE" => TokenKind::Lte,
            "IN" => TokenKind::In,
            "BETWEEN" => TokenKind::Between,
            "LIKE" => TokenKind::Like,
            "EMPTY" => TokenKind::Empty,
            "EXISTS" => TokenKind::Exists,
            "ADD" => TokenKind::Add,
            "SUB" => TokenKind::Sub,
            "MUL" => TokenKind::Mul,
            "DIV" => TokenKind::Div,
            "MOD" => TokenKind::Mod,
            "ROUND" => TokenKind::Round,
            "CEIL" => TokenKind::Ceil,
            "FLOOR" => TokenKind::Floor,
            "ABS" => TokenKind::Abs,
            "COUNT" => TokenKind::Count,
            "SUM" => TokenKind::Sum,
            "AVG" => TokenKind::Avg,
            "MIN" => TokenKind::Min,
            "MAX" => TokenKind::Max,
            "FIRST" => TokenKind::First,
            "LAST" => TokenKind::Last,
            "CONCAT" => TokenKind::Concat,
            "LOWER" => TokenKind::Lower,
            "UPPER" => TokenKind::Upper,
            "TRIM" => TokenKind::Trim,
            "SUBSTRING" => TokenKind::Substring,
            "LENGTH" => TokenKind::Length,
            "STARTS_WITH" => TokenKind::StartsWith,
            "ENDS_WITH" => TokenKind::EndsWith,
            "CONTAINS" => TokenKind::Contains,
            "DAYS_BETWEEN" => TokenKind::DaysBetween,
            "HOURS_BETWEEN" => TokenKind::HoursBetween,
            "MINUTES_BETWEEN" => TokenKind::MinutesBetween,
            "NOW" => TokenKind::Now,
            "NOW_PLUS" => TokenKind::NowPlus,
            "NOW_MINUS" => TokenKind::NowMinus,
            "FORMAT_DATE" => TokenKind::FormatDate,
            "COALESCE" => TokenKind::Coalesce,
            "LITERAL" => TokenKind::Literal,
            "TO_INT" => TokenKind::ToInt,
            "TO_DECIMAL" => TokenKind::ToDecimal,
            "TO_STRING" => TokenKind::ToString_,
            "UUID" => TokenKind::Uuid,
            "STRING" => TokenKind::String_,
            "TEXT" => TokenKind::Text,
            "INT" => TokenKind::Int,
            "DECIMAL" => TokenKind::Decimal,
            "BOOL" => TokenKind::Bool,
            "DATE" => TokenKind::Date,
            "TIMESTAMP" => TokenKind::Timestamp,
            "ENUM" => TokenKind::Enum,
            "REF" => TokenKind::Ref,
            "LIST" => TokenKind::List,
            "MAP" => TokenKind::Map,
            "JSON" => TokenKind::Json,
            "MAYBE" => TokenKind::Maybe,
            "PK" => TokenKind::Pk,
            "AUTO" => TokenKind::Auto,
            "REQUIRED" => TokenKind::Required,
            "UNIQUE" => TokenKind::Unique,
            "PRECISION" => TokenKind::Precision,
            "SCALE" => TokenKind::Scale,
            "INDEX" => TokenKind::Index,
            "TENANT" => TokenKind::Tenant,
            "CAPABILITY" => TokenKind::Capability,
            "FIELD" => TokenKind::Field,
            "HIDE" => TokenKind::Hide,
            "EXPOSE" => TokenKind::Expose,
            "DEPRECATE" => TokenKind::Deprecate,
            "ROUTE" => TokenKind::Route,
            "STEP" => TokenKind::Step,
            "VERIFY" => TokenKind::Verify,
            "COMPENSATE" => TokenKind::Compensate,
            "YIELD" => TokenKind::Yield_,
            "ON_SUCCESS" => TokenKind::OnSuccess,
            "ON_FAILURE" => TokenKind::OnFailure,
            "RUN_COMPENSATIONS" => TokenKind::RunCompensations,
            "TEMPLATE" => TokenKind::Template,
            "DATA" => TokenKind::Data,
            "TASK" => TokenKind::Task,
            "APPLIES_TO" => TokenKind::AppliesTo,
            "WRITES" => TokenKind::Writes,
            "READS" => TokenKind::Reads,
            "METHOD" => TokenKind::Method,
            "WHERE" => TokenKind::Where,
            "SET" => TokenKind::Set,
            "COPY" => TokenKind::Copy,
            "COMPUTE" => TokenKind::Compute,
            "DROP" => TokenKind::Drop,
            "ANY" => TokenKind::Any,
            "NONE" => TokenKind::None_,
            "TRUE" => TokenKind::True_,
            "FALSE" => TokenKind::False_,
            "HASH" => TokenKind::Hash,
            "ASYNC" => TokenKind::Async_,
            "STREAM" => TokenKind::Stream,
            "EACH" => TokenKind::Each,
            "REDUCE" => TokenKind::Reduce,
            "SPLIT" => TokenKind::Split,
            "REPLACE" => TokenKind::Replace,
            "FUNC" => TokenKind::Func,
            "TRY" => TokenKind::Try,
            "RECOVER" => TokenKind::Recover,
            "FORMAT" => TokenKind::Format,
            "SELECT" => TokenKind::Select,
            "EVENT" => TokenKind::Event,
            "WEBHOOK" => TokenKind::Webhook,
            "SIGNATURE" => TokenKind::Signature,
            "HMAC" => TokenKind::Hmac,
            "ITEMS" => TokenKind::Items,
            "TOTAL" => TokenKind::Total,
            "NEXT_CURSOR" => TokenKind::NextCursor,
            "HAS_MORE" => TokenKind::HasMore,
            "AS" => TokenKind::As,
            "TO" => TokenKind::To,
            "RENAME" => TokenKind::Rename,
            "SUNSET" => TokenKind::Sunset,
            "BASE_PATH" => TokenKind::BasePath,
            "ENDPOINT" => TokenKind::Endpoint,
            "VAULT" => TokenKind::Vault,
            "INPUT" => TokenKind::Input,
            "OUTPUT" => TokenKind::Output,
            "TIMEOUT" => TokenKind::Timeout,
            "RETRY" => TokenKind::Retry,
            "BACKOFF" => TokenKind::Backoff,
            "TTL" => TokenKind::Ttl,
            "VARY" => TokenKind::Vary,
            "WITH" => TokenKind::With,
            "PARALLEL" => TokenKind::Parallel,
            "RECEIVE" => TokenKind::Receive,
            "ON" => TokenKind::On,
            "BLOB" => TokenKind::Blob,
            "MULTIPART" => TokenKind::Multipart,
            "RENDER" => TokenKind::Render,
            "T" => TokenKind::Translate,
            "BACKEND" => TokenKind::Backend,
            "BUCKET" => TokenKind::Bucket,
            "PREFIX" => TokenKind::Prefix,
            "ACCESS" => TokenKind::Access,
            "MAX_SIZE" => TokenKind::MaxSize,
            "TYPES" => TokenKind::Types,
            "UPLOAD" => TokenKind::Upload,
            "GET" | "POST" | "PUT" | "PATCH" => {
                TokenKind::Ident(word.to_lowercase())
            }
            _ => {
                if word.chars().next().unwrap().is_ascii_uppercase()
                    && word.chars().any(|c| c.is_ascii_lowercase())
                {
                    TokenKind::ShapeName(word)
                } else {
                    TokenKind::Ident(word.to_lowercase())
                }
            }
        };
        Ok(Token { kind, span })
    }

    fn lex_lower_word(&mut self) -> AxisResult<Token> {
        let start = self.pos;
        let mut word = String::new();
        while self.pos < self.chars.len() && (self.chars[self.pos].is_ascii_alphanumeric() || self.chars[self.pos] == '_') {
            word.push(self.chars[self.pos]);
            self.advance();
        }
        let len = self.pos - start;
        let span = self.span_from(start, len);
        let kind = match word.as_str() {
            "true" => TokenKind::True_,
            "false" => TokenKind::False_,
            "none" => TokenKind::None_,
            "read" | "write" | "call" | "effect" | "admin" => TokenKind::Ident(word),
            "per_second" => TokenKind::Ident(word),
            "per_minute" => TokenKind::Ident(word),
            "per_hour" => TokenKind::Ident(word),
            "per_day" => TokenKind::Ident(word),
            "per_user" => TokenKind::Ident(word),
            "per_ip" => TokenKind::Ident(word),
            "per_key" => TokenKind::Ident(word),
            "global" => TokenKind::Ident(word),
            "session" => TokenKind::Ident(word),
            "bearer" => TokenKind::Ident(word),
            "api_key" => TokenKind::Ident(word),
            "seconds" => TokenKind::Seconds,
            "minutes" => TokenKind::Minutes,
            "hours" => TokenKind::Hours,
            "days" => TokenKind::Days,
            "weeks" => TokenKind::Weeks,
            "months" => TokenKind::Months,
            "years" => TokenKind::Years,
            "exponential" => TokenKind::Ident(word),
            "linear" => TokenKind::Ident(word),
            "backoff" => TokenKind::Backoff,
            "email" => TokenKind::Ident(word),
            "push_notification" => TokenKind::Ident(word),
            "webhook" => TokenKind::Ident(word),
            _ => TokenKind::Ident(word),
        };
        Ok(Token { kind, span })
    }

    fn skip_inline_whitespace(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos] == ' ' {
            self.advance();
        }
    }

    fn skip_comment(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
            self.advance();
        }
    }

    fn advance(&mut self) {
        self.pos += 1;
        self.col += 1;
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn make_token(&self, kind: TokenKind, len: usize) -> Token {
        Token {
            kind,
            span: Span {
                offset: self.pos,
                len,
                line: self.line,
                col: self.col,
            },
        }
    }

    fn span_from(&self, start: usize, len: usize) -> Span {
        Span {
            offset: start,
            len,
            line: self.line,
            col: self.col - len,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_shape() {
        let input = "SHAPE Booking\n  id UUID PK AUTO\n  status ENUM pending confirmed\n";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();

        assert!(matches!(kinds[0], TokenKind::Shape));
        assert!(matches!(kinds[1], TokenKind::ShapeName(s) if s == "Booking"));
        assert!(matches!(kinds[2], TokenKind::Newline));
        assert!(matches!(kinds[3], TokenKind::Indent));
        assert!(matches!(kinds[4], TokenKind::Ident(s) if s == "id"));
        assert!(matches!(kinds[5], TokenKind::Uuid));
        assert!(matches!(kinds[6], TokenKind::Pk));
        assert!(matches!(kinds[7], TokenKind::Auto));
    }

    #[test]
    fn test_lex_flow() {
        let input = "FLOW get_booking GET /bookings/:id\n  AUTH session\n  RETURN 200 booking\n";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();

        assert!(matches!(kinds[0], TokenKind::Flow));
        assert!(matches!(kinds[1], TokenKind::Ident(s) if s == "get_booking"));
        assert!(matches!(kinds[2], TokenKind::Ident(s) if s == "get"));
        assert!(matches!(kinds[3], TokenKind::Path(s) if s == "/bookings/:id"));
    }

    #[test]
    fn test_lex_numbers() {
        let input = "42 -1 99.95 0.12\n";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();

        assert!(matches!(&tokens[0].kind, TokenKind::IntLit(42)));
        assert!(matches!(&tokens[1].kind, TokenKind::IntLit(-1)));
        assert!(matches!(&tokens[2].kind, TokenKind::DecimalLit(s) if s == "99.95"));
        assert!(matches!(&tokens[3].kind, TokenKind::DecimalLit(s) if s == "0.12"));
    }

    #[test]
    fn test_lex_string() {
        let input = "\"hello world\"\n";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();

        assert!(matches!(&tokens[0].kind, TokenKind::StringLit(s) if s == "hello world"));
    }

    #[test]
    fn test_lex_indentation() {
        let input = "SHAPE Foo\n  id UUID\n    nested INT\n  bar BOOL\n";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();

        let indent_count = kinds.iter().filter(|k| matches!(k, TokenKind::Indent)).count();
        let dedent_count = kinds.iter().filter(|k| matches!(k, TokenKind::Dedent)).count();
        assert_eq!(indent_count, 2); // level 0→2, level 2→4
        assert!(dedent_count >= 2);  // level 4→2, level 2→0
    }

    #[test]
    fn test_reject_tabs() {
        let input = "SHAPE Foo\n\tid UUID\n";
        let mut lexer = Lexer::new(input);
        let result = lexer.tokenize();
        assert!(result.is_err());
    }

    #[test]
    fn test_lex_arrow() {
        let input = "ROUTE GET /bookings -> get_bookings\n";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Arrow));
    }

    #[test]
    fn test_lex_dot_path() {
        let input = "auth.user_id\n";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::Ident(s) if s == "auth"));
        assert!(matches!(&tokens[1].kind, TokenKind::Dot));
        assert!(matches!(&tokens[2].kind, TokenKind::Ident(s) if s == "user_id"));
    }

    #[test]
    fn test_lex_comment() {
        let input = "-- this is a comment\nSHAPE Foo\n";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::Newline));
        assert!(matches!(&tokens[1].kind, TokenKind::Shape));
    }
}
