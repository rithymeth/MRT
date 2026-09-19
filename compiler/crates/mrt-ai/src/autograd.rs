//! Reverse-mode automatic differentiation.
//!
//! Every layer beyond the simplest needs a backward pass, and writing them by
//! hand does not scale: a convolution, an attention head and a layer norm each
//! require a derivative derived on paper, transcribed, and then *verified* --
//! and a wrong gradient does not crash. It trains slightly worse, which is the
//! most expensive kind of bug to find, because the symptom is a number that
//! looks plausible.
//!
//! So the derivatives are computed rather than written. A `Tape` records each
//! operation as it happens, along with which earlier values it consumed; a
//! backward pass walks that recording in reverse, and each operation
//! contributes only its own local rule. Adding a layer then means writing its
//! *forward* pass and nothing else, which is the whole argument for building
//! this before the layers rather than after them.
//!
//! -- Why a tape of indices --
//!
//! The obvious shape, a graph of nodes holding references to their parents,
//! fights Rust's ownership rules: the graph is cyclic in practice (a value
//! feeds several operations) and every node would need `Rc<RefCell<..>>`. A
//! tape sidesteps it. Nodes live in one `Vec`, parents are indices into it,
//! and an index recorded when a node was pushed necessarily points backwards
//! -- so a single reverse scan visits every node after all of its consumers,
//! which is exactly the order the chain rule needs. No topological sort, no
//! reference counting, no cycles possible by construction.
//!
//! -- What is and is not differentiable here --
//!
//! Only what the tape records. A tensor pulled out with `value()` and put back
//! with `input()` has been detached: the chain stops there. That is the
//! standard escape hatch and it is deliberate, but it is silent, which is why
//! it is said out loud here.

use crate::{AiError, Tensor};

/// A handle to a value on the tape.
///
/// Deliberately not `Copy`-cheap-and-anonymous: a `Var` is only meaningful
/// against the tape that produced it, and mixing two tapes' handles would read
/// the wrong node. Every method takes `&mut self` on the tape, so the borrow
/// checker makes that hard to do by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Var(usize);

/// How a value was produced, and from what.
///
/// Each variant carries the indices it consumed, which is all the backward
/// pass needs: the values themselves are already on the tape.
#[derive(Debug)]
enum Op {
    /// A value with no parents: an input, or a trainable parameter.
    Leaf {
        trainable: bool,
    },
    Add(Var, Var),
    Sub(Var, Var),
    /// Elementwise, not matrix multiplication.
    Mul(Var, Var),
    MatMul(Var, Var),
    Scale(Var, f64),
    /// `rows x cols` plus a `1 x cols` bias, broadcast down the rows.
    AddBias(Var, Var),
    Relu(Var),
    Sigmoid(Var),
    Tanh(Var),
    Exp(Var),
    Transpose(Var),
    /// Row-wise softmax, which is what attention needs.
    Softmax(Var),
    /// Every element summed into a 1x1 value.
    Sum(Var),
    /// Every element averaged into a 1x1 value.
    Mean(Var),
    /// The same values under a different shape. A view, so its backward is
    /// the reverse view.
    Reshape(Var, usize, usize),
    /// Lay each convolution window out as a row, so that a convolution is a
    /// matrix multiply. See `Tape::im2col`.
    Im2Col(Var, Window),
    /// Reorder a convolution's output from one-row-per-window to
    /// one-row-per-image with the channels laid out in planes. See
    /// `Tape::planar`. Carries `(batch, spatial, channels)`.
    Planar(Var, usize, usize, usize),
}

/// How a convolution window moves over an image.
///
/// Held on the tape rather than passed around because the backward pass needs
/// exactly the same geometry the forward pass used: a gradient scattered back
/// through a different stride would be silently wrong rather than wrong in a
/// way that shows.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub batch: usize,
    pub channels: usize,
    pub height: usize,
    pub width: usize,
    pub kernel_h: usize,
    pub kernel_w: usize,
    pub stride: usize,
}

impl Window {
    /// The output height and width this window produces. No padding: an
    /// image that does not divide evenly simply yields fewer positions, which
    /// is the convention every other part of this crate would have to agree
    /// with before padding is worth adding.
    pub fn output(&self) -> (usize, usize) {
        (
            (self.height - self.kernel_h) / self.stride + 1,
            (self.width - self.kernel_w) / self.stride + 1,
        )
    }

    /// How many values one window covers: the width of an im2col row.
    pub fn patch(&self) -> usize {
        self.channels * self.kernel_h * self.kernel_w
    }

