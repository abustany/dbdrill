use anyhow::{Context, Result};

use crate::{json_helpers::extract_single_value, model::SearchParamType};

pub fn sql_value_from_string(
    str_val: &str,
    ty: SearchParamType,
) -> Result<Box<dyn postgres::types::ToSql + Sync>> {
    match ty {
        SearchParamType::Bool => {
            let bool_val: bool = str_val
                .parse()
                .with_context(|| format!("error parsing value as bool: {str_val}"))?;
            Ok(Box::new(bool_val))
        }
        SearchParamType::BoolArray => {
            let array_val: Vec<bool> = str_val
                .split(',')
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as bool[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Float4 => {
            let float_val: f32 = str_val
                .parse()
                .with_context(|| format!("error parsing value as float4: {str_val}"))?;
            Ok(Box::new(float_val))
        }
        SearchParamType::Float4Array => {
            let array_val: Vec<f32> = str_val
                .split(',')
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as float4[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Float8 => {
            let float_val: f64 = str_val
                .parse()
                .with_context(|| format!("error parsing value as float8: {str_val}"))?;
            Ok(Box::new(float_val))
        }
        SearchParamType::Float8Array => {
            let array_val: Vec<f64> = str_val
                .split(',')
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as float8[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Int2 => {
            let int_val: i16 = str_val
                .parse()
                .with_context(|| format!("error parsing value as int2: {str_val}"))?;
            Ok(Box::new(int_val))
        }
        SearchParamType::Int4 => {
            let integer_val: i32 = str_val
                .parse()
                .with_context(|| format!("error parsing value as int4: {str_val}",))?;
            Ok(Box::new(integer_val))
        }
        SearchParamType::Int2Array => {
            let array_val: Vec<i16> = str_val
                .split(',')
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as int2[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Int4Array => {
            let array_val: Vec<i32> = str_val
                .split(',')
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as int4[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Int8 => {
            let int_val: i64 = str_val
                .parse()
                .with_context(|| format!("error parsing value as int8: {str_val}"))?;
            Ok(Box::new(int_val))
        }
        SearchParamType::Int8Array => {
            let array_val: Vec<i64> = str_val
                .split(',')
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as int8[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Json | SearchParamType::Jsonb => {
            let json_val: serde_json::Value = serde_json::from_str(str_val)
                .with_context(|| format!("error parsing value as json: {str_val}"))?;
            Ok(Box::new(json_val))
        }
        SearchParamType::JsonbArray => {
            let array_val: Vec<serde_json::Value> = str_val
                .split(',')
                .map(serde_json::from_str)
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as json[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Text => Ok(Box::new(str_val.to_owned())),
        SearchParamType::TextArray => {
            let array_val: Vec<String> = str_val.split(',').map(|s| s.to_string()).collect();
            Ok(Box::new(array_val))
        }
        SearchParamType::Timestamptz => {
            let ts: jiff::Timestamp = str_val
                .parse()
                .with_context(|| format!("error parsing value as timestamptz: {str_val}"))?;
            Ok(Box::new(ts))
        }
        SearchParamType::TimestamptzArray => {
            let array_val: Vec<jiff::Timestamp> = str_val
                .split(',')
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as timestamptz[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Uuid => {
            let ts: uuid::Uuid = str_val
                .parse()
                .with_context(|| format!("error parsing value as uuid: {str_val}"))?;
            Ok(Box::new(ts))
        }
        SearchParamType::UuidArray => {
            let array_val: Vec<uuid::Uuid> = str_val
                .split(',')
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("error parsing value as uuid[]: {str_val}"))?;
            Ok(Box::new(array_val))
        }
        SearchParamType::Varchar => Ok(Box::new(str_val.to_owned())),
        SearchParamType::VarcharArray => {
            let array_val: Vec<String> = str_val.split(',').map(|s| s.to_string()).collect();
            Ok(Box::new(array_val))
        }
    }
}

pub fn sql_value_from_json_slice(
    val: &[&serde_json::Value],
    ty: SearchParamType,
) -> Result<Box<dyn postgres::types::ToSql + Sync>> {
    match ty {
        SearchParamType::Bool => Ok(Box::new(
            extract_single_value(val)?
                .as_bool()
                .with_context(|| format!("value is not a boolean: {:?}", val[0]))?,
        )),
        SearchParamType::BoolArray => Ok(Box::new(
            val.iter()
                .map(|val| {
                    val.as_bool()
                        .with_context(|| format!("array element is not a boolean: {val:?}"))
                })
                .collect::<Result<Vec<bool>>>()?,
        )),
        SearchParamType::Float4 => Ok(Box::new(
            extract_single_value(val)?
                .as_f64()
                .with_context(|| format!("value is not a number: {:?}", val[0]))?
                as f32,
        )),
        SearchParamType::Float4Array => Ok(Box::new(
            val.iter()
                .map(|val| {
                    Ok(val
                        .as_f64()
                        .with_context(|| format!("array element is not a number: {val:?}"))?
                        as f32)
                })
                .collect::<Result<Vec<f32>>>()?,
        )),
        SearchParamType::Float8 => {
            Ok(Box::new(extract_single_value(val)?.as_f64().with_context(
                || format!("value is not a number: {:?}", val[0]),
            )?))
        }
        SearchParamType::Float8Array => Ok(Box::new(
            val.iter()
                .map(|val| {
                    val.as_f64()
                        .with_context(|| format!("array element is not a number: {val:?}"))
                })
                .collect::<Result<Vec<f64>>>()?,
        )),
        SearchParamType::Int2 => Ok(Box::new(
            TryInto::<i16>::try_into(
                extract_single_value(val)?
                    .as_i64()
                    .with_context(|| format!("value is not a number: {:?}", val[0]))?,
            )
            .with_context(|| format!("value overflows target type: {:?}", val[0]))?,
        )),
        SearchParamType::Int2Array => todo!(),
        SearchParamType::Int4 => Ok(Box::new(
            TryInto::<i32>::try_into(
                extract_single_value(val)?
                    .as_i64()
                    .with_context(|| format!("value is not a number: {:?}", val[0]))?,
            )
            .with_context(|| format!("value overflows target type: {:?}", val[0]))?,
        )),
        SearchParamType::Int4Array => Ok(Box::new(
            val.iter()
                .map(|val| {
                    TryInto::<i32>::try_into(
                        val.as_i64()
                            .with_context(|| format!("array element is not a number: {val:?}"))?,
                    )
                    .with_context(|| format!("array element overflows target type: {val:?}"))
                })
                .collect::<Result<Vec<i32>>>()?,
        )),
        SearchParamType::Int8 => {
            Ok(Box::new(extract_single_value(val)?.as_i64().with_context(
                || format!("value is not a number: {:?}", val[0]),
            )?))
        }
        SearchParamType::Int8Array => Ok(Box::new(
            val.iter()
                .map(|val| {
                    val.as_i64()
                        .with_context(|| format!("array element is not a number: {val:?}"))
                })
                .collect::<Result<Vec<i64>>>()?,
        )),
        SearchParamType::Json | SearchParamType::Jsonb => {
            Ok(Box::new(extract_single_value(val)?.clone()))
        }
        SearchParamType::JsonbArray => Ok(Box::new(
            val.iter()
                .map(|&v| v.clone())
                .collect::<Vec<serde_json::Value>>(),
        )),
        SearchParamType::Text => Ok(Box::new(
            extract_single_value(val)?
                .as_str()
                .with_context(|| format!("value is not a string: {:?}", val[0]))?
                .to_owned(),
        )),
        SearchParamType::TextArray => Ok(Box::new(
            val.iter()
                .map(|val| {
                    val.as_str()
                        .with_context(|| format!("array element is not a string: {val:?}",))
                        .map(|x| x.to_owned())
                })
                .collect::<Result<Vec<String>>>()?,
        )),
        SearchParamType::Timestamptz => Ok(Box::new(
            extract_single_value(val)?
                .as_str()
                .with_context(|| format!("value is not a string: {:?}", val[0]))?
                .parse::<jiff::Timestamp>()
                .with_context(|| format!("value is not a valid timestamp: {:?}", val[0]))?,
        )),
        SearchParamType::TimestamptzArray => Ok(Box::new(
            val.iter()
                .map(|val| {
                    val.as_str()
                        .with_context(|| format!("array element is not a string: {val:?}"))?
                        .parse::<jiff::Timestamp>()
                        .with_context(|| format!("array element is not a valid timestamp: {val:?}"))
                })
                .collect::<Result<Vec<jiff::Timestamp>>>()?,
        )),
        SearchParamType::Uuid => Ok(Box::new(
            extract_single_value(val)?
                .as_str()
                .with_context(|| format!("value is not a string: {:?}", val[0]))?
                .parse::<uuid::Uuid>()
                .with_context(|| format!("value is not a valid uuid: {:?}", val[0]))?,
        )),
        SearchParamType::UuidArray => Ok(Box::new(
            val.iter()
                .map(|val| {
                    val.as_str()
                        .with_context(|| format!("array element is not a string: {val:?}"))?
                        .parse::<uuid::Uuid>()
                        .with_context(|| format!("array element is not a valid uuid: {val:?}"))
                })
                .collect::<Result<Vec<uuid::Uuid>>>()?,
        )),
        SearchParamType::Varchar => Ok(Box::new(
            extract_single_value(val)?
                .as_str()
                .with_context(|| format!("value is not a string: {:?}", val[0]))?
                .to_owned(),
        )),
        SearchParamType::VarcharArray => Ok(Box::new(
            val.iter()
                .map(|val| {
                    val.as_str()
                        .with_context(|| format!("array element is not a string: {val:?}"))
                        .map(|x| x.to_owned())
                })
                .collect::<Result<Vec<String>>>()?,
        )),
    }
}
