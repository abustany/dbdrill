use anyhow::anyhow;

pub struct SQLValueAsString(String);

impl SQLValueAsString {
    pub fn new(value: String) -> Self {
        SQLValueAsString(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn take_string(self) -> String {
        self.0
    }
}

impl<T: std::fmt::Display> From<T> for SQLValueAsString {
    fn from(value: T) -> Self {
        SQLValueAsString(value.to_string())
    }
}

impl postgres::types::FromSql<'_> for SQLValueAsString {
    fn from_sql(
        ty: &postgres::types::Type,
        raw: &'_ [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        if ty == &postgres::types::Type::BOOL {
            return Ok(SQLValueAsString::from(bool::from_sql(ty, raw)?));
        }

        if ty == &postgres::types::Type::INT2 {
            return Ok(SQLValueAsString::from(i16::from_sql(ty, raw)?));
        }

        if ty == &postgres::types::Type::INT4 {
            return Ok(SQLValueAsString::from(i32::from_sql(ty, raw)?));
        }

        if ty == &postgres::types::Type::INT8 {
            return Ok(SQLValueAsString::from(i64::from_sql(ty, raw)?));
        }

        if ty == &postgres::types::Type::JSONB {
            return Ok(SQLValueAsString::from(serde_json::Value::from_sql(
                ty, raw,
            )?));
        }

        if ty == &postgres::types::Type::TEXT {
            return Ok(SQLValueAsString::from(String::from_sql(ty, raw)?));
        }

        if ty == &postgres::types::Type::TEXT_ARRAY {
            return Ok(SQLValueAsString(format!(
                "{:?}",
                Vec::<String>::from_sql(ty, raw)?
            )));
        }

        if ty == &postgres::types::Type::TIMESTAMPTZ {
            return Ok(SQLValueAsString::from(jiff::Timestamp::from_sql(ty, raw)?));
        }

        Err(anyhow!("unsupported type: {}", ty).into_boxed_dyn_error())
    }

    fn from_sql_nullable(
        ty: &postgres::types::Type,
        raw: Option<&'_ [u8]>,
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        match raw {
            Some(val) => Self::from_sql(ty, val),
            None => Ok(SQLValueAsString(String::from("<NULL>"))),
        }
    }

    fn accepts(ty: &postgres::types::Type) -> bool {
        ty == &postgres::types::Type::BOOL
            || ty == &postgres::types::Type::INT2
            || ty == &postgres::types::Type::INT4
            || ty == &postgres::types::Type::INT8
            || ty == &postgres::types::Type::JSONB
            || ty == &postgres::types::Type::TEXT
            || ty == &postgres::types::Type::TEXT_ARRAY
            || ty == &postgres::types::Type::TIMESTAMPTZ
    }
}
