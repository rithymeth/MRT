//! The MRT-facing side of the AI extension.
//!
//! Everything in `mrt-ai` was reachable only from Rust until now, which made
//! it a library that happened to live in this repository rather than a feature
//! of anything. This is the bridge.
//!
//! -- A model is ordinary MRT data --
//!
//! The obvious implementation gives the language a new opaque value kind: a
//! handle you pass back to `aiTrain` and cannot look inside. That is rejected
//! here. A model is an **array of layer objects**, each an ordinary MRT object
//! with a `kind` and its weights as nested arrays:
//!
//! ```text
//! [ {kind: "dense", weights: [[..], ..], bias: [[..]]},
//!   {kind: "relu"} ]
//! ```
//!
//! So `print(model)` shows the weights, `model[0].bias` reads one, and the
//! whole thing survives `toString` and comparison like any other value. The
//! language gains nothing it has to know about -- which matters more for an
//! *extension* than for core, because an extension that adds a type is no
//! longer optional to understand.
//!
//! The cost is honest: every call converts between nested arrays and
//! `Tensor`, so training in MRT is slower than training in Rust. For a
//! language whose point is teaching what an implementation does, an
//! inspectable model beats a fast opaque one.

use std::rc::Rc;

use mrt_ai::autograd::{Tape, Var, Window};
use mrt_ai::nn::{Adam, Attention, Conv2d, Dense, LayerNorm};
use mrt_ai::Tensor;

use crate::error::{type_error, value_error, Signal};
use crate::value::{ObjKey, ObjMap, Value};

/// One layer of a model, read out of its MRT object.
// `LayerNorm` repeats the enum's name, which clippy dislikes. It stays: the
// variant names are the `kind` strings the language sees, and "layerNorm" is
// what the layer is called everywhere outside this file.
#[allow(clippy::enum_variant_names)]
enum Layer {
    Dense {
        weights: Tensor,
        bias: Tensor,
    },
    Conv2d {
        channels: usize,
        height: usize,
        width: usize,
        kernel_h: usize,
        kernel_w: usize,
        stride: usize,
        filters: usize,
        weights: Tensor,
        bias: Tensor,
    },
    Attention {
        query: Tensor,
        key: Tensor,
        value: Tensor,
    },
    LayerNorm {
        gain: Tensor,
        shift: Tensor,
    },
    Relu,
    Sigmoid,
    Tanh,
    Softmax,
}

impl Layer {
    /// How many trainable tensors this layer contributes, in the order
    /// `put_params` and `forward` use. Positional throughout, because the
    /// optimiser keys its moment state by position.
    fn arity(&self) -> usize {
        match self {
            Layer::Dense { .. } | Layer::LayerNorm { .. } | Layer::Conv2d { .. } => 2,
            Layer::Attention { .. } => 3,
            Layer::Relu | Layer::Sigmoid | Layer::Tanh | Layer::Softmax => 0,
        }
    }

    fn params(&self) -> Vec<Tensor> {
        match self {
            Layer::Dense { weights, bias } => vec![weights.clone(), bias.clone()],
            Layer::Conv2d { weights, bias, .. } => vec![weights.clone(), bias.clone()],
            Layer::Attention { query, key, value } => {
                vec![query.clone(), key.clone(), value.clone()]
            }
            Layer::LayerNorm { gain, shift } => vec![gain.clone(), shift.clone()],
            Layer::Relu | Layer::Sigmoid | Layer::Tanh | Layer::Softmax => Vec::new(),
        }
    }

