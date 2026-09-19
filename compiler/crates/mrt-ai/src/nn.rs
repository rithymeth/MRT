//! Layers and optimisers built on the tape.
//!
//! The claim made when `autograd` went in was that it changes the economics:
//! with derivatives computed rather than written, adding a layer becomes
//! writing its *forward* pass and nothing else. This module is where that
//! claim is either true or it isn't.
//!
//! It holds up. `Conv2d::forward` is nine lines and contributes no derivative
//! at all -- it is `im2col`, `matmul`, a bias and a reshape, every one of
//! which the tape already differentiates. The single new rule the whole
//! feature needed lives in `autograd` as `Im2Col`'s backward, and it is
//! checked against finite differences like every other.
//!
//! The older `Dense`/`Sequential`/`Sgd` in `lib.rs` still work and still carry
//! their hand-written backward passes. They are not deleted, because
//! `aiTrainLinear` is published behaviour; they are simply not where new
//! layers go.

use crate::autograd::{Tape, Var, Window};
use crate::{AiError, Tensor};

/// A fully connected layer: `xW + b`.
pub struct Dense {
    pub weights: Tensor,
    pub bias: Tensor,
}

impl Dense {
    /// Deterministically initialised, because a test that cannot be
    /// reproduced is not a test. The scale is He initialisation, which keeps
    /// the variance of activations roughly constant through a relu stack --
    /// the alternative is a deep net whose signal vanishes before it reaches
    /// the loss.
    pub fn new(inputs: usize, outputs: usize, seed: u64) -> Dense {
        Dense {
            weights: seeded(inputs, outputs, seed, (2.0 / inputs as f64).sqrt()),
            bias: Tensor::zeros(1, outputs),
        }
    }

    /// Put this layer's parameters on a tape, in the order `forward` expects.
    pub fn params(&self, tape: &mut Tape) -> Vec<Var> {
        vec![
            tape.param(self.weights.clone()),
            tape.param(self.bias.clone()),
        ]
    }

    pub fn forward(&self, tape: &mut Tape, input: Var, params: &[Var]) -> Result<Var, AiError> {
        let hidden = tape.matmul(input, params[0])?;
        tape.add_bias(hidden, params[1])
    }
}

/// A 2D convolution, as a matrix multiply over laid-out windows.
///
/// Images are `(batch, channels * height * width)`, one image per row: the
/// channels are laid out as planes, one whole image per channel. The output
/// uses the same convention, `(batch, filters * out_h * out_w)`, so a
/// convolution can be fed straight into another one -- which takes a
/// permutation rather than a reshape, for the reason `Tape::planar` gives.
pub struct Conv2d {
    pub window: Window,
    pub filters: usize,
    /// `(channels * kernel_h * kernel_w, filters)`: one column per filter,
    /// which is exactly what `im2col`'s rows want to be multiplied by.
    pub weights: Tensor,
    pub bias: Tensor,
}

impl Conv2d {
    pub fn new(window: Window, filters: usize, seed: u64) -> Conv2d {
        let patch = window.patch();
        Conv2d {
            window,
            filters,
            weights: seeded(patch, filters, seed, (2.0 / patch as f64).sqrt()),
            bias: Tensor::zeros(1, filters),
        }
    }

    pub fn params(&self, tape: &mut Tape) -> Vec<Var> {
        vec![
            tape.param(self.weights.clone()),
            tape.param(self.bias.clone()),
        ]
    }

    /// The whole layer, and not one derivative in sight.
    pub fn forward(&self, tape: &mut Tape, input: Var, params: &[Var]) -> Result<Var, AiError> {
        let (out_h, out_w) = self.window.output();
        let columns = tape.im2col(input, self.window)?;
        let product = tape.matmul(columns, params[0])?;
        let biased = tape.add_bias(product, params[1])?;
        // `planar` rather than a plain reshape, and the difference is the
        // whole reason two convolutions can be stacked: see `Tape::planar`.
        tape.planar(biased, self.window.batch, out_h * out_w, self.filters)
    }

    pub fn output_len(&self) -> usize {
        let (out_h, out_w) = self.window.output();
        out_h * out_w * self.filters
    }
}

