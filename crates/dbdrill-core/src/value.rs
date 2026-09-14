use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use bytes::BytesMut;
use postgres::types::{FromSql, IsNull, ToSql, Type, to_sql_checked};

type SqlResult<T> = std::result::Result<T, Box<dyn std::error::Error + Sync + Send>>;

/// How SQL NULL values are rendered.
pub const NULL_STR: &str = "<NULL>";

/// Defines [`Value`] along with its conversions to and from SQL.
///
/// Each entry maps a variant to the Rust type holding the decoded value, and to
/// the PostgreSQL types that decode into it. Scalars are displayed using their
/// `Display` implementation, arrays using their `Debug` one.
macro_rules! define_value {
    (
        scalars { $($s_variant:ident($s_ty:ty) => [$($s_pg:ident),+]),* $(,)? }
        arrays { $($a_variant:ident($a_ty:ty) => [$($a_pg:ident),+]),* $(,)? }
    ) => {
        /// A decoded SQL value.
        ///
        /// Decoding never fails: unsupported types and values we cannot read
        /// are kept as [`Value::Error`], so that one bad column does not make a
        /// whole result set unreadable.
        #[derive(Clone, Debug, PartialEq)]
        pub enum Value {
            Null,
            Error(String),
            $($s_variant($s_ty),)*
            $($a_variant(Vec<$a_ty>),)*
        }

        impl fmt::Display for Value {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    Value::Null => f.write_str(NULL_STR),
                    Value::Error(msg) => f.write_str(msg),
                    $(Value::$s_variant(v) => write!(f, "{v}"),)*
                    $(Value::$a_variant(v) => write!(f, "{v:?}"),)*
                }
            }
        }

        impl FromSql<'_> for Value {
            fn from_sql(ty: &Type, raw: &[u8]) -> SqlResult<Self> {
                $(if $(ty == &Type::$s_pg)||+ {
                    return Ok(match <$s_ty>::from_sql(ty, raw) {
                        Ok(v) => Value::$s_variant(v),
                        Err(err) => Value::Error(err.to_string()),
                    });
                })*
                $(if $(ty == &Type::$a_pg)||+ {
                    return Ok(match Vec::<$a_ty>::from_sql(ty, raw) {
                        Ok(v) => Value::$a_variant(v),
                        Err(err) => Value::Error(err.to_string()),
                    });
                })*

                Ok(Value::Error(format!("unsupported type: {ty}")))
            }

            fn from_sql_nullable(ty: &Type, raw: Option<&[u8]>) -> SqlResult<Self> {
                match raw {
                    Some(raw) => Self::from_sql(ty, raw),
                    None => Ok(Value::Null),
                }
            }

            /// Accepts every type: unsupported ones decode to [`Value::Error`].
            fn accepts(_ty: &Type) -> bool {
                true
            }
        }

        impl ToSql for Value {
            fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> SqlResult<IsNull> {
                match self {
                    Value::Null => Ok(IsNull::Yes),
                    Value::Error(msg) => {
                        Err(format!("value could not be decoded: {msg}").into())
                    }
                    $(Value::$s_variant(v) => v.to_sql_checked(ty, out),)*
                    $(Value::$a_variant(v) => v.to_sql_checked(ty, out),)*
                }
            }

            /// Accepts every type: mismatches are reported by [`ToSql::to_sql`],
            /// which checks the value against the type it is used as.
            fn accepts(_ty: &Type) -> bool {
                true
            }

            to_sql_checked!();
        }
    };
}

