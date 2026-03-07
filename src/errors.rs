use std::error::Error;
use std::fmt;

pub type Result<T> = std::result::Result<T, CompileError>;

#[derive(Debug)]
pub struct CompileError {
    msg: String
}

impl CompileError {
    pub fn new(msg: String) -> Self {
        Self {
            msg
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl Error for CompileError {

}
