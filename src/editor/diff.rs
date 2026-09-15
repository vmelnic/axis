use std::fmt::Write;

use crate::ast::*;

pub struct DiffResult {
    pub migrations: Vec<MigrationDiff>,
    pub stream_changes: Vec<StreamChange>,
}

pub enum StreamChange {
    Added(String),
    Removed(String),
    EventAdded {
        stream: String,
        event: String,
    },
    EventRemoved {
        stream: String,
        event: String,
    },
    TransportChanged {
        stream: String,
        from: String,
        to: String,
    },
}

pub struct MigrationDiff {
    pub shape: String,
    pub from_version: String,
    pub to_version: String,
    pub ops: Vec<DiffOp>,
}

pub enum DiffOp {
    AddField(FieldDef),
    DropField(String),
    RenameField {
        from: String,
        to: String,
    },
    ChangeType {
        field: String,
        from: TypeExpr,
        to: TypeExpr,
    },
    AddModifier {
        field: String,
        modifier: String,
    },
    DropModifier {
        field: String,
        modifier: String,
    },
}

impl DiffResult {
    pub fn has_changes(&self) -> bool {
        self.migrations.iter().any(|m| !m.ops.is_empty()) || !self.stream_changes.is_empty()
    }
}

pub fn diff_programs(
    old: &Program,
    new: &Program,
    from_version: &str,
    to_version: &str,
) -> DiffResult {
    let old_shapes = collect_shapes(old);
    let new_shapes = collect_shapes(new);

    let mut migrations = Vec::new();

    for (name, new_shape) in &new_shapes {
        if let Some(old_shape) = old_shapes.get(name) {
            let ops = diff_shape(old_shape, new_shape);
            if !ops.is_empty() {
                migrations.push(MigrationDiff {
                    shape: name.clone(),
                    from_version: from_version.to_string(),
                    to_version: to_version.to_string(),
                    ops,
                });
            }
        } else {
            let fields: Vec<DiffOp> = new_shape
                .fields
                .iter()
                .map(|f| DiffOp::AddField(f.clone()))
                .collect();
            if !fields.is_empty() {
                migrations.push(MigrationDiff {
                    shape: name.clone(),
                    from_version: from_version.to_string(),
                    to_version: to_version.to_string(),
                    ops: fields,
                });
            }
        }
    }

    for name in old_shapes.keys() {
        if !new_shapes.contains_key(name) {
            let old_shape = &old_shapes[name];
            let ops = old_shape
                .fields
                .iter()
                .map(|f| DiffOp::DropField(f.name.clone()))
                .collect();
            migrations.push(MigrationDiff {
                shape: name.to_string(),
                from_version: from_version.to_string(),
                to_version: to_version.to_string(),
                ops,
            });
        }
    }

    let old_streams = collect_streams(old);
    let new_streams = collect_streams(new);
    let mut stream_changes = Vec::new();

    for (name, new_stream) in &new_streams {
        if let Some(old_stream) = old_streams.get(name) {
            let old_transport = format!("{:?}", old_stream.transport);
            let new_transport = format!("{:?}", new_stream.transport);
            if old_transport != new_transport {
                stream_changes.push(StreamChange::TransportChanged {
                    stream: name.clone(),
                    from: old_transport,
                    to: new_transport,
                });
            }
            let old_events: std::collections::HashSet<&str> =
                old_stream.events.iter().map(|e| e.name.as_str()).collect();
            let new_events: std::collections::HashSet<&str> =
                new_stream.events.iter().map(|e| e.name.as_str()).collect();
            for evt in &new_events {
                if !old_events.contains(evt) {
                    stream_changes.push(StreamChange::EventAdded {
                        stream: name.clone(),
                        event: evt.to_string(),
                    });
                }
            }
            for evt in &old_events {
                if !new_events.contains(evt) {
                    stream_changes.push(StreamChange::EventRemoved {
                        stream: name.clone(),
                        event: evt.to_string(),
                    });
                }
            }
        } else {
            stream_changes.push(StreamChange::Added(name.clone()));
        }
    }
    for name in old_streams.keys() {
        if !new_streams.contains_key(name) {
            stream_changes.push(StreamChange::Removed(name.clone()));
        }
    }

    DiffResult {
        migrations,
        stream_changes,
    }
}