    fn fits(&self) -> bool {
        self.kernel_h <= self.height && self.kernel_w <= self.width && self.stride > 0
    }
}

struct Node {
    value: Tensor,
    op: Op,
}

/// A recording of a computation, and the means to differentiate it.
#[derive(Default)]
pub struct Tape {
    nodes: Vec<Node>,
}

impl Tape {
    pub fn new() -> Tape {
        Tape::default()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The tensor a handle refers to.
    pub fn value(&self, var: Var) -> &Tensor {
        &self.nodes[var.0].value
    }

    /// Data flowing in: differentiated *through*, but nothing asks for its
    /// gradient.
    pub fn input(&mut self, value: Tensor) -> Var {
        self.push(value, Op::Leaf { trainable: false })
    }

    /// A weight: a leaf whose gradient is the point of the whole exercise.
    pub fn param(&mut self, value: Tensor) -> Var {
        self.push(value, Op::Leaf { trainable: true })
    }

    fn push(&mut self, value: Tensor, op: Op) -> Var {
        self.nodes.push(Node { value, op });
        Var(self.nodes.len() - 1)
    }

    // -- forward operations ------------------------------------------------
    //
    // Each one computes a value and records how it got there. None of them
    // knows anything about differentiation; that lives in `backward`, once per
    // operation rather than once per use.

    pub fn add(&mut self, a: Var, b: Var) -> Result<Var, AiError> {
        let value = self.value(a).add(self.value(b))?;
        Ok(self.push(value, Op::Add(a, b)))
    }

    pub fn sub(&mut self, a: Var, b: Var) -> Result<Var, AiError> {
        let value = self.value(a).sub(self.value(b))?;
        Ok(self.push(value, Op::Sub(a, b)))
    }

    /// Elementwise product. `matmul` is the other one.
    pub fn mul(&mut self, a: Var, b: Var) -> Result<Var, AiError> {
        let (x, y) = (self.value(a), self.value(b));
        if x.rows != y.rows || x.cols != y.cols {
            return Err(AiError::Shape("tensor shapes must match".into()));
        }
        let data = x
            .data
            .iter()
            .zip(y.data.iter())
            .map(|(p, q)| p * q)
            .collect();
        let value = Tensor::from_vec(x.rows, x.cols, data)?;
        Ok(self.push(value, Op::Mul(a, b)))
    }

    pub fn matmul(&mut self, a: Var, b: Var) -> Result<Var, AiError> {
        let value = self.value(a).matmul(self.value(b))?;
        Ok(self.push(value, Op::MatMul(a, b)))
    }

    pub fn scale(&mut self, a: Var, factor: f64) -> Result<Var, AiError> {
        let value = self.value(a).scale(factor);
        Ok(self.push(value, Op::Scale(a, factor)))
    }

    /// Add a `1 x cols` bias row to every row of a `rows x cols` value.
    ///
    /// Broadcasting is its own operation rather than a reshape plus `add`,
    /// because its backward rule is the interesting one: the bias took part in
    /// every row, so its gradient is the sum down the columns.
    pub fn add_bias(&mut self, a: Var, bias: Var) -> Result<Var, AiError> {
        let (x, b) = (self.value(a), self.value(bias));
        if b.rows != 1 || b.cols != x.cols {
            return Err(AiError::Shape(format!(
                "bias must be (1, {}), got ({}, {})",
                x.cols, b.rows, b.cols
            )));
        }
        let mut value = x.clone();
        for row in 0..value.rows {
            for col in 0..value.cols {
                value.data[row * value.cols + col] += b.data[col];
            }
        }
        Ok(self.push(value, Op::AddBias(a, bias)))
    }

    pub fn relu(&mut self, a: Var) -> Result<Var, AiError> {
        let value = self.value(a).map(|v| if v > 0.0 { v } else { 0.0 });
        Ok(self.push(value, Op::Relu(a)))
    }

    pub fn sigmoid(&mut self, a: Var) -> Result<Var, AiError> {
        let value = self.value(a).map(|v| 1.0 / (1.0 + (-v).exp()));
        Ok(self.push(value, Op::Sigmoid(a)))
    }

    pub fn tanh(&mut self, a: Var) -> Result<Var, AiError> {
        let value = self.value(a).map(|v| v.tanh());
        Ok(self.push(value, Op::Tanh(a)))
    }

    pub fn exp(&mut self, a: Var) -> Result<Var, AiError> {
        let value = self.value(a).map(|v| v.exp());
        Ok(self.push(value, Op::Exp(a)))
    }

    pub fn transpose(&mut self, a: Var) -> Result<Var, AiError> {
        let value = self.value(a).transpose();
        Ok(self.push(value, Op::Transpose(a)))
    }

    /// Row-wise softmax.
    ///
    /// The maximum is subtracted before exponentiating. That is not a
    /// numerical nicety: attention scores reach magnitudes where `exp`
    /// overflows to infinity, and the result is exactly equal either way
    /// because the constant cancels in the ratio.
    pub fn softmax(&mut self, a: Var) -> Result<Var, AiError> {
        let x = self.value(a);
        let mut value = Tensor::zeros(x.rows, x.cols);
        for row in 0..x.rows {
            let start = row * x.cols;
            let slice = &x.data[start..start + x.cols];
            let max = slice.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let exps: Vec<f64> = slice.iter().map(|v| (v - max).exp()).collect();
            let total: f64 = exps.iter().sum();
            for (col, e) in exps.iter().enumerate() {
                value.data[start + col] = e / total;
            }
        }
        Ok(self.push(value, Op::Softmax(a)))
    }

    /// Every element summed into a 1x1 value.
    pub fn sum(&mut self, a: Var) -> Result<Var, AiError> {
        let total: f64 = self.value(a).data.iter().sum();
        let value = Tensor::from_vec(1, 1, vec![total])?;
        Ok(self.push(value, Op::Sum(a)))
    }

    /// Every element averaged into a 1x1 value.
    pub fn mean(&mut self, a: Var) -> Result<Var, AiError> {
        let x = self.value(a);
        let n = x.data.len();
        if n == 0 {
            return Err(AiError::Shape(
                "cannot take the mean of an empty tensor".into(),
            ));
        }
        let value = Tensor::from_vec(1, 1, vec![x.data.iter().sum::<f64>() / n as f64])?;
        Ok(self.push(value, Op::Mean(a)))
    }

    /// The same values under a different shape.
    pub fn reshape(&mut self, a: Var, rows: usize, cols: usize) -> Result<Var, AiError> {
        let x = self.value(a);
        if rows * cols != x.data.len() {
            return Err(AiError::Shape(format!(
                "cannot reshape {} values into ({rows}, {cols})",
                x.data.len()
            )));
        }
        let value = Tensor::from_vec(rows, cols, x.data.clone())?;
        Ok(self.push(value, Op::Reshape(a, x.rows, x.cols)))
    }

    /// Lay every convolution window out as a row.
    ///
    /// This is what makes a convolution cheap to add: once the windows are
    /// rows, convolving *is* `matmul`, and the gradient of the whole layer
    /// falls out of the rules already here. The only genuinely new derivative
    /// is this one, and it is the reverse of the copy the forward pass does --
    /// scatter the gradients back to the pixels each window took, adding where
    /// windows overlap. Overlap is the part worth stating: a pixel covered by
    /// four windows receives four contributions, and summing them is not an
    /// optimisation but the chain rule.
    ///
    /// Input is `(batch, channels * height * width)`, one image per row.
    /// Output is `(batch * out_h * out_w, channels * kernel_h * kernel_w)`.
    pub fn im2col(&mut self, a: Var, window: Window) -> Result<Var, AiError> {
        if !window.fits() {
            return Err(AiError::Shape(format!(
                "a {}x{} kernel with stride {} does not fit a {}x{} image",
                window.kernel_h, window.kernel_w, window.stride, window.height, window.width
            )));
        }
        let x = self.value(a);
        let image = window.channels * window.height * window.width;
        if x.rows != window.batch || x.cols != image {
            return Err(AiError::Shape(format!(
                "im2col expects ({}, {image}), got ({}, {})",
                window.batch, x.rows, x.cols
            )));
        }

        let (out_h, out_w) = window.output();
        let patch = window.patch();
        let mut value = Tensor::zeros(window.batch * out_h * out_w, patch);
        for n in 0..window.batch {
            for oy in 0..out_h {
                for ox in 0..out_w {
                    let row = (n * out_h + oy) * out_w + ox;
                    for c in 0..window.channels {
                        for ky in 0..window.kernel_h {
                            for kx in 0..window.kernel_w {
                                let iy = oy * window.stride + ky;
                                let ix = ox * window.stride + kx;
                                let col = (c * window.kernel_h + ky) * window.kernel_w + kx;
                                let at = c * window.height * window.width + iy * window.width + ix;
                                value.data[row * patch + col] = x.data[n * image + at];
                            }
                        }
                    }
                }
            }
        }
        Ok(self.push(value, Op::Im2Col(a, window)))
    }

    /// Turn a convolution's output into the layout its input arrived in.
    ///
    /// `matmul` over `im2col` rows produces `(batch * positions, channels)`:
    /// one row per window position, one column per filter. Flattening that
    /// directly gives a channel per position -- interleaved -- while `im2col`
    /// reads channels as *planes*, one whole image per channel. So a second
    /// convolution fed the first one's output would read pixels of different
    /// filters as neighbouring pixels of one filter, quietly, and train to
    /// something meaningless rather than fail.
    ///
    /// This is the permutation that makes them agree, and therefore the reason
    /// convolutions can be stacked at all. Its backward is the inverse
    /// permutation: no value is combined or dropped, so every gradient goes
    /// back to exactly one place.
    ///
    /// `(batch * spatial, channels)` becomes `(batch, channels * spatial)`.
    pub fn planar(
        &mut self,
        a: Var,
        batch: usize,
        spatial: usize,
        channels: usize,
    ) -> Result<Var, AiError> {
        let x = self.value(a);
        if x.rows != batch * spatial || x.cols != channels {
            return Err(AiError::Shape(format!(
                "planar expects ({}, {channels}), got ({}, {})",
                batch * spatial,
                x.rows,
                x.cols
            )));
        }
        let mut value = Tensor::zeros(batch, channels * spatial);
        for n in 0..batch {
            for s in 0..spatial {
                for c in 0..channels {
                    value.data[n * channels * spatial + c * spatial + s] =
                        x.data[(n * spatial + s) * channels + c];
                }
            }
        }
        Ok(self.push(value, Op::Planar(a, batch, spatial, channels)))
    }

    /// Mean squared error, as a composition rather than a primitive.
    ///
    /// Built from `sub`, `mul` and `mean`, so it needs no backward rule of its
    /// own and cannot disagree with the three it is made of. Every loss should
    /// be written this way until one is measurably too slow.
    pub fn mse(&mut self, prediction: Var, target: Var) -> Result<Var, AiError> {
        let diff = self.sub(prediction, target)?;
        let squared = self.mul(diff, diff)?;
        self.mean(squared)
    }

    // -- backward ----------------------------------------------------------

    /// Gradients of `output` with respect to every node that fed it.
    ///
    /// `output` must be a single value -- a loss. A gradient is only defined
    /// against a scalar, and rejecting this here is much kinder than the
    /// silently-wrong answer that seeding a matrix with ones would give.
    pub fn backward(&self, output: Var) -> Result<Grads, AiError> {
        let value = self.value(output);
        if value.rows != 1 || value.cols != 1 {
            return Err(AiError::Shape(format!(
                "backward() needs a single value to differentiate, got ({}, {})",
                value.rows, value.cols
            )));
        }

        // One accumulator per node, same shape as the node's value.
        let mut grads: Vec<Tensor> = self
            .nodes
            .iter()
            .map(|n| Tensor::zeros(n.value.rows, n.value.cols))
            .collect();
        grads[output.0].data[0] = 1.0;

        // In reverse: an index recorded when a node was pushed points strictly
        // backwards, so by the time this reaches a node every consumer of it
        // has already contributed. That property is why the tape needs no
        // topological sort.
        for i in (0..self.nodes.len()).rev() {
            let g = grads[i].clone();
            if g.data.iter().all(|v| *v == 0.0) {
                // Nothing downstream used this value. Skipping is not just an
                // optimisation: it keeps a detached subgraph from contributing.
                continue;
            }
            match &self.nodes[i].op {
                Op::Leaf { .. } => {}
                Op::Add(a, b) => {
                    accumulate(&mut grads, *a, &g);
                    accumulate(&mut grads, *b, &g);
                }
                Op::Sub(a, b) => {
                    accumulate(&mut grads, *a, &g);
                    accumulate(&mut grads, *b, &g.scale(-1.0));
                }
                Op::Mul(a, b) => {
                    // d(ab) = b da + a db, elementwise.
                    let (x, y) = (&self.nodes[a.0].value, &self.nodes[b.0].value);
                    accumulate(&mut grads, *a, &elementwise(&g, y));
                    accumulate(&mut grads, *b, &elementwise(&g, x));
                }
                Op::MatMul(a, b) => {
                    // For C = A @ B: dA = G @ B^T and dB = A^T @ G. The
                    // transposes are what make the shapes work out, and that
                    // is also the check that the rule is the right way round.
                    let (x, y) = (&self.nodes[a.0].value, &self.nodes[b.0].value);
                    accumulate(&mut grads, *a, &g.matmul(&y.transpose())?);
                    accumulate(&mut grads, *b, &x.transpose().matmul(&g)?);
                }
                Op::Scale(a, factor) => {
                    accumulate(&mut grads, *a, &g.scale(*factor));
                }
                Op::AddBias(a, bias) => {
                    accumulate(&mut grads, *a, &g);
                    // The bias took part in every row, so its gradient is the
                    // sum down the columns -- the one place broadcasting stops
                    // being a reshape and becomes a real operation.
                    let mut summed = Tensor::zeros(1, g.cols);
                    for row in 0..g.rows {
                        for col in 0..g.cols {
                            summed.data[col] += g.data[row * g.cols + col];
                        }
                    }
                    accumulate(&mut grads, *bias, &summed);
                }
                Op::Relu(a) => {
                    let x = &self.nodes[a.0].value;
                    let gated = Tensor {
                        data: g
                            .data
                            .iter()
                            .zip(x.data.iter())
                            .map(|(d, v)| if *v > 0.0 { *d } else { 0.0 })
                            .collect(),
                        rows: g.rows,
                        cols: g.cols,
                    };
                    accumulate(&mut grads, *a, &gated);
                }
                Op::Sigmoid(a) => {
                    // s' = s(1 - s), using the output rather than recomputing.
                    let s = &self.nodes[i].value;
                    let d = Tensor {
                        data: g
                            .data
                            .iter()
                            .zip(s.data.iter())
                            .map(|(d, v)| d * v * (1.0 - v))
                            .collect(),
                        rows: g.rows,
                        cols: g.cols,
                    };
                    accumulate(&mut grads, *a, &d);
                }
                Op::Tanh(a) => {
                    let t = &self.nodes[i].value;
                    let d = Tensor {
                        data: g
                            .data
                            .iter()
                            .zip(t.data.iter())
                            .map(|(d, v)| d * (1.0 - v * v))
                            .collect(),
                        rows: g.rows,
                        cols: g.cols,
                    };
                    accumulate(&mut grads, *a, &d);
                }
                Op::Exp(a) => {
                    let e = &self.nodes[i].value;
                    accumulate(&mut grads, *a, &elementwise(&g, e));
                }
                Op::Transpose(a) => {
                    accumulate(&mut grads, *a, &g.transpose());
                }
                Op::Softmax(a) => {
                    // Softmax couples every element of a row to every other,
                    // so this is a Jacobian-vector product rather than an
                    // elementwise rule: ds = s * (g - sum(g * s)) per row.
                    let s = &self.nodes[i].value;
                    let mut d = Tensor::zeros(s.rows, s.cols);
                    for row in 0..s.rows {
                        let start = row * s.cols;
                        let dot: f64 = (0..s.cols)
                            .map(|c| g.data[start + c] * s.data[start + c])
                            .sum();
                        for col in 0..s.cols {
                            let at = start + col;
                            d.data[at] = s.data[at] * (g.data[at] - dot);
                        }
                    }
                    accumulate(&mut grads, *a, &d);
                }
                Op::Sum(a) => {
                    let x = &self.nodes[a.0].value;
                    let seed = g.data[0];
                    accumulate(
                        &mut grads,
                        *a,
                        &Tensor {
                            data: vec![seed; x.data.len()],
                            rows: x.rows,
                            cols: x.cols,
                        },
                    );
                }
                Op::Reshape(a, rows, cols) => {
                    // A view both ways: the values are the same, so the
                    // gradient is the same numbers under the original shape.
                    accumulate(
                        &mut grads,
                        *a,
                        &Tensor {
                            data: g.data.clone(),
                            rows: *rows,
                            cols: *cols,
                        },
                    );
                }
                Op::Im2Col(a, window) => {
                    // col2im: send each window's gradient back to the pixels
                    // it copied, *adding* where windows overlapped.
                    let image = window.channels * window.height * window.width;
                    let (out_h, out_w) = window.output();
                    let patch = window.patch();
                    let mut d = Tensor::zeros(window.batch, image);
                    for n in 0..window.batch {
                        for oy in 0..out_h {
                            for ox in 0..out_w {
                                let row = (n * out_h + oy) * out_w + ox;
                                for c in 0..window.channels {
                                    for ky in 0..window.kernel_h {
                                        for kx in 0..window.kernel_w {
                                            let iy = oy * window.stride + ky;
                                            let ix = ox * window.stride + kx;
                                            let col =
                                                (c * window.kernel_h + ky) * window.kernel_w + kx;
                                            let at = c * window.height * window.width
                                                + iy * window.width
                                                + ix;
                                            d.data[n * image + at] += g.data[row * patch + col];
                                        }
                                    }
                                }
                            }
                        }
                    }
                    accumulate(&mut grads, *a, &d);
                }
                Op::Planar(a, batch, spatial, channels) => {
                    // The inverse permutation. Assignment rather than
                    // accumulation is right here precisely because the forward
                    // pass moved each value exactly once.
                    let mut d = Tensor::zeros(batch * spatial, *channels);
                    for n in 0..*batch {
                        for s in 0..*spatial {
                            for c in 0..*channels {
                                d.data[(n * spatial + s) * channels + c] =
                                    g.data[n * channels * spatial + c * spatial + s];
                            }
                        }
                    }
                    accumulate(&mut grads, *a, &d);
                }
                Op::Mean(a) => {
                    let x = &self.nodes[a.0].value;
                    let seed = g.data[0] / x.data.len() as f64;
                    accumulate(
                        &mut grads,
                        *a,
                        &Tensor {
                            data: vec![seed; x.data.len()],
                            rows: x.rows,
                            cols: x.cols,
                        },
                    );
                }
            }
        }

        let trainable = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n.op, Op::Leaf { trainable: true }))
            .map(|(i, _)| Var(i))
            .collect();
        Ok(Grads { grads, trainable })
    }
}

