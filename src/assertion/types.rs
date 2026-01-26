use std::fmt;

/// 断言错误类型
#[derive(Debug, thiserror::Error)]
pub enum AssertError {
    #[error("Invalid assertion syntax: {0}")]
    InvalidSyntax(String),

    #[error("Invalid operator: {0}")]
    InvalidOperator(String),

    #[error("Invalid value: {0}")]
    InvalidValue(String),

    #[error("Path not found: {0}")]
    PathNotFound(String),

    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Value extraction failed: {0}")]
    ExtractionError(String),
}

/// 断言表达式
#[derive(Debug, Clone, PartialEq)]
pub enum AssertExpr {
    /// 比较断言: left op right
    Compare {
        left: ValuePath,
        op: CompareOp,
        right: AssertValue,
    },
    /// 存在性断言: path exists
    Exists { path: ValuePath },
}

/// 值路径 - 用于从响应中提取值
#[derive(Debug, Clone, PartialEq)]
pub enum ValuePath {
    /// HTTP 状态码
    Status,
    /// 响应 Header
    Header(String),
    /// JSON Body 路径（点号分隔的路径段）
    Body(Vec<String>),
    /// 响应时间（毫秒）
    ResponseTime,
}

impl fmt::Display for ValuePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValuePath::Status => write!(f, "status"),
            ValuePath::Header(name) => write!(f, "headers.{}", name),
            ValuePath::Body(segments) => write!(f, "body.{}", segments.join(".")),
            ValuePath::ResponseTime => write!(f, "response.time"),
        }
    }
}

/// 比较运算符
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Equal,          // ==
    NotEqual,       // !=
    Greater,        // >
    Less,           // <
    GreaterOrEqual, // >=
    LessOrEqual,    // <=
    Contains,       // contains
}

impl CompareOp {
    /// 从字符串解析运算符
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "==" => Some(Self::Equal),
            "!=" => Some(Self::NotEqual),
            ">" => Some(Self::Greater),
            "<" => Some(Self::Less),
            ">=" => Some(Self::GreaterOrEqual),
            "<=" => Some(Self::LessOrEqual),
            "contains" => Some(Self::Contains),
            _ => None,
        }
    }

    /// 转换为字符串表示
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Greater => ">",
            Self::Less => "<",
            Self::GreaterOrEqual => ">=",
            Self::LessOrEqual => "<=",
            Self::Contains => "contains",
        }
    }
}

impl fmt::Display for CompareOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 断言值
#[derive(Debug, Clone, PartialEq)]
pub enum AssertValue {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
}

impl fmt::Display for AssertValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssertValue::Number(n) => write!(f, "{}", n),
            AssertValue::String(s) => write!(f, "\"{}\"", s),
            AssertValue::Bool(b) => write!(f, "{}", b),
            AssertValue::Null => write!(f, "null"),
        }
    }
}

impl AssertValue {
    /// 比较两个值
    pub fn compare(&self, op: CompareOp, other: &AssertValue) -> Result<bool, AssertError> {
        match (self, other) {
            // 数字比较
            (AssertValue::Number(a), AssertValue::Number(b)) => Ok(match op {
                CompareOp::Equal => (a - b).abs() < f64::EPSILON,
                CompareOp::NotEqual => (a - b).abs() >= f64::EPSILON,
                CompareOp::Greater => a > b,
                CompareOp::Less => a < b,
                CompareOp::GreaterOrEqual => a >= b,
                CompareOp::LessOrEqual => a <= b,
                CompareOp::Contains => {
                    return Err(AssertError::TypeMismatch {
                        expected: "string".to_string(),
                        actual: "number".to_string(),
                    });
                }
            }),

            // 字符串比较
            (AssertValue::String(a), AssertValue::String(b)) => Ok(match op {
                CompareOp::Equal => a == b,
                CompareOp::NotEqual => a != b,
                CompareOp::Contains => a.contains(b),
                _ => {
                    return Err(AssertError::TypeMismatch {
                        expected: "number".to_string(),
                        actual: "string".to_string(),
                    });
                }
            }),

            // 布尔比较
            (AssertValue::Bool(a), AssertValue::Bool(b)) => Ok(match op {
                CompareOp::Equal => a == b,
                CompareOp::NotEqual => a != b,
                _ => {
                    return Err(AssertError::InvalidOperator(format!(
                        "Operator {} not supported for boolean values",
                        op
                    )));
                }
            }),

            // Null 比较
            (AssertValue::Null, AssertValue::Null) => Ok(match op {
                CompareOp::Equal => true,
                CompareOp::NotEqual => false,
                _ => {
                    return Err(AssertError::InvalidOperator(format!(
                        "Operator {} not supported for null",
                        op
                    )));
                }
            }),

            // Null 与其他类型
            (AssertValue::Null, _) | (_, AssertValue::Null) => Ok(match op {
                CompareOp::Equal => false,
                CompareOp::NotEqual => true,
                _ => {
                    return Err(AssertError::InvalidOperator(format!(
                        "Operator {} not supported for null comparison",
                        op
                    )));
                }
            }),

            // 类型不匹配
            _ => Err(AssertError::TypeMismatch {
                expected: format!("{:?}", other),
                actual: format!("{:?}", self),
            }),
        }
    }
}

