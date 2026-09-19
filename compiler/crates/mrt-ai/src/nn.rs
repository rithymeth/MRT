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
