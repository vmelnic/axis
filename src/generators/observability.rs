use crate::ast::*;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ObservabilitySchema {
    pub metrics: Vec<MetricDef>,
    pub trace_schema: TraceSchema,
    pub log_fields: Vec<LogField>,
}

#[derive(Debug, Serialize)]
pub struct MetricDef {
    pub name: String,
    pub metric_type: MetricType,
    pub labels: Vec<String>,
    pub description: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricType {
    Counter,
    Histogram,
    Gauge,
}

#[derive(Debug, Serialize)]
pub struct TraceSchema {
    pub root_span: SpanSchema,
    pub child_spans: Vec<SpanSchema>,
}

#[derive(Debug, Serialize)]
pub struct SpanSchema {
    pub name: String,
    pub attributes: Vec<SpanAttribute>,
}

#[derive(Debug, Serialize)]
pub struct SpanAttribute {
    pub key: String,
    pub value_type: String,
}

#[derive(Debug, Serialize)]
pub struct LogField {
    pub field: String,
    pub value_type: String,
    pub source: String,
}

pub fn generate_observability(program: &Program) -> ObservabilitySchema {
    let mut metrics = vec![
        MetricDef {
            name: "axis_request_duration_seconds".into(),
            metric_type: MetricType::Histogram,
            labels: vec!["flow".into(), "method".into(), "status".into()],
            description: "Request duration in seconds per flow".into(),
        },
        MetricDef {
            name: "axis_request_total".into(),
            metric_type: MetricType::Counter,
            labels: vec!["flow".into(), "method".into(), "status".into()],
            description: "Total requests per flow".into(),
        },
    ];

    let mut sources_seen = std::collections::HashSet::new();
    let mut services_seen = std::collections::HashSet::new();
    let mut effects_seen = std::collections::HashSet::new();
    let mut has_saga = false;
    let mut has_stream = false;

    for construct in &program.constructs {
        match construct {
            Construct::Source(s) => {
                if sources_seen.insert(s.name.clone()) {
                    // already tracked
                }
            }
            Construct::Flow(f) => {
                collect_flow_observability(
                    f,
                    &mut sources_seen,
                    &mut services_seen,
                    &mut effects_seen,
                );
            }
            Construct::Saga(_) => {
                has_saga = true;
            }
            Construct::Stream(_) => {
                has_stream = true;
            }
            _ => {}
        }
    }

    if !sources_seen.is_empty() {
        metrics.push(MetricDef {
            name: "axis_query_duration_seconds".into(),
            metric_type: MetricType::Histogram,
            labels: vec!["source".into(), "index_used".into()],
            description: "Query duration per source".into(),
        });
        metrics.push(MetricDef {
            name: "axis_query_rows_returned".into(),
            metric_type: MetricType::Histogram,
            labels: vec!["source".into()],
            description: "Number of rows returned per query".into(),
        });
    }

    if !services_seen.is_empty() {
        metrics.push(MetricDef {
            name: "axis_call_duration_seconds".into(),
            metric_type: MetricType::Histogram,
            labels: vec!["service".into(), "method".into(), "status".into()],
            description: "External service call duration".into(),
        });
    }

    if !effects_seen.is_empty() {
        metrics.push(MetricDef {
            name: "axis_effect_queue_depth".into(),
            metric_type: MetricType::Gauge,
            labels: vec!["type".into()],
            description: "Current effect queue depth".into(),
        });
    }

    if has_saga {
        metrics.push(MetricDef {
            name: "axis_saga_step_duration_seconds".into(),
            metric_type: MetricType::Histogram,
            labels: vec!["saga".into(), "step".into(), "status".into()],
            description: "Saga step duration".into(),
        });
        metrics.push(MetricDef {
            name: "axis_saga_compensation_total".into(),
            metric_type: MetricType::Counter,
            labels: vec!["saga".into(), "step".into()],
            description: "Total saga compensations triggered".into(),
        });
    }

    if has_stream {
        metrics.push(MetricDef {
            name: "axis_stream_connections".into(),
            metric_type: MetricType::Gauge,
            labels: vec!["stream".into(), "transport".into()],
            description: "Active stream connections".into(),
        });
        metrics.push(MetricDef {
            name: "axis_stream_messages_total".into(),
            metric_type: MetricType::Counter,
            labels: vec!["stream".into(), "direction".into(), "event".into()],
            description: "Total stream messages sent/received".into(),
        });
    }

    let root_span = SpanSchema {
        name: "axis.request".into(),
        attributes: vec![
            SpanAttribute {
                key: "flow".into(),
                value_type: "string".into(),
            },
            SpanAttribute {
                key: "method".into(),
                value_type: "string".into(),
            },
            SpanAttribute {
                key: "path".into(),
                value_type: "string".into(),
            },
            SpanAttribute {
                key: "surface".into(),
                value_type: "string".into(),
            },
            SpanAttribute {
                key: "auth.user_id".into(),
                value_type: "string".into(),
            },
            SpanAttribute {
                key: "tenant".into(),
                value_type: "string".into(),
            },
            SpanAttribute {
                key: "response.code".into(),
                value_type: "int".into(),
            },
        ],
    };

    let mut child_spans = vec![
        SpanSchema {
            name: "axis.guard".into(),
            attributes: vec![
                SpanAttribute {
                    key: "guard.name".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "guard.result".into(),
                    value_type: "string".into(),
                },
            ],
        },
        SpanSchema {
            name: "axis.fetch".into(),
            attributes: vec![
                SpanAttribute {
                    key: "source".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "result".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "index_used".into(),
                    value_type: "string".into(),
                },
            ],
        },
        SpanSchema {
            name: "axis.query".into(),
            attributes: vec![
                SpanAttribute {
                    key: "source".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "rows_returned".into(),
                    value_type: "int".into(),
                },
                SpanAttribute {
                    key: "index_used".into(),
                    value_type: "string".into(),
                },
            ],
        },
        SpanSchema {
            name: "axis.insert".into(),
            attributes: vec![SpanAttribute {
                key: "source".into(),
                value_type: "string".into(),
            }],
        },
        SpanSchema {
            name: "axis.update".into(),
            attributes: vec![
                SpanAttribute {
                    key: "source".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "rows_affected".into(),
                    value_type: "int".into(),
                },
            ],
        },
        SpanSchema {
            name: "axis.delete".into(),
            attributes: vec![
                SpanAttribute {
                    key: "source".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "rows_affected".into(),
                    value_type: "int".into(),
                },
            ],
        },
    ];

    if !services_seen.is_empty() {
        child_spans.push(SpanSchema {
            name: "axis.call".into(),
            attributes: vec![
                SpanAttribute {
                    key: "service".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "method".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "status".into(),
                    value_type: "string".into(),
                },
            ],
        });
    }

    if !effects_seen.is_empty() {
        child_spans.push(SpanSchema {
            name: "axis.effect".into(),
            attributes: vec![
                SpanAttribute {
                    key: "type".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "template".into(),
                    value_type: "string".into(),
                },
                SpanAttribute {
                    key: "queued".into(),
                    value_type: "bool".into(),
                },
            ],
        });
    }

    let log_fields = vec![
        LogField {
            field: "trace_id".into(),
            value_type: "string".into(),
            source: "generated".into(),
        },
        LogField {
            field: "flow".into(),
            value_type: "string".into(),
            source: "request".into(),
        },
        LogField {
            field: "surface".into(),
            value_type: "string".into(),
            source: "routing".into(),
        },
        LogField {
            field: "method".into(),
            value_type: "string".into(),
            source: "request".into(),
        },
        LogField {
            field: "path".into(),
            value_type: "string".into(),
            source: "request".into(),
        },
        LogField {
            field: "auth.user_id".into(),
            value_type: "string".into(),
            source: "auth".into(),
        },
        LogField {
            field: "auth.role".into(),
            value_type: "string".into(),
            source: "auth".into(),
        },
        LogField {
            field: "tenant".into(),
            value_type: "string".into(),
            source: "scope".into(),
        },
        LogField {
            field: "duration_ms".into(),
            value_type: "int".into(),
            source: "timing".into(),
        },
        LogField {
            field: "response.code".into(),
            value_type: "int".into(),
            source: "response".into(),
        },
        LogField {
            field: "steps".into(),
            value_type: "array".into(),
            source: "execution".into(),
        },
        LogField {
            field: "queries".into(),
            value_type: "array".into(),
            source: "execution".into(),
        },
    ];

    ObservabilitySchema {
        metrics,
        trace_schema: TraceSchema {
            root_span,
            child_spans,
        },
        log_fields,
    }
}

fn collect_flow_observability(
    flow: &FlowDef,
    sources: &mut std::collections::HashSet<String>,
    services: &mut std::collections::HashSet<String>,
    effects: &mut std::collections::HashSet<String>,
) {
    for step in &flow.steps {
        collect_step_deps(step, sources, services, effects);
    }
}

fn collect_step_deps(
    step: &FlowStep,
    sources: &mut std::collections::HashSet<String>,
    services: &mut std::collections::HashSet<String>,
    effects: &mut std::collections::HashSet<String>,
) {
    match step {
        FlowStep::Let(l) => collect_expr_deps(&l.expr, sources, services),
        FlowStep::Insert(i) => {
            sources.insert(i.source.clone());
        }
        FlowStep::Upsert(u) => {
            sources.insert(u.source.clone());
        }
        FlowStep::Update(u) => {
            sources.insert(u.source.clone());
        }
        FlowStep::Delete(d) => {
            sources.insert(d.source.clone());
        }
        FlowStep::Fanout(f) => {
            sources.insert(f.insert.source.clone());
        }
        FlowStep::Effect(e) => {
            effects.insert(format!("{:?}", e.kind).to_lowercase());
        }
        FlowStep::Match(m) => {
            for branch in &m.branches {
                for s in &branch.steps {
                    collect_step_deps(s, sources, services, effects);
                }
            }
            if let Some(default_steps) = &m.default {
                for s in default_steps {
                    collect_step_deps(s, sources, services, effects);
                }
            }
        }
        FlowStep::Set(_) => {}
        FlowStep::Each(e) => {
            for s in &e.steps {
                collect_step_deps(s, sources, services, effects);
            }
        }
        FlowStep::Try(t) => {
            for s in &t.body {
                collect_step_deps(s, sources, services, effects);
            }
            for s in &t.recover {
                collect_step_deps(s, sources, services, effects);
            }
        }
        FlowStep::Rule(_) | FlowStep::Guard(_) | FlowStep::Upload(_) => {}
    }
}

fn collect_expr_deps(
    expr: &Expr,
    sources: &mut std::collections::HashSet<String>,
    services: &mut std::collections::HashSet<String>,
) {
    match expr {
        Expr::Fetch { source, .. } | Expr::Query { source, .. } => {
            sources.insert(source.clone());
        }
        Expr::Call { service, .. } => {
            services.insert(service.clone());
        }
        Expr::Cached { expr, .. } => collect_expr_deps(expr, sources, services),
        _ => {}
    }
}

pub fn export_prometheus_config(schema: &ObservabilitySchema) -> String {
    let mut out = String::new();
    for metric in &schema.metrics {
        let ty = match metric.metric_type {
            MetricType::Counter => "counter",
            MetricType::Histogram => "histogram",
            MetricType::Gauge => "gauge",
        };
        out.push_str(&format!("# HELP {} {}\n", metric.name, metric.description));
        out.push_str(&format!("# TYPE {} {}\n", metric.name, ty));
        out.push_str(&format!("# LABELS: {}\n\n", metric.labels.join(", ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse(input: &str) -> Program {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        parser.parse_program().unwrap()
    }

    #[test]
    fn test_basic_observability() {
        let program = parse(
            r#"SHAPE User
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
"#,
        );
        let schema = generate_observability(&program);
        assert!(schema.metrics.len() >= 3);
        assert_eq!(schema.trace_schema.root_span.name, "axis.request");
        assert!(schema.log_fields.len() >= 10);
    }

    #[test]
    fn test_service_metrics() {
        let program = parse(
            r#"SHAPE Order
  id UUID PK AUTO
  total DECIMAL PRECISION 10 SCALE 2

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
    INPUT amount DECIMAL
    OUTPUT id STRING 100
    TIMEOUT 30 s
    RETRY 3 BACKOFF exponential

FLOW charge_order post /orders/:id/charge
  REALM api
  AUTH session
  LET order
    FETCH orders
      FILTER id EQ path.id
    OR 404
  LET payment
    CALL payments.charge
      amount order.total
    OR 500
  RETURN 200 payment
"#,
        );
        let schema = generate_observability(&program);
        let call_metric = schema
            .metrics
            .iter()
            .find(|m| m.name == "axis_call_duration_seconds");
        assert!(call_metric.is_some());
    }

    #[test]
    fn test_saga_metrics() {
        let program = parse(
            r#"SHAPE Order
  id UUID PK AUTO
  status ENUM pending confirmed

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

REALM api
  CAPABILITY read orders
  CAPABILITY write orders

SAGA process POST /orders/process
  REALM api
  AUTH session
  BODY ProcessReq
    order_id UUID REQUIRED
  STEP load
    LET order
      FETCH orders
        FILTER id EQ body.order_id
      OR 404
    VERIFY
      EQ order.status pending
    YIELD order
    COMPENSATE NONE
  ON_FAILURE RUN_COMPENSATIONS
  ON_SUCCESS
    RETURN 200 order
"#,
        );
        let schema = generate_observability(&program);
        let saga_metric = schema
            .metrics
            .iter()
            .find(|m| m.name == "axis_saga_step_duration_seconds");
        assert!(saga_metric.is_some());
    }

    #[test]
    fn test_prometheus_export() {
        let program = parse(
            r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM api
  CAPABILITY read users

FLOW get_user get /users/:id
  REALM api
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#,
        );
        let schema = generate_observability(&program);
        let prom = export_prometheus_config(&schema);
        assert!(prom.contains("# TYPE axis_request_duration_seconds histogram"));
        assert!(prom.contains("# LABELS: flow, method, status"));
    }

    #[test]
    fn test_stream_metrics() {
        let program = parse(
            r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id

STREAM updates ws /ws/updates
  EVENT user_online
    user_id UUID
"#,
        );
        let schema = generate_observability(&program);
        let conn_metric = schema
            .metrics
            .iter()
            .find(|m| m.name == "axis_stream_connections");
        assert!(conn_metric.is_some());
        let msg_metric = schema
            .metrics
            .iter()
            .find(|m| m.name == "axis_stream_messages_total");
        assert!(msg_metric.is_some());
    }
}