    /// Run this layer, taking its parameters from the tape.
    ///
    /// The layer types from `mrt-ai::nn` are reconstructed here rather than
    /// stored, because their weights live in the MRT value between calls. Only
    /// the shape travels; the numbers come off the tape.
    fn forward(&self, tape: &mut Tape, input: Var, params: &[Var]) -> Result<Var, Signal> {
        let rows = tape.value(input).rows;
        let result = match self {
            Layer::Dense { weights, bias } => Dense {
                weights: weights.clone(),
                bias: bias.clone(),
            }
            .forward(tape, input, params),
            Layer::Conv2d {
                channels,
                height,
                width,
                kernel_h,
                kernel_w,
                stride,
                filters,
                weights,
                bias,
            } => {
                // The batch size is whatever was handed in, so it is read from
                // the input rather than stored in the model: the same model has
                // to work on one image and on a hundred.
                let window = Window {
                    batch: rows,
                    channels: *channels,
                    height: *height,
                    width: *width,
                    kernel_h: *kernel_h,
                    kernel_w: *kernel_w,
                    stride: *stride,
                };
                Conv2d {
                    window,
                    filters: *filters,
                    weights: weights.clone(),
                    bias: bias.clone(),
                }
                .forward(tape, input, params)
            }
            Layer::Attention { query, key, value } => {
                let head = query.cols;
                Attention {
                    features: query.rows,
                    head,
                    query: query.clone(),
                    key: key.clone(),
                    value: value.clone(),
                }
                .forward(tape, input, params)
            }
            Layer::LayerNorm { gain, shift } => LayerNorm {
                epsilon: 1e-5,
                gain: gain.clone(),
                shift: shift.clone(),
            }
            .forward(tape, input, params),
            Layer::Relu => tape.relu(input),
            Layer::Sigmoid => tape.sigmoid(input),
            Layer::Tanh => tape.tanh(input),
            Layer::Softmax => tape.softmax(input),
        };
        result.map_err(|e| value_error(e.to_string()))
    }

    /// Rebuild this layer's MRT object from updated parameters.
    fn to_value(&self, params: &[Tensor]) -> Value {
        let mut map = ObjMap::new();
        let kind = match self {
            Layer::Dense { .. } => "dense",
            Layer::Conv2d { .. } => "conv2d",
            Layer::Attention { .. } => "attention",
            Layer::LayerNorm { .. } => "layerNorm",
            Layer::Relu => "relu",
            Layer::Sigmoid => "sigmoid",
            Layer::Tanh => "tanh",
            Layer::Softmax => "softmax",
        };
        map.insert(key("kind"), Value::str(kind));
        match self {
            Layer::Conv2d {
                channels,
                height,
                width,
                kernel_h,
                kernel_w,
                stride,
                filters,
                ..
            } => {
                map.insert(key("channels"), Value::Number(*channels as f64));
                map.insert(key("height"), Value::Number(*height as f64));
                map.insert(key("width"), Value::Number(*width as f64));
                map.insert(key("kernelH"), Value::Number(*kernel_h as f64));
                map.insert(key("kernelW"), Value::Number(*kernel_w as f64));
                map.insert(key("stride"), Value::Number(*stride as f64));
                map.insert(key("filters"), Value::Number(*filters as f64));
                map.insert(key("weights"), tensor_to_mrt(&params[0]));
                map.insert(key("bias"), tensor_to_mrt(&params[1]));
            }
            Layer::Dense { .. } => {
                map.insert(key("weights"), tensor_to_mrt(&params[0]));
                map.insert(key("bias"), tensor_to_mrt(&params[1]));
            }
            Layer::LayerNorm { .. } => {
                map.insert(key("gain"), tensor_to_mrt(&params[0]));
                map.insert(key("shift"), tensor_to_mrt(&params[1]));
            }
            Layer::Attention { .. } => {
                map.insert(key("query"), tensor_to_mrt(&params[0]));
                map.insert(key("key"), tensor_to_mrt(&params[1]));
                map.insert(key("value"), tensor_to_mrt(&params[2]));
            }
            Layer::Relu | Layer::Sigmoid | Layer::Tanh | Layer::Softmax => {}
        }
        Value::object(map)
    }
}

fn key(name: &str) -> ObjKey {
    ObjKey::Str(Rc::from(name))
}

/// Read a model: an array of layer objects.
fn read_model(value: &Value, who: &str) -> Result<Vec<Layer>, Signal> {
    let Value::Array(layers) = value else {
        return Err(type_error(format!(
            "{who}() expects a model, which is an array of layers."
        )));
    };
    layers
        .borrow()
        .iter()
        .enumerate()
        .map(|(index, layer)| read_layer(layer, index, who))
        .collect()
}

