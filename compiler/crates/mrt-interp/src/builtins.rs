//! The built-in library.
//!
//! Every message here is matched word for word against the reference
//! implementation, because programs catch errors and print `e.message`: a
//! reworded message is a behaviour change, not a cosmetic one.

use std::cell::RefCell;
use std::rc::Rc;

use crate::env::Env;
use crate::error::{arity_error, type_error, value_error, Eval, Kind, Signal};
use crate::generator::{step_result, Generator, Transform};
use crate::value::*;
use crate::{is_iterable, object_fields, Interpreter};

const NAMES: &[&str] = &[
    "len",
    "push",
    "pop",
    "slice",
    "join",
    "indexOf",
    "split",
    "substring",
    "toUpper",
    "toLower",
    "trim",
    "replace",
    "startsWith",
    "endsWith",
    "contains",
    "type",
    "toNumber",
    "toString",
    "abs",
    "min",
    "max",
    "round",
    "floor",
    "ceil",
    "sqrt",
    "pow",
    "keys",
    "values",
    "has",
    "get",
    "reverse",
    "unique",
    "flatten",
    "zip",
    "enumerate",
    "count",
    "sum",
    "range",
    "repeat",
    "padStart",
    "padEnd",
    "random",
    "map",
    "filter",
    "reduce",
    "find",
    "some",
    "every",
    "sort",
    "toArray",
    "take",
    "next",
    "send",
    "print",
    "aiTrainLinear",
];

pub fn install(globals: &Env) {
    for name in NAMES {
        globals.define(name, Builtin::named(name));
    }
}