define_value! {
    scalars {
        Bool(bool) => [BOOL],
        Float4(f32) => [FLOAT4],
        Float8(f64) => [FLOAT8],
        Int2(i16) => [INT2],
        Int4(i32) => [INT4],
        Int8(i64) => [INT8],
        Json(serde_json::Value) => [JSON, JSONB],
        Text(String) => [TEXT, VARCHAR],
        Timestamptz(jiff::Timestamp) => [TIMESTAMPTZ],
        Uuid(uuid::Uuid) => [UUID],
    }
    arrays {
        BoolArray(bool) => [BOOL_ARRAY],
        Float4Array(f32) => [FLOAT4_ARRAY],
        Float8Array(f64) => [FLOAT8_ARRAY],
        Int2Array(i16) => [INT2_ARRAY],
        Int4Array(i32) => [INT4_ARRAY],
        Int8Array(i64) => [INT8_ARRAY],
        JsonArray(serde_json::Value) => [JSON_ARRAY, JSONB_ARRAY],
        TextArray(String) => [TEXT_ARRAY, VARCHAR_ARRAY],
        TimestamptzArray(jiff::Timestamp) => [TIMESTAMPTZ_ARRAY],
        UuidArray(uuid::Uuid) => [UUID_ARRAY],
    }
}

impl Value {
    /// Orders two values for sorting.
    ///
    /// Numbers are compared as numbers; everything else is compared as it is
    /// displayed. That is only a shortcut for types whose text form already
    /// sorts correctly: timestamps are RFC 3339, booleans are "false" before
    /// "true", and for the rest any stable order will do.
    pub fn compare(&self, other: &Value) -> Ordering {
        if let (Some(a), Some(b)) = (self.as_int(), other.as_int()) {
            return a.cmp(&b);
        }

        if let (Some(a), Some(b)) = (self.as_float(), other.as_float()) {
            return a.total_cmp(&b);
        }

        self.to_string().cmp(&other.to_string())
    }

    /// The value laid out for reading on its own, rather than in a table cell.
    ///
    /// Only JSON differs from [`Display`](fmt::Display): it is indented, since
    /// the one-line form is unreadable past a couple of fields.
    pub fn to_pretty_string(&self) -> String {
        match self {
            Value::Json(json) => {
                serde_json::to_string_pretty(json).unwrap_or_else(|_| json.to_string())
            }
            other => other.to_string(),
        }
    }

    /// The value as an integer, when it is one, so that whole numbers compare
    /// exactly rather than through a float.
    fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int2(v) => Some(i64::from(*v)),
            Value::Int4(v) => Some(i64::from(*v)),
            Value::Int8(v) => Some(*v),
            _ => None,
        }
    }

    fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float4(v) => Some(f64::from(*v)),
            Value::Float8(v) => Some(*v),
            _ => self.as_int().map(|v| v as f64),
        }
    }
}

/// A column of a [`ResultSet`].
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub name: String,
    pub ty: Type,
}

/// A row of a [`ResultSet`].
///
/// Rows carry a shared handle on the column list, so that a row taken out of
/// its result set still knows what its values are called. Cloning a row copies
/// its values but shares the column list.
#[derive(Clone, Debug)]
pub struct Row {
    columns: Arc<Vec<Column>>,
    values: Vec<Value>,
}

impl Row {
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    pub fn values(&self) -> &[Value] {
        &self.values
    }

    pub fn get(&self, idx: usize) -> Option<&Value> {
        self.values.get(idx)
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|col| col.name == name)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&Value> {
        self.get(self.column_index(name)?)
    }
}

/// The fully decoded result of a query.
///
/// Values are decoded once, when the result set is built, so that displaying
/// and searching them does not touch the database connection again.
#[derive(Clone, Debug)]
pub struct ResultSet {
    columns: Arc<Vec<Column>>,
    rows: Vec<Row>,
}

impl ResultSet {
    pub fn new(columns: Vec<Column>, rows: Vec<Vec<Value>>) -> Self {
        let columns = Arc::new(columns);
        let rows = rows
            .into_iter()
            .map(|values| Row {
                columns: Arc::clone(&columns),
                values,
            })
            .collect();

        ResultSet { columns, rows }
    }