fn read_layer(value: &Value, index: usize, who: &str) -> Result<Layer, Signal> {
    let Value::Object(map) = value else {
        return Err(type_error(format!(
            "{who}(): layer {index} is not an object."
        )));
    };
    let map = map.borrow();
    let Some(Value::Str(kind)) = map.get(&key("kind")) else {
        return Err(value_error(format!(
            "{who}(): layer {index} has no 'kind'."
        )));
    };
    let field = |name: &str| -> Result<Tensor, Signal> {
        match map.get(&key(name)) {
            Some(v) => tensor_from_mrt(v, who),
            None => Err(value_error(format!(
                "{who}(): a {kind} layer needs '{name}'."
            ))),
        }
    };
    let count = |name: &str| -> Result<usize, Signal> {
        match map.get(&key(name)) {
            Some(Value::Number(n)) if *n >= 0.0 && n.fract() == 0.0 => Ok(*n as usize),
            _ => Err(value_error(format!(
                "{who}(): a {kind} layer needs '{name}' as a whole number."
            ))),
        }
    };

    match &**kind {
        "dense" => Ok(Layer::Dense {
            weights: field("weights")?,
            bias: field("bias")?,
        }),
        "conv2d" => Ok(Layer::Conv2d {
            channels: count("channels")?,
            height: count("height")?,
            width: count("width")?,
            kernel_h: count("kernelH")?,
            kernel_w: count("kernelW")?,
            stride: count("stride")?,
            filters: count("filters")?,
            weights: field("weights")?,
            bias: field("bias")?,
        }),
        "attention" => Ok(Layer::Attention {
            query: field("query")?,
            key: field("key")?,
            value: field("value")?,
        }),
        "layerNorm" => Ok(Layer::LayerNorm {
            gain: field("gain")?,
            shift: field("shift")?,
        }),
        "relu" => Ok(Layer::Relu),
        "sigmoid" => Ok(Layer::Sigmoid),
        "tanh" => Ok(Layer::Tanh),
        "softmax" => Ok(Layer::Softmax),
        other => Err(value_error(format!(
            "{who}(): layer {index} has an unknown kind {other:?}."
        ))),
    }
}

/// Build a dense layer's MRT object, with weights spread deterministically.
///
/// A seed rather than a hidden global generator: two runs of the same program
/// have to produce the same model, or nothing about training is reproducible
/// and no test of it means anything.
pub fn dense(inputs: usize, outputs: usize, seed: u64) -> Value {
    let layer = Dense::new(inputs, outputs, seed);
    Layer::Dense {
        weights: layer.weights.clone(),
        bias: layer.bias.clone(),
    }
    .to_value(&[layer.weights, layer.bias])
}

/// Build a convolution layer's MRT object.
#[allow(clippy::too_many_arguments)]
pub fn conv2d(
    channels: usize,
    height: usize,
    width: usize,
    kernel: usize,
    stride: usize,
    filters: usize,
    seed: u64,
) -> Result<Value, Signal> {
    let window = Window {
        // A placeholder: the real batch comes from the input at forward time,
        // so a model is not tied to the batch it was built with.
        batch: 1,
        channels,
        height,
        width,
        kernel_h: kernel,
        kernel_w: kernel,
        stride,
    };
    if kernel == 0 || stride == 0 || kernel > height || kernel > width {
        return Err(value_error(format!(
            "aiConv2d(): a {kernel}x{kernel} kernel with stride {stride} does not fit a {height}x{width} image."
        )));
    }
    let layer = Conv2d::new(window, filters, seed);
    Ok(Layer::Conv2d {
        channels,
        height,
        width,
        kernel_h: kernel,
        kernel_w: kernel,
        stride,
        filters,
        weights: layer.weights.clone(),
        bias: layer.bias.clone(),
    }
    .to_value(&[layer.weights, layer.bias]))
}

/// Build a layer-norm layer's MRT object.
pub fn layer_norm(features: usize) -> Value {
    let layer = LayerNorm::new(features);
    Layer::LayerNorm {
        gain: layer.gain.clone(),
        shift: layer.shift.clone(),
    }
    .to_value(&[layer.gain, layer.shift])
}

/// Build a single-head attention layer's MRT object.
pub fn attention(features: usize, head: usize, seed: u64) -> Value {
    let layer = Attention::new(features, head, seed);
    Layer::Attention {
        query: layer.query.clone(),
        key: layer.key.clone(),
        value: layer.value.clone(),
    }
    .to_value(&[layer.query, layer.key, layer.value])
}

/// An activation layer, which has no parameters at all.
pub fn activation(kind: &str) -> Result<Value, Signal> {
    let layer = match kind {
        "relu" => Layer::Relu,
        "sigmoid" => Layer::Sigmoid,
        "tanh" => Layer::Tanh,
        "softmax" => Layer::Softmax,
        other => {
            return Err(value_error(format!(
                "aiActivation(): unknown activation {other:?}; expected \"relu\", \"sigmoid\", \"tanh\" or \"softmax\"."
            )))
        }
    };
    Ok(layer.to_value(&[]))
}

