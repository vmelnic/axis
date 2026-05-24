use std::fmt::Write;

use serde::Serialize;

use crate::ast::{
    Construct, MigrateDef, MigrateOp, Modifier, Program, SourceDef, SourceType, TypeExpr,
};
use crate::codegens::sql as codegen;

#[derive(Debug, Serialize)]
pub struct MigrationPlan {
    pub migrations: Vec<MigrationStep>,
    pub current_schema: String,
}

#[derive(Debug, Serialize)]
pub struct MigrationStep {
    pub id: String,
    pub shape: String,
    pub from_version: String,
    pub to_version: String,
    pub up: String,
    pub down: String,
    pub operations: Vec<MigrationOp>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MigrationOp {
    AddColumn { table: String, column: String, column_type: String, nullable: bool },
    DropColumn { table: String, column: String },
    RenameColumn { table: String, from: String, to: String },
    CopyData { fields: Vec<String> },
    ComputeField { table: String, field: String },
}

pub fn plan_migrations(program: &Program) -> MigrationPlan {
    let sources: Vec<&SourceDef> = program.constructs.iter().filter_map(|c| {
        if let Construct::Source(s) = c { Some(s) } else { None }
    }).collect();

    let migrates: Vec<&MigrateDef> = program.constructs.iter().filter_map(|c| {
        if let Construct::Migrate(m) = c { Some(m) } else { None }
    }).collect();

    let migrations: Vec<MigrationStep> = migrates.iter().enumerate().map(|(i, m)| {
        let source = sources.iter().find(|s| s.shape == m.shape);
        let table = source.map(|s| s.name.clone())
            .unwrap_or_else(|| m.shape.to_lowercase());
        let stype = source.map(|s| s.source_type).unwrap_or(SourceType::Postgres);

        let (up, down, operations) = generate_up_down(m, &table, stype);

        MigrationStep {
            id: format!("{:04}_{}_{}_to_{}", i + 1, m.shape.to_lowercase(), m.from_version, m.to_version),
            shape: m.shape.clone(),
            from_version: m.from_version.clone(),
            to_version: m.to_version.clone(),
            up,
            down,
            operations,
        }
    }).collect();

    let codegen_result = codegen::generate(program);

    MigrationPlan {
        migrations,
        current_schema: codegen_result.sql,
    }
}

fn generate_up_down(m: &MigrateDef, table: &str, stype: SourceType) -> (String, String, Vec<MigrationOp>) {
    let mut up = String::new();
    let mut down = String::new();
    let mut ops = Vec::new();

    writeln!(up, "-- migrate {} from {} to {}", m.shape, m.from_version, m.to_version).unwrap();
    writeln!(up, "BEGIN;").unwrap();
    writeln!(down, "-- rollback {} from {} to {}", m.shape, m.to_version, m.from_version).unwrap();
    writeln!(down, "BEGIN;").unwrap();

    for op in &m.ops {
        match op {
            MigrateOp::Add(field) => {
                let ty = sql_type(&field.ty, stype);
                let nullable = !field.modifiers.iter().any(|m| matches!(m, Modifier::Required));
                let null_str = if nullable { "" } else { " NOT NULL" };
                writeln!(up, "ALTER TABLE {table} ADD COLUMN {} {ty}{null_str};", field.name).unwrap();
                writeln!(down, "ALTER TABLE {table} DROP COLUMN IF EXISTS {};", field.name).unwrap();
                ops.push(MigrationOp::AddColumn {
                    table: table.into(),
                    column: field.name.clone(),
                    column_type: ty,
                    nullable,
                });
            }
            MigrateOp::Drop(field) => {
                writeln!(up, "ALTER TABLE {table} DROP COLUMN {field};").unwrap();
                writeln!(down, "-- manual: recreate column {field}").unwrap();
                ops.push(MigrationOp::DropColumn {
                    table: table.into(),
                    column: field.clone(),
                });
            }
            MigrateOp::Rename { from, to } => {
                writeln!(up, "ALTER TABLE {table} RENAME COLUMN {from} TO {to};").unwrap();
                writeln!(down, "ALTER TABLE {table} RENAME COLUMN {to} TO {from};").unwrap();
                ops.push(MigrationOp::RenameColumn {
                    table: table.into(),
                    from: from.clone(),
                    to: to.clone(),
                });
            }
            MigrateOp::Copy(fields) => {
                writeln!(up, "-- copy fields: {}", fields.join(", ")).unwrap();
                ops.push(MigrationOp::CopyData { fields: fields.clone() });
            }
            MigrateOp::Compute { field, .. } => {
                writeln!(up, "-- compute field: {field}").unwrap();
                ops.push(MigrationOp::ComputeField {
                    table: table.into(),
                    field: field.clone(),
                });
            }
        }
    }

    writeln!(up, "COMMIT;").unwrap();
    writeln!(down, "COMMIT;").unwrap();

    (up, down, ops)
}

fn sql_type(ty: &TypeExpr, stype: SourceType) -> String {
    match (ty, stype) {
        (TypeExpr::Uuid, SourceType::Postgres) => "UUID".into(),
        (TypeExpr::Uuid, SourceType::Mysql) => "CHAR(36)".into(),
        (TypeExpr::Uuid, _) => "TEXT".into(),

        (TypeExpr::String(Some(_)), SourceType::Sqlite) => "TEXT".into(),
        (TypeExpr::String(Some(len)), _) => format!("VARCHAR({len})"),
        (TypeExpr::String(None) | TypeExpr::Text, _) => "TEXT".into(),

        (TypeExpr::Int { .. }, _) => "INTEGER".into(),

        (TypeExpr::Bool, SourceType::Mysql) => "TINYINT(1)".into(),
        (TypeExpr::Bool, SourceType::Sqlite) => "INTEGER".into(),
        (TypeExpr::Bool, _) => "BOOLEAN".into(),

        (TypeExpr::Timestamp, SourceType::Postgres) => "TIMESTAMPTZ".into(),
        (TypeExpr::Timestamp, SourceType::Mysql) => "DATETIME".into(),
        (TypeExpr::Timestamp, _) => "TEXT".into(),

        (TypeExpr::Date, SourceType::Sqlite) => "TEXT".into(),
        (TypeExpr::Date, _) => "DATE".into(),

        (TypeExpr::Decimal { .. }, SourceType::Sqlite) => "REAL".into(),
        (TypeExpr::Decimal { precision, scale }, _) => {
            let p = precision.unwrap_or(10);
            let s = scale.unwrap_or(2);
            format!("DECIMAL({p},{s})")
        }

        (TypeExpr::Json, SourceType::Postgres) => "JSONB".into(),
        (TypeExpr::Json, SourceType::Mysql) => "JSON".into(),
        (TypeExpr::Json, _) => "TEXT".into(),

        (TypeExpr::Enum(variants), _) => format!("VARCHAR({})", variants.iter().map(|v| v.len()).max().unwrap_or(50)),

        (TypeExpr::List(inner), SourceType::Postgres) => format!("{}[]", sql_type(inner, stype)),
        (TypeExpr::List(_), SourceType::Mysql) => "JSON".into(),
        (TypeExpr::List(_), _) => "TEXT".into(),

        (TypeExpr::Maybe(inner), _) => sql_type(inner, stype),

        (TypeExpr::Map(_, _), SourceType::Postgres) => "JSONB".into(),
        (TypeExpr::Map(_, _), SourceType::Mysql) => "JSON".into(),
        (TypeExpr::Map(_, _), _) => "TEXT".into(),

        (TypeExpr::Ref { .. }, SourceType::Postgres) => "UUID".into(),
        (TypeExpr::Ref { .. }, SourceType::Mysql) => "CHAR(36)".into(),
        (TypeExpr::Ref { .. }, _) => "TEXT".into(),

        (TypeExpr::Blob, SourceType::Postgres) => "BYTEA".into(),
        (TypeExpr::Blob, SourceType::Mysql) => "LONGBLOB".into(),
        (TypeExpr::Blob, _) => "BLOB".into(),
    }
}

pub fn format_migration_files(plan: &MigrationPlan) -> Vec<(String, String)> {
    plan.migrations.iter().map(|m| {
        let filename = format!("{}.sql", m.id);
        let mut content = String::new();
        writeln!(content, "-- UP").unwrap();
        content.push_str(&m.up);
        writeln!(content).unwrap();
        writeln!(content, "-- DOWN").unwrap();
        content.push_str(&m.down);
        (filename, content)
    }).collect()
}

pub struct MigrationRunner {
    pub cargo_toml: String,
    pub main_rs: String,
    pub migrations_sql: Vec<(String, String)>,
}

pub fn generate_runner(plan: &MigrationPlan) -> MigrationRunner {
    let migrations_sql = format_migration_files(plan);

    let cargo_toml = r#"[package]
name = "axis-migrate"
version = "0.1.0"
edition = "2024"

[dependencies]
sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "postgres"] }
tokio = { version = "1", features = ["full"] }
chrono = "0.4"
"#.to_string();

    let mut main_rs = String::new();
    writeln!(main_rs, "use sqlx::PgPool;").unwrap();
    writeln!(main_rs, "use chrono::Utc;").unwrap();
    writeln!(main_rs).unwrap();

    // Embed migrations as const arrays
    writeln!(main_rs, "struct Migration {{").unwrap();
    writeln!(main_rs, "    id: &'static str,").unwrap();
    writeln!(main_rs, "    up: &'static str,").unwrap();
    writeln!(main_rs, "    down: &'static str,").unwrap();
    writeln!(main_rs, "}}").unwrap();
    writeln!(main_rs).unwrap();

    writeln!(main_rs, "const MIGRATIONS: &[Migration] = &[").unwrap();
    for step in &plan.migrations {
        let up_escaped = step.up.replace('\\', "\\\\").replace('"', "\\\"");
        let down_escaped = step.down.replace('\\', "\\\\").replace('"', "\\\"");
        writeln!(main_rs, "    Migration {{ id: \"{}\", up: \"{}\", down: \"{}\" }},",
            step.id, up_escaped, down_escaped).unwrap();
    }
    writeln!(main_rs, "];").unwrap();
    writeln!(main_rs).unwrap();

    writeln!(main_rs, "#[tokio::main]").unwrap();
    writeln!(main_rs, "async fn main() {{").unwrap();
    writeln!(main_rs, "    let args: Vec<String> = std::env::args().collect();").unwrap();
    writeln!(main_rs, "    let command = args.get(1).map(|s| s.as_str()).unwrap_or(\"up\");").unwrap();
    writeln!(main_rs).unwrap();
    writeln!(main_rs, "    let url = std::env::var(\"DATABASE_URL\").expect(\"DATABASE_URL required\");").unwrap();
    writeln!(main_rs, "    let pool = PgPool::connect(&url).await.expect(\"db connect failed\");").unwrap();
    writeln!(main_rs).unwrap();
    writeln!(main_rs, "    ensure_migrations_table(&pool).await;").unwrap();
    writeln!(main_rs).unwrap();
    writeln!(main_rs, "    match command {{").unwrap();
    writeln!(main_rs, "        \"up\" => run_pending(&pool).await,").unwrap();
    writeln!(main_rs, "        \"down\" => rollback_last(&pool).await,").unwrap();
    writeln!(main_rs, "        \"status\" => show_status(&pool).await,").unwrap();
    writeln!(main_rs, "        other => eprintln!(\"unknown command: {{}}. use: up, down, status\", other),").unwrap();
    writeln!(main_rs, "    }}").unwrap();
    writeln!(main_rs, "}}").unwrap();
    writeln!(main_rs).unwrap();

    writeln!(main_rs, "async fn ensure_migrations_table(pool: &PgPool) {{").unwrap();
    writeln!(main_rs, "    sqlx::query(\"CREATE TABLE IF NOT EXISTS _axis_migrations (").unwrap();
    writeln!(main_rs, "        id VARCHAR(255) PRIMARY KEY,").unwrap();
    writeln!(main_rs, "        applied_at TIMESTAMPTZ NOT NULL DEFAULT now()").unwrap();
    writeln!(main_rs, "    )\").execute(pool).await.unwrap();").unwrap();
    writeln!(main_rs, "}}").unwrap();
    writeln!(main_rs).unwrap();

    writeln!(main_rs, "async fn get_applied(pool: &PgPool) -> Vec<String> {{").unwrap();
    writeln!(main_rs, "    sqlx::query_scalar::<_, String>(\"SELECT id FROM _axis_migrations ORDER BY id\")").unwrap();
    writeln!(main_rs, "        .fetch_all(pool).await.unwrap()").unwrap();
    writeln!(main_rs, "}}").unwrap();
    writeln!(main_rs).unwrap();

    writeln!(main_rs, "async fn run_pending(pool: &PgPool) {{").unwrap();
    writeln!(main_rs, "    let applied = get_applied(pool).await;").unwrap();
    writeln!(main_rs, "    let mut count = 0;").unwrap();
    writeln!(main_rs, "    for m in MIGRATIONS {{").unwrap();
    writeln!(main_rs, "        if !applied.contains(&m.id.to_string()) {{").unwrap();
    writeln!(main_rs, "            println!(\"applying {{}}\", m.id);").unwrap();
    writeln!(main_rs, "            sqlx::query(m.up).execute(pool).await").unwrap();
    writeln!(main_rs, "                .unwrap_or_else(|e| panic!(\"migration {{}} failed: {{}}\", m.id, e));").unwrap();
    writeln!(main_rs, "            sqlx::query(\"INSERT INTO _axis_migrations (id) VALUES ($1)\")").unwrap();
    writeln!(main_rs, "                .bind(m.id).execute(pool).await.unwrap();").unwrap();
    writeln!(main_rs, "            count += 1;").unwrap();
    writeln!(main_rs, "        }}").unwrap();
    writeln!(main_rs, "    }}").unwrap();
    writeln!(main_rs, "    println!(\"{{}} migration(s) applied\", count);").unwrap();
    writeln!(main_rs, "}}").unwrap();
    writeln!(main_rs).unwrap();

    writeln!(main_rs, "async fn rollback_last(pool: &PgPool) {{").unwrap();
    writeln!(main_rs, "    let applied = get_applied(pool).await;").unwrap();
    writeln!(main_rs, "    if let Some(last) = applied.last() {{").unwrap();
    writeln!(main_rs, "        let m = MIGRATIONS.iter().find(|m| m.id == last).expect(\"migration not found\");").unwrap();
    writeln!(main_rs, "        println!(\"rolling back {{}}\", m.id);").unwrap();
    writeln!(main_rs, "        sqlx::query(m.down).execute(pool).await").unwrap();
    writeln!(main_rs, "            .unwrap_or_else(|e| panic!(\"rollback {{}} failed: {{}}\", m.id, e));").unwrap();
    writeln!(main_rs, "        sqlx::query(\"DELETE FROM _axis_migrations WHERE id = $1\")").unwrap();
    writeln!(main_rs, "            .bind(m.id).execute(pool).await.unwrap();").unwrap();
    writeln!(main_rs, "        println!(\"rolled back {{}}\", m.id);").unwrap();
    writeln!(main_rs, "    }} else {{").unwrap();
    writeln!(main_rs, "        println!(\"no migrations to roll back\");").unwrap();
    writeln!(main_rs, "    }}").unwrap();
    writeln!(main_rs, "}}").unwrap();
    writeln!(main_rs).unwrap();

    writeln!(main_rs, "async fn show_status(pool: &PgPool) {{").unwrap();
    writeln!(main_rs, "    let applied = get_applied(pool).await;").unwrap();
    writeln!(main_rs, "    for m in MIGRATIONS {{").unwrap();
    writeln!(main_rs, "        let status = if applied.contains(&m.id.to_string()) {{ \"applied\" }} else {{ \"pending\" }};").unwrap();
    writeln!(main_rs, "        println!(\"{{}} [{{}}]\", m.id, status);").unwrap();
    writeln!(main_rs, "    }}").unwrap();
    writeln!(main_rs, "}}").unwrap();

    MigrationRunner {
        cargo_toml,
        main_rs,
        migrations_sql,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::compile_source;

    #[test]
    fn test_no_migrations() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id
"#;
        let program = compile_source(input).unwrap();
        let plan = plan_migrations(&program);
        assert!(plan.migrations.is_empty());
        assert!(plan.current_schema.contains("CREATE TABLE"));
    }

    #[test]
    fn test_add_column_migration() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  total DECIMAL PRECISION 10 SCALE 2
  status ENUM pending confirmed shipped
  tracking_number STRING 100

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