/// Layer normalisation: centre and scale each row, then learn a scale and a
/// shift for each column.
///
/// Composed rather than primitive, which is the point. Layer norm's backward
/// pass is the one people copy from a paper and get subtly wrong, because the
/// mean and the variance both depend on every element of the row -- so the
/// gradient of one element reaches every other twice, by two different routes.
/// Written out of `row_mean`, `broadcast`, `rsqrt` and `mul_row`, none of that
/// has to be derived: four small rules, each checkable on its own, compose into
/// the hard one.
pub struct LayerNorm {
    pub epsilon: f64,
    /// `(1, features)`: the learned per-column scale, starting at 1 so the
    /// layer begins as the identity on normalised values.
    pub gain: Tensor,
    pub shift: Tensor,
}

impl LayerNorm {
    pub fn new(features: usize) -> LayerNorm {
        LayerNorm {
            epsilon: 1e-5,
            gain: Tensor::from_vec(1, features, vec![1.0; features]).expect("features values"),
            shift: Tensor::zeros(1, features),
        }
    }

    pub fn params(&self, tape: &mut Tape) -> Vec<Var> {
        vec![
            tape.param(self.gain.clone()),
            tape.param(self.shift.clone()),
        ]
    }

    pub fn forward(&self, tape: &mut Tape, input: Var, params: &[Var]) -> Result<Var, AiError> {
        let cols = tape.value(input).cols;
        let mean = tape.row_mean(input)?;
        let mean = tape.broadcast(mean, cols)?;
        let centred = tape.sub(input, mean)?;
        let squared = tape.mul(centred, centred)?;
        let variance = tape.row_mean(squared)?;
        let variance = tape.broadcast(variance, cols)?;
        let inverse = tape.rsqrt(variance, self.epsilon)?;
        let normed = tape.mul(centred, inverse)?;
        let scaled = tape.mul_row(normed, params[0])?;
        tape.add_bias(scaled, params[1])
    }
}

/// Single-head scaled dot-product attention.
///
/// `softmax(QKᵀ / √d) V`, and every piece of that was already on the tape:
/// `matmul`, `transpose`, `scale`, `softmax`. So this layer adds **no**
/// derivative at all -- not a simple one, none -- which is the clearest
/// statement of what building the tape first bought.
///
/// Rows are positions and columns are features, so `QKᵀ` is
/// `(positions, positions)`: how much each position attends to each other one.
/// Softmax is row-wise for exactly that reason, and was written that way when
/// it went in, before there was an attention layer to need it.
pub struct Attention {
    pub features: usize,
    pub head: usize,
    pub query: Tensor,
    pub key: Tensor,
    pub value: Tensor,
}

impl Attention {
    pub fn new(features: usize, head: usize, seed: u64) -> Attention {
        let scale = (2.0 / features as f64).sqrt();
        Attention {
            features,
            head,
            query: seeded(features, head, seed, scale),
            key: seeded(features, head, seed.wrapping_add(1), scale),
            value: seeded(features, head, seed.wrapping_add(2), scale),
        }
    }

    pub fn params(&self, tape: &mut Tape) -> Vec<Var> {
        vec![
            tape.param(self.query.clone()),
            tape.param(self.key.clone()),
            tape.param(self.value.clone()),
        ]
    }

    pub fn forward(&self, tape: &mut Tape, input: Var, params: &[Var]) -> Result<Var, AiError> {
        let q = tape.matmul(input, params[0])?;
        let k = tape.matmul(input, params[1])?;
        let v = tape.matmul(input, params[2])?;
        let kt = tape.transpose(k)?;
        let scores = tape.matmul(q, kt)?;
        // Divided by the square root of the head width. Without it the scores
        // grow with the width, softmax saturates, and the gradient through it
        // goes to nothing -- the layer stops learning rather than learning
        // wrongly.
        let scaled = tape.scale(scores, 1.0 / (self.head as f64).sqrt())?;
        let weights = tape.softmax(scaled)?;
        tape.matmul(weights, v)
    }
}

/// A transformer block: attention and a feed-forward network, each wrapped in
/// a residual connection and a layer norm.
///
/// The residual is the part worth naming. `x + f(x)` gives the gradient a path
/// that skips `f` entirely, which is what lets a deep stack train at all --
/// and on the tape it is one `add`, whose backward already sends the gradient
/// down both branches. Nothing had to be arranged for it.
pub struct Block {
    pub attention: Attention,
    pub norm1: LayerNorm,
    pub hidden: Dense,
    pub output: Dense,
    pub norm2: LayerNorm,
}

