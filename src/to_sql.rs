use anyhow::{Context, Result};

use crate::{json_helpers::extract_single_value, model::SearchParamType};

macro_rules! parse_single_value {
    ($str_val:expr, $type:ty, $type_name:expr) => {{
        let val: $type = $str_val
            .parse()
            .with_context(|| format!("error parsing value as {}: {}", $type_name, $str_val))?;
        Ok(Box::new(val))
    }};
}

macro_rules! parse_array {
    ($str_val:expr, $type:ty, $type_name:expr) => {{
        let array_val: Vec<$type> = $str_val
            .split(',')
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()
            .with_context(|| format!("error parsing value as {}[]: {}", $type_name, $str_val))?;
        Ok(Box::new(array_val))
    }};
}

pub fn sql_value_from_string(
    str_val: &str,
    ty: SearchParamType,
) -> Result<Box<dyn postgres::types::ToSql + Sync>> {
    match ty {
        SearchParamType::Bool => parse_single_value!(str_val, bool, "bool"),
        SearchParamType::BoolArray => parse_array!(str_val, bool, "bool"),
        SearchParamType::Float4 => parse_single_value!(str_val, f32, "float4"),
        SearchParamType::Float4Array => parse_array!(str_val, f32, "float4"),
        SearchParamType::Float8 => parse_single_value!(str_val, f64, "float8"),
        SearchParamType::Float8Array => parse_array!(str_val, f64, "float8"),
        SearchParamType::Int2 => parse_single_value!(str_val, i16, "int2"),
        SearchParamType::Int4 => parse_single_value!(str_val, i32, "int4"),
        SearchParamType::Int2Array => parse_array!(str_val, i16, "int2"),
        SearchParamType::Int4Array => parse_array!(str_val, i32, "int4"),
        SearchParamType::Int8 => parse_single_value!(str_val, i64, "int8"),
        SearchParamType::Int8Array => parse_array!(str_val, i64, "int8"),
        SearchParamType::Json | SearchParamType::Jsonb => {
            let json_val: serde_json::Value = serde_json::from_str(str_val)
                .with_context(|| format!("error parsing value as json: {str_val}"))?;
            Ok(Box::new(json_val))
        }
        SearchParamType::JsonArray | SearchParamType::JsonbArray => {
            let array_val: Vec<serde_json::Value> = str_val
                .split(',')
                .map(serde_json::from_str)
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as json[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Text | SearchParamType::Varchar => Ok(Box::new(str_val.to_owned())),
        SearchParamType::TextArray | SearchParamType::VarcharArray => {
            let array_val: Vec<String> = str_val.split(',').map(|s| s.to_string()).collect();
            Ok(Box::new(array_val))
        }
        SearchParamType::Timestamptz => {
            parse_single_value!(str_val, jiff::Timestamp, "timestamptz")
        }
        SearchParamType::TimestamptzArray => parse_array!(str_val, jiff::Timestamp, "timestamptz"),
        SearchParamType::Uuid => parse_single_value!(str_val, uuid::Uuid, "uuid"),
        SearchParamType::UuidArray => parse_array!(str_val, uuid::Uuid, "uuid"),
    }
}

macro_rules! extract_json_value {
    ($val:expr, $method:ident, $type:ty, $type_name:expr) => {{
        Ok(Box::new(
            extract_single_value($val)?
                .$method()
                .with_context(|| format!("value is not a {}: {:?}", $type_name, $val[0]))?
                as $type,
        ))
    }};
}

macro_rules! extract_json_array {
    ($val:expr, $method:ident, $type:ty, $type_name:expr) => {{
        Ok(Box::new(
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
    ($val:expr, $type:ty, $type_name:expr) => {{
        Ok(Box::new(
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
    ($val:expr, $type:ty, $type_name:expr) => {{
        Ok(Box::new(
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
    ($val:expr, $type:ty, $type_name:expr) => {{
        Ok(Box::new(
            extract_single_value($val)?
                .as_str()
                .with_context(|| format!("value is not a string: {:?}", $val[0]))?
                .parse::<$type>()
                .with_context(|| format!("value is not a valid {}: {:?}", $type_name, $val[0]))?,
        ))
    }};
}

macro_rules! extract_json_parse_array {
    ($val:expr, $type:ty, $type_name:expr) => {{
        Ok(Box::new(
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

pub fn sql_value_from_json_slice(
    val: &[&serde_json::Value],
    ty: SearchParamType,
) -> Result<Box<dyn postgres::types::ToSql + Sync>> {
    match ty {
        SearchParamType::Bool => extract_json_value!(val, as_bool, bool, "boolean"),
        SearchParamType::BoolArray => extract_json_array!(val, as_bool, bool, "boolean"),
        SearchParamType::Float4 => extract_json_value!(val, as_f64, f32, "number"),
        SearchParamType::Float4Array => extract_json_array!(val, as_f64, f32, "number"),
        SearchParamType::Float8 => extract_json_value!(val, as_f64, f64, "number"),
        SearchParamType::Float8Array => extract_json_array!(val, as_f64, f64, "number"),
        SearchParamType::Int2 => extract_json_int!(val, i16, "int2"),
        SearchParamType::Int2Array => extract_json_int_array!(val, i16, "int2"),
        SearchParamType::Int4 => extract_json_int!(val, i32, "int4"),
        SearchParamType::Int4Array => extract_json_int_array!(val, i32, "int4"),
        SearchParamType::Int8 => extract_json_value!(val, as_i64, i64, "number"),
        SearchParamType::Int8Array => extract_json_array!(val, as_i64, i64, "number"),
        SearchParamType::Json | SearchParamType::Jsonb => {
            Ok(Box::new(extract_single_value(val)?.clone()))
        }
        SearchParamType::JsonArray | SearchParamType::JsonbArray => Ok(Box::new(
            val.iter()
                .map(|&v| v.clone())
                .collect::<Vec<serde_json::Value>>(),
        )),
        SearchParamType::Text | SearchParamType::Varchar => Ok(Box::new(
            extract_single_value(val)?
                .as_str()
                .with_context(|| format!("value is not a string: {:?}", val[0]))?
                .to_owned(),
        )),
        SearchParamType::TextArray | SearchParamType::VarcharArray => Ok(Box::new(
            val.iter()
                .map(|val| {
                    val.as_str()
                        .with_context(|| format!("array element is not a string: {val:?}",))
                        .map(|x| x.to_owned())
                })
                .collect::<Result<Vec<String>>>()?,
        )),
        SearchParamType::Timestamptz => extract_json_parse!(val, jiff::Timestamp, "timestamp"),
        SearchParamType::TimestamptzArray => {
            extract_json_parse_array!(val, jiff::Timestamp, "timestamp")
        }
        SearchParamType::Uuid => extract_json_parse!(val, uuid::Uuid, "uuid"),
        SearchParamType::UuidArray => extract_json_parse_array!(val, uuid::Uuid, "uuid"),
    }
}