MIGRATE Order v1 TO v2
  COPY id total status
  ADD tracking_number STRING 100
  ADD shipped_at TIMESTAMP
  DROP old_field
  RENAME status TO order_status
"#;
        let program = compile_source(input).unwrap();
        let plan = plan_migrations(&program);

        assert_eq!(plan.migrations.len(), 1);
        let m = &plan.migrations[0];
        assert_eq!(m.shape, "Order");
        assert_eq!(m.from_version, "v1");
        assert_eq!(m.to_version, "v2");
        assert!(m.up.contains("ALTER TABLE orders ADD COLUMN tracking_number"));
        assert!(m.up.contains("ALTER TABLE orders ADD COLUMN shipped_at"));
        assert!(m.up.contains("ALTER TABLE orders DROP COLUMN old_field"));
        assert!(m.up.contains("ALTER TABLE orders RENAME COLUMN status TO order_status"));
        assert!(m.up.contains("BEGIN;"));
        assert!(m.up.contains("COMMIT;"));

        assert!(m.down.contains("DROP COLUMN IF EXISTS tracking_number"));
        assert!(m.down.contains("DROP COLUMN IF EXISTS shipped_at"));
        assert!(m.down.contains("RENAME COLUMN order_status TO status"));

        assert_eq!(m.operations.len(), 5);
    }

    #[test]
    fn test_migration_id_format() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  email STRING 200

