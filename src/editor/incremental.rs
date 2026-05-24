use crate::ast::*;
use crate::constrain::{self, Constraint, ParseState};
use crate::token::{Token, TokenKind};

pub struct IncrementalResult {
    pub state: ParseState,
    pub valid_next: Constraint,
    pub constructs_so_far: usize,
    pub errors: Vec<IncrementalError>,
    pub partial_program: Option<Program>,
    pub complete: bool,
}

pub struct IncrementalError {
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub expected: Vec<String>,
}

pub fn validate_partial(source: &str) -> IncrementalResult {
    let mut lexer = crate::lexer::Lexer::new(source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            return IncrementalResult {
                state: ParseState::TopLevel,
                valid_next: constrain::valid_tokens(&ParseState::TopLevel),
                constructs_so_far: 0,
                errors: vec![IncrementalError {
                    line: 0,
                    col: 0,
                    message: format!("{e}"),
                    expected: Vec::new(),
                }],
                partial_program: None,
                complete: false,
            };
        }
    };

    let closed = source_ends_closed(source);
    let state = infer_state(&tokens, closed);
    let valid_next = constrain::valid_tokens(&state);

    let partial_program = try_parse_partial(source);
    let constructs_so_far = partial_program.as_ref()
        .map(|p| p.constructs.len())
        .unwrap_or(0);

    let errors = validate_tokens_incremental(&tokens);
    let complete = !tokens.is_empty()
        && tokens.last().is_some_and(|t| t.kind == TokenKind::Eof)
        && matches!(state, ParseState::TopLevel)
        && errors.is_empty();

    IncrementalResult {
        state,
        valid_next,
        constructs_so_far,
        errors,
        partial_program,
        complete,
    }
}

pub fn completions_at(source: &str) -> Vec<String> {
    let mut lexer = crate::lexer::Lexer::new(source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(_) => return constrain::valid_tokens(&ParseState::TopLevel).token_names(),
    };

    let closed = source_ends_closed(source);
    let state = infer_state(&tokens, closed);
    constrain::valid_tokens(&state).token_names()
}

fn source_ends_closed(source: &str) -> bool {
    let trimmed = source.trim_end_matches([' ', '\t']);
    trimmed.ends_with("\n\n") || trimmed.ends_with("\r\n\r\n")
}