/// 断言结果
#[derive(Debug, Clone)]
pub struct AssertionResult {
    /// 原始断言字符串
    pub raw: String,

    /// 是否通过
    pub passed: bool,

    /// 实际值（字符串表示）
    pub actual: Option<String>,

    /// 期望描述
    pub expected: String,

    /// 失败消息
    pub message: Option<String>,
}

impl AssertionResult {
    /// 创建成功的断言结果
    pub fn success(raw: String, actual: String, expected: String) -> Self {
        Self {
            raw,
            passed: true,
            actual: Some(actual),
            expected,
            message: None,
        }
    }

    /// 创建失败的断言结果
    pub fn failure(raw: String, actual: String, expected: String, message: String) -> Self {
        Self {
            raw,
            passed: false,
            actual: Some(actual),
            expected,
            message: Some(message),
        }
    }

    /// 创建错误的断言结果（解析或执行错误）
    pub fn error(raw: String, error: AssertError) -> Self {
        Self {
            raw,
            passed: false,
            actual: None,
            expected: String::new(),
            message: Some(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_op_from_str() {
        assert_eq!(CompareOp::parse("=="), Some(CompareOp::Equal));
        assert_eq!(CompareOp::parse("!="), Some(CompareOp::NotEqual));
        assert_eq!(CompareOp::parse(">"), Some(CompareOp::Greater));
        assert_eq!(CompareOp::parse("<"), Some(CompareOp::Less));
        assert_eq!(CompareOp::parse(">="), Some(CompareOp::GreaterOrEqual));
        assert_eq!(CompareOp::parse("<="), Some(CompareOp::LessOrEqual));
        assert_eq!(CompareOp::parse("contains"), Some(CompareOp::Contains));
        assert_eq!(CompareOp::parse("invalid"), None);
    }

    #[test]
    fn test_assert_value_compare_numbers() {
        let a = AssertValue::Number(10.0);
        let b = AssertValue::Number(5.0);

        assert!(
            a.compare(CompareOp::Equal, &AssertValue::Number(10.0))
                .unwrap()
        );
        assert!(a.compare(CompareOp::Greater, &b).unwrap());
        assert!(b.compare(CompareOp::Less, &a).unwrap());
        assert!(a.compare(CompareOp::GreaterOrEqual, &b).unwrap());
        assert!(b.compare(CompareOp::LessOrEqual, &a).unwrap());
        assert!(!a.compare(CompareOp::Equal, &b).unwrap());
    }

    #[test]
    fn test_assert_value_compare_strings() {
        let a = AssertValue::String("hello world".to_string());
        let b = AssertValue::String("world".to_string());

        assert!(a.compare(CompareOp::Contains, &b).unwrap());
        assert!(
            a.compare(
                CompareOp::Equal,
                &AssertValue::String("hello world".to_string())
            )
            .unwrap()
        );
        assert!(!a.compare(CompareOp::Equal, &b).unwrap());
    }

    #[test]
    fn test_value_path_display() {
        assert_eq!(ValuePath::Status.to_string(), "status");
        assert_eq!(
            ValuePath::Header("content-type".to_string()).to_string(),
            "headers.content-type"
        );
        assert_eq!(
            ValuePath::Body(vec!["user".to_string(), "id".to_string()]).to_string(),
            "body.user.id"
        );
        assert_eq!(ValuePath::ResponseTime.to_string(), "response.time");
    }
}