SOURCE users POSTGRES
  SHAPE User
  INDEX id

MIGRATE User v1 TO v2
  COPY id name
  ADD email STRING 200
"#;
        let program = compile_source(input).unwrap();
        let plan = plan_migrations(&program);
        assert_eq!(plan.migrations[0].id, "0001_user_v1_to_v2");
    }

    #[test]
    fn test_migration_files() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  email STRING 200

SOURCE users POSTGRES
  SHAPE User
  INDEX id

MIGRATE User v1 TO v2
  COPY id name
  ADD email STRING 200
"#;
        let program = compile_source(input).unwrap();
        let plan = plan_migrations(&program);
        let files = format_migration_files(&plan);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "0001_user_v1_to_v2.sql");
        assert!(files[0].1.contains("-- UP"));
        assert!(files[0].1.contains("-- DOWN"));
    }

    #[test]
    fn test_migration_json_serializes() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  total DECIMAL PRECISION 10 SCALE 2
  tracking STRING 50

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

MIGRATE Order v1 TO v2
  COPY id total
  ADD tracking STRING 50
"#;
        let program = compile_source(input).unwrap();
        let plan = plan_migrations(&program);
        let json = serde_json::to_string_pretty(&plan).unwrap();
        assert!(json.contains("add_column"));
        assert!(json.contains("tracking"));
        assert!(json.contains("0001_order"));
    }

    #[test]
    fn test_full_example_migrations() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/full.axis")
        ).unwrap();
        let program = compile_source(&input).unwrap();
        let plan = plan_migrations(&program);
        assert_eq!(plan.migrations.len(), 1);
        assert!(plan.current_schema.contains("CREATE TABLE"));
    }

    #[test]
    fn test_migration_runner() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  amount DECIMAL

SOURCE orders POSTGRES
  SHAPE Order

MIGRATE Order v1 TO v2
  ADD tracking STRING 100
"#;
        let program = compile_source(input).unwrap();
        let plan = plan_migrations(&program);
        let runner = generate_runner(&plan);
        assert!(runner.cargo_toml.contains("axis-migrate"));
        assert!(runner.cargo_toml.contains("sqlx"));
        assert!(runner.main_rs.contains("_axis_migrations"));
        assert!(runner.main_rs.contains("run_pending"));
        assert!(runner.main_rs.contains("rollback_last"));
        assert!(runner.main_rs.contains("show_status"));
        assert_eq!(runner.migrations_sql.len(), 1);
    }
}