fn diff_shape(old: &ShapeDef, new: &ShapeDef) -> Vec<DiffOp> {
    let mut ops = Vec::new();

    let old_fields: std::collections::HashMap<&str, &FieldDef> =
        old.fields.iter().map(|f| (f.name.as_str(), f)).collect();
    let new_fields: std::collections::HashMap<&str, &FieldDef> =
        new.fields.iter().map(|f| (f.name.as_str(), f)).collect();

    for (name, new_field) in &new_fields {
        if let Some(old_field) = old_fields.get(name) {
            if !types_equal(&old_field.ty, &new_field.ty) {
                ops.push(DiffOp::ChangeType {
                    field: name.to_string(),
                    from: old_field.ty.clone(),
                    to: new_field.ty.clone(),
                });
            }

            let old_mods = modifier_set(&old_field.modifiers);
            let new_mods = modifier_set(&new_field.modifiers);
            for m in &new_mods {
                if !old_mods.contains(m) {
                    ops.push(DiffOp::AddModifier {
                        field: name.to_string(),
                        modifier: m.clone(),
                    });
                }
            }
            for m in &old_mods {
                if !new_mods.contains(m) {
                    ops.push(DiffOp::DropModifier {
                        field: name.to_string(),
                        modifier: m.clone(),
                    });
                }
            }
        } else {
            ops.push(DiffOp::AddField((*new_field).clone()));
        }
    }

    for name in old_fields.keys() {
        if !new_fields.contains_key(name) {
            ops.push(DiffOp::DropField(name.to_string()));
        }
    }

    ops
}

fn types_equal(a: &TypeExpr, b: &TypeExpr) -> bool {
    match (a, b) {
        (TypeExpr::Uuid, TypeExpr::Uuid) => true,
        (TypeExpr::Bool, TypeExpr::Bool) => true,
        (TypeExpr::Date, TypeExpr::Date) => true,
        (TypeExpr::Timestamp, TypeExpr::Timestamp) => true,
        (TypeExpr::Text, TypeExpr::Text) => true,
        (TypeExpr::Json, TypeExpr::Json) => true,
        (TypeExpr::String(a), TypeExpr::String(b)) => a == b,
        (TypeExpr::Int { min: a1, max: a2 }, TypeExpr::Int { min: b1, max: b2 }) => {
            a1 == b1 && a2 == b2
        }
        (
            TypeExpr::Decimal {
                precision: a1,
                scale: a2,
            },
            TypeExpr::Decimal {
                precision: b1,
                scale: b2,
            },
        ) => a1 == b1 && a2 == b2,
        (TypeExpr::Enum(a), TypeExpr::Enum(b)) => a == b,
        (
            TypeExpr::Ref {
                shape: s1,
                field: f1,
            },
            TypeExpr::Ref {
                shape: s2,
                field: f2,
            },
        ) => s1 == s2 && f1 == f2,
        (TypeExpr::List(a), TypeExpr::List(b)) => types_equal(a, b),
        (TypeExpr::Map(ak, av), TypeExpr::Map(bk, bv)) => {
            types_equal(ak, bk) && types_equal(av, bv)
        }
        (TypeExpr::Maybe(a), TypeExpr::Maybe(b)) => types_equal(a, b),
        _ => false,
    }
}

