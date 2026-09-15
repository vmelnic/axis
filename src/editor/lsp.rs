use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::*;

use crate::ast::{self, Construct, EffectField, Expr, FlowStep, Program};
use crate::incremental;
use crate::project;
use crate::verify::Verifier;

pub fn run_lsp() -> Result<(), Box<dyn Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec!["\n".into(), " ".into()]),
            ..Default::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };

    let init_params = serde_json::to_value(InitializeResult {
        capabilities,
        server_info: Some(ServerInfo {
            name: "axis-ls".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
    })?;

    connection.initialize(init_params)?;

    let mut state = ServerState::new();
    main_loop(&connection, &mut state)?;
    io_threads.join()?;
    Ok(())
}

struct ConstructEntry {
    name: String,
    kind: ConstructKind,
    span: crate::token::Span,
    path: PathBuf,
    construct: Construct,
}

struct WorkspaceIndex {
    entries: Vec<ConstructEntry>,
}

impl WorkspaceIndex {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    #[allow(clippy::mutable_key_type)]
    fn rebuild(&mut self, root: &std::path::Path, open_docs: &HashMap<Uri, String>) {
        self.entries.clear();
        let paths = project::collect_axis_files(root);
        for path in &paths {
            let source = if let Some(uri) = path_to_uri(path) {
                if let Some(doc) = open_docs.get(&uri) {
                    doc.clone()
                } else {
                    match std::fs::read_to_string(path) {
                        Ok(s) => s,
                        Err(_) => continue,
                    }
                }
            } else {
                match std::fs::read_to_string(path) {
                    Ok(s) => s,
                    Err(_) => continue,
                }
            };

            let program = match project::compile_source(&source) {
                Ok(p) => p,
                Err(_) => continue,
            };

            for c in program.constructs {
                if let Some((name, kind, span)) = construct_identity(&c) {
                    self.entries.push(ConstructEntry {
                        name,
                        kind,
                        span,
                        path: path.clone(),
                        construct: c,
                    });
                }
            }
        }
    }

    fn find_definition(&self, name: &str, kind: ConstructKind) -> Option<&ConstructEntry> {
        self.entries
            .iter()
            .find(|e| e.name == name && e.kind == kind)
    }

    fn find_definition_any(&self, name: &str) -> Option<&ConstructEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    fn find_references(&self, name: &str) -> Vec<(PathBuf, crate::token::Span)> {
        let mut results = Vec::new();
        for entry in &self.entries {
            let refs = collect_references(&entry.construct, name);
            for span in refs {
                results.push((entry.path.clone(), span));
            }
        }
        results
    }

    fn merged_program(&self) -> Program {
        Program {
            constructs: self.entries.iter().map(|e| e.construct.clone()).collect(),
        }
    }
}

fn construct_identity(c: &Construct) -> Option<(String, ConstructKind, crate::token::Span)> {
    match c {
        Construct::Shape(s) => Some((s.name.clone(), ConstructKind::Shape, s.span)),
        Construct::Source(s) => Some((s.name.clone(), ConstructKind::Source, s.span)),
        Construct::Realm(r) => Some((r.name.clone(), ConstructKind::Realm, r.span)),
        Construct::Flow(f) => Some((f.name.clone(), ConstructKind::Flow, f.span)),
        Construct::Service(s) => Some((s.name.clone(), ConstructKind::Service, s.span)),
        Construct::Saga(s) => Some((s.name.clone(), ConstructKind::Saga, s.span)),
        Construct::Policy(p) => Some((p.name.clone(), ConstructKind::Policy, p.span)),
        Construct::Surface(s) => Some((s.name.clone(), ConstructKind::Surface, s.span)),
        Construct::Migrate(m) => Some((m.shape.clone(), ConstructKind::Migrate, m.span)),
        Construct::Stream(s) => Some((s.name.clone(), ConstructKind::Stream, s.span)),
        Construct::Func(f) => Some((f.name.clone(), ConstructKind::Func, f.span)),
        Construct::Storage(s) => Some((s.name.clone(), ConstructKind::Storage, s.span)),
    }
}

struct ServerState {
    documents: HashMap<Uri, String>,
    workspace_root: Option<PathBuf>,
    index: WorkspaceIndex,
}

impl ServerState {
    fn new() -> Self {
        Self {
            documents: HashMap::new(),
            workspace_root: None,
            index: WorkspaceIndex::new(),
        }
    }

    fn rebuild_index(&mut self) {
        if let Some(root) = &self.workspace_root {
            let root = root.clone();
            self.index.rebuild(&root, &self.documents);
        }
    }

    fn merged_program(&self) -> Program {
        if !self.index.entries.is_empty() {
            return self.index.merged_program();
        }
        let mut constructs = Vec::new();
        for source in self.documents.values() {
            if let Ok(prog) = project::compile_source(source) {
                constructs.extend(prog.constructs);
            }
        }
        Program { constructs }
    }
}

fn main_loop(
    connection: &Connection,
    state: &mut ServerState,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                handle_request(connection, state, req)?;
            }
            Message::Notification(notif) => {
                handle_notification(connection, state, notif)?;
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn handle_request(
    connection: &Connection,
    state: &mut ServerState,
    req: Request,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    match req.method.as_str() {
        "textDocument/completion" => {
            let params: CompletionParams = serde_json::from_value(req.params)?;
            let result = handle_completion(state, &params);
            let resp = Response::new_ok(req.id, result);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/definition" => {
            let params: GotoDefinitionParams = serde_json::from_value(req.params)?;
            let result = handle_goto_definition(state, &params);
            let resp = Response::new_ok(req.id, result);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/references" => {
            let params: ReferenceParams = serde_json::from_value(req.params)?;
            let result = handle_references(state, &params);
            let resp = Response::new_ok(req.id, result);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/formatting" => {
            let params: DocumentFormattingParams = serde_json::from_value(req.params)?;
            let result = handle_formatting(state, &params);
            let resp = Response::new_ok(req.id, result);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/hover" => {
            let params: HoverParams = serde_json::from_value(req.params)?;
            let result = handle_hover(state, &params);
            let resp = Response::new_ok(req.id, result);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/documentSymbol" => {
            let params: DocumentSymbolParams = serde_json::from_value(req.params)?;
            let result = handle_document_symbols(state, &params);
            let resp = Response::new_ok(req.id, result);
            connection.sender.send(Message::Response(resp))?;
        }
        "workspace/symbol" => {
            let params: WorkspaceSymbolParams = serde_json::from_value(req.params)?;
            let result = handle_workspace_symbol(state, &params);
            let resp = Response::new_ok(req.id, result);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/prepareRename" => {
            let params: TextDocumentPositionParams = serde_json::from_value(req.params)?;
            let result = handle_prepare_rename(state, &params);
            let resp = Response::new_ok(req.id, result);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/rename" => {
            let params: RenameParams = serde_json::from_value(req.params)?;
            let result = handle_rename(state, &params);
            let resp = Response::new_ok(req.id, result);
            connection.sender.send(Message::Response(resp))?;
        }
        _ => {
            let resp = Response::new_err(
                req.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                format!("unhandled method: {}", req.method),
            );
            connection.sender.send(Message::Response(resp))?;
        }
    }
    Ok(())
}

fn handle_notification(
    connection: &Connection,
    state: &mut ServerState,
    notif: Notification,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    match notif.method.as_str() {
        "initialized" => {
            if let Some((uri, _)) = state.documents.iter().next() {
                if let Some(p) = uri_to_path(uri) {
                    state.workspace_root = find_workspace_root(&p);
                }
            }
        }
        "textDocument/didOpen" => {
            let params: DidOpenTextDocumentParams = serde_json::from_value(notif.params)?;
            let uri = params.text_document.uri.clone();
            state
                .documents
                .insert(uri.clone(), params.text_document.text);
            if state.workspace_root.is_none() {
                if let Some(p) = uri_to_path(&uri) {
                    state.workspace_root = find_workspace_root(&p);
                }
            }
            state.rebuild_index();
            publish_diagnostics_all(connection, state)?;
        }
        "textDocument/didChange" => {
            let params: DidChangeTextDocumentParams = serde_json::from_value(notif.params)?;
            let uri = params.text_document.uri.clone();
            if let Some(change) = params.content_changes.into_iter().last() {
                state.documents.insert(uri, change.text);
            }
            state.rebuild_index();
            publish_diagnostics_all(connection, state)?;
        }
        "textDocument/didClose" => {
            let params: DidCloseTextDocumentParams = serde_json::from_value(notif.params)?;
            let uri = params.text_document.uri.clone();
            state.documents.remove(&uri);
            let clear = PublishDiagnosticsParams {
                uri,
                diagnostics: Vec::new(),
                version: None,
            };
            connection
                .sender
                .send(Message::Notification(Notification::new(
                    "textDocument/publishDiagnostics".into(),
                    clear,
                )))?;
        }
        "textDocument/didSave" => {
            state.rebuild_index();
            publish_diagnostics_all(connection, state)?;
        }
        _ => {}
    }
    Ok(())
}

fn publish_diagnostics_all(
    connection: &Connection,
    state: &ServerState,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    for uri in state.documents.keys() {
        publish_diagnostics(connection, state, uri)?;
    }

    if let Some(root) = &state.workspace_root {
        let result = project::compile_project(root);
        if result.is_ok() {
            let verifier = Verifier::new();
            let verify_result = verifier.verify(&result.program);
            for e in verify_result
                .errors
                .iter()
                .chain(verify_result.warnings.iter())
            {
                for file in &result.files {
                    if let Ok(file_uri) = format!("file://{}", file.path.display()).parse::<Uri>() {
                        if !state.documents.contains_key(&file_uri) {
                            let severity =
                                if verify_result.errors.iter().any(|ve| std::ptr::eq(ve, e)) {
                                    DiagnosticSeverity::ERROR
                                } else {
                                    DiagnosticSeverity::WARNING
                                };
                            let params = PublishDiagnosticsParams {
                                uri: file_uri,
                                diagnostics: vec![Diagnostic {
                                    range: span_to_range(e.span),
                                    severity: Some(severity),
                                    source: Some("axis".into()),
                                    message: e.message.clone(),
                                    ..Default::default()
                                }],
                                version: None,
                            };
                            connection
                                .sender
                                .send(Message::Notification(Notification::new(
                                    "textDocument/publishDiagnostics".into(),
                                    params,
                                )))?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn publish_diagnostics(
    connection: &Connection,
    state: &ServerState,
    uri: &Uri,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let source = match state.documents.get(uri) {
        Some(s) => s,
        None => return Ok(()),
    };

    let mut diagnostics = Vec::new();

    match project::compile_source(source) {
        Ok(program) => {
            let merged = state.merged_program();
            let merged = if merged.constructs.is_empty() {
                program
            } else {
                merged
            };
            let verifier = Verifier::new();
            let result = verifier.verify(&merged);
            for e in &result.errors {
                if span_in_source(e.span, source) {
                    diagnostics.push(Diagnostic {
                        range: span_to_range(e.span),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some("axis".into()),
                        message: e.message.clone(),
                        related_information: e.hint.as_ref().map(|h| {
                            vec![DiagnosticRelatedInformation {
                                location: Location {
                                    uri: uri.clone(),
                                    range: span_to_range(e.span),
                                },
                                message: h.clone(),
                            }]
                        }),
                        ..Default::default()
                    });
                }
            }
            for w in &result.warnings {
                if span_in_source(w.span, source) {
                    diagnostics.push(Diagnostic {
                        range: span_to_range(w.span),
                        severity: Some(DiagnosticSeverity::WARNING),
                        source: Some("axis".into()),
                        message: w.message.clone(),
                        ..Default::default()
                    });
                }
            }
        }
        Err(e) => {
            let (line, col, msg) = error_position(&e);
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position {
                        line: line.saturating_sub(1) as u32,
                        character: col as u32,
                    },
                    end: Position {
                        line: line.saturating_sub(1) as u32,
                        character: (col + 1) as u32,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("axis".into()),
                message: msg,
                ..Default::default()
            });
        }
    }

    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics,
        version: None,
    };
    connection
        .sender
        .send(Message::Notification(Notification::new(
            "textDocument/publishDiagnostics".into(),
            params,
        )))?;
    Ok(())
}

fn handle_completion(state: &ServerState, params: &CompletionParams) -> Option<CompletionResponse> {
    let uri = &params.text_document_position.text_document.uri;
    let source = state.documents.get(uri)?;
    let pos = params.text_document_position.position;

    let prefix = source_up_to(source, pos.line as usize, pos.character as usize);
    let completions = incremental::completions_at(&prefix);

    let mut items: Vec<CompletionItem> = completions
        .into_iter()
        .map(|label| {
            let kind = if label.chars().all(|c| c.is_uppercase() || c == '_') {
                CompletionItemKind::KEYWORD
            } else {
                CompletionItemKind::TEXT
            };
            CompletionItem {
                label,
                kind: Some(kind),
                ..Default::default()
            }
        })
        .collect();

    let line_str = source.lines().nth(pos.line as usize).unwrap_or("");
    let trimmed = line_str.trim();
    let ctx = completion_context(trimmed);
    let ctx_items = workspace_completions(state, ctx);
    items.extend(ctx_items);

    Some(CompletionResponse::Array(items))
}

#[derive(Debug, Clone, Copy)]
enum CompletionContext {
    ShapeName,
    SourceName,
    RealmName,
    ServiceMethod,
    FieldName,
    None,
}

fn completion_context(line: &str) -> CompletionContext {
    let upper = line.to_uppercase();
    if upper.starts_with("SHAPE ") && upper.contains("SOURCE") {
        return CompletionContext::ShapeName;
    }
    if upper.starts_with("SHAPE") || upper.contains("SHAPE ") {
        return CompletionContext::ShapeName;
    }
    if upper.starts_with("FETCH ")
        || upper.starts_with("QUERY ")
        || upper.starts_with("INSERT ")
        || upper.starts_with("UPSERT ")
        || upper.starts_with("UPDATE ")
        || upper.starts_with("DELETE ")
    {
        return CompletionContext::SourceName;
    }
    if upper.starts_with("REALM ") && !upper.contains("CAPABILITY") {
        return CompletionContext::RealmName;
    }
    if upper.starts_with("CALL ") && !upper.contains("WASM") {
        return CompletionContext::ServiceMethod;
    }
    if upper.starts_with("FILTER ") {
        return CompletionContext::FieldName;
    }
    CompletionContext::None
}

fn workspace_completions(state: &ServerState, ctx: CompletionContext) -> Vec<CompletionItem> {
    let kind_filter = match ctx {
        CompletionContext::ShapeName => Some(ConstructKind::Shape),
        CompletionContext::SourceName => Some(ConstructKind::Source),
        CompletionContext::RealmName => Some(ConstructKind::Realm),
        CompletionContext::ServiceMethod => Some(ConstructKind::Service),
        CompletionContext::FieldName => return field_completions(state),
        CompletionContext::None => return Vec::new(),
    };

    let Some(filter) = kind_filter else {
        return Vec::new();
    };
    state
        .index
        .entries
        .iter()
        .filter(|e| e.kind == filter)
        .map(|e| CompletionItem {
            label: e.name.clone(),
            kind: Some(match filter {
                ConstructKind::Shape => CompletionItemKind::STRUCT,
                ConstructKind::Source => CompletionItemKind::MODULE,
                ConstructKind::Realm => CompletionItemKind::ENUM,
                ConstructKind::Service => CompletionItemKind::INTERFACE,
                _ => CompletionItemKind::TEXT,
            }),
            detail: Some(format!("{:?}", filter)),
            ..Default::default()
        })
        .collect()
}

fn field_completions(state: &ServerState) -> Vec<CompletionItem> {
    let mut fields = Vec::new();
    for entry in &state.index.entries {
        if entry.kind == ConstructKind::Shape {
            if let Construct::Shape(shape) = &entry.construct {
                for field in &shape.fields {
                    fields.push(CompletionItem {
                        label: field.name.clone(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(format!("{}.{}", shape.name, field.name)),
                        ..Default::default()
                    });
                }
            }
        }
    }
    fields
}

fn handle_goto_definition(
    state: &ServerState,
    params: &GotoDefinitionParams,
) -> Option<GotoDefinitionResponse> {
    let uri = &params.text_document_position_params.text_document.uri;
    let source = state.documents.get(uri)?;
    let pos = params.text_document_position_params.position;

    let line_str = source.lines().nth(pos.line as usize)?;
    let word = word_at(line_str, pos.character as usize)?;

    let context = reference_context(source, pos.line as usize, &word);

    let kind_filter = match context {
        RefContext::ShapeName => Some(ConstructKind::Shape),
        RefContext::SourceName => Some(ConstructKind::Source),
        RefContext::RealmName => Some(ConstructKind::Realm),
        RefContext::FlowName => Some(ConstructKind::Flow),
        RefContext::ServiceName => Some(ConstructKind::Service),
        RefContext::Unknown => None,
    };

    if let Some(kind) = kind_filter {
        if let Some(entry) = state.index.find_definition(&word, kind) {
            let target_uri = path_to_uri(&entry.path).unwrap_or_else(|| uri.clone());
            return Some(goto_location(target_uri, entry.span));
        }
    }

    if let Some(entry) = state.index.find_definition_any(&word) {
        let target_uri = path_to_uri(&entry.path).unwrap_or_else(|| uri.clone());
        return Some(goto_location(target_uri, entry.span));
    }

    let local_prog = project::compile_source(source).ok()?;
    if let Some(kind) = kind_filter {
        find_construct_span(&local_prog, &word, kind).map(|span| goto_location(uri.clone(), span))
    } else {
        for kind in &[
            ConstructKind::Shape,
            ConstructKind::Source,
            ConstructKind::Realm,
            ConstructKind::Flow,
            ConstructKind::Service,
        ] {
            if let Some(span) = find_construct_span(&local_prog, &word, *kind) {
                return Some(goto_location(uri.clone(), span));
            }
        }
        None
    }
}

fn handle_references(state: &ServerState, params: &ReferenceParams) -> Option<Vec<Location>> {
    let uri = &params.text_document_position.text_document.uri;
    let source = state.documents.get(uri)?;
    let pos = params.text_document_position.position;

    let line_str = source.lines().nth(pos.line as usize)?;
    let word = word_at(line_str, pos.character as usize)?;

    let mut locations = Vec::new();

    let workspace_refs = state.index.find_references(&word);
    for (path, span) in &workspace_refs {
        let ref_uri = path_to_uri(path).unwrap_or_else(|| uri.clone());
        locations.push(Location {
            uri: ref_uri,
            range: span_to_range(*span),
        });
    }

    if locations.is_empty() {
        let program = project::compile_source(source).ok()?;
        for c in &program.constructs {
            let refs = collect_references(c, &word);
            for span in refs {
                locations.push(Location {
                    uri: uri.clone(),
                    range: span_to_range(span),
                });
            }
        }
    }

    if locations.is_empty() {
        None
    } else {
        Some(locations)
    }
}

fn handle_document_symbols(
    state: &ServerState,
    params: &DocumentSymbolParams,
) -> Option<DocumentSymbolResponse> {
    let uri = &params.text_document.uri;
    let source = state.documents.get(uri)?;
    let program = project::compile_source(source).ok()?;

    let symbols: Vec<SymbolInformation> = program
        .constructs
        .iter()
        .map(|c| {
            let (name, kind, span) = match c {
                Construct::Shape(s) => (s.name.clone(), SymbolKind::STRUCT, s.span),
                Construct::Source(s) => (s.name.clone(), SymbolKind::NAMESPACE, s.span),
                Construct::Realm(r) => (r.name.clone(), SymbolKind::MODULE, r.span),
                Construct::Policy(p) => (p.name.clone(), SymbolKind::INTERFACE, p.span),
                Construct::Service(s) => (s.name.clone(), SymbolKind::CLASS, s.span),
                Construct::Flow(f) => (f.name.clone(), SymbolKind::FUNCTION, f.span),
                Construct::Saga(s) => (s.name.clone(), SymbolKind::FUNCTION, s.span),
                Construct::Surface(s) => (s.name.clone(), SymbolKind::PACKAGE, s.span),
                Construct::Migrate(m) => (m.shape.clone(), SymbolKind::EVENT, m.span),
                Construct::Stream(s) => (s.name.clone(), SymbolKind::EVENT, s.span),
                Construct::Func(f) => (f.name.clone(), SymbolKind::FUNCTION, f.span),
                Construct::Storage(s) => (s.name.clone(), SymbolKind::NAMESPACE, s.span),
            };
            #[allow(deprecated)]
            SymbolInformation {
                name,
                kind,
                tags: None,
                deprecated: None,
                location: Location {
                    uri: uri.clone(),
                    range: span_to_range(span),
                },
                container_name: None,
            }
        })
        .collect();

    Some(DocumentSymbolResponse::Flat(symbols))
}

fn handle_formatting(
    state: &ServerState,
    params: &DocumentFormattingParams,
) -> Option<Vec<TextEdit>> {
    let uri = &params.text_document.uri;
    let source = state.documents.get(uri)?;

    let program = project::compile_source(source).ok()?;
    let formatted = crate::fmt::format_program(&program);

    if formatted == *source {
        return Some(Vec::new());
    }

    let line_count = source.lines().count().max(1);
    Some(vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: line_count as u32,
                character: 0,
            },
        },
        new_text: formatted,
    }])
}

fn handle_hover(state: &ServerState, params: &HoverParams) -> Option<Hover> {
    let uri = &params.text_document_position_params.text_document.uri;
    let source = state.documents.get(uri)?;
    let pos = params.text_document_position_params.position;

    let line = source.lines().nth(pos.line as usize)?;
    let word = word_at(line, pos.character as usize)?;

    let program = state.merged_program();
    let fallback;
    let constructs = if !program.constructs.is_empty() {
        &program.constructs
    } else {
        fallback = project::compile_source(source).ok()?;
        &fallback.constructs
    };

    for c in constructs {
        match c {
            Construct::Shape(s) if s.name == word => {
                let fields: Vec<String> = s
                    .fields
                    .iter()
                    .map(|f| format!("  {} {}", f.name, format_type(&f.ty)))
                    .collect();
                let md = format!("**SHAPE** `{}`\n\n```\n{}\n```", s.name, fields.join("\n"));
                return Some(hover_md(md));
            }
            Construct::Source(s) if s.name == word => {
                let idx: Vec<String> = s
                    .indexes
                    .iter()
                    .map(|ix| {
                        let fields: Vec<String> = ix
                            .fields
                            .iter()
                            .map(|f| {
                                if let Some(ref suf) = f.suffix {
                                    format!("{} {suf:?}", f.name)
                                } else {
                                    f.name.clone()
                                }
                            })
                            .collect();
                        format!("  INDEX {}", fields.join(" "))
                    })
                    .collect();
                let md = format!(
                    "**SOURCE** `{}` ({:?})\n\n```\nSHAPE {}\n{}\n```",
                    s.name,
                    s.source_type,
                    s.shape,
                    idx.join("\n")
                );
                return Some(hover_md(md));
            }
            Construct::Flow(f) if f.name == word => {
                let method = format!("{:?}", f.method).to_uppercase();
                let auth = f
                    .auth
                    .as_ref()
                    .map(|a| format!("\n  AUTH {a:?}"))
                    .unwrap_or_default();
                let realm = f
                    .realm
                    .as_ref()
                    .map(|r| format!("\n  REALM {r}"))
                    .unwrap_or_default();
                let md = format!(
                    "**FLOW** `{}` `{} {}`{}{}",
                    f.name, method, f.path, realm, auth
                );
                return Some(hover_md(md));
            }
            Construct::Realm(r) if r.name == word => {
                let caps: Vec<String> = r
                    .capabilities
                    .iter()
                    .map(|c| format!("  {:?} {}", c.kind, c.target))
                    .collect();
                let tenant = r
                    .tenant
                    .as_ref()
                    .map(|t| format!("  TENANT {t}\n"))
                    .unwrap_or_default();
                let md = format!(
                    "**REALM** `{}`\n\n```\n{}{}\n```",
                    r.name,
                    tenant,
                    caps.join("\n")
                );
                return Some(hover_md(md));
            }
            Construct::Service(s) if s.name == word => {
                let methods: Vec<String> = s
                    .methods
                    .iter()
                    .map(|m| format!("  METHOD {}", m.name))
                    .collect();
                let md = format!(
                    "**SERVICE** `{}`\n\n```\n{}\n```",
                    s.name,
                    methods.join("\n")
                );
                return Some(hover_md(md));
            }
            Construct::Saga(s) if s.name == word => {
                let steps: Vec<String> = s
                    .steps
                    .iter()
                    .map(|st| format!("  STEP {}", st.name))
                    .collect();
                let md = format!("**SAGA** `{}`\n\n```\n{}\n```", s.name, steps.join("\n"));
                return Some(hover_md(md));
            }
            Construct::Surface(s) if s.name == word => {
                let md = format!("**SURFACE** `{}` v{}", s.name, s.version);
                return Some(hover_md(md));
            }
            Construct::Policy(p) if p.name == word => {
                let reqs: Vec<String> = p.requires.iter().map(|r| format!("  {:?}", r)).collect();
                let md = format!("**POLICY** `{}`\n\n```\n{}\n```", p.name, reqs.join("\n"));
                return Some(hover_md(md));
            }
            Construct::Stream(s) if s.name == word => {
                let events: Vec<String> = s
                    .events
                    .iter()
                    .map(|e| {
                        let fields: Vec<String> = e
                            .fields
                            .iter()
                            .map(|f| format!("{} {}", f.name, format_type(&f.ty)))
                            .collect();
                        format!("  EVENT {} [{}]", e.name, fields.join(", "))
                    })
                    .collect();
                let transport = match s.transport {
                    ast::StreamTransport::WebSocket => "ws",
                    ast::StreamTransport::Sse => "sse",
                };
                let md = format!(
                    "**STREAM** `{}` `{} {}`\n\n```\n{}\n```",
                    s.name,
                    transport,
                    s.path,
                    events.join("\n")
                );
                return Some(hover_md(md));
            }
            Construct::Migrate(m) if m.shape == word => {
                let md = format!(
                    "**MIGRATE** `{}` {} → {}",
                    m.shape, m.from_version, m.to_version
                );
                return Some(hover_md(md));
            }
            _ => {}
        }

        if let Construct::Shape(s) = c {
            for field in &s.fields {
                if field.name == word {
                    let md = format!(
                        "**field** `{}.{}`: `{}`",
                        s.name,
                        field.name,
                        format_type(&field.ty)
                    );
                    return Some(hover_md(md));
                }
            }
        }
    }
    None
}

fn handle_workspace_symbol(
    state: &ServerState,
    params: &WorkspaceSymbolParams,
) -> Option<Vec<SymbolInformation>> {
    let query = params.query.to_lowercase();
    let symbols: Vec<SymbolInformation> = state
        .index
        .entries
        .iter()
        .filter(|e| query.is_empty() || e.name.to_lowercase().contains(&query))
        .filter_map(|e| {
            let uri = path_to_uri(&e.path)?;
            let kind = construct_kind_to_symbol(e.kind);
            #[allow(deprecated)]
            Some(SymbolInformation {
                name: e.name.clone(),
                kind,
                tags: None,
                deprecated: None,
                location: Location {
                    uri,
                    range: span_to_range(e.span),
                },
                container_name: None,
            })
        })
        .collect();

    if symbols.is_empty() {
        None
    } else {
        Some(symbols)
    }
}

fn handle_prepare_rename(
    state: &ServerState,
    params: &TextDocumentPositionParams,
) -> Option<PrepareRenameResponse> {
    let uri = &params.text_document.uri;
    let source = state.documents.get(uri)?;
    let pos = params.position;

    let line_str = source.lines().nth(pos.line as usize)?;
    let word = word_at(line_str, pos.character as usize)?;

    let is_definition = state.index.find_definition_any(&word).is_some();
    let is_local_def = if !is_definition {
        let prog = project::compile_source(source).ok()?;
        construct_identity_by_name(&prog, &word).is_some()
    } else {
        false
    };

    if !is_definition && !is_local_def {
        return None;
    }

    let col = pos.character as usize;
    let bytes = line_str.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let start = (0..col)
        .rev()
        .take_while(|&i| is_word(bytes[i]))
        .last()
        .unwrap_or(col);
    let end = (col..bytes.len())
        .take_while(|&i| is_word(bytes[i]))
        .last()
        .unwrap_or(col)
        + 1;

    Some(PrepareRenameResponse::Range(Range {
        start: Position {
            line: pos.line,
            character: start as u32,
        },
        end: Position {
            line: pos.line,
            character: end as u32,
        },
    }))
}

fn handle_rename(state: &ServerState, params: &RenameParams) -> Option<WorkspaceEdit> {
    let uri = &params.text_document_position.text_document.uri;
    let source = state.documents.get(uri)?;
    let pos = params.text_document_position.position;

    let line_str = source.lines().nth(pos.line as usize)?;
    let old_name = word_at(line_str, pos.character as usize)?;
    let new_name = &params.new_name;

    #[allow(clippy::mutable_key_type)]
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();

    for entry in &state.index.entries {
        let file_uri = path_to_uri(&entry.path)?;

        let file_source = if let Some(doc) = state.documents.get(&file_uri) {
            doc.clone()
        } else {
            std::fs::read_to_string(&entry.path).ok()?
        };

        let edits = find_rename_edits(&file_source, &old_name, new_name);
        if !edits.is_empty() {
            changes.entry(file_uri).or_default().extend(edits);
        }
    }

    if changes.is_empty() {
        let edits = find_rename_edits(source, &old_name, new_name);
        if !edits.is_empty() {
            changes.insert(uri.clone(), edits);
        }
    }

    if changes.is_empty() {
        return None;
    }

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

fn find_rename_edits(source: &str, old_name: &str, new_name: &str) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    for (line_idx, line) in source.lines().enumerate() {
        let bytes = line.as_bytes();
        let mut col = 0;
        while col < bytes.len() {
            if let Some(pos) = line[col..].find(old_name) {
                let abs_pos = col + pos;
                let end_pos = abs_pos + old_name.len();

                let before_ok = abs_pos == 0 || !is_word(bytes[abs_pos - 1]);
                let after_ok = end_pos >= bytes.len() || !is_word(bytes[end_pos]);

                if before_ok && after_ok {
                    edits.push(TextEdit {
                        range: Range {
                            start: Position {
                                line: line_idx as u32,
                                character: abs_pos as u32,
                            },
                            end: Position {
                                line: line_idx as u32,
                                character: end_pos as u32,
                            },
                        },
                        new_text: new_name.to_string(),
                    });
                }
                col = end_pos;
            } else {
                break;
            }
        }
    }
    edits
}

fn construct_identity_by_name(program: &Program, name: &str) -> Option<ConstructKind> {
    for c in &program.constructs {
        if let Some((n, kind, _)) = construct_identity(c) {
            if n == name {
                return Some(kind);
            }
        }
    }
    None
}

fn construct_kind_to_symbol(kind: ConstructKind) -> SymbolKind {
    match kind {
        ConstructKind::Shape => SymbolKind::STRUCT,
        ConstructKind::Source => SymbolKind::NAMESPACE,
        ConstructKind::Realm => SymbolKind::MODULE,
        ConstructKind::Flow => SymbolKind::FUNCTION,
        ConstructKind::Service => SymbolKind::CLASS,
        ConstructKind::Saga => SymbolKind::FUNCTION,
        ConstructKind::Policy => SymbolKind::INTERFACE,
        ConstructKind::Surface => SymbolKind::PACKAGE,
        ConstructKind::Migrate => SymbolKind::EVENT,
        ConstructKind::Stream => SymbolKind::EVENT,
        ConstructKind::Func => SymbolKind::FUNCTION,
        ConstructKind::Storage => SymbolKind::NAMESPACE,
    }
}

// --- Reference resolution ---

#[derive(Debug, Clone, Copy, PartialEq)]
enum ConstructKind {
    Shape,
    Source,
    Realm,
    Flow,
    Service,
    Saga,
    Policy,
    Surface,
    Migrate,
    Stream,
    Func,
    Storage,
}

enum RefContext {
    ShapeName,
    SourceName,
    RealmName,
    FlowName,
    ServiceName,
    Unknown,
}

fn reference_context(source: &str, line_idx: usize, _word: &str) -> RefContext {
    let line = match source.lines().nth(line_idx) {
        Some(l) => l.trim(),
        None => return RefContext::Unknown,
    };

    if line.starts_with("SHAPE ") && line_idx == 0 || line.starts_with("SOURCE ") {
        return RefContext::Unknown;
    }

    let first_word = line.split_whitespace().next().unwrap_or("");
    match first_word {
        "SHAPE" => RefContext::ShapeName,
        "FETCH" | "QUERY" | "INSERT" | "UPDATE" | "DELETE" => RefContext::SourceName,
        "REALM" => RefContext::RealmName,
        "ROUTE" => RefContext::FlowName,
        "CALL" => RefContext::ServiceName,
        "REF" => RefContext::ShapeName,
        _ => {
            if line.contains("REF ") {
                return RefContext::ShapeName;
            }
            RefContext::Unknown
        }
    }
}

fn find_construct_span(
    program: &Program,
    name: &str,
    kind: ConstructKind,
) -> Option<crate::token::Span> {
    for c in &program.constructs {
        match (c, kind) {
            (Construct::Shape(s), ConstructKind::Shape) if s.name == name => return Some(s.span),
            (Construct::Source(s), ConstructKind::Source) if s.name == name => return Some(s.span),
            (Construct::Realm(r), ConstructKind::Realm) if r.name == name => return Some(r.span),
            (Construct::Flow(f), ConstructKind::Flow) if f.name == name => return Some(f.span),
            (Construct::Service(s), ConstructKind::Service) if s.name == name => {
                return Some(s.span);
            }
            _ => {}
        }
    }
    None
}

fn collect_references(construct: &Construct, name: &str) -> Vec<crate::token::Span> {
    let mut refs = Vec::new();
    match construct {
        Construct::Source(s) => {
            if s.shape == name {
                refs.push(s.span);
            }
        }
        Construct::Flow(f) => {
            if f.realm.as_deref() == Some(name) {
                refs.push(f.span);
            }
            collect_refs_in_steps(&f.steps, name, &mut refs);
            collect_refs_in_expr_option(&f.return_stmt.body, name, &mut refs, f.return_stmt.span);
        }
        Construct::Saga(s) => {
            if s.realm.as_deref() == Some(name) {
                refs.push(s.span);
            }
            for step in &s.steps {
                collect_refs_in_steps(&step.flow_steps, name, &mut refs);
            }
        }
        Construct::Surface(s) => {
            for route in &s.routes {
                if route.target == name {
                    refs.push(s.span);
                }
            }
        }
        Construct::Migrate(m) => {
            if m.shape == name {
                refs.push(m.span);
            }
        }
        Construct::Realm(r) => {
            for cap in &r.capabilities {
                if cap.target == name {
                    refs.push(r.span);
                }
            }
        }
        _ => {}
    }
    refs
}

fn collect_refs_in_steps(steps: &[FlowStep], name: &str, refs: &mut Vec<crate::token::Span>) {
    for step in steps {
        match step {
            FlowStep::Let(l) => {
                collect_refs_in_expr(&l.expr, name, refs, l.span);
            }
            FlowStep::Insert(i) => {
                if i.source == name {
                    refs.push(i.span);
                }
                for (_, expression) in &i.fields {
                    collect_refs_in_expr(expression, name, refs, i.span);
                }
            }
            FlowStep::Upsert(upsert) => {
                if upsert.source == name {
                    refs.push(upsert.span);
                }
                for (_, expression) in &upsert.keys {
                    collect_refs_in_expr(expression, name, refs, upsert.span);
                }
                for set in &upsert.sets {
                    collect_refs_in_expr(&set.value, name, refs, upsert.span);
                }
            }
            FlowStep::Update(u) => {
                if u.source == name {
                    refs.push(u.span);
                }
                for filter in &u.wheres {
                    collect_refs_in_expr(&filter.value, name, refs, u.span);
                }
                for set in &u.sets {
                    collect_refs_in_expr(&set.value, name, refs, u.span);
                }
            }
            FlowStep::Delete(d) => {
                if d.source == name {
                    refs.push(d.span);
                }
                for filter in &d.wheres {
                    collect_refs_in_expr(&filter.value, name, refs, d.span);
                }
            }
            FlowStep::Fanout(fanout) => {
                collect_refs_in_expr(&fanout.source, name, refs, fanout.span);
                if fanout.insert.source == name {
                    refs.push(fanout.span);
                }
                for (_, expression) in &fanout.insert.fields {
                    collect_refs_in_expr(expression, name, refs, fanout.span);
                }
            }
            FlowStep::Match(m) => {
                for branch in &m.branches {
                    collect_refs_in_expr(&branch.condition, name, refs, m.span);
                    collect_refs_in_steps(&branch.steps, name, refs);
                }
                if let Some(default) = &m.default {
                    collect_refs_in_steps(default, name, refs);
                }
            }
            FlowStep::Each(each) => {
                collect_refs_in_expr(&each.source, name, refs, each.span);
                collect_refs_in_steps(&each.steps, name, refs);
            }
            FlowStep::Try(step) => {
                collect_refs_in_steps(&step.body, name, refs);
                collect_refs_in_steps(&step.recover, name, refs);
            }
            FlowStep::Rule(rule) => {
                for requirement in &rule.requires {
                    collect_refs_in_expr(&requirement.value, name, refs, rule.span);
                }
            }
            FlowStep::Guard(guard) => {
                collect_refs_in_expr(&guard.expr, name, refs, guard.span);
            }
            FlowStep::Set(set) => collect_refs_in_expr(&set.expr, name, refs, set.span),
            FlowStep::Effect(effect) => {
                for field in &effect.fields {
                    match field {
                        EffectField::To(expression) | EffectField::Url(expression) => {
                            collect_refs_in_expr(expression, name, refs, effect.span);
                        }
                        EffectField::Data(expressions) => {
                            for expression in expressions {
                                collect_refs_in_expr(expression, name, refs, effect.span);
                            }
                        }
                        EffectField::Template(_) | EffectField::Event(_) | EffectField::Task(_) => {
                        }
                    }
                }
            }
            FlowStep::Upload(upload) => {
                if upload.storage == name {
                    refs.push(upload.span);
                }
                collect_refs_in_expr(&upload.file_expr, name, refs, upload.span);
            }
        }
    }
}

fn collect_refs_in_expr(
    expr: &Expr,
    name: &str,
    refs: &mut Vec<crate::token::Span>,
    span: crate::token::Span,
) {
    match expr {
        Expr::Fetch {
            source, filters, ..
        }
        | Expr::Query {
            source, filters, ..
        } => {
            if source == name {
                refs.push(span);
            }
            for filter in filters {
                collect_refs_in_expr(&filter.value, name, refs, span);
            }
        }
        Expr::Call { service, args, .. } => {
            if service == name {
                refs.push(span);
            }
            for (_, expression) in args {
                collect_refs_in_expr(expression, name, refs, span);
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
        } => {
            collect_refs_in_expr(operand, name, refs, span);
        }
        Expr::Binary { left, right, .. }
        | Expr::Coalesce {
            value: left,
            default: right,
        }
        | Expr::SplitExpr {
            value: left,
            delimiter: right,
        } => {
            collect_refs_in_expr(left, name, refs, span);
            collect_refs_in_expr(right, name, refs, span);
        }
        Expr::Ternary { a, b, c, .. } => {
            collect_refs_in_expr(a, name, refs, span);
            collect_refs_in_expr(b, name, refs, span);
            collect_refs_in_expr(c, name, refs, span);
        }
        Expr::If { cond, then, else_ } => {
            collect_refs_in_expr(cond, name, refs, span);
            collect_refs_in_expr(then, name, refs, span);
            collect_refs_in_expr(else_, name, refs, span);
        }
        Expr::FilterExpr { source, condition } => {
            collect_refs_in_expr(source, name, refs, span);
            collect_refs_in_expr(condition, name, refs, span);
        }
        Expr::ReplaceExpr { value, from, to } => {
            collect_refs_in_expr(value, name, refs, span);
            collect_refs_in_expr(from, name, refs, span);
            collect_refs_in_expr(to, name, refs, span);
        }
        Expr::FormatExpr { args, .. } | Expr::FuncCall { args, .. } => {
            for expression in args {
                collect_refs_in_expr(expression, name, refs, span);
            }
        }
        Expr::Render { vars, .. } | Expr::Translate { vars, .. } => {
            for (_, expression) in vars {
                collect_refs_in_expr(expression, name, refs, span);
            }
        }
        Expr::Literal(_) | Expr::DotPath(_) | Expr::WasmCall { .. } => {}
    }
}

fn collect_refs_in_expr_option(
    body: &Option<ast::ReturnBody>,
    name: &str,
    refs: &mut Vec<crate::token::Span>,
    span: crate::token::Span,
) {
    if let Some(ast::ReturnBody::Binding(b)) = body {
        if b == name {
            refs.push(span);
        }
    }
}

fn goto_location(uri: Uri, span: crate::token::Span) -> GotoDefinitionResponse {
    GotoDefinitionResponse::Scalar(Location {
        uri,
        range: span_to_range(span),
    })
}

fn hover_md(md: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: None,
    }
}

// --- Utilities ---

fn span_to_range(span: crate::token::Span) -> Range {
    let start_line = span.line.saturating_sub(1) as u32;
    let start_char = span.col as u32;
    Range {
        start: Position {
            line: start_line,
            character: start_char,
        },
        end: Position {
            line: start_line,
            character: start_char + span.len as u32,
        },
    }
}

fn span_in_source(span: crate::token::Span, source: &str) -> bool {
    let line_count = source.lines().count();
    span.line <= line_count
}

fn error_position(e: &crate::error::AxisError) -> (usize, usize, String) {
    match e {
        crate::error::AxisError::LexError { line, col, message } => (*line, *col, message.clone()),
        crate::error::AxisError::ParseError { line, col, message } => {
            (*line, *col, message.clone())
        }
        crate::error::AxisError::TypeError { span, message } => {
            (span.line, span.col, message.clone())
        }
        crate::error::AxisError::IndexError { span, message } => {
            (span.line, span.col, message.clone())
        }
        crate::error::AxisError::CapabilityError { span, message } => {
            (span.line, span.col, message.clone())
        }
        crate::error::AxisError::PolicyViolation { span, message } => {
            (span.line, span.col, message.clone())
        }
        crate::error::AxisError::TenantError { span, message } => {
            (span.line, span.col, message.clone())
        }
    }
}

fn source_up_to(source: &str, line: usize, col: usize) -> String {
    let mut result = String::new();
    for (i, l) in source.lines().enumerate() {
        if i < line {
            result.push_str(l);
            result.push('\n');
        } else if i == line {
            let end = col.min(l.len());
            result.push_str(&l[..end]);
            break;
        }
    }
    result
}

fn word_at(line: &str, col: usize) -> Option<String> {
    let bytes = line.as_bytes();
    if col >= bytes.len() {
        return None;
    }
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    if !is_word(bytes[col]) {
        return None;
    }
    let start = (0..col)
        .rev()
        .take_while(|&i| is_word(bytes[i]))
        .last()
        .unwrap_or(col);
    let end = (col..bytes.len())
        .take_while(|&i| is_word(bytes[i]))
        .last()
        .unwrap_or(col)
        + 1;
    Some(line[start..end].to_string())
}

fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let s = uri.as_str();
    s.strip_prefix("file://").map(PathBuf::from)
}

fn path_to_uri(path: &std::path::Path) -> Option<Uri> {
    format!("file://{}", path.display()).parse().ok()
}

fn find_workspace_root(file_path: &std::path::Path) -> Option<PathBuf> {
    let mut dir = file_path.parent()?;
    loop {
        let has_axis = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .any(|e| e.path().extension().is_some_and(|ext| ext == "axis"));
        if has_axis {
            if let Some(parent) = dir.parent() {
                let parent_has_axis = std::fs::read_dir(parent)
                    .ok()
                    .map(|entries| {
                        entries
                            .flatten()
                            .any(|e| e.path().extension().is_some_and(|ext| ext == "axis"))
                    })
                    .unwrap_or(false);
                if parent_has_axis {
                    dir = parent;
                    continue;
                }
            }
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

fn format_type(ty: &ast::TypeExpr) -> String {
    match ty {
        ast::TypeExpr::Uuid => "UUID".into(),
        ast::TypeExpr::Bool => "BOOL".into(),
        ast::TypeExpr::Date => "DATE".into(),
        ast::TypeExpr::Timestamp => "TIMESTAMP".into(),
        ast::TypeExpr::Text => "TEXT".into(),
        ast::TypeExpr::String(None) => "STRING".into(),
        ast::TypeExpr::String(Some(n)) => format!("STRING {n}"),
        ast::TypeExpr::Int { .. } => "INT".into(),
        ast::TypeExpr::Decimal { .. } => "DECIMAL".into(),
        ast::TypeExpr::Enum(variants) => format!("ENUM({})", variants.join(", ")),
        ast::TypeExpr::Ref { shape, field } => format!("REF {shape}.{field}"),
        ast::TypeExpr::List(inner) => format!("LIST {}", format_type(inner)),
        ast::TypeExpr::Map(k, v) => format!("MAP {} {}", format_type(k), format_type(v)),
        ast::TypeExpr::Json => "JSON".into(),
        ast::TypeExpr::Blob => "BLOB".into(),
        ast::TypeExpr::Maybe(inner) => format!("MAYBE {}", format_type(inner)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_up_to() {
        let src = "SHAPE User\n  id UUID PK AUTO\n  name STRING\n";
        assert_eq!(source_up_to(src, 0, 5), "SHAPE");
        assert_eq!(source_up_to(src, 1, 5), "SHAPE User\n  id ");
        assert_eq!(source_up_to(src, 2, 2), "SHAPE User\n  id UUID PK AUTO\n  ");
    }

    #[test]
    fn test_word_at() {
        assert_eq!(word_at("SHAPE User", 0), Some("SHAPE".into()));
        assert_eq!(word_at("SHAPE User", 6), Some("User".into()));
        assert_eq!(word_at("SHAPE User", 5), None);
        assert_eq!(word_at("  id UUID PK AUTO", 2), Some("id".into()));
    }

    #[test]
    fn test_span_to_range() {
        let span = crate::token::Span {
            offset: 0,
            len: 5,
            line: 1,
            col: 0,
        };
        let range = span_to_range(span);
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.character, 5);
    }

    #[test]
    fn test_error_position_lex() {
        let e = crate::error::AxisError::LexError {
            line: 3,
            col: 5,
            message: "bad token".into(),
        };
        let (l, c, m) = error_position(&e);
        assert_eq!(l, 3);
        assert_eq!(c, 5);
        assert_eq!(m, "bad token");
    }

    #[test]
    fn test_completions_for_empty() {
        let completions = incremental::completions_at("");
        assert!(completions.contains(&"SHAPE".to_string()));
        assert!(completions.contains(&"FLOW".to_string()));
    }

    #[test]
    fn test_completions_for_flow_body() {
        let completions = incremental::completions_at("FLOW get get /users\n");
        assert!(completions.contains(&"AUTH".to_string()));
        assert!(completions.contains(&"LET".to_string()));
    }

    #[test]
    fn test_format_type_uuid() {
        assert_eq!(format_type(&ast::TypeExpr::Uuid), "UUID");
    }

    #[test]
    fn test_format_type_enum() {
        assert_eq!(
            format_type(&ast::TypeExpr::Enum(vec!["a".into(), "b".into()])),
            "ENUM(a, b)"
        );
    }

    #[test]
    fn test_format_type_string_sized() {
        assert_eq!(format_type(&ast::TypeExpr::String(Some(100))), "STRING 100");
    }

    #[test]
    fn test_format_type_list() {
        assert_eq!(
            format_type(&ast::TypeExpr::List(Box::new(ast::TypeExpr::Uuid))),
            "LIST UUID"
        );
    }

    #[test]
    fn test_reference_context_fetch() {
        let src = "FLOW get get /u\n  FETCH users\n";
        assert!(matches!(
            reference_context(src, 1, "users"),
            RefContext::SourceName
        ));
    }

    #[test]
    fn test_reference_context_realm() {
        let src = "FLOW get get /u\n  REALM api\n";
        assert!(matches!(
            reference_context(src, 1, "api"),
            RefContext::RealmName
        ));
    }

    #[test]
    fn test_reference_context_shape_header() {
        let src = "SOURCE users POSTGRES\n  SHAPE User\n";
        assert!(matches!(
            reference_context(src, 1, "User"),
            RefContext::ShapeName
        ));
    }

    #[test]
    fn test_reference_context_route() {
        let src = "SURFACE pub v1\n  ROUTE GET /u -> list\n";
        assert!(matches!(
            reference_context(src, 1, "list"),
            RefContext::FlowName
        ));
    }

    #[test]
    fn test_find_construct_shape() {
        let src = "SHAPE User\n  id UUID PK AUTO\n\nSHAPE Order\n  id UUID PK AUTO\n";
        let program = project::compile_source(src).unwrap();
        let span = find_construct_span(&program, "Order", ConstructKind::Shape);
        assert!(span.is_some());
        assert_eq!(span.unwrap().line, 4);
    }

    #[test]
    fn test_find_construct_source() {
        let src =
            "SHAPE User\n  id UUID PK AUTO\n\nSOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n";
        let program = project::compile_source(src).unwrap();
        let span = find_construct_span(&program, "users", ConstructKind::Source);
        assert!(span.is_some());
    }

    #[test]
    fn test_collect_references_source_in_flow() {
        let src = r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM api
  CAPABILITY read users

FLOW get get /users/:id
  REALM api
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let program = project::compile_source(src).unwrap();
        let flow = program
            .constructs
            .iter()
            .find(|c| matches!(c, Construct::Flow(_)))
            .unwrap();
        let refs = collect_references(flow, "users");
        assert!(!refs.is_empty());
    }

    #[test]
    fn test_document_symbols() {
        let src =
            "SHAPE User\n  id UUID PK AUTO\n\nSOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n";
        let program = project::compile_source(src).unwrap();
        let symbols: Vec<_> = program
            .constructs
            .iter()
            .map(|c| match c {
                Construct::Shape(s) => s.name.clone(),
                Construct::Source(s) => s.name.clone(),
                _ => String::new(),
            })
            .collect();
        assert!(symbols.contains(&"User".to_string()));
        assert!(symbols.contains(&"users".to_string()));
    }

    #[test]
    fn test_find_workspace_root() {
        let tmp = tempdir();
        std::fs::write(tmp.join("test.axis"), "SHAPE X\n  id UUID PK AUTO\n").unwrap();
        let root = find_workspace_root(&tmp.join("test.axis"));
        assert!(root.is_some());
        assert_eq!(root.unwrap(), tmp);
    }

    #[test]
    fn test_span_in_source() {
        let src = "line 1\nline 2\nline 3\n";
        let span = crate::token::Span {
            offset: 0,
            len: 5,
            line: 2,
            col: 0,
        };
        assert!(span_in_source(span, src));
        let span_out = crate::token::Span {
            offset: 0,
            len: 5,
            line: 10,
            col: 0,
        };
        assert!(!span_in_source(span_out, src));
    }

    #[test]
    fn test_cross_file_definition() {
        let tmp = tempdir();
        std::fs::write(
            tmp.join("shapes.axis"),
            "SHAPE User\n  id UUID PK AUTO\n  name STRING 100 REQUIRED\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("sources.axis"),
            "SOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n",
        )
        .unwrap();

        let mut index = WorkspaceIndex::new();
        index.rebuild(&tmp, &HashMap::new());

        let shape = index.find_definition("User", ConstructKind::Shape);
        assert!(shape.is_some());
        assert!(shape.unwrap().path.ends_with("shapes.axis"));

        let source = index.find_definition("users", ConstructKind::Source);
        assert!(source.is_some());
        assert!(source.unwrap().path.ends_with("sources.axis"));
    }

    #[test]
    fn test_cross_file_references() {
        let tmp = tempdir();
        std::fs::write(tmp.join("shapes.axis"), "SHAPE User\n  id UUID PK AUTO\n").unwrap();
        std::fs::write(
            tmp.join("sources.axis"),
            "SOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n",
        )
        .unwrap();

        let mut index = WorkspaceIndex::new();
        index.rebuild(&tmp, &HashMap::new());

        let refs = index.find_references("User");
        assert!(!refs.is_empty());
        assert!(refs.iter().any(|(p, _)| p.ends_with("sources.axis")));
    }

    #[test]
    fn test_cross_file_merged_program() {
        let tmp = tempdir();
        std::fs::write(tmp.join("shapes.axis"), "SHAPE User\n  id UUID PK AUTO\n").unwrap();
        std::fs::write(
            tmp.join("sources.axis"),
            "SOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n",
        )
        .unwrap();

        let mut index = WorkspaceIndex::new();
        index.rebuild(&tmp, &HashMap::new());

        let program = index.merged_program();
        assert_eq!(program.constructs.len(), 2);
    }

    #[test]
    #[allow(clippy::mutable_key_type)] // lsp_types::Uri is required by the workspace index API.
    fn test_index_prefers_open_docs() {
        let tmp = tempdir();
        std::fs::write(tmp.join("shapes.axis"), "SHAPE User\n  id UUID PK AUTO\n").unwrap();

        let uri = path_to_uri(&tmp.join("shapes.axis")).unwrap();
        let mut open = HashMap::new();
        open.insert(
            uri,
            "SHAPE User\n  id UUID PK AUTO\n  email STRING 255 REQUIRED\n".to_string(),
        );

        let mut index = WorkspaceIndex::new();
        index.rebuild(&tmp, &open);

        let entry = index.find_definition("User", ConstructKind::Shape).unwrap();
        if let Construct::Shape(s) = &entry.construct {
            assert_eq!(s.fields.len(), 2);
        } else {
            panic!("expected shape");
        }
    }

    #[test]
    fn test_cross_file_find_any() {
        let tmp = tempdir();
        std::fs::write(
            tmp.join("realms.axis"),
            "REALM api\n  CAPABILITY read users\n",
        )
        .unwrap();

        let mut index = WorkspaceIndex::new();
        index.rebuild(&tmp, &HashMap::new());

        let entry = index.find_definition_any("api");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().kind, ConstructKind::Realm);
    }

    #[test]
    fn test_find_rename_edits_basic() {
        let src = "SHAPE User\n  id UUID PK AUTO\n  name STRING 100 REQUIRED\n";
        let edits = find_rename_edits(src, "User", "Customer");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start.line, 0);
        assert_eq!(edits[0].range.start.character, 6);
        assert_eq!(edits[0].new_text, "Customer");
    }

    #[test]
    fn test_find_rename_edits_multiple() {
        let src = "SOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n\nFLOW get get /users/:id\n  REALM api\n  LET user\n    FETCH users\n      FILTER id EQ path.id\n    OR 404\n  RETURN 200 user\n";
        let edits = find_rename_edits(src, "users", "accounts");
        assert!(edits.len() >= 2);
        for edit in &edits {
            assert_eq!(edit.new_text, "accounts");
        }
    }

    #[test]
    fn test_find_rename_edits_word_boundary() {
        let src = "SHAPE UserProfile\n  user_id UUID PK AUTO\n";
        let edits = find_rename_edits(src, "User", "Customer");
        assert!(edits.is_empty());
    }

    #[test]
    fn test_construct_identity_by_name() {
        let src =
            "SHAPE User\n  id UUID PK AUTO\n\nSOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n";
        let program = project::compile_source(src).unwrap();
        assert_eq!(
            construct_identity_by_name(&program, "User"),
            Some(ConstructKind::Shape)
        );
        assert_eq!(
            construct_identity_by_name(&program, "users"),
            Some(ConstructKind::Source)
        );
        assert_eq!(construct_identity_by_name(&program, "nope"), None);
    }

    #[test]
    fn test_construct_kind_to_symbol() {
        assert_eq!(
            construct_kind_to_symbol(ConstructKind::Shape),
            SymbolKind::STRUCT
        );
        assert_eq!(
            construct_kind_to_symbol(ConstructKind::Flow),
            SymbolKind::FUNCTION
        );
        assert_eq!(
            construct_kind_to_symbol(ConstructKind::Realm),
            SymbolKind::MODULE
        );
        assert_eq!(
            construct_kind_to_symbol(ConstructKind::Service),
            SymbolKind::CLASS
        );
    }

    #[test]
    fn test_workspace_symbol_filtering() {
        let tmp = tempdir();
        std::fs::write(
            tmp.join("shapes.axis"),
            "SHAPE User\n  id UUID PK AUTO\n\nSHAPE Order\n  id UUID PK AUTO\n",
        )
        .unwrap();
        std::fs::write(tmp.join("flows.axis"), "FLOW get_user get /users/:id\n  AUTH session\n  LET u\n    FETCH users\n      FILTER id EQ path.id\n    OR 404\n  RETURN 200 u\n").unwrap();

        let mut index = WorkspaceIndex::new();
        index.rebuild(&tmp, &HashMap::new());

        let all_entries: Vec<_> = index
            .entries
            .iter()
            .filter(|e| e.name.to_lowercase().contains("user"))
            .collect();
        assert!(all_entries.len() >= 2);

        let shapes: Vec<_> = index
            .entries
            .iter()
            .filter(|e| e.kind == ConstructKind::Shape)
            .collect();
        assert_eq!(shapes.len(), 2);
    }

    #[test]
    fn test_cross_file_rename_edits() {
        let src1 = "SHAPE User\n  id UUID PK AUTO\n";
        let src2 = "SOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n";
        let edits1 = find_rename_edits(src1, "User", "Customer");
        let edits2 = find_rename_edits(src2, "User", "Customer");
        assert_eq!(edits1.len(), 1);
        assert_eq!(edits2.len(), 1);
        assert_eq!(edits2[0].range.start.line, 1);
    }

    #[test]
    fn test_completion_context_fetch() {
        assert!(matches!(
            completion_context("FETCH users"),
            CompletionContext::SourceName
        ));
        assert!(matches!(
            completion_context("QUERY users"),
            CompletionContext::SourceName
        ));
        assert!(matches!(
            completion_context("INSERT orders"),
            CompletionContext::SourceName
        ));
        assert!(matches!(
            completion_context("FILTER id EQ"),
            CompletionContext::FieldName
        ));
    }

    #[test]
    fn test_completion_context_realm() {
        assert!(matches!(
            completion_context("REALM api"),
            CompletionContext::RealmName
        ));
    }

    #[test]
    fn test_workspace_completions_shapes() {
        let tmp = tempdir();
        std::fs::write(
            tmp.join("shapes.axis"),
            "SHAPE User\n  id UUID PK AUTO\n\nSHAPE Order\n  id UUID PK AUTO\n",
        )
        .unwrap();

        let mut state = ServerState::new();
        state.workspace_root = Some(tmp.clone());
        state.rebuild_index();

        let items = workspace_completions(&state, CompletionContext::ShapeName);
        let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(names.contains(&"User"));
        assert!(names.contains(&"Order"));
    }

    #[test]
    fn test_workspace_completions_sources() {
        let tmp = tempdir();
        std::fs::write(
            tmp.join("sources.axis"),
            "SOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n",
        )
        .unwrap();

        let mut state = ServerState::new();
        state.workspace_root = Some(tmp.clone());
        state.rebuild_index();

        let items = workspace_completions(&state, CompletionContext::SourceName);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "users");
    }

    #[test]
    fn test_field_completions() {
        let tmp = tempdir();
        std::fs::write(
            tmp.join("shapes.axis"),
            "SHAPE User\n  id UUID PK AUTO\n  name STRING 100 REQUIRED\n  email STRING 200\n",
        )
        .unwrap();

        let mut state = ServerState::new();
        state.workspace_root = Some(tmp.clone());
        state.rebuild_index();

        let items = field_completions(&state);
        let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(names.contains(&"id"));
        assert!(names.contains(&"name"));
        assert!(names.contains(&"email"));
    }

    #[test]
    fn test_stream_construct_kind() {
        let tmp = tempdir();
        std::fs::write(
            tmp.join("stream.axis"),
            "STREAM updates ws /ws/updates\n  EVENT user_online\n    user_id UUID\n",
        )
        .unwrap();

        let mut index = WorkspaceIndex::new();
        index.rebuild(&tmp, &HashMap::new());

        let entry = index.find_definition_any("updates");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().kind, ConstructKind::Stream);
    }

    fn tempdir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("axis_lsp_test_{}_{}", std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