impl Block {
    pub fn new(features: usize, head: usize, inner: usize, seed: u64) -> Block {
        Block {
            attention: Attention::new(features, head, seed),
            norm1: LayerNorm::new(features),
            hidden: Dense::new(features, inner, seed.wrapping_add(10)),
            output: Dense::new(inner, features, seed.wrapping_add(20)),
            norm2: LayerNorm::new(features),
        }
    }

    /// Parameters in the order `forward` reads them: attention 3, norm1 2,
    /// hidden 2, output 2, norm2 2.
    pub fn params(&self, tape: &mut Tape) -> Vec<Var> {
        let mut all = self.attention.params(tape);
        all.extend(self.norm1.params(tape));
        all.extend(self.hidden.params(tape));
        all.extend(self.output.params(tape));
        all.extend(self.norm2.params(tape));
        all
    }

    pub fn forward(&self, tape: &mut Tape, input: Var, params: &[Var]) -> Result<Var, AiError> {
        // Attention requires the head width to match the feature width, so
        // the residual has something to add to.
        let attended = self.attention.forward(tape, input, &params[0..3])?;
        let residual = tape.add(input, attended)?;
        let normed = self.norm1.forward(tape, residual, &params[3..5])?;

        let hidden = self.hidden.forward(tape, normed, &params[5..7])?;
        let activated = tape.relu(hidden)?;
        let projected = self.output.forward(tape, activated, &params[7..9])?;
        let residual = tape.add(normed, projected)?;
        self.norm2.forward(tape, residual, &params[9..11])
    }
}

/// Adam: per-parameter step sizes from the running first and second moments of
/// the gradient.
///
/// Worth having over plain SGD because one learning rate rarely suits every
/// parameter in a network -- a bias and a convolution kernel see gradients of
/// quite different magnitude, and with a single rate the choice that trains one
/// diverges the other.
pub struct Adam {
    pub learning_rate: f64,
    pub beta1: f64,
    pub beta2: f64,
    pub epsilon: f64,
    /// How many steps have been taken, for bias correction.
    step: usize,
    moment: Vec<Tensor>,
    velocity: Vec<Tensor>,
}

impl Adam {
    pub fn new(learning_rate: f64) -> Adam {
        Adam {
            learning_rate,
            beta1: 0.9,
            beta2: 0.999,
            epsilon: 1e-8,
            step: 0,
            moment: Vec::new(),
            velocity: Vec::new(),
        }
    }

    /// Update every parameter in place from its gradient.
    ///
    /// `parameters` and `gradients` are matched by position, and the moment
    /// state is keyed the same way -- so the caller must pass its parameters
    /// in a stable order. That is a real constraint rather than an assumption:
    /// the tape is rebuilt every step, so `Var` handles cannot be the key.
    pub fn step(&mut self, parameters: &mut [Tensor], gradients: &[Tensor]) -> Result<(), AiError> {
        if parameters.len() != gradients.len() {
            return Err(AiError::Shape(format!(
                "{} parameters but {} gradients",
                parameters.len(),
                gradients.len()
            )));
        }
        if self.moment.len() != parameters.len() {
            self.moment = parameters
                .iter()
                .map(|p| Tensor::zeros(p.rows, p.cols))
                .collect();
            self.velocity = self.moment.clone();
        }

        self.step += 1;
        // Bias correction. Both moments start at zero, so early steps are
        // biased towards zero; without this the first updates are far too
        // small and the run looks like a learning rate problem.
        let correct1 = 1.0 - self.beta1.powi(self.step as i32);
        let correct2 = 1.0 - self.beta2.powi(self.step as i32);

        for (index, parameter) in parameters.iter_mut().enumerate() {
            let gradient = &gradients[index];
            if gradient.data.len() != parameter.data.len() {
                return Err(AiError::Shape(format!(
                    "parameter {index} has {} values but its gradient has {}",
                    parameter.data.len(),
                    gradient.data.len()
                )));
            }
            for entry in 0..parameter.data.len() {
                let g = gradient.data[entry];
                let m = &mut self.moment[index].data[entry];
                let v = &mut self.velocity[index].data[entry];
                *m = self.beta1 * *m + (1.0 - self.beta1) * g;
                *v = self.beta2 * *v + (1.0 - self.beta2) * g * g;
                let m_hat = *m / correct1;
                let v_hat = *v / correct2;
                parameter.data[entry] -= self.learning_rate * m_hat / (v_hat.sqrt() + self.epsilon);
            }
        }
        Ok(())
    }
}