fn modifier_set(mods: &[Modifier]) -> std::collections::HashSet<String> {
    mods.iter()
        .map(|m| match m {
            Modifier::Pk => "PK".to_string(),
            Modifier::Auto => "AUTO".to_string(),
            Modifier::Required => "REQUIRED".to_string(),
            Modifier::Unique => "UNIQUE".to_string(),
            Modifier::Default(v) => format!("DEFAULT {v:?}"),
            Modifier::Precision(n) => format!("PRECISION {n}"),
            Modifier::Scale(n) => format!("SCALE {n}"),
            Modifier::Min(n) => format!("MIN {n}"),
            Modifier::Max(n) => format!("MAX {n}"),
            Modifier::Ref { shape, field } => format!("REF {shape}.{field}"),
        })
        .collect()
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

fn collect_streams(program: &Program) -> std::collections::HashMap<String, &StreamDef> {
    let mut map = std::collections::HashMap::new();
    for c in &program.constructs {
        if let Construct::Stream(s) = c {
            map.insert(s.name.clone(), s);
        }
    }
    map
}

pub fn format_migration(diff: &MigrationDiff) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "MIGRATE {} {} TO {}",
        diff.shape, diff.from_version, diff.to_version
    )
    .unwrap();

    let copies: Vec<&str> = Vec::new();
    let mut adds = Vec::new();
    let mut drops = Vec::new();
    let mut renames = Vec::new();

    for op in &diff.ops {
        match op {
            DiffOp::AddField(f) => adds.push(f),
            DiffOp::DropField(name) => drops.push(name.as_str()),
            DiffOp::RenameField { from, to } => renames.push((from.as_str(), to.as_str())),
            DiffOp::ChangeType { field, .. } => {
                drops.push(field.as_str());
            }
            DiffOp::AddModifier { .. } | DiffOp::DropModifier { .. } => {}
        }
    }

    if !copies.is_empty() {
        writeln!(out, "  COPY {}", copies.join(" ")).unwrap();
    }

    for field in &adds {
        let ty_str = format_type(&field.ty);
        let mods = format_modifiers(&field.modifiers);
        if mods.is_empty() {
            writeln!(out, "  ADD {} {}", field.name, ty_str).unwrap();
        } else {
            writeln!(out, "  ADD {} {} {}", field.name, ty_str, mods).unwrap();
        }
    }

    for name in &drops {
        writeln!(out, "  DROP {name}").unwrap();
    }

    for (from, to) in &renames {
        writeln!(out, "  RENAME {from} TO {to}").unwrap();
    }

    out
}

pub fn format_all_migrations(result: &DiffResult) -> String {
    let mut out = String::new();
    for (i, m) in result.migrations.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format_migration(m));
    }
    for change in &result.stream_changes {
        match change {
            StreamChange::Added(name) => writeln!(out, "STREAM_ADDED {name}").unwrap(),
            StreamChange::Removed(name) => writeln!(out, "STREAM_REMOVED {name}").unwrap(),
            StreamChange::EventAdded { stream, event } => {
                writeln!(out, "STREAM_EVENT_ADDED {stream}.{event}").unwrap()
            }
            StreamChange::EventRemoved { stream, event } => {
                writeln!(out, "STREAM_EVENT_REMOVED {stream}.{event}").unwrap()
            }
            StreamChange::TransportChanged { stream, from, to } => {
                writeln!(out, "STREAM_TRANSPORT_CHANGED {stream} {from} -> {to}").unwrap()
            }
        }
    }
    out
}

fn format_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Uuid => "UUID".to_string(),
        TypeExpr::Bool => "BOOL".to_string(),
        TypeExpr::Date => "DATE".to_string(),
        TypeExpr::Timestamp => "TIMESTAMP".to_string(),
        TypeExpr::Text => "TEXT".to_string(),
        TypeExpr::Json => "JSON".to_string(),
        TypeExpr::String(Some(n)) => format!("STRING {n}"),
        TypeExpr::String(None) => "STRING".to_string(),
        TypeExpr::Int { min, max } => {
            let mut s = "INT".to_string();
            if let Some(min) = min {
                write!(s, " MIN {min}").unwrap();
            }
            if let Some(max) = max {
                write!(s, " MAX {max}").unwrap();
            }
            s
        }
        TypeExpr::Decimal { precision, scale } => {
            let mut s = "DECIMAL".to_string();
            if let Some(p) = precision {
                write!(s, " PRECISION {p}").unwrap();
            }
            if let Some(sc) = scale {
                write!(s, " SCALE {sc}").unwrap();
            }
            s
        }
        TypeExpr::Enum(variants) => format!("ENUM {}", variants.join(" ")),
        TypeExpr::Ref { shape, field } => format!("UUID REF {shape}.{field}"),
        TypeExpr::List(inner) => format!("LIST {}", format_type(inner)),
        TypeExpr::Map(k, v) => format!("MAP {} {}", format_type(k), format_type(v)),
        TypeExpr::Blob => "BLOB".to_string(),
        TypeExpr::Maybe(inner) => format!("MAYBE {}", format_type(inner)),
    }
}