fn infer_state(tokens: &[Token], source_closed: bool) -> ParseState {
    let mut state = ParseState::TopLevel;
    let skip_structural = |t: &TokenKind| matches!(t,
        TokenKind::ShapeName(_) | TokenKind::Path(_) |
        TokenKind::Arrow | TokenKind::Dot | TokenKind::Colon
    );

    let filtered: Vec<&Token> = tokens.iter()
        .filter(|t| !skip_structural(&t.kind))
        .collect();

    let has_trailing_indent = !source_closed && has_unclosed_indent(tokens);

    let mut i = 0;
    while i < filtered.len() {
        let token = filtered[i];

        if token.kind == TokenKind::Eof {
            break;
        }

        if matches!(token.kind, TokenKind::Indent | TokenKind::Newline) {
            i += 1;
            continue;
        }

        if has_trailing_indent && token.kind == TokenKind::Dedent {
            let remaining = filtered[i..].iter()
                .all(|t| matches!(t.kind, TokenKind::Dedent | TokenKind::Eof | TokenKind::Newline));
            if remaining {
                break;
            }
        }

        if matches!(state, ParseState::TopLevel) {
            match &token.kind {
                TokenKind::Shape => {
                    state = ParseState::ShapeBody;
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                    continue;
                }
                TokenKind::Source => {
                    state = ParseState::SourceBody;
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                    continue;
                }
                TokenKind::Realm => {
                    state = ParseState::RealmBody;
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                    continue;
                }
                TokenKind::Policy => {
                    state = ParseState::PolicyBody;
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                    continue;
                }
                TokenKind::Service => {
                    state = ParseState::ServiceBody;
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                    continue;
                }
                TokenKind::Migrate => {
                    state = ParseState::MigrateBody;
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                    continue;
                }
                TokenKind::Flow => {
                    state = ParseState::FlowBody;
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                    continue;
                }
                TokenKind::Saga => {
                    state = ParseState::SagaBody;
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                    continue;
                }
                TokenKind::Surface => {
                    state = ParseState::SurfaceBody;
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                    continue;
                }
                _ => {
                    i += 1;
                    continue;
                }
            }
        }

        if token.kind == TokenKind::Dedent {
            let next = constrain::next_states(&state, &TokenKind::Dedent);
            if let Some(s) = next.first() {
                state = s.clone();
            }
            i += 1;
            continue;
        }

        match (&state, &token.kind) {
            (ParseState::FlowBody, TokenKind::Realm) |
            (ParseState::FlowBody, TokenKind::Auth) |
            (ParseState::FlowBody, TokenKind::Scope) |
            (ParseState::FlowBody, TokenKind::Limit) |
            (ParseState::FlowBody, TokenKind::Cache) |
            (ParseState::FlowBody, TokenKind::Param) |
            (ParseState::FlowBody, TokenKind::Header) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::FlowBody, TokenKind::Body) |
            (ParseState::FlowBody, TokenKind::Rule) |
            (ParseState::FlowBody, TokenKind::Guard) |
            (ParseState::FlowBody, TokenKind::Let) |
            (ParseState::FlowBody, TokenKind::Insert) |
            (ParseState::FlowBody, TokenKind::Update) |
            (ParseState::FlowBody, TokenKind::Delete) |
            (ParseState::FlowBody, TokenKind::Effect) |
            (ParseState::FlowBody, TokenKind::Match) => {
                i += 1;
                skip_block(&filtered, &mut i);
                if i < filtered.len() && filtered[i].kind == TokenKind::As {
                    i += 1;
                    skip_to_newline(&filtered, &mut i);
                }
                continue;
            }
            (ParseState::FlowBody, TokenKind::Return) => {
                state = ParseState::TopLevel;
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::ShapeBody, _) if !matches!(token.kind, TokenKind::Dedent) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::SourceBody, _) if !matches!(token.kind, TokenKind::Dedent) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::RealmBody, _) if !matches!(token.kind, TokenKind::Dedent) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::PolicyBody, _) if !matches!(token.kind, TokenKind::Dedent) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::ServiceBody, TokenKind::Method) => {
                state = ParseState::ServiceMethodBody;
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::ServiceBody, _) if !matches!(token.kind, TokenKind::Dedent) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::ServiceMethodBody, _) if !matches!(token.kind, TokenKind::Dedent) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::SurfaceBody, TokenKind::Expose) => {
                i += 1;
                skip_block(&filtered, &mut i);
                continue;
            }
            (ParseState::SurfaceBody, _) if !matches!(token.kind, TokenKind::Dedent) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::MigrateBody, _) if !matches!(token.kind, TokenKind::Dedent) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            (ParseState::SagaBody, TokenKind::Body) |
            (ParseState::SagaBody, TokenKind::Step) |
            (ParseState::SagaBody, TokenKind::OnFailure) |
            (ParseState::SagaBody, TokenKind::OnSuccess) => {
                i += 1;
                skip_block(&filtered, &mut i);
                continue;
            }
            (ParseState::SagaBody, _) if !matches!(token.kind, TokenKind::Dedent) => {
                i += 1;
                skip_to_newline(&filtered, &mut i);
                continue;
            }
            _ => {
                let next = constrain::next_states(&state, &token.kind);
                if let Some(s) = next.first() {
                    state = s.clone();
                }
                i += 1;
            }
        }
    }

    state
}