    /// Decodes the rows returned by a query.
    ///
    /// `columns` comes from the prepared statement rather than from the rows,
    /// so a query returning no rows still describes what it would have
    /// returned.
    pub(crate) fn from_postgres_rows(columns: &[postgres::Column], rows: &[postgres::Row]) -> Self {
        let columns: Vec<Column> = columns
            .iter()
            .map(|col| Column {
                name: col.name().to_owned(),
                ty: col.type_().clone(),
            })
            .collect();

        let values = rows
            .iter()
            .map(|row| {
                (0..columns.len())
                    .map(|idx| {
                        row.try_get(idx)
                            .unwrap_or_else(|err| Value::Error(err.to_string()))
                    })
                    .collect()
            })
            .collect();

        ResultSet::new(columns, values)
    }

    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp(s: &str) -> jiff::Timestamp {
        s.parse().expect("invalid timestamp")
    }

    fn uuid(s: &str) -> uuid::Uuid {
        s.parse().expect("invalid uuid")
    }

    #[test]
    fn scalars_display_as_their_value() {
        assert_eq!(Value::Bool(true).to_string(), "true");
        assert_eq!(Value::Int2(-3).to_string(), "-3");
        assert_eq!(Value::Int4(42).to_string(), "42");
        assert_eq!(Value::Int8(1 << 40).to_string(), "1099511627776");
        assert_eq!(Value::Float4(1.5).to_string(), "1.5");
        assert_eq!(Value::Float8(-0.25).to_string(), "-0.25");
        assert_eq!(Value::Text("hi".to_owned()).to_string(), "hi");
        assert_eq!(
            Value::Json(serde_json::json!({"a": 1})).to_string(),
            r#"{"a":1}"#
        );
        assert_eq!(
            Value::Timestamptz(timestamp("2024-01-02T03:04:05Z")).to_string(),
            "2024-01-02T03:04:05Z"
        );
        assert_eq!(
            Value::Uuid(uuid("a3b8c1de-0000-4000-8000-000000000001")).to_string(),
            "a3b8c1de-0000-4000-8000-000000000001"
        );
    }

    #[test]
    fn arrays_display_with_debug_formatting() {
        assert_eq!(Value::Int4Array(vec![1, 2]).to_string(), "[1, 2]");
        assert_eq!(Value::BoolArray(Vec::new()).to_string(), "[]");
        assert_eq!(
            Value::TextArray(vec!["a".to_owned(), "b\"c".to_owned()]).to_string(),
            r#"["a", "b\"c"]"#
        );
    }

    #[test]
    fn null_and_undecodable_values_display_as_markers() {
        assert_eq!(Value::Null.to_string(), "<NULL>");
        assert_eq!(Value::Null.to_string(), NULL_STR);
        assert_eq!(Value::Error("boom".to_owned()).to_string(), "boom");
    }

    #[test]
    fn null_is_sent_as_a_sql_null() {
        let mut out = BytesMut::new();
        assert!(matches!(
            Value::Null.to_sql_checked(&Type::INT4, &mut out),
            Ok(IsNull::Yes)
        ));
    }

    #[test]
    fn undecodable_values_cannot_be_sent() {
        let mut out = BytesMut::new();
        let Err(err) = Value::Error("boom".to_owned()).to_sql_checked(&Type::INT4, &mut out) else {
            panic!("undecodable value was sent");
        };

        assert!(err.to_string().contains("boom"), "{err}");
    }

    #[test]
    fn values_are_checked_against_the_type_they_are_sent_as() {
        let mut out = BytesMut::new();

        assert!(Value::Int4(1).to_sql_checked(&Type::INT4, &mut out).is_ok());

        out.clear();
        assert!(
            Value::Int4(1)
                .to_sql_checked(&Type::INT8, &mut out)
                .is_err()
        );

        // TEXT and VARCHAR decode into the same variant, and both accept it back.
        out.clear();
        assert!(
            Value::Text("x".to_owned())
                .to_sql_checked(&Type::VARCHAR, &mut out)
                .is_ok()
        );
    }