fn format_modifiers(mods: &[Modifier]) -> String {
    let parts: Vec<String> = mods
        .iter()
        .filter_map(|m| match m {
            Modifier::Pk => Some("PK".to_string()),
            Modifier::Auto => Some("AUTO".to_string()),
            Modifier::Required => Some("REQUIRED".to_string()),
            Modifier::Unique => Some("UNIQUE".to_string()),
            Modifier::Default(v) => {
                let val = match v {
                    LiteralValue::Int(n) => n.to_string(),
                    LiteralValue::Decimal(s) => s.clone(),
                    LiteralValue::String(s) => format!("\"{s}\""),
                    LiteralValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                    LiteralValue::Ident(s) => s.clone(),
                    LiteralValue::Now => "NOW".to_string(),
                    LiteralValue::None => "NONE".to_string(),
                };
                Some(format!("DEFAULT {val}"))
            }
            Modifier::Precision(_) | Modifier::Scale(_) => None,
            Modifier::Min(n) => Some(format!("MIN {n}")),
            Modifier::Max(n) => Some(format!("MAX {n}")),
            Modifier::Ref { shape, field } => Some(format!("REF {shape}.{field}")),
        })
        .collect();
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Program {
        let mut lexer = crate::lexer::Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = crate::parser::Parser::new(tokens);
        parser.parse_program().unwrap()
    }

    #[test]
    fn test_no_changes() {
        let src = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#;
        let old = parse(src);
        let new = parse(src);
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(!result.has_changes());
    }

    #[test]
    fn test_add_field() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  email STRING 255 REQUIRED
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        assert_eq!(result.migrations.len(), 1);
        let m = &result.migrations[0];
        assert_eq!(m.shape, "User");
        assert!(
            m.ops
                .iter()
                .any(|op| matches!(op, DiffOp::AddField(f) if f.name == "email"))
        );
    }

    #[test]
    fn test_drop_field() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  bio TEXT
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        assert!(
            result.migrations[0]
                .ops
                .iter()
                .any(|op| matches!(op, DiffOp::DropField(n) if n == "bio"))
        );
    }

    #[test]
    fn test_change_type() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 200 REQUIRED
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        assert!(
            result.migrations[0]
                .ops
                .iter()
                .any(|op| matches!(op, DiffOp::ChangeType { field, .. } if field == "name"))
        );
    }

    #[test]
    fn test_add_modifier() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        assert!(result.migrations[0].ops.iter().any(|op|
            matches!(op, DiffOp::AddModifier { field, modifier } if field == "name" && modifier == "REQUIRED")));
    }

    #[test]
    fn test_new_shape() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO

SHAPE Order
  id UUID PK AUTO
  total DECIMAL PRECISION 10 SCALE 2
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        let order_migration = result
            .migrations
            .iter()
            .find(|m| m.shape == "Order")
            .unwrap();
        assert!(
            order_migration
                .ops
                .iter()
                .all(|op| matches!(op, DiffOp::AddField(_)))
        );
    }

    #[test]
    fn test_dropped_shape() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO

SHAPE Legacy
  id UUID PK AUTO
  data TEXT
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        let legacy = result
            .migrations
            .iter()
            .find(|m| m.shape == "Legacy")
            .unwrap();
        assert!(
            legacy
                .ops
                .iter()
                .all(|op| matches!(op, DiffOp::DropField(_)))
        );
    }

    #[test]
    fn test_format_migration_output() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  email STRING 255 REQUIRED
  bio MAYBE TEXT
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        let output = format_all_migrations(&result);
        assert!(output.contains("MIGRATE User v1 TO v2"));
        assert!(output.contains("ADD email STRING 255 REQUIRED"));
        assert!(output.contains("ADD bio MAYBE TEXT"));
    }

    #[test]
    fn test_format_drop_migration() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  legacy TEXT
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        let output = format_all_migrations(&result);
        assert!(output.contains("DROP legacy"));
    }

    #[test]
    fn test_enum_change() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
  role ENUM admin user REQUIRED
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO
  role ENUM admin user moderator REQUIRED
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        assert!(
            result.migrations[0]
                .ops
                .iter()
                .any(|op| matches!(op, DiffOp::ChangeType { field, .. } if field == "role"))
        );
    }

    #[test]
    fn test_multiple_shapes_changed() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100

SHAPE Order
  id UUID PK AUTO
  total DECIMAL
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO
  name STRING 100
  email STRING 255

SHAPE Order
  id UUID PK AUTO
  total DECIMAL
  status ENUM pending completed
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert_eq!(result.migrations.len(), 2);
    }

    #[test]
    fn test_diff_booking_to_extended() {
        let old = parse(
            r#"SHAPE Booking
  id UUID PK AUTO
  user_id UUID REF User.id REQUIRED
  listing_id UUID REF Listing.id REQUIRED
  check_in DATE REQUIRED
  check_out DATE REQUIRED
  status ENUM pending confirmed cancelled REQUIRED
  total_price DECIMAL PRECISION 10 SCALE 2 REQUIRED
"#,
        );
        let new = parse(
            r#"SHAPE Booking
  id UUID PK AUTO
  user_id UUID REF User.id REQUIRED
  listing_id UUID REF Listing.id REQUIRED
  check_in DATE REQUIRED
  check_out DATE REQUIRED
  status ENUM pending confirmed cancelled completed REQUIRED
  total_price DECIMAL PRECISION 10 SCALE 2 REQUIRED
  guest_count INT MIN 1 MAX 16 DEFAULT 1
  notes MAYBE TEXT
  created_at TIMESTAMP AUTO
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        let m = &result.migrations[0];
        assert!(
            m.ops
                .iter()
                .any(|op| matches!(op, DiffOp::AddField(f) if f.name == "guest_count"))
        );
        assert!(
            m.ops
                .iter()
                .any(|op| matches!(op, DiffOp::AddField(f) if f.name == "notes"))
        );
        assert!(
            m.ops
                .iter()
                .any(|op| matches!(op, DiffOp::AddField(f) if f.name == "created_at"))
        );
        assert!(
            m.ops
                .iter()
                .any(|op| matches!(op, DiffOp::ChangeType { field, .. } if field == "status"))
        );

        let output = format_all_migrations(&result);
        assert!(output.contains("MIGRATE Booking v1 TO v2"));
    }

    #[test]
    fn test_stream_added() {
        let old = parse(
            r#"SHAPE User
  id UUID PK AUTO
"#,
        );
        let new = parse(
            r#"SHAPE User
  id UUID PK AUTO

STREAM updates ws /ws/updates
  EVENT user_online
    user_id UUID
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        assert!(
            result
                .stream_changes
                .iter()
                .any(|c| matches!(c, StreamChange::Added(n) if n == "updates"))
        );
        let output = format_all_migrations(&result);
        assert!(output.contains("STREAM_ADDED updates"));
    }

    #[test]
    fn test_stream_event_added() {
        let old = parse(
            r#"STREAM updates ws /ws/updates
  EVENT user_online
    user_id UUID
"#,
        );
        let new = parse(
            r#"STREAM updates ws /ws/updates
  EVENT user_online
    user_id UUID
  EVENT user_offline
    user_id UUID
"#,
        );
        let result = diff_programs(&old, &new, "v1", "v2");
        assert!(result.has_changes());
        assert!(result.stream_changes.iter().any(
            |c| matches!(c, StreamChange::EventAdded { event, .. } if event == "user_offline")
        ));
    }
}