fn has_unclosed_indent(tokens: &[Token]) -> bool {
    let last_content = tokens.iter().rposition(|t|
        !matches!(t.kind, TokenKind::Eof | TokenKind::Dedent | TokenKind::Newline));
    if let Some(pos) = last_content {
        let indent_at_last = tokens[..=pos].iter()
            .map(|t| match t.kind {
                TokenKind::Indent => 1i32,
                TokenKind::Dedent => -1,
                _ => 0,
            })
            .sum::<i32>();
        if indent_at_last <= 0 {
            return false;
        }
        let trailing = &tokens[(pos + 1)..];
        let mut seen_dedent = false;
        for t in trailing {
            match t.kind {
                TokenKind::Dedent => seen_dedent = true,
                TokenKind::Newline if seen_dedent => return false,
                _ => {}
            }
        }
        true
    } else {
        false
    }
}

fn skip_to_newline(tokens: &[&Token], i: &mut usize) {
    while *i < tokens.len() && !matches!(tokens[*i].kind, TokenKind::Newline | TokenKind::Eof) {
        *i += 1;
    }
    if *i < tokens.len() && tokens[*i].kind == TokenKind::Newline {
        *i += 1;
    }
}

fn skip_block(tokens: &[&Token], i: &mut usize) {
    skip_to_newline(tokens, i);
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

fn try_parse_partial(source: &str) -> Option<Program> {
    let trimmed = ensure_complete(source);
    let mut lexer = crate::lexer::Lexer::new(&trimmed);
    let tokens = lexer.tokenize().ok()?;
    let mut parser = crate::parser::Parser::new(tokens);
    parser.parse_program().ok()
}

fn ensure_complete(source: &str) -> String {
    let mut s = source.to_string();
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

fn validate_tokens_incremental(tokens: &[Token]) -> Vec<IncrementalError> {
    let mut errors = Vec::new();
    let mut state = ParseState::TopLevel;
    let mut indent_depth: i32 = 0;

    for token in tokens {
        match &token.kind {
            TokenKind::Indent => indent_depth += 1,
            TokenKind::Dedent => indent_depth -= 1,
            TokenKind::Newline | TokenKind::Eof => {}
            TokenKind::ShapeName(_) | TokenKind::Path(_) |
            TokenKind::Arrow | TokenKind::Dot | TokenKind::Colon => {}
            kind => {
                if matches!(state, ParseState::TopLevel) {
                    let constraint = constrain::valid_tokens(&state);
                    if !constraint.allows(kind) && !matches!(kind, TokenKind::Ident(_) | TokenKind::IntLit(_) | TokenKind::DecimalLit(_) | TokenKind::StringLit(_)) {
                        errors.push(IncrementalError {
                            line: token.span.line,
                            col: token.span.col,
                            message: format!("unexpected {kind} at top level"),
                            expected: constraint.token_names(),
                        });
                    } else {
                        let next = constrain::next_states(&state, kind);
                        if let Some(s) = next.first() {
                            state = s.clone();
                        }
                    }
                }
            }
        }
    }

    if indent_depth < 0 {
        errors.push(IncrementalError {
            line: 0,
            col: 0,
            message: "unbalanced indentation: more dedents than indents".to_string(),
            expected: Vec::new(),
        });
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let result = validate_partial("");
        assert_eq!(result.state, ParseState::TopLevel);
        assert!(result.valid_next.allows(&TokenKind::Shape));
        assert!(result.valid_next.allows(&TokenKind::Flow));
        assert_eq!(result.constructs_so_far, 0);
    }

    #[test]
    fn test_shape_still_open() {
        let result = validate_partial("SHAPE User\n  id UUID PK AUTO\n  name STRING 100 REQUIRED\n");
        assert_eq!(result.state, ParseState::ShapeBody);
        assert!(!result.complete);
        assert_eq!(result.constructs_so_far, 1);
    }

    #[test]
    fn test_complete_shape() {
        let result = validate_partial("SHAPE User\n  id UUID PK AUTO\n  name STRING 100 REQUIRED\n\n");
        assert_eq!(result.state, ParseState::TopLevel);
        assert!(result.complete);
        assert_eq!(result.constructs_so_far, 1);
    }

    #[test]
    fn test_partial_shape_header() {
        let result = validate_partial("SHAPE User\n");
        assert_eq!(result.state, ParseState::ShapeBody);
        assert!(result.valid_next.allows(&TokenKind::Ident("field".into())));
        assert!(result.valid_next.allows(&TokenKind::Dedent));
        assert!(!result.complete);
    }

    #[test]
    fn test_partial_shape_body() {
        let result = validate_partial("SHAPE User\n  id UUID PK AUTO\n");
        assert_eq!(result.state, ParseState::ShapeBody);
        assert!(!result.complete);
    }

    #[test]
    fn test_partial_flow_header() {
        let result = validate_partial("FLOW get_user get /users/:id\n");
        assert_eq!(result.state, ParseState::FlowBody);
        assert!(result.valid_next.allows(&TokenKind::Auth));
        assert!(result.valid_next.allows(&TokenKind::Let));
        assert!(result.valid_next.allows(&TokenKind::Return));
    }

    #[test]
    fn test_partial_flow_after_let() {
        let result = validate_partial(r#"SHAPE User
  id UUID PK AUTO

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
"#);
        assert_eq!(result.state, ParseState::FlowBody);
        assert!(result.valid_next.allows(&TokenKind::Return));
        assert!(result.valid_next.allows(&TokenKind::Guard));
    }

    #[test]
    fn test_complete_flow() {
        let result = validate_partial(r#"SHAPE User
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
        assert_eq!(result.state, ParseState::TopLevel);
        assert!(result.complete);
        assert_eq!(result.constructs_so_far, 4);
    }

    #[test]
    fn test_completions_at_top_level() {
        let completions = completions_at("");
        assert!(completions.contains(&"SHAPE".to_string()));
        assert!(completions.contains(&"FLOW".to_string()));
        assert!(completions.contains(&"SOURCE".to_string()));
    }

    #[test]
    fn test_completions_in_flow_body() {
        let completions = completions_at("FLOW get_user get /users/:id\n");
        assert!(completions.contains(&"AUTH".to_string()));
        assert!(completions.contains(&"LET".to_string()));
        assert!(completions.contains(&"RETURN".to_string()));
    }

    #[test]
    fn test_completions_in_shape_body() {
        let completions = completions_at("SHAPE User\n");
        assert!(completions.contains(&"<identifier>".to_string()));
        assert!(completions.contains(&"DEDENT".to_string()));
    }

    #[test]
    fn test_completions_after_complete_shape() {
        let completions = completions_at("SHAPE User\n  id UUID PK AUTO\n\n");
        assert!(completions.contains(&"SHAPE".to_string()));
        assert!(completions.contains(&"FLOW".to_string()));
    }

    #[test]
    fn test_partial_program_available() {
        let result = validate_partial(r#"SHAPE User
  id UUID PK AUTO

SHAPE Order
  id UUID PK AUTO
"#);
        assert!(result.partial_program.is_some());
        assert_eq!(result.constructs_so_far, 2);
    }

    #[test]
    fn test_multiple_constructs_partial() {
        let result = validate_partial(r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM api
  CAPABILITY read users

FLOW get_user get /users/:id
  REALM api
  AUTH session
"#);
        assert_eq!(result.state, ParseState::FlowBody);
        assert!(!result.complete);
        assert!(result.constructs_so_far >= 3);
    }

    #[test]
    fn test_source_body_state() {
        let result = validate_partial("SOURCE users POSTGRES\n");
        assert_eq!(result.state, ParseState::SourceBody);
        assert!(result.valid_next.allows(&TokenKind::Shape));
        assert!(result.valid_next.allows(&TokenKind::Index));
    }

    #[test]
    fn test_saga_body_state() {
        let result = validate_partial("SAGA process POST /process\n");
        assert_eq!(result.state, ParseState::SagaBody);
        assert!(result.valid_next.allows(&TokenKind::Step));
        assert!(result.valid_next.allows(&TokenKind::Auth));
    }

    #[test]
    fn test_surface_body_state() {
        let result = validate_partial("SURFACE public v1\n");
        assert_eq!(result.state, ParseState::SurfaceBody);
        assert!(result.valid_next.allows(&TokenKind::Route));
        assert!(result.valid_next.allows(&TokenKind::Expose));
    }
}