pub fn call(interp: &mut Interpreter, name: &str, args: Vec<Value>) -> Eval {
    match name {
        "aiTrainLinear" => {
            exactly(
                &args,
                4,
                "aiTrainLinear() takes x, y, epochs, and learningRate.",
            )?;
            let x = tensor_from_mrt(&args[0], "aiTrainLinear")?;
            let y = tensor_from_mrt(&args[1], "aiTrainLinear")?;
            let epochs = number(&args[2], "aiTrainLinear")? as usize;
            let lr = number(&args[3], "aiTrainLinear")?;
            if epochs == 0 {
                return Err(value_error(
                    "aiTrainLinear() epochs must be greater than zero.",
                ));
            }
            if lr <= 0.0 {
                return Err(value_error(
                    "aiTrainLinear() learningRate must be greater than zero.",
                ));
            }

            let mut model = mrt_ai::Sequential::new();
            model.add(mrt_ai::Dense::new(x.cols, y.cols, 42));
            let mut trainer = mrt_ai::Trainer::new(epochs, lr);
            let history = trainer
                .train(&mut model, &x, &y)
                .map_err(|e| value_error(e.to_string()))?;
            let prediction = model.forward(&x).map_err(|e| value_error(e.to_string()))?;

            let mut result = ObjMap::new();
            result.insert(
                ObjKey::Str(Rc::from("loss")),
                Value::Number(*history.last().unwrap_or(&0.0)),
            );
            result.insert(
                ObjKey::Str(Rc::from("initialLoss")),
                Value::Number(history[0]),
            );
            result.insert(
                ObjKey::Str(Rc::from("predictions")),
                tensor_to_mrt(&prediction),
            );
            result.insert(
                ObjKey::Str(Rc::from("epochs")),
                Value::Number(epochs as f64),
            );
            Ok(Value::object(result))
        }
        // -- arrays --------------------------------------------------------
        "len" => {
            one(&args, "len")?;
            if let Some(fields) = object_fields(&args[0]) {
                return Ok(Value::Number(fields.len() as f64));
            }
            match &args[0] {
                Value::Array(a) => Ok(Value::Number(a.borrow().len() as f64)),
                Value::Str(s) => Ok(Value::Number(s.chars().count() as f64)),
                _ => Err(type_error(
                    "len() argument must be an array, object, or string.",
                )),
            }
        }
        "push" => {
            exactly(&args, 2, "push() takes exactly two arguments.")?;
            let array = array_arg(&args[0], "push")?;
            array.borrow_mut().push(args[1].clone());
            Ok(args[1].clone())
        }
        "pop" => {
            one(&args, "pop")?;
            let Value::Array(a) = &args[0] else {
                return Err(type_error("pop() argument must be an array."));
            };
            a.borrow_mut()
                .pop()
                .ok_or_else(|| value_error("Cannot pop from empty array."))
        }
        "slice" => {
            between(&args, 2, 3, "slice() takes 2 or 3 arguments.")?;
            let array = array_arg(&args[0], "slice")?;
            let items = array.borrow();
            let (start, end) = span(&args, items.len());
            Ok(Value::array(items[start..end].to_vec()))
        }
        "join" => {
            between(&args, 1, 2, "join() takes 1 or 2 arguments.")?;
            let array = array_arg(&args[0], "join")?;
            let sep = args.get(1).map(stringify).unwrap_or_default();
            let parts: Vec<String> = array.borrow().iter().map(stringify).collect();
            Ok(Value::str(parts.join(&sep)))
        }
        "indexOf" => {
            exactly(&args, 2, "indexOf() takes exactly 2 arguments.")?;
            let array = array_arg(&args[0], "indexOf")?;
            let found = array
                .borrow()
                .iter()
                .position(|item| values_equal(item, &args[1]));
            Ok(Value::Number(found.map(|i| i as f64).unwrap_or(-1.0)))
        }
        "reverse" => {
            one(&args, "reverse")?;
            match &args[0] {
                Value::Array(a) => {
                    let mut items = a.borrow().clone();
                    items.reverse();
                    Ok(Value::array(items))
                }
                Value::Str(s) => Ok(Value::str(s.chars().rev().collect::<String>())),
                _ => Err(type_error("reverse() argument must be an array or string.")),
            }
        }
        "unique" => {
            one(&args, "unique")?;
            let array = array_arg(&args[0], "unique")?;
            let mut out: Vec<Value> = Vec::new();
            for item in array.borrow().iter() {
                if !out.iter().any(|seen| values_equal(seen, item)) {
                    out.push(item.clone());
                }
            }
            Ok(Value::array(out))
        }
        "flatten" => {
            between(&args, 1, 2, "flatten() takes 1 or 2 arguments.")?;
            let array = array_arg(&args[0], "flatten")?;
            let depth = match args.get(1) {
                Some(v) => number(v, "flatten")? as i64,
                None => 1,
            };
            let mut out = Vec::new();
            flatten_into(&array.borrow(), depth, &mut out);
            Ok(Value::array(out))
        }
        "zip" => {
            exactly(&args, 2, "zip() takes exactly 2 arguments.")?;
            let a = array_arg(&args[0], "zip")?;
            let b = array_arg(&args[1], "zip")?;
            let (a, b) = (a.borrow(), b.borrow());
            Ok(Value::array(
                a.iter()
                    .zip(b.iter())
                    .map(|(x, y)| Value::array(vec![x.clone(), y.clone()]))
                    .collect(),
            ))
        }
        "enumerate" => {
            one(&args, "enumerate")?;
            let array = array_arg(&args[0], "enumerate")?;
            let pairs: Vec<Value> = array
                .borrow()
                .iter()
                .enumerate()
                .map(|(i, v)| Value::array(vec![Value::Number(i as f64), v.clone()]))
                .collect();
            Ok(Value::array(pairs))
        }
        "count" => {
            exactly(&args, 2, "count() takes exactly 2 arguments.")?;
            let array = array_arg(&args[0], "count")?;
            let n = array
                .borrow()
                .iter()
                .filter(|item| values_equal(item, &args[1]))
                .count();
            Ok(Value::Number(n as f64))
        }
        "sum" => {
            one(&args, "sum")?;
            let array = array_arg(&args[0], "sum")?;
            let mut total = 0.0;
            for item in array.borrow().iter() {
                total += number(item, "sum")?;
            }
            Ok(Value::Number(total))
        }
        "range" => {
            between(&args, 1, 3, "range() takes one to three number arguments.")?;
            let nums: Result<Vec<f64>, Signal> = args.iter().map(|a| number(a, "range")).collect();
            let nums = nums?;
            let (start, end, step) = match nums.len() {
                1 => (0.0, nums[0], 1.0),
                2 => (nums[0], nums[1], 1.0),
                _ => (nums[0], nums[1], nums[2]),
            };
            if step == 0.0 {
                return Err(value_error("range() step must not be zero."));
            }
            let mut out = Vec::new();
            let mut current = start;
            while (step > 0.0 && current < end) || (step < 0.0 && current > end) {
                out.push(Value::Number(current));
                current += step;
            }
            Ok(Value::array(out))
        }

        // -- strings -------------------------------------------------------
        "split" => {
            between(&args, 1, 2, "split() takes 1 or 2 arguments.")?;
            let text = string_arg(&args[0], "split")?;
            let sep = args.get(1).map(stringify).unwrap_or_else(|| " ".into());
            Ok(Value::array(
                text.split(&sep).map(Value::str).collect::<Vec<_>>(),
            ))
        }
        "substring" => {
            between(&args, 2, 3, "substring() takes 2 or 3 arguments.")?;
            let text = string_arg(&args[0], "substring")?;
            let chars: Vec<char> = text.chars().collect();
            let (start, end) = span(&args, chars.len());
            Ok(Value::str(chars[start..end].iter().collect::<String>()))
        }
        "toUpper" => {
            one(&args, "toUpper")?;
            Ok(Value::str(string_arg1(&args[0], "toUpper")?.to_uppercase()))
        }
        "toLower" => {
            one(&args, "toLower")?;
            Ok(Value::str(string_arg1(&args[0], "toLower")?.to_lowercase()))
        }
        "trim" => {
            one(&args, "trim")?;
            Ok(Value::str(
                string_arg1(&args[0], "trim")?.trim().to_string(),
            ))
        }
        "replace" => {
            exactly(&args, 3, "replace() takes exactly 3 arguments.")?;
            let text = string_arg(&args[0], "replace")?;
            Ok(Value::str(
                text.replace(&stringify(&args[1]), &stringify(&args[2])),
            ))
        }
        "startsWith" => {
            exactly(&args, 2, "startsWith() takes exactly 2 arguments.")?;
            let text = string_arg(&args[0], "startsWith")?;
            Ok(Value::Bool(text.starts_with(&stringify(&args[1]))))
        }
        "endsWith" => {
            exactly(&args, 2, "endsWith() takes exactly 2 arguments.")?;
            let text = string_arg(&args[0], "endsWith")?;
            Ok(Value::Bool(text.ends_with(&stringify(&args[1]))))
        }
        "contains" => {
            exactly(&args, 2, "contains() takes exactly 2 arguments.")?;
            let text = string_arg(&args[0], "contains")?;
            Ok(Value::Bool(text.contains(&stringify(&args[1]))))
        }
        "repeat" => {
            exactly(&args, 2, "repeat() takes exactly 2 arguments.")?;
            let text = string_arg(&args[0], "repeat")?;
            let n = number(&args[1], "repeat")?;
            if n < 0.0 {
                return Err(value_error("repeat() count must not be negative."));
            }
            Ok(Value::str(text.repeat(n as usize)))
        }
        "padStart" | "padEnd" => {
            between(&args, 2, 3, format!("{name}() takes 2 or 3 arguments."))?;
            let text = string_arg(&args[0], name)?;
            let width = number(&args[1], name)? as usize;
            let pad = match args.get(2) {
                Some(v) => stringify(v),
                None => " ".into(),
            };
            let current = text.chars().count();
            if current >= width || pad.is_empty() {
                return Ok(Value::str(text));
            }
            let needed = width - current;
            let filler: String = pad.chars().cycle().take(needed).collect();
            Ok(Value::str(if name == "padStart" {
                format!("{filler}{text}")
            } else {
                format!("{text}{filler}")
            }))
        }

        // -- types and maths -----------------------------------------------
        "type" => {
            one(&args, "type")?;
            Ok(Value::str(type_name(&args[0])))
        }
        "toString" => {
            one(&args, "toString")?;
            Ok(Value::str(stringify(&args[0])))
        }
        "toNumber" => {
            one(&args, "toNumber")?;
            match &args[0] {
                Value::Bool(b) => Ok(Value::Number(if *b { 1.0 } else { 0.0 })),
                Value::Number(n) => Ok(Value::Number(*n)),
                Value::Str(s) => s
                    .trim()
                    .parse::<f64>()
                    .map(Value::Number)
                    .map_err(|_| value_error(format!("Cannot convert '{s}' to a number."))),
                _ => Err(type_error(
                    "toNumber() argument must be a string, number, or boolean.",
                )),
            }
        }
        "abs" => {
            one(&args, "abs")?;
            Ok(Value::Number(number(&args[0], "abs")?.abs()))
        }
        "floor" => {
            one(&args, "floor")?;
            Ok(Value::Number(number(&args[0], "floor")?.floor()))
        }
        "ceil" => {
            one(&args, "ceil")?;
            Ok(Value::Number(number(&args[0], "ceil")?.ceil()))
        }
        "sqrt" => {
            one(&args, "sqrt")?;
            let n = number(&args[0], "sqrt")?;
            if n < 0.0 {
                return Err(value_error(
                    "Cannot take the square root of a negative number.",
                ));
            }
            Ok(Value::Number(n.sqrt()))
        }
        "pow" => {
            exactly(&args, 2, "pow() takes exactly 2 arguments.")?;
            Ok(Value::Number(
                number(&args[0], "pow")?.powf(number(&args[1], "pow")?),
            ))
        }
        "min" | "max" => {
            let values: Vec<Value> = match args.as_slice() {
                [Value::Array(a)] => a.borrow().clone(),
                other => other.to_vec(),
            };
            if values.is_empty() {
                return Err(arity_error(format!(
                    "{name}() requires at least one argument."
                )));
            }
            let mut best = number(&values[0], name)?;
            for v in &values[1..] {
                let n = number(v, name)?;
                if (name == "min" && n < best) || (name == "max" && n > best) {
                    best = n;
                }
            }
            Ok(Value::Number(best))
        }
        "round" => {
            between(&args, 1, 2, "round() takes 1 or 2 arguments.")?;
            let value = number(&args[0], "round")?;
            let digits = match args.get(1) {
                Some(v) => number(v, "round")? as i32,
                None => 0,
            };
            // Half away from zero, scaled first. Deliberately not Rust's
            // `round`, so the three implementations agree on a .5 boundary.
            let factor = 10f64.powi(digits);
            let scaled = value * factor;
            let rounded = (scaled.abs() + 0.5).floor();
            Ok(Value::Number(
                if scaled < 0.0 { -rounded } else { rounded } / factor,
            ))
        }

        // -- objects -------------------------------------------------------
        "keys" => {
            let fields = object_arg(&args, "keys")?;
            Ok(Value::array(
                fields.into_iter().map(|(k, _)| Value::str(k)).collect(),
            ))
        }
        "values" => {
            let fields = object_arg(&args, "values")?;
            Ok(Value::array(fields.into_iter().map(|(_, v)| v).collect()))
        }
        "has" => {
            exactly(&args, 2, "has() takes exactly 2 arguments.")?;
            if let Value::Object(map) = &args[0] {
                let Some(key) = ObjKey::from_value(&args[1]) else {
                    return Ok(Value::Bool(false));
                };
                return Ok(Value::Bool(map.borrow().contains_key(&key)));
            }
            if let Some(fields) = object_fields(&args[0]) {
                let wanted = stringify(&args[1]);
                return Ok(Value::Bool(fields.iter().any(|(k, _)| *k == wanted)));
            }
            if let Value::Array(a) = &args[0] {
                return Ok(Value::Bool(
                    a.borrow().iter().any(|item| values_equal(item, &args[1])),
                ));
            }
            Err(type_error(
                "First argument to has() must be an array or object.",
            ))
        }
        "get" => {
            between(&args, 2, 3, "get() takes 2 or 3 arguments.")?;
            let default = args.get(2).cloned().unwrap_or(Value::Null);
            if let Value::Object(map) = &args[0] {
                let Some(key) = ObjKey::from_value(&args[1]) else {
                    return Ok(default);
                };
                return Ok(map.borrow().get(&key).cloned().unwrap_or(default));
            }
            if let Some(fields) = object_fields(&args[0]) {
                let wanted = stringify(&args[1]);
                return Ok(fields
                    .into_iter()
                    .find(|(k, _)| *k == wanted)
                    .map(|(_, v)| v)
                    .unwrap_or(default));
            }
            if let Value::Array(a) = &args[0] {
                if let Some(n) = args[1].as_number() {
                    let items = a.borrow();
                    let i = n as i64;
                    if i >= 0 && (i as usize) < items.len() {
                        return Ok(items[i as usize].clone());
                    }
                }
                return Ok(default);
            }
            // A string indexes by position like an array does, because `s[0]`
            // does too.
            if let Value::Str(s) = &args[0] {
                if let Some(n) = args[1].as_number() {
                    let chars: Vec<char> = s.chars().collect();
                    let i = n as i64;
                    if i >= 0 && (i as usize) < chars.len() {
                        return Ok(Value::str(chars[i as usize].to_string()));
                    }
                }
                return Ok(default);
            }
            // `get` is total on purpose: it is the guard idiom for a value
            // that may not be an object at all.
            Ok(default)
        }

        // -- higher order ---------------------------------------------------
        "map" | "filter" => {
            exactly(
                &args,
                2,
                format!("{name}() takes a sequence and a function."),
            )?;
            if !is_iterable(&args[0]) {
                return Err(type_error(format!(
                    "{name}() needs something iterable, not {}.",
                    type_name(&args[0])
                )));
            }
            let kind = if name == "map" {
                Transform::Map
            } else {
                Transform::Filter
            };
            // Over a generator the result is itself lazy, so `map` over an
            // endless sequence is usable -- and, just as importantly, applies
            // the function only to the items something actually pulls. Over an
            // eager sequence the answer is an array, because that is what the
            // source was: laziness is inherited, not imposed.
            if matches!(&args[0], Value::Generator(_)) {
                let source = interp.cursor(&args[0], None)?;
                return Ok(Value::Generator(Rc::new(Generator::transform(
                    format!("<generator {name}>"),
                    kind,
                    source,
                    args[1].clone(),
                ))));
            }
            let items = items_arg(interp, &args[0], name)?;
            let mut out = Vec::new();
            for item in items {
                let produced = interp.call_value(args[1].clone(), vec![item.clone()], None)?;
                if name == "map" {
                    out.push(produced);
                } else if produced.is_truthy() {
                    out.push(item);
                }
            }
            Ok(Value::array(out))
        }
        "reduce" => {
            between(
                &args,
                2,
                3,
                "reduce() takes a sequence, a function, and an optional initial value.",
            )?;
            let items = items_arg(interp, &args[0], "reduce")?;
            let (mut acc, rest) = match args.get(2) {
                Some(init) => (init.clone(), &items[..]),
                None => {
                    if items.is_empty() {
                        return Err(value_error(
                            "reduce() of an empty sequence needs an initial value.",
                        ));
                    }
                    (items[0].clone(), &items[1..])
                }
            };
            for item in rest {
                acc = interp.call_value(args[1].clone(), vec![acc, item.clone()], None)?;
            }
            Ok(acc)
        }
        "find" => {
            exactly(&args, 2, "find() takes a sequence and a function.")?;
            for item in items_arg(interp, &args[0], "find")? {
                if interp
                    .call_value(args[1].clone(), vec![item.clone()], None)?
                    .is_truthy()
                {
                    return Ok(item);
                }
            }
            Ok(Value::Null)
        }
        "some" | "every" => {
            exactly(
                &args,
                2,
                format!("{name}() takes a sequence and a function."),
            )?;
            let want_any = name == "some";
            for item in items_arg(interp, &args[0], name)? {
                let hit = interp
                    .call_value(args[1].clone(), vec![item], None)?
                    .is_truthy();
                if hit == want_any {
                    return Ok(Value::Bool(want_any));
                }
            }
            Ok(Value::Bool(!want_any))
        }
        "sort" => {
            between(
                &args,
                1,
                2,
                "sort() takes an array and an optional compare function.",
            )?;
            let array = array_arg(&args[0], "sort")?;
            let mut items = array.borrow().clone();
            match args.get(1) {
                Some(comparator) => {
                    // Insertion sort: stable, and it lets the comparator
                    // return an error without unwinding a sort in progress.
                    for i in 1..items.len() {
                        let mut j = i;
                        while j > 0 {
                            let ordering = interp.call_value(
                                comparator.clone(),
                                vec![items[j - 1].clone(), items[j].clone()],
                                None,
                            )?;
                            let Some(n) = ordering.as_number() else {
                                return Err(type_error(
                                    "sort() compare function must return a number.",
                                ));
                            };
                            if n <= 0.0 {
                                break;
                            }
                            items.swap(j - 1, j);
                            j -= 1;
                        }
                    }
                }
                None => {
                    if items.iter().all(|v| matches!(v, Value::Number(_))) {
                        items.sort_by(|a, b| {
                            a.as_number()
                                .unwrap()
                                .partial_cmp(&b.as_number().unwrap())
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    } else if items.iter().all(|v| matches!(v, Value::Str(_))) {
                        items.sort_by(|a, b| a.as_str().unwrap().cmp(b.as_str().unwrap()));
                    } else {
                        return Err(value_error(
                            "sort() without a compare function needs an array of all numbers or all strings.",
                        ));
                    }
                }
            }
            Ok(Value::array(items))
        }
        "toArray" => {
            one(&args, "toArray")?;
            Ok(Value::array(interp.iterate(&args[0], None)?))
        }
        "take" => {
            exactly(&args, 2, "take() takes an iterable and a count.")?;
            let count = number(&args[1], "take")?;
            if count < 0.0 {
                return Err(value_error("take() count must not be negative."));
            }
            // Pulled one at a time rather than collected and sliced: `take`
            // over an endless generator is the whole reason it exists, and it
            // must leave the generator parked where it stopped.
            // No iterability check of its own: a non-sequence has to report
            // the language's own "Can only iterate over ..." here, which
            // building the cursor already does.
            let mut cursor = interp.cursor(&args[0], None)?;
            let mut items = Vec::new();
            while items.len() < count as usize {
                match interp.advance(&mut cursor, None)? {
                    Some(item) => items.push(item),
                    None => break,
                }
            }
            Ok(Value::array(items))
        }

        // -- driving a generator by hand ------------------------------------
        // `for`-`in` is the usual way to consume a sequence; these are for
        // when a program wants one value at a time, and `send` for when it
        // wants to pass something back in. Both report `{done, value}` rather
        // than a bare item, because "the sequence ended" and "the sequence
        // yielded null" are different answers.
        "next" | "send" => {
            let sent = if name == "next" {
                exactly(&args, 1, "next() takes a generator.")?;
                Value::Null
            } else {
                exactly(&args, 2, "send() takes a generator and a value.")?;
                args[1].clone()
            };
            let Value::Generator(generator) = &args[0] else {
                return Err(type_error(format!(
                    "{name}() needs a generator, not {}.",
                    type_name(&args[0])
                )));
            };
            // Stepping a generator that has run out is not an error -- only
            // *iterating* one is. A hand-driven consumer has no other way to
            // ask whether there is more.
            let generator = generator.clone();
            match interp.step_generator(&generator, sent, None)? {
                Some(value) => Ok(step_result(false, value)),
                None => Ok(step_result(true, Value::Null)),
            }
        }

        // -- randomness -----------------------------------------------------
        "random" => {
            one(&args, "random")?;
            let seed = number(&args[0], "random")? as i64 as u32;
            Ok(Value::Builtin(Rc::new(Builtin {
                name: "<random>",
                rng: Some(RefCell::new(if seed == 0 { 1 } else { seed })),
            })))
        }

        "print" => {
            interp.print_values(&args);
            Ok(Value::Null)
        }

        _ => Err(Signal::error(
            Kind::RuntimeError,
            format!("Unknown built-in '{name}'."),
        )),
    }
}

/// One step of a `random(seed)` stream: xorshift32 over uint32 state, matching
/// the other implementations bit for bit so a seeded program prints the same
/// numbers everywhere.
pub fn next_random(state: &RefCell<u32>, args: &[Value]) -> Eval {
    if !args.is_empty() {
        return Err(arity_error("A random generator takes no arguments."));
    }
    let mut x = *state.borrow();
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state.borrow_mut() = x;
    Ok(Value::Number(x as f64 / 4294967296.0))
}

// -- argument helpers --------------------------------------------------------

fn one(args: &[Value], who: &str) -> Result<(), Signal> {
    if args.len() != 1 {
        return Err(arity_error(format!("{who}() takes exactly one argument.")));
    }
    Ok(())
}

fn exactly(args: &[Value], n: usize, message: impl Into<String>) -> Result<(), Signal> {
    if args.len() != n {
        return Err(arity_error(message));
    }
    Ok(())
}

fn between(args: &[Value], lo: usize, hi: usize, message: impl Into<String>) -> Result<(), Signal> {
    if args.len() < lo || args.len() > hi {
        return Err(arity_error(message));
    }
    Ok(())
}

fn number(value: &Value, who: &str) -> Result<f64, Signal> {
    value
        .as_number()
        .ok_or_else(|| type_error(format!("{who}() argument must be a number.")))
}

fn array_arg(value: &Value, who: &str) -> Result<Rc<RefCell<Vec<Value>>>, Signal> {
    match value {
        Value::Array(a) => Ok(a.clone()),
        _ => Err(type_error(format!(
            "First argument to {who}() must be an array."
        ))),
    }
}

fn string_arg(value: &Value, who: &str) -> Result<String, Signal> {
    match value {
        Value::Str(s) => Ok(s.to_string()),
        _ => Err(type_error(format!(
            "First argument to {who}() must be a string."
        ))),
    }
}

fn string_arg1(value: &Value, who: &str) -> Result<String, Signal> {
    match value {
        Value::Str(s) => Ok(s.to_string()),
        _ => Err(type_error(format!("{who}() argument must be a string."))),
    }
}

fn object_arg(args: &[Value], who: &str) -> Result<Vec<(String, Value)>, Signal> {
    if args.len() != 1 {
        return Err(arity_error(format!(
            "{who}() takes exactly one object argument."
        )));
    }
    object_fields(&args[0])
        .ok_or_else(|| arity_error(format!("{who}() takes exactly one object argument.")))
}

/// The first argument of a built-in that walks a sequence: anything `for`-`in`
/// accepts, including a struct that implements `iter()`.
fn items_arg(interp: &mut Interpreter, value: &Value, who: &str) -> Result<Vec<Value>, Signal> {
    if !is_iterable(value) {
        return Err(type_error(format!(
            "{who}() needs something iterable, not {}.",
            type_name(value)
        )));
    }
    interp.iterate(value, None)
}

/// `slice`/`substring` share a start/end convention, including negative
/// indices counting from the end.
fn span(args: &[Value], len: usize) -> (usize, usize) {
    let resolve = |v: Option<&Value>, fallback: usize| -> usize {
        match v.and_then(|v| v.as_number()) {
            Some(n) => {
                let i = n as i64;
                let i = if i < 0 { len as i64 + i } else { i };
                i.clamp(0, len as i64) as usize
            }
            None => fallback,
        }
    };
    let start = resolve(args.get(1), 0);
    let end = resolve(args.get(2), len);
    (start, end.max(start))
}

fn flatten_into(items: &[Value], depth: i64, out: &mut Vec<Value>) {
    for item in items {
        match item {
            Value::Array(inner) if depth > 0 => flatten_into(&inner.borrow(), depth - 1, out),
            other => out.push(other.clone()),
        }
    }
}

fn tensor_from_mrt(value: &Value, who: &str) -> Result<mrt_ai::Tensor, Signal> {
    let Value::Array(rows) = value else {
        return Err(type_error(format!("{who}() expects a 2D array.")));
    };
    let rows = rows.borrow();
    if rows.is_empty() {
        return Err(value_error(format!(
            "{who}() cannot train on an empty tensor."
        )));
    }
    let mut data = Vec::new();
    let mut cols = None;
    for row in rows.iter() {
        let Value::Array(items) = row else {
            return Err(type_error(format!("{who}() expects rows to be arrays.")));
        };
        let items = items.borrow();
        if items.is_empty() {
            return Err(value_error(format!("{who}() rows cannot be empty.")));
        }
        if let Some(expected) = cols {
            if expected != items.len() {
                return Err(value_error(format!(
                    "{who}() rows must have equal lengths."
                )));
            }
        } else {
            cols = Some(items.len());
        }
        for item in items.iter() {
            data.push(number(item, who)?);
        }
    }
    mrt_ai::Tensor::from_vec(rows.len(), cols.unwrap(), data)
        .map_err(|e| value_error(e.to_string()))
}

fn tensor_to_mrt(t: &mrt_ai::Tensor) -> Value {
    Value::array(
        (0..t.rows)
            .map(|r| Value::array((0..t.cols).map(|c| Value::Number(t.get(r, c))).collect()))
            .collect(),
    )
}