/// Add `contribution` into the accumulator for `var`.
///
/// Accumulation rather than assignment, because a value used twice receives a
/// gradient from each use: `x * x` is the smallest case, and getting it wrong
/// halves a gradient rather than breaking anything visibly.
fn accumulate(grads: &mut [Tensor], var: Var, contribution: &Tensor) {
    let target = &mut grads[var.0];
    for (slot, value) in target.data.iter_mut().zip(contribution.data.iter()) {
        *slot += value;
    }
}

fn elementwise(a: &Tensor, b: &Tensor) -> Tensor {
    Tensor {
        data: a
            .data
            .iter()
            .zip(b.data.iter())
            .map(|(x, y)| x * y)
            .collect(),
        rows: a.rows,
        cols: a.cols,
    }
}

/// The gradients from one backward pass.
#[derive(Debug)]
pub struct Grads {
    grads: Vec<Tensor>,
    trainable: Vec<Var>,
}

impl Grads {
    /// The gradient with respect to one value.
    pub fn get(&self, var: Var) -> &Tensor {
        &self.grads[var.0]
    }

    /// Every value created with `param`, in the order they were created.
    pub fn trainable(&self) -> &[Var] {
        &self.trainable
    }
}

/// Compare analytic gradients against numerical ones.
///
/// This is the only reason to trust any of the rules above. Each parameter
/// entry is nudged up and down and the loss re-measured: the slope that
/// results owes nothing to the backward pass, so agreement between the two is
/// real evidence rather than a restatement. It is far too slow for training --
/// two forward passes per parameter -- and exactly right for a test.
///
/// `build` must construct the whole computation from scratch on each call,
/// because a tape records values, not a formula: changing an input means
/// recording again.
pub fn check_gradient<F>(
    parameters: &[Tensor],
    mut build: F,
    epsilon: f64,
) -> Result<Vec<Tensor>, AiError>
where
    F: FnMut(&mut Tape, &[Var]) -> Result<Var, AiError>,
{
    let mut numerical = Vec::new();
    for (index, parameter) in parameters.iter().enumerate() {
        let mut grad = Tensor::zeros(parameter.rows, parameter.cols);
        for entry in 0..parameter.data.len() {
            let mut nudge = |delta: f64| -> Result<f64, AiError> {
                let mut shifted = parameters.to_vec();
                shifted[index].data[entry] += delta;
                let mut tape = Tape::new();
                let vars: Vec<Var> = shifted.into_iter().map(|t| tape.param(t)).collect();
                let out = build(&mut tape, &vars)?;
                Ok(tape.value(out).data[0])
            };
            // The central difference, not the forward one: its error falls off
            // as the square of epsilon, which is the difference between a
            // check that confirms a gradient and one that merely fails to
            // contradict it.
            let up = nudge(epsilon)?;
            let down = nudge(-epsilon)?;
            grad.data[entry] = (up - down) / (2.0 * epsilon);
        }
        numerical.push(grad);
    }
    Ok(numerical)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tensor(rows: usize, cols: usize, data: &[f64]) -> Tensor {
        Tensor::from_vec(rows, cols, data.to_vec()).expect("a well-shaped tensor")
    }

    /// Assert that every analytic gradient matches the numerical one.
    ///
    /// This is the whole test strategy for this module. Each backward rule is
    /// a derivative someone wrote down, and the only evidence that it is the
    /// *right* derivative is agreement with a slope measured from the forward
    /// pass alone -- which owes nothing to the rule being checked.
    fn assert_gradients<F>(parameters: &[Tensor], mut build: F)
    where
        F: FnMut(&mut Tape, &[Var]) -> Result<Var, AiError>,
    {
        let mut tape = Tape::new();
        let vars: Vec<Var> = parameters.iter().cloned().map(|t| tape.param(t)).collect();
        let out = build(&mut tape, &vars).expect("the forward pass to succeed");
        let grads = tape.backward(out).expect("the backward pass to succeed");

        // `&mut F` is itself `FnMut`, so the same closure drives both passes
        // without having to be copyable.
        let numerical =
            check_gradient(parameters, &mut build, 1e-6).expect("the numerical check to run");

        for (index, var) in vars.iter().enumerate() {
            let analytic = grads.get(*var);
            let expected = &numerical[index];
            assert_eq!(
                analytic.data.len(),
                expected.data.len(),
                "parameter {index}: gradient shape must match the parameter's"
            );
            for (entry, (a, n)) in analytic.data.iter().zip(expected.data.iter()).enumerate() {
                // Loose because the numerical side is a finite difference, not
                // because the analytic side is approximate.
                assert!(
                    (a - n).abs() < 1e-4,
                    "parameter {index} entry {entry}: analytic {a} vs numerical {n}"
                );
            }
        }
    }

    #[test]
    fn add_and_sub() {
        let a = tensor(2, 2, &[1.0, -2.0, 3.0, 0.5]);
        let b = tensor(2, 2, &[0.25, 4.0, -1.5, 2.0]);
        assert_gradients(&[a.clone(), b.clone()], |t, v| {
            let sum = t.add(v[0], v[1])?;
            let diff = t.sub(v[0], v[1])?;
            let combined = t.mul(sum, diff)?;
            t.mean(combined)
        });
    }

    #[test]
    fn matmul_both_operands() {
        // The rule most easily written the wrong way round: dA = G @ B^T and
        // dB = A^T @ G, not the reverse. A square test would not catch a swap,
        // so the shapes here are deliberately not square.
        let a = tensor(2, 3, &[1.0, 2.0, -1.0, 0.5, 3.0, 0.25]);
        let b = tensor(3, 2, &[2.0, -1.0, 0.5, 1.5, -2.0, 0.75]);
        assert_gradients(&[a, b], |t, v| {
            let product = t.matmul(v[0], v[1])?;
            t.mean(product)
        });
    }

    #[test]
    fn a_value_used_twice_accumulates() {
        // x * x sends a gradient down both edges. Assigning instead of
        // accumulating halves it, which trains at half the rate and breaks
        // nothing visibly -- exactly the failure this module exists to avoid.
        let x = tensor(1, 3, &[1.5, -2.0, 0.25]);
        assert_gradients(&[x], |t, v| {
            let squared = t.mul(v[0], v[0])?;
            t.sum(squared)
        });
    }

    #[test]
    fn activations() {
        let x = tensor(2, 3, &[0.5, -1.0, 2.0, -0.25, 1.5, 0.0]);
        assert_gradients(std::slice::from_ref(&x), |t, v| {
            let y = t.sigmoid(v[0])?;
            t.mean(y)
        });
        assert_gradients(std::slice::from_ref(&x), |t, v| {
            let y = t.tanh(v[0])?;
            t.mean(y)
        });
        assert_gradients(std::slice::from_ref(&x), |t, v| {
            let y = t.exp(v[0])?;
            t.mean(y)
        });
        // Relu is checked away from zero: the derivative genuinely does not
        // exist at 0, so a numerical check there compares against a slope the
        // analytic rule never claimed to have.
        let away = tensor(2, 3, &[0.5, -1.0, 2.0, -0.25, 1.5, 3.0]);
        assert_gradients(&[away], |t, v| {
            let y = t.relu(v[0])?;
            t.mean(y)
        });
    }

    #[test]
    fn softmax_couples_a_whole_row() {
        // Every element of a row affects every other, so an elementwise rule
        // would pass a one-column test and fail this one.
        let x = tensor(2, 3, &[1.0, 2.0, 0.5, -1.0, 0.25, 3.0]);
        assert_gradients(&[x], |t, v| {
            let s = t.softmax(v[0])?;
            // Weighted, so the row's gradient is not uniform -- a uniform
            // upstream gradient makes softmax's Jacobian term vanish and the
            // test vacuous.
            let weights = t.input(tensor(2, 3, &[1.0, -2.0, 0.5, 3.0, 0.25, -1.0]));
            let weighted = t.mul(s, weights)?;
            t.sum(weighted)
        });
    }

    #[test]
    fn softmax_rows_sum_to_one_and_survive_large_inputs() {
        let mut tape = Tape::new();
        // Without subtracting the row maximum this overflows to inf/inf = NaN.
        let x = tape.input(tensor(2, 3, &[1000.0, 1001.0, 999.0, -1000.0, 0.0, 5.0]));
        let s = tape.softmax(x).expect("softmax to succeed");
        let value = tape.value(s);
        assert!(value.data.iter().all(|v| v.is_finite()), "{value:?}");
        for row in 0..value.rows {
            let total: f64 = (0..value.cols).map(|c| value.get(row, c)).sum();
            assert!((total - 1.0).abs() < 1e-12, "row {row} summed to {total}");
        }
    }

    #[test]
    fn bias_gradient_sums_down_the_columns() {
        // The bias took part in every row, so its gradient is a column sum --
        // the one place broadcasting is a real operation rather than a reshape.
        let x = tensor(3, 2, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let bias = tensor(1, 2, &[0.5, -1.0]);
        assert_gradients(&[x, bias], |t, v| {
            let y = t.add_bias(v[0], v[1])?;
            let squared = t.mul(y, y)?;
            t.mean(squared)
        });
    }

    #[test]
    fn a_whole_layer_end_to_end() {
        // What a Dense layer actually is: xW + b, an activation, then a loss.
        // If the composition of rules is right, this is the evidence.
        let x = tensor(2, 3, &[0.5, -1.0, 2.0, 1.5, 0.25, -0.5]);
        let w = tensor(3, 2, &[0.1, -0.2, 0.3, 0.4, -0.5, 0.6]);
        let b = tensor(1, 2, &[0.05, -0.1]);
        let target = tensor(2, 2, &[1.0, 0.0, 0.0, 1.0]);
        assert_gradients(&[x, w, b], move |t, v| {
            let hidden = t.matmul(v[0], v[1])?;
            let biased = t.add_bias(hidden, v[2])?;
            let activated = t.tanh(biased)?;
            let expected = t.input(target.clone());
            t.mse(activated, expected)
        });
    }

    #[test]
    fn transpose_and_scale() {
        let a = tensor(2, 3, &[1.0, -2.0, 0.5, 3.0, 0.25, -1.5]);
        assert_gradients(&[a], |t, v| {
            let scaled = t.scale(v[0], 2.5)?;
            let flipped = t.transpose(scaled)?;
            let squared = t.mul(flipped, flipped)?;
            t.sum(squared)
        });
    }

    #[test]
    fn backward_refuses_a_non_scalar() {
        let mut tape = Tape::new();
        let a = tape.param(tensor(2, 2, &[1.0, 2.0, 3.0, 4.0]));
        let err = tape.backward(a).expect_err("a matrix has no gradient");
        assert!(
            format!("{err}").contains("single value"),
            "unhelpful message: {err}"
        );
    }

    #[test]
    fn a_detached_tensor_stops_the_chain() {
        // The escape hatch, pinned so that it stays deliberate: a value taken
        // out and put back is a leaf, and nothing upstream of it hears about
        // the loss.
        let mut tape = Tape::new();
        let x = tape.param(tensor(1, 2, &[2.0, 3.0]));
        let doubled = tape.scale(x, 2.0).expect("scale");
        let detached = tape.value(doubled).clone();
        let reentered = tape.input(detached);
        let loss = tape.sum(reentered).expect("sum");
        let grads = tape.backward(loss).expect("backward");
        assert!(
            grads.get(x).data.iter().all(|v| *v == 0.0),
            "detaching should stop the gradient, got {:?}",
            grads.get(x).data
        );
    }

    #[test]
    fn trainable_leaves_are_reported_in_order() {
        let mut tape = Tape::new();
        let w = tape.param(tensor(1, 1, &[2.0]));
        let x = tape.input(tensor(1, 1, &[3.0]));
        let b = tape.param(tensor(1, 1, &[1.0]));
        let wx = tape.mul(w, x).expect("mul");
        let out = tape.add(wx, b).expect("add");
        let grads = tape.backward(out).expect("backward");
        assert_eq!(grads.trainable(), &[w, b], "inputs are not trainable");
        // d(wx + b)/dw = x = 3, d/db = 1.
        assert_eq!(grads.get(w).data, vec![3.0]);
        assert_eq!(grads.get(b).data, vec![1.0]);
    }
}
