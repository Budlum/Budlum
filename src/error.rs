use crate::cross_domain::BridgeError;
use crate::domain::DomainError;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum BudlumError {
    Storage { code: &'static str, message: String },
    Consensus { code: &'static str, message: String },
    Settlement { code: &'static str, message: String },
    Bridge { code: &'static str, message: String },
    Network { code: &'static str, message: String },
    Rpc { code: &'static str, message: String },
    Validation { code: &'static str, message: String },
}

pub type BudlumResult<T> = Result<T, BudlumError>;

impl BudlumError {
    pub fn code(&self) -> &'static str {
        match self {
            BudlumError::Storage { code, .. }
            | BudlumError::Consensus { code, .. }
            | BudlumError::Settlement { code, .. }
            | BudlumError::Bridge { code, .. }
            | BudlumError::Network { code, .. }
            | BudlumError::Rpc { code, .. }
            | BudlumError::Validation { code, .. } => code,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            BudlumError::Storage { message, .. }
            | BudlumError::Consensus { message, .. }
            | BudlumError::Settlement { message, .. }
            | BudlumError::Bridge { message, .. }
            | BudlumError::Network { message, .. }
            | BudlumError::Rpc { message, .. }
            | BudlumError::Validation { message, .. } => message,
        }
    }

    pub fn settlement(code: &'static str, message: impl Into<String>) -> Self {
        BudlumError::Settlement {
            code,
            message: message.into(),
        }
    }

    pub fn validation(code: &'static str, message: impl Into<String>) -> Self {
        BudlumError::Validation {
            code,
            message: message.into(),
        }
    }
}

impl Display for BudlumError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for BudlumError {}

impl From<std::io::Error> for BudlumError {
    fn from(value: std::io::Error) -> Self {
        BudlumError::Storage {
            code: "storage_error",
            message: value.to_string(),
        }
    }
}

impl From<BridgeError> for BudlumError {
    fn from(value: BridgeError) -> Self {
        BudlumError::Bridge {
            code: "bridge_error",
            message: value.to_string(),
        }
    }
}

impl From<DomainError> for BudlumError {
    fn from(value: DomainError) -> Self {
        BudlumError::Settlement {
            code: "domain_error",
            message: value.to_string(),
        }
    }
}
