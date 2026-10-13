macro_rules! identifier {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            /// Validate a nonempty identifier of at most 128 ASCII letters, digits, `-`, `_`, or `.`.
            /// The special path components `.` and `..` are rejected.
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                if value.is_empty() || value.len()>128 || !value.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)) || value=="." || value==".." {
                    return Err(Error::InvalidInput(concat!(stringify!($name), " must be a safe identifier").into()));
                }
                Ok(Self(value))
            }
            /// Generate a new UUID identity. Persist it before retrying an operation.
            pub fn generate() -> Self { Self(Uuid::new_v4().to_string()) }
            /// Borrow the validated identifier as text.
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl TryFrom<String> for $name { type Error=Error; fn try_from(v:String)->Result<Self>{Self::new(v)} }
        impl From<$name> for String { fn from(v:$name)->Self{v.0} }
        impl fmt::Display for $name { fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{self.0.fmt(f)} }
    };
}

pub(super) use identifier;
