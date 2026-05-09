pub mod crates;
pub mod tokens;

use sea_orm::{DatabaseBackend, Statement, Value};

fn stmt(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

fn opt_string_value(value: Option<String>) -> Value {
    Value::String(value.map(Box::new))
}

fn json_value(value: serde_json::Value) -> Value {
    Value::Json(Some(Box::new(value)))
}