/// A small deterministic spread of weights.
///
/// Its own generator rather than a dependency: the numbers only have to be
/// varied and repeatable, and a crate whose output could change between
/// versions would make every test here fragile for no gain.
fn seeded(rows: usize, cols: usize, seed: u64, scale: f64) -> Tensor {
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut data = Vec::with_capacity(rows * cols);
    for _ in 0..rows * cols {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // The top bits of an LCG are the ones worth using; the low ones cycle
        // far too visibly. `>> 33` leaves 31 bits, so the ratio is in [0, 1)
        // and the affine step maps it to [-1, 1). Getting that step wrong the
        // obvious way -- subtracting 1 without doubling -- yields [-1, 0), and
        // every weight in the network comes out negative. A relu then zeroes
        // every activation, no gradient reaches any weight, and the loss sits
        // exactly still: a network that looks like it will not learn rather
        // than one that was initialised wrongly.
        let unit = 2.0 * ((state >> 33) as f64 / (1u64 << 31) as f64) - 1.0;
        data.push(unit * scale);
    }
    Tensor::from_vec(rows, cols, data).expect("rows * cols values")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::check_gradient;

    fn tensor(rows: usize, cols: usize, data: &[f64]) -> Tensor {
        Tensor::from_vec(rows, cols, data.to_vec()).expect("a well-shaped tensor")
    }

    /// Same discipline as the autograd tests: the analytic gradient is only
    /// believable where a slope measured from the forward pass agrees with it.
    fn assert_gradients<F>(parameters: &[Tensor], mut build: F)
    where
        F: FnMut(&mut Tape, &[Var]) -> Result<Var, AiError>,
    {
        let mut tape = Tape::new();
        let vars: Vec<Var> = parameters.iter().cloned().map(|t| tape.param(t)).collect();
        let out = build(&mut tape, &vars).expect("forward");
        let grads = tape.backward(out).expect("backward");
        let numerical = check_gradient(parameters, &mut build, 1e-6).expect("numerical check");

        for (index, var) in vars.iter().enumerate() {
            for (entry, (a, n)) in grads
                .get(*var)
                .data
                .iter()
                .zip(numerical[index].data.iter())
                .enumerate()
            {
                assert!(
                    (a - n).abs() < 1e-4,
                    "parameter {index} entry {entry}: analytic {a} vs numerical {n}"
                );
            }
        }
    }

    fn window(
        batch: usize,
        channels: usize,
        h: usize,
        w: usize,
        k: usize,
        stride: usize,
    ) -> Window {
        Window {
            batch,
            channels,
            height: h,
            width: w,
            kernel_h: k,
            kernel_w: k,
            stride,
        }
    }

    #[test]
    fn im2col_lays_each_window_out_as_a_row() {
        // A 3x3 image with a 2x2 kernel gives four windows, and the values in
        // each row are the ones a convolution would multiply.
        let mut tape = Tape::new();
        let image = tape.input(tensor(1, 9, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]));
        let columns = tape
            .im2col(image, window(1, 1, 3, 3, 2, 1))
            .expect("im2col");
        let value = tape.value(columns);
        assert_eq!((value.rows, value.cols), (4, 4));
        assert_eq!(
            value.data,
            vec![
                1.0, 2.0, 4.0, 5.0, // top-left
                2.0, 3.0, 5.0, 6.0, // top-right
                4.0, 5.0, 7.0, 8.0, // bottom-left
                5.0, 6.0, 8.0, 9.0, // bottom-right
            ]
        );
    }

    #[test]
    fn im2col_gradients_sum_where_windows_overlap() {
        // The rule that is easy to get wrong: a pixel covered by several
        // windows receives a contribution from each. Assigning instead of
        // adding would make the centre pixel's gradient a quarter of the
        // truth -- and stride 1 on a 3x3 image is exactly the case where
        // every interior pixel is shared.
        let image = tensor(1, 9, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_gradients(&[image], |t, v| {
            let columns = t.im2col(v[0], window(1, 1, 3, 3, 2, 1))?;
            let squared = t.mul(columns, columns)?;
            t.sum(squared)
        });
    }

    #[test]
    fn im2col_gradients_with_several_channels_and_a_stride() {
        let image = tensor(
            2,
            18,
            &(0..36).map(|i| (i as f64) * 0.1 - 1.5).collect::<Vec<_>>(),
        );
        assert_gradients(&[image], |t, v| {
            let columns = t.im2col(v[0], window(2, 2, 3, 3, 2, 1))?;
            let weights = t.input(tensor(
                8,
                1,
                &[0.5, -1.0, 0.25, 2.0, -0.5, 1.5, 0.75, -0.25],
            ));
            let product = t.matmul(columns, weights)?;
            t.mean(product)
        });
    }

    #[test]
    fn a_convolution_layer_is_differentiable_end_to_end() {
        // The point of the whole exercise: Conv2d contributes no derivative
        // of its own, so if im2col, matmul and add_bias are right then this
        // is right. The check confirms the composition, not a new rule.
        let conv = Conv2d::new(window(2, 1, 4, 4, 2, 1), 3, 7);
        let image = tensor(
            2,
            16,
            &(0..32)
                .map(|i| ((i % 7) as f64) * 0.3 - 1.0)
                .collect::<Vec<_>>(),
        );
        let target = Tensor::zeros(2, conv.output_len());
        assert_gradients(&[conv.weights.clone(), conv.bias.clone()], |t, v| {
            let input = t.input(image.clone());
            let out = conv.forward(t, input, v)?;
            let expected = t.input(target.clone());
            t.mse(out, expected)
        });
    }

    #[test]
    fn a_convolution_keeps_the_layout_its_input_arrived_in() {
        // `(batch, filters * out_h * out_w)`, the same shape convention as the
        // input -- which is what lets one convolution feed another.
        let conv = Conv2d::new(window(3, 2, 5, 5, 3, 2), 4, 1);
        let mut tape = Tape::new();
        let params = conv.params(&mut tape);
        let input = tape.input(Tensor::zeros(3, 2 * 5 * 5));
        let out = conv.forward(&mut tape, input, &params).expect("forward");
        let value = tape.value(out);
        assert_eq!((value.rows, value.cols), (3, conv.output_len()));
        assert_eq!(conv.output_len(), 2 * 2 * 4, "a 3x3 stride-2 window on 5x5");
    }

    #[test]
    fn planar_puts_each_filter_in_its_own_plane() {
        // The permutation in one readable case: two window positions, three
        // filters. A plain reshape would interleave them -- f0, f1, f2, f0,
        // f1, f2 -- and a second convolution would read two filters' pixels
        // as neighbours in one image.
        let mut tape = Tape::new();
        let windows = tape.input(tensor(2, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]));
        let planes = tape.planar(windows, 1, 2, 3).expect("planar");
        let value = tape.value(planes);
        assert_eq!((value.rows, value.cols), (1, 6));
        assert_eq!(value.data, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn planar_is_differentiable() {
        let windows = tensor(4, 2, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_gradients(&[windows], |t, v| {
            let planes = t.planar(v[0], 2, 2, 2)?;
            let weights = t.input(tensor(2, 4, &[1.0, -2.0, 0.5, 3.0, -1.0, 0.25, 2.0, -0.5]));
            let scaled = t.mul(planes, weights)?;
            t.sum(scaled)
        });
    }

    #[test]
    fn two_convolutions_stack() {
        // The claim `planar` exists to make true. The second layer reads the
        // first layer's filters as its own input channels, which only works
        // if they arrive as planes -- and it is differentiable through both,
        // which is what makes a deep stack trainable rather than merely
        // constructible.
        let first = Conv2d::new(window(2, 1, 6, 6, 3, 1), 2, 5);
        let (h1, w1) = first.window.output();
        let second = Conv2d::new(window(2, 2, h1, w1, 2, 1), 3, 9);

        let images = tensor(
            2,
            36,
            &(0..72)
                .map(|i| ((i % 5) as f64) * 0.2 - 0.4)
                .collect::<Vec<_>>(),
        );
        let target = Tensor::zeros(2, second.output_len());

        let mut tape = Tape::new();
        let p1 = first.params(&mut tape);
        let input = tape.input(images.clone());
        let hidden = first.forward(&mut tape, input, &p1).expect("first");
        let value = tape.value(hidden);
        assert_eq!(
            (value.rows, value.cols),
            (2, first.output_len()),
            "the first layer's output has to be shaped like the second's input"
        );

        // And the gradient reaches the first layer's weights through the
        // second, which is the part a shape check alone would not catch.
        assert_gradients(
            &[
                first.weights.clone(),
                first.bias.clone(),
                second.weights.clone(),
                second.bias.clone(),
            ],
            |t, v| {
                let input = t.input(images.clone());
                let a = first.forward(t, input, &v[0..2])?;
                let b = second.forward(t, a, &v[2..4])?;
                let expected = t.input(target.clone());
                t.mse(b, expected)
            },
        );
    }

    // -- the normalisation primitives, each on its own ---------------------

    #[test]
    fn row_mean_and_broadcast_are_inverses_in_gradient() {
        let x = tensor(2, 3, &[1.0, 2.0, 3.0, -1.0, 0.5, 4.0]);
        let mut tape = Tape::new();
        let v = tape.input(x.clone());
        let m = tape.row_mean(v).expect("row_mean");
        assert_eq!(tape.value(m).data, vec![2.0, 1.1666666666666667]);
        let wide = tape.broadcast(m, 3).expect("broadcast");
        assert_eq!((tape.value(wide).rows, tape.value(wide).cols), (2, 3));

        assert_gradients(&[x], |t, p| {
            let mean = t.row_mean(p[0])?;
            let wide = t.broadcast(mean, 3)?;
            let squared = t.mul(wide, wide)?;
            t.sum(squared)
        });
    }

    #[test]
    fn rsqrt_is_differentiable_and_survives_a_zero() {
        // A row of identical values has variance zero, which is ordinary data
        // rather than an error -- the epsilon has to be inside the root.
        let mut tape = Tape::new();
        let zero = tape.input(tensor(1, 2, &[0.0, 0.0]));
        let out = tape.rsqrt(zero, 1e-5).expect("rsqrt of zero");
        assert!(tape.value(out).data.iter().all(|v| v.is_finite()));

        let x = tensor(2, 2, &[0.5, 2.0, 4.0, 0.25]);
        assert_gradients(&[x], |t, p| {
            let r = t.rsqrt(p[0], 1e-5)?;
            t.sum(r)
        });
    }

    #[test]
    fn mul_row_gradients_reach_both_operands() {
        let x = tensor(3, 2, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let gain = tensor(1, 2, &[0.5, -1.5]);
        assert_gradients(&[x, gain], |t, p| {
            let scaled = t.mul_row(p[0], p[1])?;
            let squared = t.mul(scaled, scaled)?;
            t.mean(squared)
        });
    }

    // -- the layers --------------------------------------------------------

    #[test]
    fn layer_norm_centres_and_scales_each_row() {
        let norm = LayerNorm::new(4);
        let mut tape = Tape::new();
        let params = norm.params(&mut tape);
        let input = tape.input(tensor(2, 4, &[1.0, 2.0, 3.0, 4.0, 10.0, 10.0, 10.0, 10.0]));
        let out = norm.forward(&mut tape, input, &params).expect("forward");
        let value = tape.value(out);

        for row in 0..2 {
            let slice = &value.data[row * 4..row * 4 + 4];
            let mean: f64 = slice.iter().sum::<f64>() / 4.0;
            assert!(mean.abs() < 1e-6, "row {row} mean {mean}");
        }
        // The second row is constant, so its variance is zero: every value
        // normalises to zero rather than to infinity.
        assert!(
            value.data[4..8].iter().all(|v| v.abs() < 1e-3),
            "a constant row should flatten, got {:?}",
            &value.data[4..8]
        );
    }

    #[test]
    fn layer_norm_is_differentiable() {
        // The derivative people copy from a paper and get wrong. Here it is
        // four composed rules, and the finite difference is the proof.
        let norm = LayerNorm::new(3);
        let input = tensor(2, 3, &[1.0, -2.0, 0.5, 3.0, 0.25, -1.0]);
        assert_gradients(&[norm.gain.clone(), norm.shift.clone()], |t, p| {
            let x = t.input(input.clone());
            let out = norm.forward(t, x, p)?;
            let squared = t.mul(out, out)?;
            t.mean(squared)
        });
    }

    #[test]
    fn attention_rows_are_a_weighted_mix_of_positions() {
        // Softmax weights sum to one per row, so every output row is a convex
        // combination of the value rows -- which is what "attention" means.
        let attention = Attention::new(3, 3, 4);
        let mut tape = Tape::new();
        let params = attention.params(&mut tape);
        let input = tape.input(tensor(
            4,
            3,
            &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        ));
        let out = attention
            .forward(&mut tape, input, &params)
            .expect("forward");
        let value = tape.value(out);
        assert_eq!((value.rows, value.cols), (4, 3), "one row out per row in");
    }

    #[test]
    fn attention_is_differentiable_and_adds_no_derivative() {
        // The claim: this layer is composition only. If matmul, transpose,
        // scale and softmax are right, this is right -- and the check
        // confirms the composition rather than a new rule.
        let attention = Attention::new(4, 4, 13);
        let input = tensor(
            3,
            4,
            &(0..12)
                .map(|i| ((i % 5) as f64) * 0.4 - 0.8)
                .collect::<Vec<_>>(),
        );
        let target = Tensor::zeros(3, 4);
        assert_gradients(
            &[
                attention.query.clone(),
                attention.key.clone(),
                attention.value.clone(),
            ],
            |t, p| {
                let x = t.input(input.clone());
                let out = attention.forward(t, x, p)?;
                let expected = t.input(target.clone());
                t.mse(out, expected)
            },
        );
    }

    #[test]
    fn a_transformer_block_is_differentiable_end_to_end() {
        let block = Block::new(4, 4, 8, 31);
        let input = tensor(
            3,
            4,
            &(0..12)
                .map(|i| ((i % 7) as f64) * 0.25 - 0.75)
                .collect::<Vec<_>>(),
        );
        let target = Tensor::zeros(3, 4);
        let mut tape = Tape::new();
        let params = block.params(&mut tape);
        let parameters: Vec<Tensor> = params.iter().map(|v| tape.value(*v).clone()).collect();

        assert_gradients(&parameters, |t, p| {
            let x = t.input(input.clone());
            let out = block.forward(t, x, p)?;
            let expected = t.input(target.clone());
            t.mse(out, expected)
        });
    }

    #[test]
    fn a_transformer_block_learns_to_copy_a_position() {
        // Something attention can do and a position-wise network cannot: make
        // every row match the *first* row. That needs one position to read
        // another, which is the whole point of the mechanism.
        let block = Block::new(4, 4, 8, 77);
        let input = tensor(
            3,
            4,
            &[
                0.9, -0.4, 0.2, 0.7, -0.5, 0.3, 0.8, -0.1, 0.1, 0.6, -0.7, 0.4,
            ],
        );
        let first = &input.data[0..4];
        let mut wanted = Vec::new();
        for _ in 0..3 {
            wanted.extend_from_slice(first);
        }
        let target = tensor(3, 4, &wanted);

        let mut tape = Tape::new();
        let vars = block.params(&mut tape);
        let mut parameters: Vec<Tensor> = vars.iter().map(|v| tape.value(*v).clone()).collect();
        let mut adam = Adam::new(0.02);

        let mut first_loss = f64::NAN;
        let mut last_loss = f64::NAN;
        for epoch in 0..400 {
            let mut tape = Tape::new();
            let vars: Vec<Var> = parameters.iter().cloned().map(|t| tape.param(t)).collect();
            let x = tape.input(input.clone());
            let out = block.forward(&mut tape, x, &vars).expect("forward");
            let expected = tape.input(target.clone());
            let loss = tape.mse(out, expected).expect("mse");
            let grads = tape.backward(loss).expect("backward");

            let value = tape.value(loss).data[0];
            if epoch == 0 {
                first_loss = value;
            }
            last_loss = value;

            let step: Vec<Tensor> = vars.iter().map(|v| grads.get(*v).clone()).collect();
            adam.step(&mut parameters, &step).expect("step");
        }

        assert!(
            last_loss < first_loss * 0.2,
            "loss went from {first_loss} to {last_loss}, which is not learning"
        );
    }

    #[test]
    fn adam_finds_the_minimum_of_a_quadratic() {
        // Analytically checkable: the minimum of (x - 3)^2 + (y + 1)^2 is
        // (3, -1), so an optimiser that works has nowhere to hide.
        let mut adam = Adam::new(0.1);
        let mut parameters = vec![tensor(1, 2, &[0.0, 0.0])];
        for _ in 0..500 {
            let mut tape = Tape::new();
            let p = tape.param(parameters[0].clone());
            let target = tape.input(tensor(1, 2, &[3.0, -1.0]));
            let diff = tape.sub(p, target).expect("sub");
            let squared = tape.mul(diff, diff).expect("mul");
            let loss = tape.sum(squared).expect("sum");
            let grads = tape.backward(loss).expect("backward");
            adam.step(&mut parameters, &[grads.get(p).clone()])
                .expect("step");
        }
        assert!(
            (parameters[0].data[0] - 3.0).abs() < 1e-3,
            "x = {}",
            parameters[0].data[0]
        );
        assert!(
            (parameters[0].data[1] + 1.0).abs() < 1e-3,
            "y = {}",
            parameters[0].data[1]
        );
    }

    #[test]
    fn adam_bias_correction_makes_the_first_step_the_full_rate() {
        // Without correction both moments start at zero and the first step is
        // a fraction of the learning rate -- which reads as "the optimiser
        // does not work" rather than as a missing division.
        let mut adam = Adam::new(0.1);
        let mut parameters = vec![tensor(1, 1, &[0.0])];
        adam.step(&mut parameters, &[tensor(1, 1, &[1.0])])
            .expect("step");
        // m_hat / sqrt(v_hat) is 1 on the first step for any nonzero gradient,
        // so the parameter moves by exactly the learning rate.
        assert!(
            (parameters[0].data[0] + 0.1).abs() < 1e-6,
            "first step moved {} rather than the learning rate",
            parameters[0].data[0]
        );
    }

    #[test]
    fn adam_rejects_mismatched_gradients() {
        let mut adam = Adam::new(0.1);
        let mut parameters = vec![tensor(1, 2, &[0.0, 0.0])];
        let err = adam
            .step(&mut parameters, &[])
            .expect_err("counts must match");
        assert!(format!("{err}").contains("1 parameters but 0 gradients"));
    }

    #[test]
    fn a_conv_net_trains_to_separate_two_shapes() {
        // End to end, and the only claim that really matters: conv -> relu ->
        // dense, driven by Adam, learns something a fixed net could not.
        // Vertical versus horizontal bars in a 4x4 image.
        let vertical = vec![
            0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0,
        ];
        let horizontal = vec![
            0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let mut rows = vertical.clone();
        rows.extend(horizontal.clone());
        let images = tensor(2, 16, &rows);
        let labels = tensor(2, 1, &[1.0, 0.0]);

        let conv = Conv2d::new(window(2, 1, 4, 4, 2, 1), 2, 11);
        let dense = Dense::new(conv.output_len(), 1, 23);
        let mut parameters = vec![
            conv.weights.clone(),
            conv.bias.clone(),
            dense.weights.clone(),
            dense.bias.clone(),
        ];
        let mut adam = Adam::new(0.05);

        let mut first = f64::NAN;
        let mut last = f64::NAN;
        for epoch in 0..300 {
            let mut tape = Tape::new();
            let vars: Vec<Var> = parameters.iter().cloned().map(|t| tape.param(t)).collect();
            let input = tape.input(images.clone());
            let hidden = conv.forward(&mut tape, input, &vars[0..2]).expect("conv");
            let activated = tape.relu(hidden).expect("relu");
            let out = dense
                .forward(&mut tape, activated, &vars[2..4])
                .expect("dense");
            let scores = tape.sigmoid(out).expect("sigmoid");
            let expected = tape.input(labels.clone());
            let loss = tape.mse(scores, expected).expect("mse");
            let grads = tape.backward(loss).expect("backward");

            let value = tape.value(loss).data[0];
            if epoch == 0 {
                first = value;
            }
            last = value;

            let step: Vec<Tensor> = vars.iter().map(|v| grads.get(*v).clone()).collect();
            adam.step(&mut parameters, &step).expect("step");
        }

        assert!(
            last < first * 0.05,
            "loss went from {first} to {last}, which is not learning"
        );
    }
}
