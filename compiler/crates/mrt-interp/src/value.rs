//! MRT runtime values.
//!
//! Two rules drive the representation, both inherited from the reference
//! implementation rather than chosen here:
//!
//! * **Aggregates have reference semantics.** `var b = a; push(b, 2)` is
//!   visible through `a`, so arrays, objects and struct instances are shared
//!   handles rather than copies. Numbers, strings and booleans are values.
//! * **Object keys follow the language's own `==`.** `true` and `1` are
//!   different keys, because `==` says they are different values. The host
//!   language does not get a vote: Python's dict conflates them and
//!   JavaScript's Map does not, which is exactly how the two implementations
//!   came to disagree.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use mrt_ast::{Param, Stmt};

use crate::env::Env;

#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Str(Rc<str>),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<ObjMap>>),
    Function(Rc<Function>),
    Builtin(Rc<Builtin>),
    Struct(Rc<StructType>),
    Instance(Rc<Instance>),
}

impl Value {
    pub fn str(s: impl Into<Rc<str>>) -> Value {
        Value::Str(s.into())
    }

    pub fn array(items: Vec<Value>) -> Value {
        Value::Array(Rc::new(RefCell::new(items)))
    }

    pub fn object(map: ObjMap) -> Value {
        Value::Object(Rc::new(RefCell::new(map)))
    }

    /// Only `null` and `false` are falsy. `0` and `""` are true, which is
    /// deliberate: a language that raises on a bad index has no business
    /// quietly treating an empty string as absence.
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Value::Null | Value::Bool(false))
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// The MRT type of a value, as `type()` reports it.
pub fn type_name(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(_) => "boolean".into(),
        Value::Number(_) => "number".into(),
        Value::Str(_) => "string".into(),
        Value::Array(_) => "array".into(),
        Value::Object(_) => "object".into(),
        Value::Function(_) | Value::Builtin(_) => "function".into(),
        Value::Struct(_) => "struct".into(),
        // An instance reports its own struct's name, so `type(p) == "Point"`.
        Value::Instance(i) => i.struct_type.name.clone(),
    }
}

/// Render a value the way MRT source would spell it.
pub fn stringify(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Number(n) => format_number(*n),
        Value::Str(s) => s.to_string(),
        Value::Array(items) => {
            let rendered: Vec<String> = items.borrow().iter().map(stringify).collect();
            format!("[{}]", rendered.join(", "))
        }
        Value::Object(map) => {
            let map = map.borrow();
            let rendered: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", stringify(&k.to_value()), stringify(v)))
                .collect();
            format!("{{{}}}", rendered.join(", "))
        }
        Value::Function(f) => match &f.name {
            Some(name) => format!("<function {name}>"),
            None => "<function>".into(),
        },
        // A built-in prints as `<builtin>` rather than leaking a host
        // function's identity, which would differ run to run and between
        // implementations.
        Value::Builtin(_) => "<builtin>".into(),
        Value::Struct(s) => format!("<struct {}>", s.name),
        Value::Instance(i) => {
            let rendered: Vec<String> = i
                .fields
                .borrow()
                .iter()
                .map(|(k, v)| format!("{k}: {}", stringify(v)))
                .collect();
            format!("{}({})", i.struct_type.name, rendered.join(", "))
        }
    }
}

/// Numbers print without a trailing `.0` when whole.
///
/// Rust's `{}` and Python's `repr` agree on shortest-round-trip for ordinary
/// magnitudes, but disagree once a value needs an exponent: Python writes
/// `1e+20` where Rust writes it out in full. The exponent form is matched
/// explicitly so the two implementations print the same text.
pub fn format_number(n: f64) -> String {
    if n == 0.0 {
        // Covers -0.0, which both implementations print as `0`.
        return "0".into();
    }
    if n.is_nan() {
        return "nan".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if n.fract() == 0.0 && n.abs() < 1e16 {
        return format!("{}", n as i64);
    }

    let magnitude = n.abs();
    if !(1e-4..1e16).contains(&magnitude) {
        return python_exponent(n);
    }
    format!("{n}")
}

/// Python's `repr` for a float that needs an exponent: at least two exponent
/// digits, an explicit sign, and the shortest mantissa that round-trips.
fn python_exponent(n: f64) -> String {
    let formatted = format!("{n:e}"); // e.g. "1e20", "1.5e-7"
    let (mantissa, exponent) = formatted.split_once('e').expect("{:e} always has an e");
    let exponent: i32 = exponent.parse().expect("exponent is an integer");
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.abs())
}