/// Run a model forward and return its output as nested arrays.
pub fn predict(model: &Value, input: &Value) -> Result<Value, Signal> {
    let layers = read_model(model, "aiPredict")?;
    let x = tensor_from_mrt(input, "aiPredict")?;
    let mut tape = Tape::new();
    let out = run(&mut tape, &layers, x)?;
    Ok(tensor_to_mrt(tape.value(out)))
}

/// Put every layer's parameters on the tape, run the model, and hand back the
/// output. The parameter order is layer by layer, which is what lets the
/// optimiser and the rebuilt model agree.
fn run(tape: &mut Tape, layers: &[Layer], x: Tensor) -> Result<Var, Signal> {
    let mut vars = Vec::new();
    for layer in layers {
        for parameter in layer.params() {
            vars.push(tape.param(parameter));
        }
    }
    let mut current = tape.input(x);
    let mut at = 0;
    for layer in layers {
        let arity = layer.arity();
        current = layer.forward(tape, current, &vars[at..at + arity])?;
        at += arity;
    }
    Ok(current)
}

/// Train a model and hand back a new one.
///
/// A *new* model rather than a mutated one, because MRT objects are shared
/// handles: mutating in place would change a model someone else is still
/// holding, and "train returns the trained model" is the reading that does not
/// surprise anyone.
pub fn train(
    model: &Value,
    input: &Value,
    target: &Value,
    epochs: usize,
    rate: f64,
) -> Result<Value, Signal> {
    let layers = read_model(model, "aiTrain")?;
    let x = tensor_from_mrt(input, "aiTrain")?;
    let y = tensor_from_mrt(target, "aiTrain")?;
    if x.rows != y.rows {
        return Err(value_error(format!(
            "aiTrain(): {} inputs but {} targets.",
            x.rows, y.rows
        )));
    }
    if epochs == 0 {
        return Err(value_error("aiTrain(): epochs must be greater than zero."));
    }
    if rate <= 0.0 {
        return Err(value_error(
            "aiTrain(): the learning rate must be greater than zero.",
        ));
    }

    let mut parameters: Vec<Tensor> = layers.iter().flat_map(|l| l.params()).collect();
    let mut optimizer = Adam::new(rate);
    let mut history = Vec::with_capacity(epochs);

    for _ in 0..epochs {
        let mut tape = Tape::new();
        let mut vars = Vec::new();
        for parameter in &parameters {
            vars.push(tape.param(parameter.clone()));
        }
        let mut current = tape.input(x.clone());
        let mut at = 0;
        for layer in &layers {
            let arity = layer.arity();
            current = layer.forward(&mut tape, current, &vars[at..at + arity])?;
            at += arity;
        }
        let expected = tape.input(y.clone());
        let loss = tape
            .mse(current, expected)
            .map_err(|e| value_error(e.to_string()))?;
        history.push(tape.value(loss).data[0]);
        let grads = tape
            .backward(loss)
            .map_err(|e| value_error(e.to_string()))?;
        let step: Vec<Tensor> = vars.iter().map(|v| grads.get(*v).clone()).collect();
        optimizer
            .step(&mut parameters, &step)
            .map_err(|e| value_error(e.to_string()))?;
    }

    // Rebuild the model from the trained parameters, layer by layer, in the
    // same order they were laid out.
    let mut rebuilt = Vec::with_capacity(layers.len());
    let mut at = 0;
    for layer in &layers {
        let arity = layer.arity();
        rebuilt.push(layer.to_value(&parameters[at..at + arity]));
        at += arity;
    }

    let mut result = ObjMap::new();
    result.insert(key("model"), Value::array(rebuilt));
    result.insert(
        key("loss"),
        Value::Number(*history.last().expect("at least one epoch")),
    );
    result.insert(key("initialLoss"), Value::Number(history[0]));
    result.insert(
        key("history"),
        Value::array(history.into_iter().map(Value::Number).collect()),
    );
    Ok(Value::object(result))
}

/// Nested arrays to a tensor: `[[1, 2], [3, 4]]` is two rows of two.
pub fn tensor_from_mrt(value: &Value, who: &str) -> Result<Tensor, Signal> {
    let Value::Array(rows) = value else {
        return Err(type_error(format!("{who}() expects a 2D array.")));
    };
    let rows = rows.borrow();
    if rows.is_empty() {
        return Err(value_error(format!("{who}() cannot use an empty tensor.")));
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
        match cols {
            Some(expected) if expected != items.len() => {
                return Err(value_error(format!(
                    "{who}() rows must have equal lengths."
                )));
            }
            Some(_) => {}
            None => cols = Some(items.len()),
        }
        for item in items.iter() {
            let Value::Number(n) = item else {
                return Err(type_error(format!("{who}() expects numbers.")));
            };
            data.push(*n);
        }
    }
    Tensor::from_vec(rows.len(), cols.expect("a non-empty tensor"), data)
        .map_err(|e| value_error(e.to_string()))
}