    fn result_set() -> ResultSet {
        ResultSet::new(
            vec![
                Column {
                    name: "id".to_owned(),
                    ty: Type::INT4,
                },
                Column {
                    name: "name".to_owned(),
                    ty: Type::TEXT,
                },
            ],
            vec![vec![Value::Int4(1), Value::Text("a".to_owned())]],
        )
    }

    #[test]
    fn numbers_sort_as_numbers_not_as_text() {
        assert_eq!(Value::Int4(9).compare(&Value::Int4(10)), Ordering::Less);
        assert_eq!(Value::Int8(9).compare(&Value::Int2(10)), Ordering::Less);
        assert_eq!(Value::Float8(9.5).compare(&Value::Int4(10)), Ordering::Less);
        assert_eq!(Value::Int4(-2).compare(&Value::Int4(1)), Ordering::Less);
    }

    #[test]
    fn large_integers_compare_exactly() {
        // Both of these are the same number once put through an f64.
        let a = Value::Int8(i64::MAX);
        let b = Value::Int8(i64::MAX - 1);

        assert_eq!(a.compare(&b), Ordering::Greater);
    }

    #[test]
    fn everything_else_sorts_the_way_it_reads() {
        let (a, b) = (
            Value::Text("apple".to_owned()),
            Value::Text("banana".to_owned()),
        );
        assert_eq!(a.compare(&b), Ordering::Less);

        // Timestamps read as RFC 3339, which sorts chronologically.
        let (early, late) = (
            Value::Timestamptz(timestamp("2024-01-02T03:04:05Z")),
            Value::Timestamptz(timestamp("2024-11-02T03:04:05Z")),
        );
        assert_eq!(early.compare(&late), Ordering::Less);

        assert_eq!(
            Value::Bool(false).compare(&Value::Bool(true)),
            Ordering::Less
        );
        assert_eq!(Value::Int4(1).compare(&Value::Int4(1)), Ordering::Equal);
    }

    #[test]
    fn values_of_different_kinds_still_have_an_order() {
        let mixed = Value::Text("x".to_owned()).compare(&Value::Null);

        assert_ne!(mixed, Ordering::Equal);
        assert_eq!(
            Value::Text("x".to_owned()).compare(&Value::Null),
            mixed,
            "the order changed between calls"
        );
    }

    #[test]
    fn json_is_laid_out_when_read_on_its_own() {
        let value = Value::Json(serde_json::json!({"a": 1}));

        assert_eq!(value.to_string(), r#"{"a":1}"#);
        assert_eq!(value.to_pretty_string(), "{\n  \"a\": 1\n}");
    }

    #[test]
    fn other_values_read_the_same_either_way() {
        for value in [
            Value::Null,
            Value::Int4(1),
            Value::Text("x".to_owned()),
            Value::TextArray(vec!["a".to_owned()]),
        ] {
            assert_eq!(value.to_pretty_string(), value.to_string());
        }
    }

    #[test]
    fn rows_expose_their_values_by_index_and_by_name() {
        let set = result_set();
        let row = &set.rows()[0];

        assert_eq!(set.len(), 1);
        assert!(!set.is_empty());
        assert_eq!(row.columns().len(), 2);
        assert_eq!(row.get(0), Some(&Value::Int4(1)));
        assert_eq!(row.get(2), None);
        assert_eq!(row.column_index("name"), Some(1));
        assert_eq!(row.get_by_name("name"), Some(&Value::Text("a".to_owned())));
        assert_eq!(row.get_by_name("missing"), None);
    }

    #[test]
    fn a_result_set_without_rows_still_has_columns() {
        let set = ResultSet::new(
            vec![Column {
                name: "id".to_owned(),
                ty: Type::INT4,
            }],
            Vec::new(),
        );

        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(set.columns().len(), 1);
    }

    #[test]
    fn rows_outlive_the_result_set_they_came_from() {
        let row = result_set().rows()[0].clone();

        assert_eq!(row.get_by_name("id"), Some(&Value::Int4(1)));
    }
}