/// Structural equality. Not the host language's `==`: MRT says `true != 1`
/// at every nesting level, which no host runtime does for free.
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            if Rc::ptr_eq(x, y) {
                return true;
            }
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| values_equal(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            if Rc::ptr_eq(x, y) {
                return true;
            }
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|other| values_equal(v, other)))
        }
        (Value::Instance(x), Value::Instance(y)) => {
            // Two instances are equal when they share a struct and every
            // field matches; an instance never equals a plain object.
            if !Rc::ptr_eq(&x.struct_type, &y.struct_type) {
                return false;
            }
            let (x, y) = (x.fields.borrow(), y.fields.borrow());
            x.iter()
                .zip(y.iter())
                .all(|((ka, va), (kb, vb))| ka == kb && values_equal(va, vb))
        }
        (Value::Function(x), Value::Function(y)) => Rc::ptr_eq(x, y),
        (Value::Builtin(x), Value::Builtin(y)) => Rc::ptr_eq(x, y),
        (Value::Struct(x), Value::Struct(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

// -- object keys -------------------------------------------------------------

/// A key in an MRT object.
///
/// Numbers are stored as bits so the key can be hashed, with `-0.0`
/// normalised to `0.0` first — `0` and `-0` are the same number, so they must
/// be the same key. `NaN` cannot occur: every operation that would produce one
/// raises instead.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ObjKey {
    Str(Rc<str>),
    Num(u64),
    Bool(bool),
}

impl ObjKey {
    pub fn from_value(value: &Value) -> Option<ObjKey> {
        Some(match value {
            Value::Str(s) => ObjKey::Str(s.clone()),
            Value::Number(n) => ObjKey::Num(if *n == 0.0 { 0.0f64 } else { *n }.to_bits()),
            Value::Bool(b) => ObjKey::Bool(*b),
            _ => return None,
        })
    }

    pub fn to_value(&self) -> Value {
        match self {
            ObjKey::Str(s) => Value::Str(s.clone()),
            ObjKey::Num(bits) => Value::Number(f64::from_bits(*bits)),
            ObjKey::Bool(b) => Value::Bool(*b),
        }
    }
}

impl fmt::Display for ObjKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", stringify(&self.to_value()))
    }
}

/// An insertion-ordered map. Order is not part of the language's contract,
/// but both existing implementations preserve it and programs print objects,
/// so the third one preserves it too.
#[derive(Default, Clone)]
pub struct ObjMap {
    entries: Vec<(ObjKey, Value)>,
    index: HashMap<ObjKey, usize>,
}

impl ObjMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: ObjKey, value: Value) {
        match self.index.get(&key) {
            Some(at) => self.entries[*at].1 = value,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }

    pub fn get(&self, key: &ObjKey) -> Option<&Value> {
        self.index.get(key).map(|at| &self.entries[*at].1)
    }

    pub fn contains_key(&self, key: &ObjKey) -> bool {
        self.index.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ObjKey, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &ObjKey> {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, v)| v)
    }
}

