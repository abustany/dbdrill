use anyhow::{Context, Result};

use crate::{json_helpers::extract_single_value, model::SearchParamType, value::Value};

macro_rules! parse_single_value {
    ($str_val:expr, $variant:ident, $type:ty, $type_name:expr) => {{
        let val: $type = $str_val
            .parse()
            .with_context(|| format!("error parsing value as {}: {}", $type_name, $str_val))?;
        Ok(Value::$variant(val))
    }};
}

macro_rules! parse_array {
    ($str_val:expr, $variant:ident, $type:ty, $type_name:expr) => {{
        let array_val: Vec<$type> = $str_val
            .split(',')
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()
            .with_context(|| format!("error parsing value as {}[]: {}", $type_name, $str_val))?;
        Ok(Value::$variant(array_val))
    }};
}

pub fn sql_value_from_string(str_val: &str, ty: SearchParamType) -> Result<Value> {
    match ty {
        SearchParamType::Bool => parse_single_value!(str_val, Bool, bool, "bool"),
        SearchParamType::BoolArray => parse_array!(str_val, BoolArray, bool, "bool"),
        SearchParamType::Float4 => parse_single_value!(str_val, Float4, f32, "float4"),
        SearchParamType::Float4Array => parse_array!(str_val, Float4Array, f32, "float4"),
        SearchParamType::Float8 => parse_single_value!(str_val, Float8, f64, "float8"),
        SearchParamType::Float8Array => parse_array!(str_val, Float8Array, f64, "float8"),
        SearchParamType::Int2 => parse_single_value!(str_val, Int2, i16, "int2"),
        SearchParamType::Int4 => parse_single_value!(str_val, Int4, i32, "int4"),
        SearchParamType::Int2Array => parse_array!(str_val, Int2Array, i16, "int2"),
        SearchParamType::Int4Array => parse_array!(str_val, Int4Array, i32, "int4"),
        SearchParamType::Int8 => parse_single_value!(str_val, Int8, i64, "int8"),
        SearchParamType::Int8Array => parse_array!(str_val, Int8Array, i64, "int8"),
        SearchParamType::Json | SearchParamType::Jsonb => {
            let json_val: serde_json::Value = serde_json::from_str(str_val)
                .with_context(|| format!("error parsing value as json: {str_val}"))?;
            Ok(Value::Json(json_val))
        }
        SearchParamType::JsonArray | SearchParamType::JsonbArray => {
            let array_val: Vec<serde_json::Value> = str_val
                .split(',')
                .map(serde_json::from_str)
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as json[]: {str_val}"))?;
            Ok(Value::JsonArray(array_val))
        }
        SearchParamType::Text | SearchParamType::Varchar => Ok(Value::Text(str_val.to_owned())),
        SearchParamType::TextArray | SearchParamType::VarcharArray => {
            let array_val: Vec<String> = str_val.split(',').map(|s| s.to_string()).collect();
            Ok(Value::TextArray(array_val))
        }
        SearchParamType::Timestamptz => {
            parse_single_value!(str_val, Timestamptz, jiff::Timestamp, "timestamptz")
        }
        SearchParamType::TimestamptzArray => {
            parse_array!(str_val, TimestamptzArray, jiff::Timestamp, "timestamptz")
        }
        SearchParamType::Uuid => parse_single_value!(str_val, Uuid, uuid::Uuid, "uuid"),
        SearchParamType::UuidArray => parse_array!(str_val, UuidArray, uuid::Uuid, "uuid"),
    }
}

macro_rules! extract_json_value {
    ($val:expr, $variant:ident, $method:ident, $type:ty, $type_name:expr) => {{
        Ok(Value::$variant(
            extract_single_value($val)?
                .$method()
                .with_context(|| format!("value is not a {}: {:?}", $type_name, $val[0]))?
                as $type,
        ))
    }};
}

macro_rules! extract_json_array {
    ($val:expr, $variant:ident, $method:ident, $type:ty, $type_name:expr) => {{
        Ok(Value::$variant(
            $val.iter()
                .map(|val| {
                    Ok(val.$method().with_context(|| {
                        format!("array element is not a {}: {:?}", $type_name, val)
                    })? as $type)
                })
                .collect::<Result<Vec<$type>>>()?,
        ))
    }};
}

macro_rules! extract_json_int {
    ($val:expr, $variant:ident, $type:ty, $type_name:expr) => {{
        Ok(Value::$variant(
            TryInto::<$type>::try_into(
                extract_single_value($val)?
                    .as_i64()
                    .with_context(|| format!("value is not a number: {:?}", $val[0]))?,
            )
            .with_context(|| format!("value overflows {}: {:?}", $type_name, $val[0]))?,
        ))
    }};
}

macro_rules! extract_json_int_array {
    ($val:expr, $variant:ident, $type:ty, $type_name:expr) => {{
        Ok(Value::$variant(
            $val.iter()
                .map(|val| {
                    TryInto::<$type>::try_into(
                        val.as_i64()
                            .with_context(|| format!("array element is not a number: {val:?}"))?,
                    )
                    .with_context(|| format!("array element overflows {}: {val:?}", $type_name))
                })
                .collect::<Result<Vec<$type>>>()?,
        ))
    }};
}

macro_rules! extract_json_parse {
    ($val:expr, $variant:ident, $type:ty, $type_name:expr) => {{
        Ok(Value::$variant(
            extract_single_value($val)?
                .as_str()
                .with_context(|| format!("value is not a string: {:?}", $val[0]))?
                .parse::<$type>()
                .with_context(|| format!("value is not a valid {}: {:?}", $type_name, $val[0]))?,
        ))
    }};
}