pub fn tensor_to_mrt(t: &Tensor) -> Value {
    Value::array(
        (0..t.rows)
            .map(|r| Value::array((0..t.cols).map(|c| Value::Number(t.get(r, c))).collect()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use crate::{run, run_vm, SourceFile};

    /// Run a program on both engines, insist they agree, and return what it
    /// printed -- with any error appended, so a test can assert on a message
    /// as easily as on output.
    ///
    /// Both engines, because these builtins are the one place where a caller
    /// reaches Rust through two different paths: an extension that worked on
    /// the tree-walker and not the VM would be found by nobody, since the
    /// conformance corpus deliberately does not cover it.
    fn go(source: &str) -> String {
        let file = SourceFile::new("test.mrt", source);
        let text = |outcome: crate::Outcome| {
            let mut lines = outcome.output;
            if let Some(error) = outcome.error {
                lines.push(error);
            }
            lines.join("\n")
        };
        let walked = text(run(&file));
        let compiled = text(run_vm(&file));
        assert_eq!(walked, compiled, "the engines disagree");
        walked
    }

    fn main_of(body: &str) -> String {
        go(&format!("func main() {{ {body} }}"))
    }

    #[test]
    fn a_model_is_data_the_language_can_look_inside() {
        // The whole reason a model is not an opaque handle: everything about
        // it is reachable with the array and object syntax already in the
        // language.
        assert_eq!(
            main_of(
                r#"var d = aiDense(2, 3, 1);
                   print(d.kind, len(d.weights), len(d.weights[0]), len(d.bias), len(d.bias[0]));"#
            ),
            "dense 2 3 1 3"
        );
        assert_eq!(
            main_of(r#"print(aiActivation("relu"));"#),
            "{kind: relu}",
            "an activation carries nothing but its name"
        );
    }

    #[test]
    fn a_model_survives_a_round_trip_through_the_language() {
        // Read out, put back, still usable: predictions from a model that has
        // been through MRT values are the same as from the one just built.
        assert_eq!(
            main_of(
                r#"var m = [aiDense(2, 1, 3)];
                   var direct = aiPredict(m, [[1, 2]]);
                   var copy = [{kind: "dense", weights: m[0].weights, bias: m[0].bias}];
                   print(aiPredict(copy, [[1, 2]])[0][0] == direct[0][0]);"#
            ),
            "true"
        );
    }

    #[test]
    fn training_learns_a_problem_no_single_layer_can() {
        // XOR is not linearly separable, so a loss that falls this far is
        // evidence the hidden layer and its gradients are both doing work --
        // a broken backward pass would leave it stuck near the 0.25 a
        // constant guess gets.
        assert_eq!(
            main_of(
                r#"var m = [aiDense(2, 4, 7), aiActivation("tanh"), aiDense(4, 1, 8), aiActivation("sigmoid")];
                   var t = aiTrain(m, [[0, 0], [0, 1], [1, 0], [1, 1]], [[0], [1], [1], [0]], 400, 0.1);
                   print(t.initialLoss > 0.2, t.loss < 0.001, len(t.history));
                   var p = aiPredict(t.model, [[0, 0], [0, 1], [1, 0], [1, 1]]);
                   print(round(p[0][0]), round(p[1][0]), round(p[2][0]), round(p[3][0]));"#
            ),
            "true true 400\n0 1 1 0"
        );
    }

    #[test]
    fn training_hands_back_a_new_model_and_leaves_the_old_one_alone() {
        // MRT objects are shared handles, so training in place would rewrite
        // a model its caller is still holding.
        assert_eq!(
            main_of(
                r#"var m = [aiDense(1, 1, 5)];
                   var before = m[0].weights[0][0];
                   var t = aiTrain(m, [[1], [2]], [[3], [5]], 50, 0.1);
                   print(m[0].weights[0][0] == before, t.model[0].weights[0][0] == before);"#
            ),
            "true false"
        );
    }

    #[test]
    fn every_layer_kind_runs_end_to_end() {
        // A 2x2 single-channel image through a convolution, then the shapes
        // the rest of the kinds expect.
        assert_eq!(
            main_of(
                r#"var m = [aiConv2d(1, 2, 2, 2, 1, 3, 4), aiActivation("relu")];
                   var out = aiPredict(m, [[1, 2, 3, 4]]);
                   print(len(out), len(out[0]));"#
            ),
            "1 3",
            "one image, one window, three filters"
        );
        assert_eq!(
            main_of(
                r#"var m = [aiAttention(4, 2, 9), aiLayerNorm(2), aiActivation("softmax")];
                   var out = aiPredict(m, [[1, 0, 0, 1], [0, 1, 1, 0]]);
                   print(len(out), len(out[0]), round((out[0][0] + out[0][1]) * 1000) / 1000);"#
            ),
            "2 2 1",
            "softmax rows sum to one"
        );
    }

    #[test]
    fn a_malformed_model_says_what_is_wrong_with_it() {
        assert_eq!(
            main_of("aiPredict(5, [[1]]);"),
            "Runtime Error: aiPredict() expects a model, which is an array of layers. [line 1]"
        );
        assert_eq!(
            main_of("aiPredict([1], [[1]]);"),
            "Runtime Error: aiPredict(): layer 0 is not an object. [line 1]"
        );
        assert_eq!(
            main_of("aiPredict([{}], [[1]]);"),
            "Runtime Error: aiPredict(): layer 0 has no 'kind'. [line 1]"
        );
        assert_eq!(
            main_of(r#"aiPredict([{kind: "linear"}], [[1]]);"#),
            "Runtime Error: aiPredict(): layer 0 has an unknown kind \"linear\". [line 1]"
        );
        assert_eq!(
            main_of(r#"aiPredict([{kind: "dense", weights: [[1]]}], [[1]]);"#),
            "Runtime Error: aiPredict(): a dense layer needs 'bias'. [line 1]"
        );
    }

    #[test]
    fn the_extension_classifies_its_errors_like_the_rest_of_the_language() {
        // The top line of a crash says "Runtime Error" for everything; the
        // kind is what a program branches on. An extension that threw
        // uncategorised errors would be catchable only by catching all of
        // them.
        assert_eq!(
            main_of(
                r#"try { aiPredict(5, [[1]]); } catch (e) { print(e.kind); }
                   try { aiDense(2.5, 1, 0); } catch (e) { print(e.kind); }"#
            ),
            "TypeError\nValueError"
        );
    }

    #[test]
    fn sizes_have_to_be_counts() {
        // Rounding 2.5 layers to 2 would train a model the program never
        // asked for, and report no problem at all.
        assert_eq!(
            main_of("aiDense(2.5, 1, 0);"),
            "Runtime Error: aiDense() needs inputs to be a whole number that is not negative, not 2.5. [line 1]"
        );
        assert_eq!(
            main_of("aiDense(-1, 1, 0);"),
            "Runtime Error: aiDense() needs inputs to be a whole number that is not negative, not -1. [line 1]"
        );
        assert_eq!(
            main_of(r#"aiDense("2", 1, 0);"#),
            "Runtime Error: aiDense() needs inputs to be a number, not string. [line 1]"
        );
        assert_eq!(
            main_of("aiConv2d(1, 2, 2, 3, 1, 1, 0);"),
            "Runtime Error: aiConv2d(): a 3x3 kernel with stride 1 does not fit a 2x2 image. [line 1]"
        );
    }

    #[test]
    fn training_rejects_arguments_that_cannot_mean_anything() {
        assert_eq!(
            main_of("aiTrain([aiDense(1, 1, 0)], [[1], [2]], [[1]], 10, 0.1);"),
            "Runtime Error: aiTrain(): 2 inputs but 1 targets. [line 1]"
        );
        assert_eq!(
            main_of("aiTrain([aiDense(1, 1, 0)], [[1]], [[1]], 0, 0.1);"),
            "Runtime Error: aiTrain(): epochs must be greater than zero. [line 1]"
        );
        assert_eq!(
            main_of("aiTrain([aiDense(1, 1, 0)], [[1]], [[1]], 10, 0);"),
            "Runtime Error: aiTrain(): the learning rate must be greater than zero. [line 1]"
        );
    }

    #[test]
    fn a_shape_the_model_cannot_take_is_reported_not_ignored() {
        assert_eq!(
            main_of("aiPredict([aiDense(3, 1, 0)], [[1, 2]]);"),
            "Runtime Error: shape error: cannot multiply (1, 2) by (3, 1) [line 1]"
        );
    }
}