impl FromIterator<(ObjKey, Value)> for ObjMap {
    fn from_iter<T: IntoIterator<Item = (ObjKey, Value)>>(iter: T) -> Self {
        let mut map = ObjMap::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

// -- callables ---------------------------------------------------------------

/// A closure: a declared `func` and an anonymous one are the same thing at
/// run time, differing only in whether `name` is set.
pub struct Function {
    pub name: Option<String>,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub closure: Env,
    pub is_generator: bool,
}

impl Function {
    /// How this function's accepted argument count reads in an error.
    pub fn arity_description(&self) -> String {
        let positional = self.params.iter().filter(|p| !p.rest).count();
        let required = self
            .params
            .iter()
            .filter(|p| !p.rest && p.pattern.default().is_none())
            .count();
        if self.params.iter().any(|p| p.rest) {
            return format!("at least {required}");
        }
        if required == positional {
            return required.to_string();
        }
        format!("between {required} and {positional}")
    }

    pub fn accepts(&self, count: usize) -> bool {
        let positional = self.params.iter().filter(|p| !p.rest).count();
        let required = self
            .params
            .iter()
            .filter(|p| !p.rest && p.pattern.default().is_none())
            .count();
        count >= required && (self.params.iter().any(|p| p.rest) || count <= positional)
    }
}

pub struct Builtin {
    pub name: &'static str,
    /// State for a generator returned by `random(seed)`. Kept on the builtin
    /// rather than as its own value kind so `type()` still answers
    /// "function", which is what it is.
    pub rng: Option<RefCell<u32>>,
}

impl Builtin {
    pub fn named(name: &'static str) -> Value {
        Value::Builtin(Rc::new(Builtin { name, rng: None }))
    }
}

/// A declared struct type, and the callable that constructs it.
pub struct StructType {
    pub name: String,
    pub fields: Vec<Param>,
    pub methods: Vec<(String, Rc<Function>)>,
}

impl StructType {
    pub fn field_names(&self) -> Vec<String> {
        self.fields
            .iter()
            .map(|f| match &f.pattern {
                mrt_ast::Pattern::Name { name, .. } => name.text.clone(),
                _ => unreachable!("struct fields are plain names"),
            })
            .collect()
    }

    pub fn method(&self, name: &str) -> Option<&Rc<Function>> {
        self.methods.iter().find(|(n, _)| n == name).map(|(_, f)| f)
    }

    pub fn arity_description(&self) -> String {
        let required = self
            .fields
            .iter()
            .filter(|f| f.pattern.default().is_none())
            .count();
        if required == self.fields.len() {
            required.to_string()
        } else {
            format!("between {required} and {}", self.fields.len())
        }
    }

    pub fn accepts(&self, count: usize) -> bool {
        let required = self
            .fields
            .iter()
            .filter(|f| f.pattern.default().is_none())
            .count();
        count >= required && count <= self.fields.len()
    }
}

/// One value of a struct type. Fields keep declaration order, so an instance
/// prints its fields in the order the struct declares them.
pub struct Instance {
    pub struct_type: Rc<StructType>,
    pub fields: RefCell<Vec<(String, Value)>>,
}

impl Instance {
    pub fn get_field(&self, name: &str) -> Option<Value> {
        self.fields
            .borrow()
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }

    pub fn set_field(&self, name: &str, value: Value) -> bool {
        let mut fields = self.fields.borrow_mut();
        match fields.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => {
                slot.1 = value;
                true
            }
            None => false,
        }
    }
}

/// Values can form cycles -- `push(a, a)` is legal -- so `Debug` stays
/// shallow: it names the type and shows scalars, and prints aggregates by
/// shape alone. It exists for the `Debug` bound the error types carry, not
/// for anything the language prints; `stringify` is the user-visible form.
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Number(n) => write!(f, "{}", format_number(*n)),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Array(items) => write!(f, "<array len {}>", items.borrow().len()),
            Value::Object(map) => write!(f, "<object len {}>", map.borrow().len()),
            Value::Function(func) => match &func.name {
                Some(name) => write!(f, "<fn {name}>"),
                None => write!(f, "<fn>"),
            },
            Value::Builtin(b) => write!(f, "<builtin {}>", b.name),
            Value::Struct(s) => write!(f, "<struct {}>", s.name),
            Value::Instance(i) => write!(f, "<{}>", i.struct_type.name),
        }
    }
}