macro_rules! extract_json_parse_array {
    ($val:expr, $variant:ident, $type:ty, $type_name:expr) => {{
        Ok(Value::$variant(
            $val.iter()
                .map(|val| {
                    val.as_str()
                        .with_context(|| format!("array element is not a string: {val:?}"))?
                        .parse::<$type>()
                        .with_context(|| {
                            format!("array element is not a valid {}: {val:?}", $type_name)
                        })
                })
                .collect::<Result<Vec<$type>>>()?,
        ))
    }};
}

pub fn sql_value_from_json_slice(val: &[&serde_json::Value], ty: SearchParamType) -> Result<Value> {
    match ty {
        SearchParamType::Bool => extract_json_value!(val, Bool, as_bool, bool, "boolean"),
        SearchParamType::BoolArray => {
            extract_json_array!(val, BoolArray, as_bool, bool, "boolean")
        }
        SearchParamType::Float4 => extract_json_value!(val, Float4, as_f64, f32, "number"),
        SearchParamType::Float4Array => {
            extract_json_array!(val, Float4Array, as_f64, f32, "number")
        }
        SearchParamType::Float8 => extract_json_value!(val, Float8, as_f64, f64, "number"),
        SearchParamType::Float8Array => {
            extract_json_array!(val, Float8Array, as_f64, f64, "number")
        }
        SearchParamType::Int2 => extract_json_int!(val, Int2, i16, "int2"),
        SearchParamType::Int2Array => extract_json_int_array!(val, Int2Array, i16, "int2"),
        SearchParamType::Int4 => extract_json_int!(val, Int4, i32, "int4"),
        SearchParamType::Int4Array => extract_json_int_array!(val, Int4Array, i32, "int4"),
        SearchParamType::Int8 => extract_json_value!(val, Int8, as_i64, i64, "number"),
        SearchParamType::Int8Array => extract_json_array!(val, Int8Array, as_i64, i64, "number"),
        SearchParamType::Json | SearchParamType::Jsonb => {
            Ok(Value::Json(extract_single_value(val)?.clone()))
        }
        SearchParamType::JsonArray | SearchParamType::JsonbArray => Ok(Value::JsonArray(
            val.iter().map(|&v| v.clone()).collect::<Vec<_>>(),
        )),
        SearchParamType::Text | SearchParamType::Varchar => Ok(Value::Text(
            extract_single_value(val)?
                .as_str()
                .with_context(|| format!("value is not a string: {:?}", val[0]))?
                .to_owned(),
        )),
        SearchParamType::TextArray | SearchParamType::VarcharArray => Ok(Value::TextArray(
            val.iter()
                .map(|val| {
                    val.as_str()
                        .with_context(|| format!("array element is not a string: {val:?}",))
                        .map(|x| x.to_owned())
                })
                .collect::<Result<Vec<String>>>()?,
        )),
        SearchParamType::Timestamptz => {
            extract_json_parse!(val, Timestamptz, jiff::Timestamp, "timestamp")
        }
        SearchParamType::TimestamptzArray => {
            extract_json_parse_array!(val, TimestamptzArray, jiff::Timestamp, "timestamp")
        }
        SearchParamType::Uuid => extract_json_parse!(val, Uuid, uuid::Uuid, "uuid"),
        SearchParamType::UuidArray => {
            extract_json_parse_array!(val, UuidArray, uuid::Uuid, "uuid")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_parse_into_the_declared_type() {
        assert_eq!(
            sql_value_from_string("42", SearchParamType::Int4).unwrap(),
            Value::Int4(42)
        );
        assert_eq!(
            sql_value_from_string("1,2", SearchParamType::Int4Array).unwrap(),
            Value::Int4Array(vec![1, 2])
        );
        assert_eq!(
            sql_value_from_string("a,b", SearchParamType::TextArray).unwrap(),
            Value::TextArray(vec!["a".to_owned(), "b".to_owned()])
        );
        assert_eq!(
            sql_value_from_string(r#"{"a":1}"#, SearchParamType::Jsonb).unwrap(),
            Value::Json(serde_json::json!({"a": 1}))
        );
    }

    #[test]
    fn varchar_and_text_share_a_representation() {
        assert_eq!(
            sql_value_from_string("x", SearchParamType::Varchar).unwrap(),
            sql_value_from_string("x", SearchParamType::Text).unwrap()
        );
    }

    #[test]
    fn parse_errors_name_the_expected_type() {
        let err = sql_value_from_string("nope", SearchParamType::Int4)
            .expect_err("parsed a number out of text");

        assert!(err.to_string().contains("int4"), "{err}");
    }

    #[test]
    fn json_values_are_extracted_into_the_declared_type() {
        let one = serde_json::json!(1);
        assert_eq!(
            sql_value_from_json_slice(&[&one], SearchParamType::Int4).unwrap(),
            Value::Int4(1)
        );

        let (a, b) = (serde_json::json!("a"), serde_json::json!("b"));
        assert_eq!(
            sql_value_from_json_slice(&[&a, &b], SearchParamType::TextArray).unwrap(),
            Value::TextArray(vec!["a".to_owned(), "b".to_owned()])
        );
    }

    #[test]
    fn scalar_extraction_rejects_several_json_values() {
        let (a, b) = (serde_json::json!(1), serde_json::json!(2));
        let err = sql_value_from_json_slice(&[&a, &b], SearchParamType::Int4)
            .expect_err("extracted a scalar out of two values");

        assert!(err.to_string().contains("expected 1 result"), "{err}");
    }

    #[test]
    fn integer_extraction_reports_overflow() {
        let big = serde_json::json!(i64::from(i32::MAX) + 1);
        let err = sql_value_from_json_slice(&[&big], SearchParamType::Int4)
            .expect_err("extracted an out of range int4");

        assert!(err.to_string().contains("overflows int4"), "{err}");
    }
}
